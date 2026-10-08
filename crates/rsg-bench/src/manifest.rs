//! The `rsg-bench` run manifest (D-07): schema, a full environment
//! snapshot (`collect_meta`), and an atomic, symlink-refusing writer
//! (T-07-13). Same depth as Phase 2's `baseline-profile.json` `meta`
//! block, extended with this harness's own reproducibility fields.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sysinfo::System;

use crate::gclog::{CoOccurrenceRow, GcRow};
use crate::metrics::EncodedHistograms;
use crate::roles::{FrontendKind, Group, Role};

pub const SCHEMA_VERSION: u32 = 1;
pub const GENERATED_BY: &str = "crates/rsg-bench";

/// Which backend the launched frontends talk to. A `clap::ValueEnum` for
/// `--backend-kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    Real,
    Mock,
    Stub,
}

/// Environment variable names `collect_meta` is allowed to read, each
/// re-filtered by `is_secret_name` (T-07-12). `collect_meta` never
/// iterates over the whole process environment.
pub const ENV_ALLOWLIST: [&str; 7] = [
    "CUDA_VISIBLE_DEVICES",
    "RUST_LOG",
    "RSGLANG_RUST_BIN",
    "PYTHONHASHSEED",
    "OMP_NUM_THREADS",
    "TOKENIZERS_PARALLELISM",
    "MALLOC_ARENA_MAX",
];

/// The full environment snapshot (D-07), extending Phase 2's
/// `baseline-profile.json` `meta` block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meta {
    pub created_utc: String,
    pub platform: String,
    pub python: Option<String>,
    pub git_commit: Option<String>,
    pub git_dirty: Option<bool>,
    pub upstream_sha: Option<String>,
    pub model: String,
    pub gpu: Option<String>,
    pub arch: String,
    pub os_long: Option<String>,
    pub kernel: Option<String>,
    pub cpu_brand: Option<String>,
    pub cpu_count: Option<usize>,
    pub total_memory_bytes: Option<u64>,
    pub gpu_driver: Option<String>,
    pub torch_cuda: Option<String>,
    pub rustc: Option<String>,
    pub harness_profile: String,
    pub env: BTreeMap<String, String>,
    pub backend_kind: BackendKind,
}

/// One A/B arm's recorded identity in the manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArmInfo {
    pub id: String,
    pub kind: FrontendKind,
    pub num_tokenizer: Option<u32>,
    pub also_best: bool,
    pub argv: Vec<String>,
}

/// Everything about how this session was invoked (D-07).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub scenario: String,
    pub seed: u64,
    pub runs: u32,
    pub schedule: Vec<String>,
    pub arms: Vec<ArmInfo>,
    pub harness_argv: Vec<String>,
    pub workload: serde_json::Value,
}

/// One timed window's observation block (BENCH-08, D-14/D-15/D-16).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowObs {
    pub label: String,
    pub start_unix_ns: u64,
    pub end_unix_ns: u64,
    /// `"collected"` | `"disabled"` | `"no_hook_records"`.
    pub gc_status: String,
    pub gc: Option<BTreeMap<Role, GcRow>>,
    pub memory: BTreeMap<Group, crate::memory::GroupMemory>,
    pub tree_memory: crate::memory::GroupMemory,
    pub cooccurrence: Option<BTreeMap<Group, CoOccurrenceRow>>,
}

/// Summary of what a trial's hook directory contained.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HookSummary {
    pub present: bool,
    pub files: usize,
    pub malformed_lines: u64,
    pub mem_records: u64,
}

/// Whether a trial completed or failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrialStatus {
    Ok,
    Failed,
}

/// One slot's full record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrialRecord {
    pub index: usize,
    pub round: u32,
    pub arm: String,
    pub status: TrialStatus,
    pub error: Option<String>,
    pub model_id: Option<String>,
    pub launched_utc: Option<String>,
    pub ready_s: Option<f64>,
    pub result: serde_json::Value,
    pub histograms: BTreeMap<String, EncodedHistograms>,
    pub windows: Vec<WindowObs>,
    pub roles: BTreeMap<i32, Role>,
    pub hook: HookSummary,
    pub teardown_graceful: Option<bool>,
    pub teardown_survivors: Vec<i32>,
}

