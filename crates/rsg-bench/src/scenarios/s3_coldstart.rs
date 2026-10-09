//! Scenario 3 (BENCH-05): hyperfine-timed end-to-end cold start, the
//! frontend's own critical-path tail reported separately (from
//! backend-ready log markers), and frontend memory at ready -- a
//! hyperfine-backed, runner-managed trial (D-08) in the shared A/B
//! orchestrator.
//!
//! Ports `python/rsglang/profiling/scenarios.py`'s `hyperfine_argv`,
//! `parse_hyperfine_json`, `coldstart_once` and `coldstart_stop` to Rust
//! (RESEARCH Pattern 2), extended with the backend-ready marker, per-run
//! `--hook-root` directories and leader-identity-checked teardown
//! (T-07-20) that plan 07-08 adds on top of the Python original.

use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sysinfo::System;

use crate::gclog::{self, GcRow};
use crate::manifest;
use crate::memory::{self, MemPoint};
use crate::orchestrator::{Lifecycle, MeasuredWindow, TrialContext, TrialMeasurement, TrialRunner};
use crate::procs::{self, DetachedGroup};
use crate::roles::{FrontendKind, Group, Role, RoleMap};

fn unix_ns_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

// --- Hyperfine argv + export parsing (RESEARCH Pattern 2) -------------------

/// Builds `hyperfine`'s own argv: `--conclude <stop_cmd>` and the timed
/// `<once_cmd>` are both shell-quoted ([`crate::cmdline::shell_join`]),
/// never built by plain string concatenation (T-07-19) -- hyperfine runs
/// both through its own shell.
pub fn hyperfine_argv(
    hyperfine: &str,
    runs: u32,
    warmup: u32,
    export_json: &Path,
    once_cmd: &[String],
    stop_cmd: &[String],
) -> Vec<String> {
    vec![
        hyperfine.to_string(),
        "--runs".to_string(),
        runs.to_string(),
        "--warmup".to_string(),
        warmup.to_string(),
        "--export-json".to_string(),
        export_json.to_string_lossy().into_owned(),
        "--conclude".to_string(),
        crate::cmdline::shell_join(stop_cmd),
        crate::cmdline::shell_join(once_cmd),
    ]
}

/// A parsed hyperfine `--export-json` document's `results[0]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperfineStats {
    pub mean_s: f64,
    pub stddev_s: Option<f64>,
    pub median_s: f64,
    pub min_s: f64,
    pub max_s: f64,
    pub times_s: Vec<f64>,
    pub runs: usize,
}

/// Parses a hyperfine `--export-json` document's `results[0]` (RESEARCH
/// Pattern 2, Security Domain V5): explicit presence/type checks at every
/// field, with every error naming `path_for_errors`, never a panic. A
/// missing or empty `results` is the one case Phase 2's own port names
/// explicitly.
pub fn parse_hyperfine_json(text: &str, path_for_errors: &str) -> anyhow::Result<HyperfineStats> {
    let doc: serde_json::Value = serde_json::from_str(text)
        .with_context(|| format!("{path_for_errors}: parse hyperfine export as JSON"))?;
    let results = doc
        .get("results")
        .and_then(serde_json::Value::as_array)
        .filter(|r| !r.is_empty());
    let Some(results) = results else {
        anyhow::bail!("{path_for_errors}: hyperfine export has no 'results'");
    };
    let result = &results[0];
    let field_f64 = |name: &str| -> anyhow::Result<f64> {
        result
            .get(name)
            .and_then(serde_json::Value::as_f64)
            .ok_or_else(|| {
                anyhow::anyhow!("{path_for_errors}: results[0] missing numeric '{name}'")
            })
    };
    let mean_s = field_f64("mean")?;
    let stddev_s = result.get("stddev").and_then(serde_json::Value::as_f64);
    let median_s = field_f64("median")?;
    let min_s = field_f64("min")?;
    let max_s = field_f64("max")?;
    let times_s: Vec<f64> = result
        .get("times")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("{path_for_errors}: results[0] missing 'times' array"))?
        .iter()
        .filter_map(serde_json::Value::as_f64)
        .collect();
    let runs = times_s.len();
    Ok(HyperfineStats {
        mean_s,
        stddev_s,
        median_s,
        min_s,
        max_s,
        times_s,
        runs,
    })
}

// --- Record-file line kinds --------------------------------------------------

