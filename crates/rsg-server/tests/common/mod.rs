//! Shared subprocess-test harness for spawning `mock-scheduler`, modeled on
//! `tests/cli.rs`'s own `Server` harness for `rsg-server`. Reused by every
//! Phase 3 test file that needs a real mock-scheduler subprocess over real
//! `ipc://` sockets, and by Phase 5's HTTP integration tests
//! (`http_client`, `test_server`).

pub mod http_client;
pub mod test_server;

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rsg_server::handshake::{EXPECTED_UPSTREAM_SHA, Handshake, parse_handshake};
use rsg_server::transport::{DetokSource, Endpoint, Role, ZmqTransport};
use rsg_wire::{BackendMsg, SamplingParams, Tensor, TokenizerMsg};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// One line of `mock-scheduler`'s `--observe-file` output: every backend
/// message it processed, in processing order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observed {
    Submit { uid: i64, input_len: usize },
    Abort { uid: i64 },
    Exit,
}

fn parse_observed_line(line: &str) -> Observed {
    if line == "exit" {
        return Observed::Exit;
    }
    if let Some(rest) = line.strip_prefix("submit ") {
        let mut parts = rest.split_whitespace();
        if let (Some(uid), Some(input_len), None) = (parts.next(), parts.next(), parts.next())
            && let (Ok(uid), Ok(input_len)) = (uid.parse::<i64>(), input_len.parse::<usize>())
        {
            return Observed::Submit { uid, input_len };
        }
    } else if let Some(rest) = line.strip_prefix("abort ")
        && let Ok(uid) = rest.trim().parse()
    {
        return Observed::Abort { uid };
    }
    panic!("unparseable observe-file line: {line:?}");
}

/// A spawned `mock-scheduler` subprocess, speaking real `ipc://` sockets.
pub struct MockScheduler {
    pub backend_addr: String,
    pub detok_addr: String,
    observe_path: PathBuf,
    child: Child,
    stdin: Option<ChildStdin>,
    stdout_lines: Arc<Mutex<Vec<String>>>,
    stderr_lines: Arc<Mutex<Vec<String>>>,
}