/// The full run manifest (D-07): schema version, a full environment
/// snapshot, session invocation, every trial, warnings and whether the
/// session was interrupted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub generated_by: String,
    pub meta: Meta,
    pub session: SessionInfo,
    pub trials: Vec<TrialRecord>,
    pub warnings: Vec<String>,
    pub interrupted: bool,
}

/// `YYYY-MM-DDTHH:MM:SSZ` for `unix_secs`, via the civil-from-days
/// algorithm (Howard Hinnant's `civil_from_days`) -- no `chrono` (T-07-SC:
/// no new crates).
pub fn utc_rfc3339(unix_secs: u64) -> String {
    let days = (unix_secs / 86_400) as i64;
    let secs_of_day = unix_secs % 86_400;
    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = if m <= 2 { y + 1 } else { y };

    format!("{year:04}-{m:02}-{d:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn run_with_timeout(mut cmd: Command, timeout: Duration) -> Option<std::process::Output> {
    // `std::process::Command` has no built-in timeout; every probe here is
    // a short-lived local command, so a dedicated OS thread with a join
    // timeout is simpler than pulling in a new crate (T-07-SC).
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let out = cmd.output();
        let _ = tx.send(out);
    });
    rx.recv_timeout(timeout).ok().and_then(|r| r.ok())
}

fn stdout_trimmed(out: &std::process::Output) -> Option<String> {
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

fn probe_git_commit(repo_root: &Path) -> Option<String> {
    let mut cmd = Command::new("git");
    cmd.args(["-C", &repo_root.to_string_lossy(), "rev-parse", "HEAD"]);
    stdout_trimmed(&run_with_timeout(cmd, Duration::from_secs(10))?)
}

fn probe_git_dirty(repo_root: &Path) -> Option<bool> {
    let mut cmd = Command::new("git");
    cmd.args([
        "-C",
        &repo_root.to_string_lossy(),
        "status",
        "--porcelain",
        "--untracked-files=no",
    ]);
    let out = run_with_timeout(cmd, Duration::from_secs(10))?;
    if !out.status.success() {
        return None;
    }
    Some(!String::from_utf8_lossy(&out.stdout).trim().is_empty())
}

fn probe_upstream_sha(repo_root: &Path) -> Option<String> {
    let path = repo_root.join("vendor").join("UPSTREAM_SHA");
    let text = std::fs::read_to_string(path).ok()?;
    let sha = text.trim();
    let valid = sha.len() == 40
        && sha
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase());
    valid.then(|| sha.to_string())
}

fn probe_gpu() -> (Option<String>, Option<String>) {
    let mut cmd = Command::new("nvidia-smi");
    cmd.args(["--query-gpu=name,driver_version", "--format=csv,noheader"]);
    let Some(out) = run_with_timeout(cmd, Duration::from_secs(10)) else {
        return (None, None);
    };
    let Some(line) = stdout_trimmed(&out) else {
        return (None, None);
    };
    let first_line = line.lines().next().unwrap_or("");
    match first_line.split_once(',') {
        Some((name, driver)) => (
            Some(name.trim().to_string()),
            Some(driver.trim().to_string()),
        ),
        None => (Some(first_line.trim().to_string()), None),
    }
}

fn probe_torch_cuda(python: &str) -> Option<String> {
    let mut cmd = Command::new(python);
    cmd.args(["-c", "import torch; print(torch.version.cuda)"]);
    let out = run_with_timeout(cmd, Duration::from_secs(10))?;
    let text = stdout_trimmed(&out)?;
    if text == "None" { None } else { Some(text) }
}