/// One line of `--record-file` (JSONL, tagged on `kind`): `coldstart-once`'s
/// own readiness timing, or `coldstart-stop`'s memory sample.
///
/// Hand-rolled `Serialize`/`Deserialize` rather than
/// `#[serde(tag = "kind")]`: serde's internally-tagged-enum deserialization
/// re-decodes the matched variant from a buffered `Content` value rather
/// than the original deserializer, and that buffered path does not
/// correctly deserialize a non-string map key (`roles: BTreeMap<i32,
/// Role>`) -- it round-trips fine standalone (outside a tagged enum, as
/// `TrialRecord.roles` in `manifest.rs` already proves), but fails inside
/// one. Deserializing through [`serde_json::Value`]/[`serde_json::from_value`]
/// instead uses the complete, non-buffered value deserializer and avoids
/// the bug entirely.
#[derive(Debug, Clone)]
pub enum ColdstartRecord {
    Ready {
        run: u32,
        launched_unix_ns: u64,
        ready_unix_ns: u64,
        e2e_ready_s: f64,
        backend_ready_s: Option<f64>,
        frontend_tail_s: Option<f64>,
    },
    Mem {
        run: u32,
        groups: BTreeMap<Group, MemPoint>,
        tree: MemPoint,
        roles: BTreeMap<i32, Role>,
    },
}

#[derive(Deserialize)]
struct ReadyFields {
    run: u32,
    launched_unix_ns: u64,
    ready_unix_ns: u64,
    e2e_ready_s: f64,
    backend_ready_s: Option<f64>,
    frontend_tail_s: Option<f64>,
}

#[derive(Deserialize)]
struct MemFields {
    run: u32,
    groups: BTreeMap<Group, MemPoint>,
    tree: MemPoint,
    roles: BTreeMap<i32, Role>,
}

impl Serialize for ColdstartRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match self {
            ColdstartRecord::Ready {
                run,
                launched_unix_ns,
                ready_unix_ns,
                e2e_ready_s,
                backend_ready_s,
                frontend_tail_s,
            } => {
                let mut map = serializer.serialize_map(Some(7))?;
                map.serialize_entry("kind", "ready")?;
                map.serialize_entry("run", run)?;
                map.serialize_entry("launched_unix_ns", launched_unix_ns)?;
                map.serialize_entry("ready_unix_ns", ready_unix_ns)?;
                map.serialize_entry("e2e_ready_s", e2e_ready_s)?;
                map.serialize_entry("backend_ready_s", backend_ready_s)?;
                map.serialize_entry("frontend_tail_s", frontend_tail_s)?;
                map.end()
            }
            ColdstartRecord::Mem {
                run,
                groups,
                tree,
                roles,
            } => {
                let mut map = serializer.serialize_map(Some(5))?;
                map.serialize_entry("kind", "mem")?;
                map.serialize_entry("run", run)?;
                map.serialize_entry("groups", groups)?;
                map.serialize_entry("tree", tree)?;
                map.serialize_entry("roles", roles)?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for ColdstartRecord {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        let value = serde_json::Value::deserialize(deserializer)?;
        let kind = value
            .get("kind")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| D::Error::custom("coldstart record missing 'kind'"))?
            .to_string();
        match kind.as_str() {
            "ready" => {
                let f: ReadyFields = serde_json::from_value(value).map_err(D::Error::custom)?;
                Ok(ColdstartRecord::Ready {
                    run: f.run,
                    launched_unix_ns: f.launched_unix_ns,
                    ready_unix_ns: f.ready_unix_ns,
                    e2e_ready_s: f.e2e_ready_s,
                    backend_ready_s: f.backend_ready_s,
                    frontend_tail_s: f.frontend_tail_s,
                })
            }
            "mem" => {
                let f: MemFields = serde_json::from_value(value).map_err(D::Error::custom)?;
                Ok(ColdstartRecord::Mem {
                    run: f.run,
                    groups: f.groups,
                    tree: f.tree,
                    roles: f.roles,
                })
            }
            other => Err(D::Error::custom(format!(
                "unknown coldstart record kind: {other}"
            ))),
        }
    }
}

fn append_record(record_file: &Path, record: &ColdstartRecord) -> anyhow::Result<()> {
    if let Some(parent) = record_file.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create record-file dir {}", parent.display()))?;
    }
    let line = serde_json::to_string(record).context("serialize coldstart record")?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(record_file)
        .with_context(|| format!("open record file {}", record_file.display()))?;
    writeln!(file, "{line}").with_context(|| format!("append to {}", record_file.display()))?;
    Ok(())
}

