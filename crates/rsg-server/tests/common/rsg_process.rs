#![allow(dead_code)]
//! Test harness for spawning the real `rsg-server` binary, mirroring
//! `tests/cli.rs`'s own `Server` struct: stdin piped, stderr lines
//! collected on a drain thread, a `kill`-based signal helper, and a
//! deadline-bounded `wait_exit`. Callers are responsible for passing
//! unique `--backend-addr`/`--detok-addr` values (e.g. from a
//! `MockScheduler`, which already generates its own unique pair) -- this
//! harness only manages the `rsg-server` subprocess itself.

use std::io::{BufRead, BufReader, Write};
use std::net::SocketAddr;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// A spawned `rsg-server` subprocess.
pub struct RsgServer {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Arc<Mutex<Vec<String>>>,
    /// The `--detok-addr` value, if present in the spawn args, with its
    /// `ipc://` prefix stripped -- removed on drop like `tests/cli.rs`'s
    /// `Server` does, since a bind-role detok socket leaves a filesystem
    /// path behind.
    detok_path: Option<String>,
}

impl RsgServer {
    /// Spawns `rsg-server` with `args` verbatim (the caller supplies every
    /// flag, including `--backend-addr`/`--detok-addr`/`--model`/
    /// `--run-id`/`--host`/`--port`). Stdin is piped for
    /// [`RsgServer::send_line`]; stderr is collected on a background
    /// thread for [`RsgServer::wait_for_log`]/[`RsgServer::listening_addr`].
    pub fn spawn(args: &[&str]) -> RsgServer {
        let detok_path = args
            .iter()
            .position(|a| *a == "--detok-addr")
            .and_then(|i| args.get(i + 1))
            .map(|a| a.trim_start_matches("ipc://").to_string());

        let mut child = Command::new(env!("CARGO_BIN_EXE_rsg-server"))
            .args(args)
            .env("RUST_LOG", "info")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn rsg-server");

        let stderr = child.stderr.take().unwrap();
        let lines = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&lines);
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                sink.lock().unwrap().push(line);
            }
        });

        let stdin = child.stdin.take();

        RsgServer {
            child,
            stdin,
            lines,
            detok_path,
        }
    }

    /// Writes `line` to the child's stdin, adding a trailing newline if
    /// `line` doesn't already have one.
    pub fn send_line(&mut self, line: &str) {
        let stdin = self.stdin.as_mut().expect("stdin open");
        stdin.write_all(line.as_bytes()).expect("write stdin");
        if !line.ends_with('\n') {
            stdin.write_all(b"\n").expect("write newline");
        }
        stdin.flush().expect("flush stdin");
    }

    /// Closes the child's stdin, simulating the parent going away.
    pub fn close_stdin(&mut self) {
        drop(self.stdin.take());
    }

    pub fn stderr(&self) -> String {
        self.lines.lock().unwrap().join("\n")
    }

    /// Waits until a stderr line contains `needle`; panics after 20 s.
    pub fn wait_for_log(&mut self, needle: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(l) = self
                .lines
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

    /// Parses the listening address from the `"http server listening"`
    /// log line's `addr=` field (waits for that line first, 20 s deadline
    /// via [`RsgServer::wait_for_log`]).
    pub fn listening_addr(&mut self) -> SocketAddr {
        let line = self.wait_for_log("http server listening");
        let addr_str = line
            .split("addr=")
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .unwrap_or_else(|| panic!("no addr= field in {line:?}"));
        addr_str
            .parse()
            .unwrap_or_else(|e| panic!("invalid addr {addr_str:?} in {line:?}: {e}"))
    }

    /// Sends a signal to the child via the `kill` command (e.g. `-TERM`, `-INT`).
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
                panic!("rsg-server did not exit; stderr:\n{}", self.stderr());
            }
            thread::sleep(Duration::from_millis(20));
        };
        // Let the stderr drain thread catch up with the final lines.
        thread::sleep(Duration::from_millis(100));
        status
            .code()
            .unwrap_or_else(|| panic!("killed by signal: {status:?}; stderr:\n{}", self.stderr()))
    }
}

impl Drop for RsgServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(path) = &self.detok_path {
            let _ = std::fs::remove_file(path);
        }
    }
}
