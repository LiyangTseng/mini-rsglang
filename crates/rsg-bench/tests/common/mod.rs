//! Shared test helpers for spawning `bench-stub` and reading its
//! `stub-event` log lines, modeled on `rsg-server`'s own `tests/common/mod.rs`
//! subprocess-harness pattern.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use rsg_bench::procs::LaunchSpec;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Binds an ephemeral port, reads it, and drops the listener so the caller
/// can hand the now-free port to a subprocess.
pub fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    listener.local_addr().expect("local_addr").port()
}

/// The path to the `bench-stub` binary built for this test run.
pub fn stub_bin() -> &'static str {
    env!("CARGO_BIN_EXE_bench-stub")
}

/// The path to the `rsg-bench` binary built for this test run.
pub fn bench_bin() -> &'static str {
    env!("CARGO_BIN_EXE_rsg-bench")
}

/// The repo checkout root, computed from this crate's own manifest dir
/// (`crates/rsg-bench` -> repo root) -- `cargo test`'s cwd is the crate
/// dir, not the workspace root, so every orchestrator invocation in a
/// test passes `--repo-root` explicitly rather than relying on the
/// process's current directory.
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

/// A unique, not-yet-existing work-root directory under the system temp
/// dir, for one test's session.
pub fn unique_work_root() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let pid = std::process::id();
    std::env::temp_dir().join(format!("rsg-bench-work-{pid}-{n}"))
}

/// A unique manifest output path under the system temp dir.
pub fn unique_manifest_path() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let pid = std::process::id();
    std::env::temp_dir().join(format!("rsg-bench-manifest-{pid}-{n}.json"))
}

/// A unique log-file path under the system temp dir for one test's stub.
pub fn unique_log_path() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let pid = std::process::id();
    std::env::temp_dir().join(format!("bench-stub-{pid}-{n}.log"))
}

/// Builds a [`LaunchSpec`] for `bench-stub --port <port> <extra_args...>`.
pub fn stub_spec(port: u16, log_path: &Path, extra: &[&str]) -> LaunchSpec {
    let mut argv = vec![
        stub_bin().to_string(),
        "--port".to_string(),
        port.to_string(),
    ];
    argv.extend(extra.iter().map(|s| s.to_string()));
    LaunchSpec {
        argv,
        env_set: Vec::new(),
        env_remove: Vec::new(),
        log_path: log_path.to_path_buf(),
    }
}

/// One parsed `stub-event kind=<kind> k=v k=v...` line.
#[derive(Debug, Clone)]
pub struct StubEvent {
    pub kind: String,
    pub fields: BTreeMap<String, String>,
}

/// Parses every `stub-event` line currently in `log`.
pub fn read_stub_events(log: &Path) -> Vec<StubEvent> {
    let Ok(text) = std::fs::read_to_string(log) else {
        return Vec::new();
    };
    text.lines().filter_map(parse_stub_event_line).collect()
}

fn parse_stub_event_line(line: &str) -> Option<StubEvent> {
    let rest = line.strip_prefix("stub-event ")?;
    let mut fields = BTreeMap::new();
    for tok in rest.split_whitespace() {
        if let Some((k, v)) = tok.split_once('=') {
            fields.insert(k.to_string(), v.to_string());
        }
    }
    let kind = fields.remove("kind")?;
    Some(StubEvent { kind, fields })
}

/// Polls `log` until an event matching `pred` appears, or panics after
/// `timeout`.
pub fn wait_for_event(
    log: &Path,
    pred: impl Fn(&StubEvent) -> bool,
    timeout: Duration,
) -> StubEvent {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(ev) = read_stub_events(log).into_iter().find(|e| pred(e)) {
            return ev;
        }
        if Instant::now() > deadline {
            let text = std::fs::read_to_string(log).unwrap_or_default();
            panic!("timed out waiting for matching stub-event; log so far:\n{text}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