fn read_records(record_file: &Path) -> Vec<ColdstartRecord> {
    let Ok(text) = std::fs::read_to_string(record_file) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|l| serde_json::from_str(l.trim()).ok())
        .collect()
}

fn count_ready_records(record_file: &Path) -> u32 {
    read_records(record_file)
        .iter()
        .filter(|r| matches!(r, ColdstartRecord::Ready { .. }))
        .count() as u32
}

fn read_pgid_file(pgid_file: &Path) -> anyhow::Result<DetachedGroup> {
    let text = std::fs::read_to_string(pgid_file)
        .with_context(|| format!("read pgid file {}", pgid_file.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parse pgid file {}", pgid_file.display()))
}

/// Self-heal (plan 07-08 Task 1): if `pgid_file` still exists (e.g. a
/// `--conclude` that never ran, left over from a prior attempt), stops that
/// group before anything new is launched, so two cold-start attempts never
/// fight over the same port.
async fn self_heal(pgid_file: &Path) -> anyhow::Result<()> {
    if !pgid_file.exists() {
        return Ok(());
    }
    if let Ok(group) = read_pgid_file(pgid_file) {
        let _ = procs::stop_detached(&group, Duration::from_secs(60)).await;
    }
    let _ = std::fs::remove_file(pgid_file);
    Ok(())
}

// --- `coldstart-once` ---------------------------------------------------------

/// `rsg-bench coldstart-once` flags.
#[derive(Debug, Clone, clap::Args)]
pub struct OnceArgs {
    #[arg(long)]
    pub record_file: PathBuf,
    #[arg(long)]
    pub pgid_file: PathBuf,
    #[arg(long)]
    pub log: PathBuf,
    #[arg(long)]
    pub port: u16,
    #[arg(long, default_value_t = 900.0)]
    pub ready_timeout_s: f64,
    #[arg(long, default_value_t = 20)]
    pub ready_poll_ms: u64,
    #[arg(long)]
    pub backend_ready_marker: String,
    #[arg(long)]
    pub hook_root: Option<PathBuf>,
    /// The server command (after `--`).
    #[arg(last = true)]
    pub argv: Vec<String>,
}

/// Picks this attempt's run index and, when `--hook-root` is set, creates
/// `<hook-root>/run-<k>` exclusively: `k` is the count of existing `run-*`
/// dirs. Without `--hook-root`, `k` is the count of `ready` records already
/// in the record file.
fn pick_run_and_hook_dir(args: &OnceArgs) -> anyhow::Result<(u32, Option<PathBuf>)> {
    match &args.hook_root {
        Some(root) => {
            std::fs::create_dir_all(root)
                .with_context(|| format!("create hook root {}", root.display()))?;
            let mut k: u32 = 0;
            loop {
                let candidate = root.join(format!("run-{k}"));
                match std::fs::create_dir(&candidate) {
                    Ok(()) => return Ok((k, Some(candidate))),
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                        k += 1;
                    }
                    Err(e) => {
                        return Err(e).with_context(|| {
                            format!("create hook run dir {}", candidate.display())
                        });
                    }
                }
            }
        }
        None => Ok((count_ready_records(&args.record_file), None)),
    }
}

/// Reads the bytes appended to `log_path` since `offset`, returning the new
/// text and the new offset. The log file always exists by the time this is
/// called ([`procs::launch`] creates it before the child can write).
fn read_new_log_bytes(log_path: &Path, offset: u64) -> std::io::Result<(String, u64)> {
    let mut file = std::fs::File::open(log_path)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    let new_offset = offset + buf.len() as u64;
    Ok((String::from_utf8_lossy(&buf).into_owned(), new_offset))
}

/// One cold-start attempt (plan 07-08 Task 1): self-heals a stale group,
/// launches `argv` in a fresh process group, polls `GET /v1/models` for
/// readiness while watching the server's own log for
/// `--backend-ready-marker`, appends a `ready` record, then leaves the
/// server running -- hyperfine's timer stops here, at readiness, not at
/// teardown. `--conclude` (`coldstart-stop`) tears it down afterward.
pub async fn coldstart_once(args: OnceArgs) -> anyhow::Result<()> {
    self_heal(&args.pgid_file).await?;

    let (run, hook_dir) = pick_run_and_hook_dir(&args)?;

    let mut env_set = Vec::new();
    if let Some(dir) = &hook_dir {
        env_set.push((
            "RSGLANG_PROFILE_DIR".to_string(),
            dir.to_string_lossy().into_owned(),
        ));
    }

    let spec = procs::LaunchSpec {
        argv: args.argv.clone(),
        env_set,
        env_remove: Vec::new(),
        log_path: args.log.clone(),
    };
    let handle = procs::launch(&spec, args.port)?;
    let launched_unix_ns = handle.launched_unix_ns;

    let client = crate::client::build_client()?;
    let url = format!("{}/v1/models", crate::client::local_base_url(args.port));
    let ready_timeout = Duration::from_secs_f64(args.ready_timeout_s.max(0.0));
    let poll = Duration::from_millis(args.ready_poll_ms.max(1));
    let deadline = Instant::now() + ready_timeout;

    let mut log_offset: u64 = 0;
    let mut trailing = String::new();
    let mut backend_ready_elapsed: Option<Duration> = None;
    let marker_len = args.backend_ready_marker.len();

    let mut loop_error: Option<String> = None;
    let e2e_elapsed: Option<Duration> = loop {
        if backend_ready_elapsed.is_none()
            && let Ok((new_text, new_offset)) = read_new_log_bytes(&args.log, log_offset)
        {
            log_offset = new_offset;
            trailing.push_str(&new_text);
            if trailing.contains(&args.backend_ready_marker) {
                backend_ready_elapsed = Some(handle.launched_at.elapsed());
            } else if trailing.len() > marker_len {
                // Round down to the nearest UTF-8 char boundary: the log
                // can contain multi-byte characters (e.g. tqdm progress-bar
                // block glyphs from CUDA graph capture), and a raw byte
                // offset can land mid-character, which `drain` rejects.
                let mut cut = trailing.len() - marker_len;
                while cut > 0 && !trailing.is_char_boundary(cut) {
                    cut -= 1;
                }
                trailing.drain(0..cut);
            }
        }

        if let Ok(resp) = client.get(&url).send().await
            && resp.status().is_success()
        {
            break Some(handle.launched_at.elapsed());
        }

        if procs::leader_exited(&handle) {
            loop_error = Some(format!(
                "leader exited before ready; log: {}",
                args.log.display()
            ));
            break None;
        }

        if Instant::now() > deadline {
            loop_error = Some(format!("server not ready after {ready_timeout:?}"));
            break None;
        }

        tokio::time::sleep(poll).await;
    };

    let Some(e2e_elapsed) = e2e_elapsed else {
        let _ = procs::teardown(handle, Duration::from_secs(5)).await;
        anyhow::bail!("coldstart-once: {}", loop_error.unwrap_or_default());
    };

    let e2e_ready_s = e2e_elapsed.as_secs_f64();
    let backend_ready_s = backend_ready_elapsed.map(|d| d.as_secs_f64());
    let frontend_tail_s = backend_ready_s.map(|b| e2e_ready_s - b);

    append_record(
        &args.record_file,
        &ColdstartRecord::Ready {
            run,
            launched_unix_ns,
            ready_unix_ns: unix_ns_now(),
            e2e_ready_s,
            backend_ready_s,
            frontend_tail_s,
        },
    )?;

    let group = handle.detach(run, hook_dir)?;
    if let Some(parent) = args.pgid_file.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create pgid-file dir {}", parent.display()))?;
    }
    manifest::write_json_atomic(&args.pgid_file, &group).context("write pgid file")?;

    Ok(())
}