fn probe_python_version(python: &str) -> Option<String> {
    let mut cmd = Command::new(python);
    cmd.arg("--version");
    let out = run_with_timeout(cmd, Duration::from_secs(10))?;
    // Python historically prints `--version` to stderr on some builds.
    let text = if out.status.success() && !out.stdout.is_empty() {
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    } else {
        String::from_utf8_lossy(&out.stderr).trim().to_string()
    };
    (!text.is_empty()).then_some(text)
}

fn probe_rustc_version() -> Option<String> {
    let mut cmd = Command::new("rustc");
    cmd.arg("--version");
    stdout_trimmed(&run_with_timeout(cmd, Duration::from_secs(10))?)
}

/// Reads `name` through `std::env::var` only when `name` is in
/// [`ENV_ALLOWLIST`] *and* `is_secret_name(name)` is false (T-07-12):
/// never a wholesale dump of the whole process environment.
fn collect_env() -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for name in ENV_ALLOWLIST {
        if crate::cmdline::is_secret_name(name) {
            continue;
        }
        if let Ok(value) = std::env::var(name) {
            out.insert(name.to_string(), value);
        }
    }
    out
}

/// Builds the full [`Meta`] block (D-07). Every external probe runs with a
/// 10s timeout; any failure gives `None`, never an abort.
pub fn collect_meta(
    repo_root: &Path,
    python: &str,
    model_arg: &str,
    backend_kind: BackendKind,
) -> Meta {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (gpu, gpu_driver) = probe_gpu();

    let mut sys = System::new();
    sys.refresh_cpu_all();
    sys.refresh_memory();
    let cpu_brand = sys.cpus().first().map(|c| c.brand().to_string());
    let cpu_count = (!sys.cpus().is_empty()).then(|| sys.cpus().len());
    let total_memory_bytes = (sys.total_memory() > 0).then_some(sys.total_memory());

    Meta {
        created_utc: utc_rfc3339(now),
        platform: std::env::consts::OS.to_string(),
        python: probe_python_version(python),
        git_commit: probe_git_commit(repo_root),
        git_dirty: probe_git_dirty(repo_root),
        upstream_sha: probe_upstream_sha(repo_root),
        model: model_arg.to_string(),
        gpu,
        arch: std::env::consts::ARCH.to_string(),
        os_long: System::long_os_version(),
        kernel: System::kernel_version(),
        cpu_brand,
        cpu_count,
        total_memory_bytes,
        gpu_driver,
        torch_cuda: probe_torch_cuda(python),
        rustc: probe_rustc_version(),
        harness_profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
        .to_string(),
        env: collect_env(),
        backend_kind,
    }
}

/// Writes `value` as pretty JSON to `path` atomically (T-07-13): refuses a
/// symlink target, creates the parent dirs, writes a
/// `.{name}.tmp-{pid}-{counter}` file in the same directory with
/// `create_new(true)`, `sync_all`s it, then renames it over `path`.
pub fn write_json_atomic(path: &Path, value: &impl Serialize) -> anyhow::Result<()> {
    if let Ok(meta) = std::fs::symlink_metadata(path)
        && meta.file_type().is_symlink()
    {
        anyhow::bail!("refusing to write through a symlink: {}", path.display());
    }

    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .with_context(|| format!("create parent dir {}", parent.display()))?;

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .context("manifest path has no file name")?;
    let pid = std::process::id();
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let tmp_path: PathBuf = parent.join(format!(".{file_name}.tmp-{pid}-{counter}"));

    let text = serde_json::to_vec_pretty(value).context("serialize manifest")?;
    {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)
            .with_context(|| format!("create temp file {}", tmp_path.display()))?;
        use std::io::Write;
        file.write_all(&text)
            .with_context(|| format!("write temp file {}", tmp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("sync temp file {}", tmp_path.display()))?;
    }
    std::fs::rename(&tmp_path, path)
        .with_context(|| format!("rename {} -> {}", tmp_path.display(), path.display()))?;
    Ok(())
}

/// Reads and parses a manifest written by [`write_json_atomic`].
pub fn read_manifest(path: &Path) -> anyhow::Result<Manifest> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("read manifest {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parse manifest {}", path.display()))
}