impl MockScheduler {
    /// Spawns `mock-scheduler` with unique backend/detok ipc addresses (the
    /// mock binds backend, connects detok — the mirror of
    /// [`MockScheduler::frontend`]'s roles), plus any `extra_args`.
    pub fn spawn(extra_args: &[&str]) -> MockScheduler {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let pid = std::process::id();
        let backend_addr = format!("ipc:///tmp/rsgm-{pid}-{n}-0");
        let detok_addr = format!("ipc:///tmp/rsgm-{pid}-{n}-1");
        let observe_path = std::env::temp_dir().join(format!("rsgm-{pid}-{n}.observe"));

        let mut child = Command::new(env!("CARGO_BIN_EXE_mock-scheduler"))
            .args([
                "--backend-addr",
                &backend_addr,
                "--backend-role",
                "bind",
                "--detok-addr",
                &detok_addr,
                "--detok-role",
                "connect",
            ])
            .arg("--observe-file")
            .arg(&observe_path)
            .args(extra_args)
            .env("RUST_LOG", "info")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn mock-scheduler");

        let stdout = child.stdout.take().unwrap();
        let stdout_lines = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&stdout_lines);
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                sink.lock().unwrap().push(line);
            }
        });

        let stderr = child.stderr.take().unwrap();
        let stderr_lines = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&stderr_lines);
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                sink.lock().unwrap().push(line);
            }
        });

        let stdin = child.stdin.take();

        MockScheduler {
            backend_addr,
            detok_addr,
            observe_path,
            child,
            stdin,
            stdout_lines,
            stderr_lines,
        }
    }

    /// Reads and parses every line of the `--observe-file`, in processing
    /// order. Panics on an unparseable line.
    pub fn observed(&self) -> Vec<Observed> {
        let content = std::fs::read_to_string(&self.observe_path).unwrap_or_default();
        content.lines().map(parse_observed_line).collect()
    }

    /// Blocks on the first stdout line (the handshake JSON) and returns the
    /// parsed [`Handshake`]. Panics after 20 s, or if the line fails to parse.
    pub fn wait_ready(&mut self) -> Handshake {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(line) = self.stdout_lines.lock().unwrap().first().cloned() {
                return parse_handshake(&line, EXPECTED_UPSTREAM_SHA)
                    .unwrap_or_else(|e| panic!("invalid handshake line {line:?}: {e}"));
            }
            if Instant::now() > deadline {
                let _ = self.child.kill();
                panic!(
                    "timed out waiting for mock-scheduler's handshake line; stderr:\n{}",
                    self.stderr()
                );
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// The raw first stdout line, if one has arrived yet.
    pub fn handshake_line(&self) -> Option<String> {
        self.stdout_lines.lock().unwrap().first().cloned()
    }

    /// Opens a frontend `ZmqTransport`: backend Connect (into the mock's
    /// backend Bind) and detok Bind (the mock connects its PUSH into it).
    pub fn frontend(&self) -> ZmqTransport {
        let backend = Endpoint {
            addr: self.backend_addr.clone(),
            role: Role::Connect,
        };
        let detok = Endpoint {
            addr: self.detok_addr.clone(),
            role: Role::Bind,
        };
        ZmqTransport::open(&backend, &detok).expect("open frontend transport")
    }

    /// All stderr lines logged so far, newline-joined.
    pub fn stderr(&self) -> String {
        self.stderr_lines.lock().unwrap().join("\n")
    }

    /// Waits until a stderr line contains `needle`; panics after 20 s.
    pub fn wait_for_log(&mut self, needle: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(l) = self
                .stderr_lines
                .lock()
                .unwrap()
                .iter()
                .find(|l| l.contains(needle))
            {
                return l.clone();
            }
            if Instant::now() > deadline {
                let _ = self.child.kill();
                panic!(
                    "timed out waiting for {needle:?}; stderr:\n{}",
                    self.stderr()
                );
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    /// Closes the mock's stdin, simulating the parent going away.
    pub fn close_stdin(&mut self) {
        drop(self.stdin.take());
    }

    /// Sends a signal to the mock via the `kill` command (e.g. `-TERM`, `-INT`).
    pub fn signal(&self, sig: &str) {
        let status = Command::new("kill")
            .args([sig, &self.child.id().to_string()])
            .status()
            .expect("run kill");
        assert!(status.success(), "kill {sig} failed");
    }

    /// Waits for exit (20 s deadline, then kill and panic). Returns the exit code.
    pub fn wait_exit(&mut self) -> i32 {
        let deadline = Instant::now() + Duration::from_secs(20);
        let status = loop {
            if let Some(status) = self.child.try_wait().expect("try_wait") {
                break status;
            }
            if Instant::now() > deadline {
                let _ = self.child.kill();
                let _ = self.child.wait();
                panic!("mock-scheduler did not exit; stderr:\n{}", self.stderr());
            }
            thread::sleep(Duration::from_millis(20));
        };
        // Let the stdout/stderr drain threads catch up with the final lines.
        thread::sleep(Duration::from_millis(50));
        status
            .code()
            .unwrap_or_else(|| panic!("killed by signal: {status:?}; stderr:\n{}", self.stderr()))
    }
}

impl Drop for MockScheduler {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(self.backend_addr.trim_start_matches("ipc://"));
        let _ = std::fs::remove_file(self.detok_addr.trim_start_matches("ipc://"));
        let _ = std::fs::remove_file(&self.observe_path);
    }
}

/// Builds a `UserMsg` with `SamplingParams::default()` except `max_tokens`.
pub fn user_msg(uid: i64, ids: &[i32], max_tokens: i64) -> BackendMsg {
    BackendMsg::UserMsg {
        uid,
        input_ids: Tensor::from_i32_slice(ids),
        sampling_params: SamplingParams {
            max_tokens,
            ..SamplingParams::default()
        },
    }
}

/// The echo rule: token k (0-based) is `ids[k % ids.len()]` (or 0 if `ids` is
/// empty), for `n` tokens.
pub fn echo_tokens(ids: &[i32], n: usize) -> Vec<i64> {
    (0..n)
        .map(|k| {
            if ids.is_empty() {
                0
            } else {
                ids[k % ids.len()] as i64
            }
        })
        .collect()
}

/// Receives one frame via `recv_detok` and decodes it as a `TokenizerMsg`.
/// Panics on a decode error; returns `None` on timeout.
pub fn recv_frame(src: &impl DetokSource, timeout_ms: i64) -> Option<TokenizerMsg> {
    let frame = src.recv_detok(timeout_ms).expect("recv_detok")?;
    Some(rsg_wire::decode_tokenizer(&frame).expect("decode TokenizerMsg"))
}