// --- `coldstart-stop` ---------------------------------------------------------

/// `rsg-bench coldstart-stop` flags (hyperfine's `--conclude` command).
#[derive(Debug, Clone, clap::Args)]
pub struct StopArgs {
    #[arg(long)]
    pub pgid_file: PathBuf,
    #[arg(long)]
    pub record_file: PathBuf,
    #[arg(long, default_value_t = 60.0)]
    pub teardown_grace_s: f64,
    #[arg(long, value_enum)]
    pub kind: FrontendKind,
    #[arg(long, default_value = "rsg-server")]
    pub rust_frontend_process_name: String,
}

/// Samples whole-tree memory at ready, records a `mem` line, then tears
/// the detached group down (T-07-20 leader-identity check inside
/// [`procs::stop_detached`]). Returns whether nothing survived.
pub async fn coldstart_stop(args: StopArgs) -> anyhow::Result<bool> {
    let group = read_pgid_file(&args.pgid_file)?;

    let mut sys = System::new();
    let sample = memory::sample_tree(&mut sys, group.pgid);

    let hook = match &group.hook_dir {
        Some(dir) => gclog::read_hook_dir(dir).unwrap_or_default(),
        None => gclog::HookLog::default(),
    };
    let process_names = memory::process_names(std::slice::from_ref(&sample));
    let role_map = RoleMap::build(
        group.pgid,
        args.kind,
        &hook,
        &process_names,
        &args.rust_frontend_process_name,
    );

    let (groups, tree) = memory::memory_at(&sample, &role_map);

    append_record(
        &args.record_file,
        &ColdstartRecord::Mem {
            run: group.run,
            groups,
            tree,
            roles: role_map.roles.clone(),
        },
    )?;

    let report = procs::stop_detached(
        &group,
        Duration::from_secs_f64(args.teardown_grace_s.max(0.0)),
    )
    .await?;
    let _ = std::fs::remove_file(&args.pgid_file);

    Ok(report.survivors.is_empty())
}

