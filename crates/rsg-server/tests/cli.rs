//! Process-level tests of rsg-server's exit-code and logging contract
//! (D-08, D-10, D-11, D-12): 0 on SIGINT/SIGTERM, 2 on a bad handshake,
//! 3 on stdin EOF before or after the handshake.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn vendor_sha() -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../vendor/UPSTREAM_SHA");
    std::fs::read_to_string(path)
        .expect("read vendor/UPSTREAM_SHA")
        .trim()
        .to_string()
}

fn handshake_line(sha: &str, eos: &str) -> String {
    format!(
        "{{\"handshake_version\":1,\"upstream_sha\":\"{sha}\",\"max_seq_len\":4096,\
         \"eos_token_id\":{eos},\"page_size\":16,\"max_running_req\":8,\"num_pages\":1024}}\n"
    )
}

struct Server {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Arc<Mutex<Vec<String>>>,
    detok_path: String,
}

impl Server {
    fn spawn() -> Server {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let pid = std::process::id();
        let backend = format!("ipc:///tmp/rsgc-{pid}-{n}-0");
        let detok = format!("ipc:///tmp/rsgc-{pid}-{n}-1");
        let mut child = Command::new(env!("CARGO_BIN_EXE_rsg-server"))
            .args([
                "--backend-addr",
                &backend,
                "--backend-role",
                "connect",
                "--detok-addr",
                &detok,
                "--detok-role",
                "bind",
                "--model",
                "test-model",
                "--run-id",
                ".rsg=test",
            ])
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
        Server {
            child,
            stdin,
            lines,
            detok_path: detok.trim_start_matches("ipc://").to_string(),
        }
    }

    fn write(&mut self, s: &str) {
        let stdin = self.stdin.as_mut().expect("stdin open");
        stdin.write_all(s.as_bytes()).expect("write stdin");
        stdin.flush().expect("flush stdin");
    }

    fn close_stdin(&mut self) {
        drop(self.stdin.take());
    }

    fn stderr(&self) -> String {
        self.lines.lock().unwrap().join("\n")
    }

    /// Wait until a stderr line contains `needle`; panics after 20 s.
    fn wait_for_log(&mut self, needle: &str) -> String {
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

    fn signal(&self, sig: &str) {
        let status = Command::new("kill")
            .args([sig, &self.child.id().to_string()])
            .status()
            .expect("run kill");
        assert!(status.success(), "kill {sig} failed");
    }

    /// Wait for exit (20 s deadline, then kill and panic). Returns the exit code.
    fn wait_exit(&mut self) -> i32 {
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

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.detok_path);
    }
}

fn assert_handshake_logged(s: &mut Server, eos: &str) {
    let line = s.wait_for_log("handshake received");
    for field in [
        "max_seq_len=4096".to_string(),
        format!("eos_token_id={eos}"),
        "page_size=16".to_string(),
        "max_running_req=8".to_string(),
        "num_pages=1024".to_string(),
        format!("upstream_sha={}", vendor_sha()),
    ] {
        assert!(line.contains(&field), "missing {field} in {line:?}");
    }
}

fn valid_then_signal(sig: &str) {
    let mut s = Server::spawn();
    s.wait_for_log("awaiting handshake on stdin");
    s.write(&handshake_line(&vendor_sha(), "151645"));
    assert_handshake_logged(&mut s, "151645");
    s.wait_for_log("idle until SIGINT/SIGTERM or stdin EOF");
    s.signal(sig);
    assert_eq!(s.wait_exit(), 0, "stderr:\n{}", s.stderr());
}

fn rejected(line: &str) -> Server {
    let mut s = Server::spawn();
    s.write(line);
    assert_eq!(s.wait_exit(), 2, "stderr:\n{}", s.stderr());
    assert!(s.stderr().contains("handshake rejected"), "{}", s.stderr());
    s
}

#[test]
fn valid_handshake_then_sigterm_exits_0() {
    valid_then_signal("-TERM");
}

#[test]
fn valid_handshake_then_sigint_exits_0() {
    valid_then_signal("-INT");
}

#[test]
fn sha_mismatch_exits_2_naming_both_shas() {
    let bad = "0000000000000000000000000000000000000000";
    let s = rejected(&handshake_line(bad, "151645"));
    let err = s.stderr();
    assert!(err.contains(bad), "{err}");
    assert!(err.contains(&vendor_sha()), "{err}");
}

#[test]
fn not_json_exits_2() {
    rejected("not json\n");
}

#[test]
fn extra_key_exits_2() {
    let line = handshake_line(&vendor_sha(), "151645")
        .replace("\"num_pages\":1024}", "\"num_pages\":1024,\"extra\":1}");
    rejected(&line);
}

#[test]
fn missing_eos_key_exits_2() {
    let line = handshake_line(&vendor_sha(), "151645").replace("\"eos_token_id\":151645,", "");
    rejected(&line);
}

#[test]
fn handshake_version_2_exits_2() {
    let line = handshake_line(&vendor_sha(), "151645")
        .replace("\"handshake_version\":1", "\"handshake_version\":2");
    rejected(&line);
}

#[test]
fn stdin_eof_before_handshake_exits_3() {
    let mut s = Server::spawn();
    s.close_stdin();
    assert_eq!(s.wait_exit(), 3, "stderr:\n{}", s.stderr());
    assert!(s.stderr().contains("stdin EOF"), "{}", s.stderr());
}

#[test]
fn stdin_eof_after_handshake_exits_3() {
    let mut s = Server::spawn();
    s.write(&handshake_line(&vendor_sha(), "151645"));
    assert_handshake_logged(&mut s, "151645");
    s.close_stdin();
    assert_eq!(s.wait_exit(), 3, "stderr:\n{}", s.stderr());
    assert!(s.stderr().contains("stdin EOF"), "{}", s.stderr());
}

#[test]
fn null_eos_logs_null_then_eof_exits_3() {
    let mut s = Server::spawn();
    s.write(&handshake_line(&vendor_sha(), "null"));
    assert_handshake_logged(&mut s, "null");
    s.close_stdin();
    assert_eq!(s.wait_exit(), 3, "stderr:\n{}", s.stderr());
}
