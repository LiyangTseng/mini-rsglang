//! Tests for `rsg-mock-stack` (D-03): it must bring up `mock-scheduler` and
//! `rsg-server`, relay the handshake, and stop both cleanly -- the Mac
//! launcher adapter the harness uses in place of `python -m rsglang.launch
//! --frontend rust`.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use sysinfo::{Pid as SysPid, ProcessesToUpdate, System};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn target_dir() -> PathBuf {
    match std::env::var("CARGO_TARGET_DIR") {
        Ok(dir) => PathBuf::from(dir),
        Err(_) => repo_root().join("target"),
    }
}

/// The `target/debug/<name>` path for a sibling-crate binary. `cargo`'s
/// `CARGO_BIN_EXE_*` env vars only cover binaries of the crate under test
/// (`rsg-bench`), not `rsg-server`'s own bins, so they are located by path.
fn require_bin(name: &str) -> PathBuf {
    let path = target_dir().join("debug").join(name);
    if !path.is_file() {
        panic!(
            "{name} not found at {}; run cargo build -p rsg-server --bins",
            path.display()
        );
    }
    path
}

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    listener.local_addr().expect("local_addr").port()
}

fn pid_alive(pid: u32) -> bool {
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::Some(&[SysPid::from_u32(pid)]), true);
    sys.process(SysPid::from_u32(pid)).is_some()
}

/// Parses `rsg-mock-stack: children rsg-server=<pid> mock-scheduler=<pid>`.
fn parse_children_line(line: &str) -> (u32, u32) {
    let mut rsg = None;
    let mut mock = None;
    for tok in line.split_whitespace() {
        if let Some(v) = tok.strip_prefix("rsg-server=") {
            rsg = v.parse().ok();
        }
        if let Some(v) = tok.strip_prefix("mock-scheduler=") {
            mock = v.parse().ok();
        }
    }
    (
        rsg.unwrap_or_else(|| panic!("no rsg-server= pid in {line:?}")),
        mock.unwrap_or_else(|| panic!("no mock-scheduler= pid in {line:?}")),
    )
}

/// A spawned `rsg-mock-stack` with its stderr captured on a background
/// thread for live assertions.
struct Stack {
    child: Child,
    lines: Arc<Mutex<Vec<String>>>,
}

impl Stack {
    fn spawn() -> Stack {
        let rsg_server_bin = require_bin("rsg-server");
        let mock_scheduler_bin = require_bin("mock-scheduler");
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let port = free_port().to_string();

        let mut child = Command::new(env!("CARGO_BIN_EXE_rsg-mock-stack"))
            .args([
                "--port",
                &port,
                "--rsg-server-bin",
                rsg_server_bin.to_str().expect("utf8 path"),
                "--mock-scheduler-bin",
                mock_scheduler_bin.to_str().expect("utf8 path"),
                "--model",
                &format!("mock-stack-test-{n}"),
            ])
            .env("RUST_LOG", "info")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn rsg-mock-stack");

        let stderr = child.stderr.take().expect("stderr piped");
        let lines = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&lines);
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                sink.lock().unwrap().push(line);
            }
        });

        Stack { child, lines }
    }

    fn stderr_snapshot(&self) -> Vec<String> {
        self.lines.lock().unwrap().clone()
    }

    fn stderr_text(&self) -> String {
        self.stderr_snapshot().join("\n")
    }

    /// Blocks until the children line, an `[rsg-server] ... handshake
    /// received` line, and the backend-ready line have all appeared, or
    /// panics after `timeout`. Returns the (rsg_server_pid, mock_scheduler_pid)
    /// parsed from the children line.
    fn wait_ready(&self, timeout: Duration) -> (u32, u32) {
        let deadline = Instant::now() + timeout;
        loop {
            let snapshot = self.stderr_snapshot();
            let children_line = snapshot
                .iter()
                .find(|l| l.starts_with("rsg-mock-stack: children "));
            let saw_handshake_received = snapshot
                .iter()
                .any(|l| l.contains("[rsg-server]") && l.contains("handshake received"));
            let saw_backend_ready = snapshot
                .iter()
                .any(|l| l.contains("rsg-mock-stack: backend ready; handshake sent to rsg-server"));

            if let Some(line) = children_line
                && saw_handshake_received
                && saw_backend_ready
            {
                return parse_children_line(line);
            }
            if Instant::now() > deadline {
                panic!(
                    "timed out waiting for rsg-mock-stack readiness; stderr so far:\n{}",
                    self.stderr_text()
                );
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn wait_exit(&mut self, timeout: Duration) -> std::process::ExitStatus {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.child.try_wait().expect("try_wait") {
                return status;
            }
            if Instant::now() > deadline {
                let _ = self.child.kill();
                panic!(
                    "rsg-mock-stack did not exit within {timeout:?}; stderr so far:\n{}",
                    self.stderr_text()
                );
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Stack {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn mock_stack_relays_handshake_and_stops() {
    let mut stack = Stack::spawn();
    let stack_pid = stack.child.id();

    let (rsg_pid, mock_pid) = stack.wait_ready(Duration::from_secs(20));

    let status = Command::new("kill")
        .args(["-TERM", &stack_pid.to_string()])
        .status()
        .expect("run kill -TERM");
    assert!(status.success(), "kill -TERM {stack_pid} failed");

    let exit_status = stack.wait_exit(Duration::from_secs(10));
    assert_eq!(
        exit_status.code(),
        Some(0),
        "rsg-mock-stack exit code; stderr:\n{}",
        stack.stderr_text()
    );

    assert!(
        !pid_alive(rsg_pid),
        "rsg-server pid {rsg_pid} still alive after stop; stderr:\n{}",
        stack.stderr_text()
    );
    assert!(
        !pid_alive(mock_pid),
        "mock-scheduler pid {mock_pid} still alive after stop; stderr:\n{}",
        stack.stderr_text()
    );

    let backend_sock = format!("/tmp/rsgb-{stack_pid}-0");
    let detok_sock = format!("/tmp/rsgb-{stack_pid}-1");
    assert!(
        !std::path::Path::new(&backend_sock).exists(),
        "backend socket {backend_sock} not removed"
    );
    assert!(
        !std::path::Path::new(&detok_sock).exists(),
        "detok socket {detok_sock} not removed"
    );
}

#[test]
fn mock_stack_exits_when_child_dies() {
    let mut stack = Stack::spawn();

    let (rsg_pid, mock_pid) = stack.wait_ready(Duration::from_secs(20));

    let status = Command::new("kill")
        .args(["-KILL", &mock_pid.to_string()])
        .status()
        .expect("run kill -KILL");
    assert!(status.success(), "kill -KILL {mock_pid} failed");

    let exit_status = stack.wait_exit(Duration::from_secs(10));
    assert_eq!(
        exit_status.code(),
        Some(1),
        "rsg-mock-stack exit code after mock-scheduler was killed; stderr:\n{}",
        stack.stderr_text()
    );

    assert!(
        !pid_alive(rsg_pid),
        "rsg-server pid {rsg_pid} still alive after mock-scheduler died; stderr:\n{}",
        stack.stderr_text()
    );
}