// --- `rsg-bench s3`: the runner-managed trial ---------------------------------

/// `rsg-bench s3` flags (flattened alongside
/// [`crate::orchestrator::SessionArgs`]).
#[derive(Debug, Clone, clap::Args)]
pub struct S3Args {
    #[arg(long, default_value = "hyperfine")]
    pub hyperfine: String,
    #[arg(long, default_value_t = 3)]
    pub hyperfine_runs: u32,
    #[arg(long, default_value_t = 1)]
    pub hyperfine_warmup: u32,
    #[arg(long, default_value = "Scheduler is ready")]
    pub python_backend_ready_marker: String,
    #[arg(long, default_value = "backend ready; handshake sent to rsg-server")]
    pub rust_backend_ready_marker: String,
    #[arg(long, default_value_t = 20)]
    pub ready_poll_ms: u64,
}

fn frontend_kind_str(kind: FrontendKind) -> &'static str {
    match kind {
        FrontendKind::Python => "python",
        FrontendKind::Rust => "rust",
    }
}

fn tail_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

/// Spawns `argv[0] argv[1..]` (hyperfine itself) with `env_set`/`env_remove`
/// applied, stdout discarded and stderr drained on a background thread (so
/// a chatty hyperfine run never blocks on a full pipe buffer), polling for
/// exit up to `timeout` and killing on expiry -- the same technique
/// `crosscheck::run_cross_tool` (07-07) already established for a
/// long-lived external subprocess.
fn run_hyperfine(
    argv: &[String],
    env_set: &[(String, String)],
    env_remove: &[String],
    timeout: Duration,
) -> anyhow::Result<()> {
    let (argv0, rest) = argv.split_first().context("empty hyperfine command")?;
    let mut cmd = std::process::Command::new(argv0);
    cmd.args(rest);
    for (k, v) in env_set {
        cmd.env(k, v);
    }
    for k in env_remove {
        cmd.env_remove(k);
    }
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .with_context(|| format!("spawn hyperfine: {}", argv.join(" ")))?;
    let mut stderr_pipe = child.stderr.take().context("hyperfine stderr not piped")?;

    let (tx, rx) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stderr_pipe.read_to_string(&mut buf);
        let _ = tx.send(buf);
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    let stderr_text = reader
                        .join()
                        .ok()
                        .and_then(|()| rx.recv().ok())
                        .unwrap_or_default();
                    anyhow::bail!(
                        "hyperfine timed out after {timeout:?}; last stderr:\n{}",
                        tail_lines(&stderr_text, 40)
                    );
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => anyhow::bail!("wait on hyperfine: {e}"),
        }
    };

    let stderr_text = reader
        .join()
        .ok()
        .and_then(|()| rx.recv().ok())
        .unwrap_or_default();
    if !status.success() {
        anyhow::bail!(
            "hyperfine exited {:?}; last stderr:\n{}",
            status.code(),
            tail_lines(&stderr_text, 40)
        );
    }
    Ok(())
}

/// A [`TrialRunner`] over `hyperfine` + `coldstart-once`/`coldstart-stop`
/// (D-08): [`Lifecycle::RunnerManaged`], since this runner owns its own
/// server lifecycle rather than the orchestrator launching one.
pub struct S3Runner {
    pub args: S3Args,
}

impl TrialRunner for S3Runner {
    fn lifecycle(&self) -> Lifecycle {
        Lifecycle::RunnerManaged
    }

    fn workload(&self) -> serde_json::Value {
        serde_json::json!({
            "hyperfine": self.args.hyperfine,
            "hyperfine_runs": self.args.hyperfine_runs,
            "hyperfine_warmup": self.args.hyperfine_warmup,
        })
    }

    async fn run_trial(&self, ctx: &TrialContext<'_>) -> anyhow::Result<TrialMeasurement> {
        let current_exe =
            std::env::current_exe().context("current_exe for coldstart-once/-stop")?;
        let current_exe_str = current_exe.to_string_lossy().into_owned();

        let record_file = ctx.trial_dir.join("coldstart.jsonl");
        let pgid_file = ctx.trial_dir.join("server.pgid.json");
        let log_path = ctx.trial_dir.join("server.log");
        let export_json = ctx.trial_dir.join("hyperfine.json");
        let hook_root = ctx.trial_dir.join("hook");

        let marker = match ctx.arm.kind {
            FrontendKind::Python => self.args.python_backend_ready_marker.clone(),
            FrontendKind::Rust => self.args.rust_backend_ready_marker.clone(),
        };

        let mut once_cmd: Vec<String> = vec![
            current_exe_str.clone(),
            "coldstart-once".to_string(),
            "--record-file".to_string(),
            record_file.to_string_lossy().into_owned(),
            "--pgid-file".to_string(),
            pgid_file.to_string_lossy().into_owned(),
            "--log".to_string(),
            log_path.to_string_lossy().into_owned(),
            "--port".to_string(),
            ctx.port.to_string(),
            "--ready-timeout-s".to_string(),
            ctx.cfg.ready_timeout.as_secs_f64().to_string(),
            "--ready-poll-ms".to_string(),
            self.args.ready_poll_ms.to_string(),
            "--backend-ready-marker".to_string(),
            marker,
        ];
        if ctx.cfg.gc_hook {
            once_cmd.push("--hook-root".to_string());
            once_cmd.push(hook_root.to_string_lossy().into_owned());
        }
        once_cmd.push("--".to_string());
        once_cmd.extend(ctx.argv.iter().cloned());

        let stop_cmd: Vec<String> = vec![
            current_exe_str,
            "coldstart-stop".to_string(),
            "--pgid-file".to_string(),
            pgid_file.to_string_lossy().into_owned(),
            "--record-file".to_string(),
            record_file.to_string_lossy().into_owned(),
            "--teardown-grace-s".to_string(),
            ctx.cfg.teardown_grace.as_secs_f64().to_string(),
            "--kind".to_string(),
            frontend_kind_str(ctx.arm.kind).to_string(),
            "--rust-frontend-process-name".to_string(),
            ctx.cfg.rust_frontend_process_name.clone(),
        ];

        let hyperfine_cmd = hyperfine_argv(
            &self.args.hyperfine,
            self.args.hyperfine_runs,
            self.args.hyperfine_warmup,
            &export_json,
            &once_cmd,
            &stop_cmd,
        );

        let attempts = (self.args.hyperfine_runs + self.args.hyperfine_warmup) as f64;
        let timeout = Duration::from_secs_f64(
            attempts
                * (ctx.cfg.ready_timeout.as_secs_f64()
                    + ctx.cfg.teardown_grace.as_secs_f64()
                    + 30.0),
        );

        let mut env_set = ctx.env_set.clone();
        env_set.push(("PYTHONUNBUFFERED".to_string(), "1".to_string()));

        let run_result = run_hyperfine(&hyperfine_cmd, &env_set, &ctx.env_remove, timeout);

        // Self-heal: if the pgid file still exists (e.g. hyperfine itself
        // was killed on timeout before `--conclude` ran), stop that group
        // before reporting this trial's own outcome.
        if pgid_file.exists()
            && let Ok(text) = std::fs::read_to_string(&pgid_file)
            && let Ok(group) = serde_json::from_str::<DetachedGroup>(&text)
        {
            let _ = procs::stop_detached(&group, ctx.cfg.teardown_grace).await;
            let _ = std::fs::remove_file(&pgid_file);
        }

        run_result?;

        let hyperfine_text = std::fs::read_to_string(&export_json)
            .with_context(|| format!("read hyperfine export {}", export_json.display()))?;
        let hyperfine_stats =
            parse_hyperfine_json(&hyperfine_text, &export_json.to_string_lossy())?;

        let records = read_records(&record_file);
        let mut ready_by_run: BTreeMap<u32, ColdstartRecord> = BTreeMap::new();
        let mut mem_by_run: BTreeMap<u32, ColdstartRecord> = BTreeMap::new();
        for r in records {
            match &r {
                ColdstartRecord::Ready { run, .. } => {
                    ready_by_run.insert(*run, r);
                }
                ColdstartRecord::Mem { run, .. } => {
                    mem_by_run.insert(*run, r);
                }
            }
        }

        let hook_log_full = gclog::read_hook_dir(&hook_root).unwrap_or_default();
        let warmup = self.args.hyperfine_warmup;

        let mut kept_runs = Vec::new();
        let (mut e2e_sum, mut e2e_n) = (0.0_f64, 0u32);
        let (mut tail_sum, mut tail_n) = (0.0_f64, 0u32);
        let (mut frontend_rss_sum, mut tree_rss_sum, mut mem_n) = (0u64, 0u64, 0u32);
        let (mut frontend_pss_sum, mut tree_pss_sum, mut pss_ok) = (0u64, 0u64, true);

        for (&run, ready_rec) in &ready_by_run {
            if run < warmup {
                continue;
            }
            let ColdstartRecord::Ready {
                launched_unix_ns,
                ready_unix_ns,
                e2e_ready_s,
                backend_ready_s,
                frontend_tail_s,
                ..
            } = ready_rec
            else {
                continue;
            };

            let (groups, tree, roles) = match mem_by_run.get(&run) {
                Some(ColdstartRecord::Mem {
                    groups,
                    tree,
                    roles,
                    ..
                }) => (groups.clone(), *tree, roles.clone()),
                _ => (
                    BTreeMap::new(),
                    MemPoint {
                        rss_bytes: 0,
                        pss_bytes: None,
                    },
                    BTreeMap::new(),
                ),
            };

            e2e_sum += *e2e_ready_s;
            e2e_n += 1;
            if let Some(t) = frontend_tail_s {
                tail_sum += t;
                tail_n += 1;
            }
            tree_rss_sum += tree.rss_bytes;
            match tree.pss_bytes {
                Some(p) => tree_pss_sum += p,
                None => pss_ok = false,
            }
            if let Some(fp) = groups.get(&Group::Frontend) {
                frontend_rss_sum += fp.rss_bytes;
                match fp.pss_bytes {
                    Some(p) => frontend_pss_sum += p,
                    None => pss_ok = false,
                }
            }
            mem_n += 1;

            let gc_boot: Option<BTreeMap<Role, GcRow>> = if hook_log_full.present {
                let role_map = RoleMap {
                    roles: roles.clone(),
                };
                Some(gclog::gc_by_role(
                    &hook_log_full,
                    &role_map,
                    ctx.arm.kind,
                    *launched_unix_ns,
                    *ready_unix_ns,
                ))
            } else {
                None
            };

            kept_runs.push(serde_json::json!({
                "run": run,
                "e2e_ready_s": e2e_ready_s,
                "backend_ready_s": backend_ready_s,
                "frontend_tail_s": frontend_tail_s,
                "memory_at_ready": { "groups": groups, "tree": tree },
                "gc_boot": gc_boot,
            }));
        }

        let means = serde_json::json!({
            "e2e_ready_s": (e2e_n > 0).then(|| e2e_sum / e2e_n as f64),
            "frontend_tail_s": (tail_n > 0).then(|| tail_sum / tail_n as f64),
            "frontend_rss_bytes": (mem_n > 0).then(|| frontend_rss_sum / mem_n as u64),
            "frontend_pss_bytes": (mem_n > 0 && pss_ok).then(|| frontend_pss_sum / mem_n as u64),
            "tree_rss_bytes": (mem_n > 0).then(|| tree_rss_sum / mem_n as u64),
            "tree_pss_bytes": (mem_n > 0 && pss_ok).then(|| tree_pss_sum / mem_n as u64),
        });

        let result = serde_json::json!({
            "hyperfine": hyperfine_stats,
            "runs": kept_runs,
            "means": means,
        });

        Ok(TrialMeasurement {
            windows: Vec::<MeasuredWindow>::new(),
            result,
        })
    }
}
