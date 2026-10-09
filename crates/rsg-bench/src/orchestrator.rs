//! The shared A/B orchestrator (D-05, D-06, D-07, D-08): arm construction,
//! strict alternation, per-trial launch/measure/teardown, BENCH-08's
//! GC/memory wiring, and atomic manifest writing. Every scenario plugs in
//! its own [`TrialRunner`]; this module is written once, not reimplemented
//! per scenario.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context;

use crate::manifest::{self, BackendKind};
use crate::roles::FrontendKind;
use crate::{client, cmdline, gclog, memory, metrics, procs, roles};

/// Whether the harness itself launches/tears down the frontend for a
/// trial (every scenario so far), or the scenario runner manages its own
/// subprocess lifecycle (reserved for a future scenario).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    HarnessManaged,
    RunnerManaged,
}

/// One A/B arm: an id, which frontend it launches, its `--num-tokenizer`
/// value (Python only), whether it doubles as the "best" Python
/// configuration, and its unrendered launch template.
#[derive(Debug, Clone)]
pub struct Arm {
    pub id: String,
    pub kind: FrontendKind,
    pub num_tokenizer: Option<u32>,
    pub also_best: bool,
    pub template: String,
}

/// Builds the fixed arm list (D-09/D-10): `python-default` (always),
/// `rust` (always, one fixed configuration), and `python-best` (only when
/// `best_nt` is `Some` and differs from `default_nt` -- otherwise
/// `python-default` absorbs it via `also_best: true`). `select`, when
/// given, keeps only arms whose id appears in it; an unknown id is an
/// error naming it.
pub fn build_arms(
    python_tpl: &str,
    rust_tpl: &str,
    default_nt: u32,
    best_nt: Option<u32>,
    select: Option<&[String]>,
) -> anyhow::Result<Vec<Arm>> {
    let also_best = best_nt.is_none_or(|v| v == default_nt);
    let mut arms = vec![
        Arm {
            id: "python-default".to_string(),
            kind: FrontendKind::Python,
            num_tokenizer: Some(default_nt),
            also_best,
            template: python_tpl.to_string(),
        },
        Arm {
            id: "rust".to_string(),
            kind: FrontendKind::Rust,
            num_tokenizer: None,
            also_best: false,
            template: rust_tpl.to_string(),
        },
    ];
    if let Some(best) = best_nt
        && best != default_nt
    {
        arms.push(Arm {
            id: "python-best".to_string(),
            kind: FrontendKind::Python,
            num_tokenizer: Some(best),
            also_best: false,
            template: python_tpl.to_string(),
        });
    }

    if let Some(names) = select {
        for name in names {
            if !arms.iter().any(|a| &a.id == name) {
                anyhow::bail!("unknown arm in --arms: {name}");
            }
        }
        arms.retain(|a| names.iter().any(|n| n == &a.id));
    }

    Ok(arms)
}

/// One scheduled trial: its global index, which round (0-based repeat) it
/// belongs to, and which arm (by index into the arms list passed to
/// [`build_arms`]) it runs.
#[derive(Debug, Clone, Copy)]
pub struct TrialSlot {
    pub index: usize,
    pub round: u32,
    pub arm: usize,
}

/// Strict round-robin alternation (D-05/D-06): `runs` rounds, each
/// visiting every arm index in order.
pub fn schedule(n_arms: usize, runs: u32) -> Vec<TrialSlot> {
    let mut slots = Vec::with_capacity(n_arms * runs as usize);
    let mut index = 0;
    for round in 0..runs {
        for arm in 0..n_arms {
            slots.push(TrialSlot { index, round, arm });
            index += 1;
        }
    }
    slots
}

/// `--gc-hook on|off` (D-15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum GcHook {
    On,
    Off,
}

/// Every CLI flag a session needs, flattened into each scenario
/// subcommand.
#[derive(Debug, Clone, clap::Args)]
pub struct SessionArgs {
    #[arg(
        long,
        default_value = "{python} -m rsglang.launch --frontend python --model {model} --port {port} --num-tokenizer {num_tokenizer}"
    )]
    pub python_cmd: String,
    #[arg(
        long,
        default_value = "{python} -m rsglang.launch --frontend rust --model {model} --port {port}"
    )]
    pub rust_cmd: String,
    #[arg(long, default_value_t = 0)]
    pub python_default_num_tokenizer: u32,
    #[arg(long)]
    pub python_best_num_tokenizer: Option<u32>,
    #[arg(long, value_delimiter = ',')]
    pub arms: Option<Vec<String>>,
    #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u32).range(1..))]
    pub runs: u32,
    #[arg(long, default_value_t = 42)]
    pub seed: u64,
    #[arg(long, default_value_t = 1919)]
    pub port: u16,
    #[arg(long, default_value = "Qwen/Qwen3-0.6B")]
    pub model_arg: String,
    #[arg(long)]
    pub python: Option<String>,
    #[arg(long, value_enum, default_value_t = GcHook::On)]
    pub gc_hook: GcHook,
    #[arg(long, default_value_t = 1.0)]
    pub hook_interval_s: f64,
    #[arg(long, default_value_t = 8)]
    pub warmup_requests: u32,
    #[arg(long, default_value_t = 900.0)]
    pub ready_timeout_s: f64,
    #[arg(long, default_value_t = 60.0)]
    pub teardown_grace_s: f64,
    #[arg(long, default_value_t = 1000)]
    pub mem_interval_ms: u64,
    #[arg(long, value_enum, default_value_t = BackendKind::Real)]
    pub backend_kind: BackendKind,
    #[arg(long, default_value = "rsg-server")]
    pub rust_frontend_process_name: String,
    #[arg(long, default_value = "target/rsg-bench/work")]
    pub work_root: PathBuf,
    #[arg(long)]
    pub repo_root: Option<PathBuf>,
    #[arg(long)]
    pub out: PathBuf,
}

/// The resolved, runtime-ready form of [`SessionArgs`] (dynamic defaults
/// resolved, durations parsed, arms built).
#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub arms: Vec<Arm>,
    pub runs: u32,
    pub seed: u64,
    pub port: u16,
    pub model_arg: String,
    pub python: String,
    pub gc_hook: bool,
    pub hook_interval_s: f64,
    pub hook_interval: Duration,
    pub warmup_requests: u32,
    pub ready_timeout: Duration,
    pub teardown_grace: Duration,
    pub mem_interval: Duration,
    pub backend_kind: BackendKind,
    pub rust_frontend_process_name: String,
    pub work_root: PathBuf,
    pub repo_root: PathBuf,
    pub out: PathBuf,
    pub scenario: String,
    pub harness_argv: Vec<String>,
}

fn resolve_default_python(repo_root: &Path) -> String {
    let candidate = repo_root.join(".venv").join("bin").join("python");
    if candidate.is_file() {
        candidate.to_string_lossy().into_owned()
    } else {
        "python3".to_string()
    }
}

impl SessionConfig {
    /// Resolves [`SessionArgs`] (plus the scenario's own name, for
    /// `session.scenario`/the session work-dir prefix) into a
    /// [`SessionConfig`].
    pub fn from_args(args: &SessionArgs, scenario: &str) -> anyhow::Result<SessionConfig> {
        let repo_root = match &args.repo_root {
            Some(p) => p.clone(),
            None => std::env::current_dir().context("determine current dir as repo root")?,
        };
        let python = args
            .python
            .clone()
            .unwrap_or_else(|| resolve_default_python(&repo_root));

        let arms = build_arms(
            &args.python_cmd,
            &args.rust_cmd,
            args.python_default_num_tokenizer,
            args.python_best_num_tokenizer,
            args.arms.as_deref(),
        )?;

        Ok(SessionConfig {
            arms,
            runs: args.runs,
            seed: args.seed,
            port: args.port,
            model_arg: args.model_arg.clone(),
            python,
            gc_hook: args.gc_hook == GcHook::On,
            hook_interval_s: args.hook_interval_s,
            hook_interval: Duration::from_secs_f64(args.hook_interval_s.max(0.0)),
            warmup_requests: args.warmup_requests,
            ready_timeout: Duration::from_secs_f64(args.ready_timeout_s.max(0.0)),
            teardown_grace: Duration::from_secs_f64(args.teardown_grace_s.max(0.0)),
            mem_interval: Duration::from_millis(args.mem_interval_ms),
            backend_kind: args.backend_kind,
            rust_frontend_process_name: args.rust_frontend_process_name.clone(),
            work_root: args.work_root.clone(),
            repo_root,
            out: args.out.clone(),
            scenario: scenario.to_string(),
            harness_argv: std::env::args().collect(),
        })
    }
}

/// Everything one [`TrialRunner::run_trial`] call needs.
pub struct TrialContext<'a> {
    pub client: &'a reqwest::Client,
    pub base_url: String,
    pub port: u16,
    pub model_id: Option<String>,
    pub seed: u64,
    pub arm: &'a Arm,
    pub argv: Vec<String>,
    pub env_set: Vec<(String, String)>,
    pub env_remove: Vec<String>,
    pub trial_dir: PathBuf,
    pub index: usize,
    pub cfg: &'a SessionConfig,
}

/// One measured window inside a trial (most scenarios have exactly one;
/// reserved for a scenario that reports more than one sub-window).
pub struct MeasuredWindow {
    pub label: String,
    pub start_unix_ns: u64,
    pub end_unix_ns: u64,
    pub requests: Vec<client::RequestRecord>,
    pub histograms: Option<metrics::LatencyHistograms>,
}

/// What [`TrialRunner::run_trial`] returns: every measured window, plus a
/// scenario-specific summary stored verbatim in the manifest.
pub struct TrialMeasurement {
    pub windows: Vec<MeasuredWindow>,
    pub result: serde_json::Value,
}

/// A scenario's single-trial driver, plugged into the shared orchestrator
/// (D-08). `#[allow(async_fn_in_trait)]`: the orchestrator always awaits
/// `run_trial` directly on its own task and never spawns it, so no `Send`
/// bound is needed.
#[allow(async_fn_in_trait)]
pub trait TrialRunner {
    fn lifecycle(&self) -> Lifecycle {
        Lifecycle::HarnessManaged
    }
    fn workload(&self) -> serde_json::Value;
    async fn run_trial(&self, ctx: &TrialContext<'_>) -> anyhow::Result<TrialMeasurement>;
}

/// What a finished (or interrupted) session produced.
#[derive(Debug, Clone)]
pub struct SessionOutcome {
    pub manifest_path: PathBuf,
    pub failed_trials: usize,
    pub interrupted: bool,
}

fn unix_secs_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `<work_root>/<scenario>-<utc compact>-<pid>`, created with
/// `create_dir_all` on `work_root` and an *exclusive* `create_dir` on the
/// session dir itself (T-07-13: never a shared, guessable `/tmp` path).
fn make_session_dir(cfg: &SessionConfig) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(&cfg.work_root)
        .with_context(|| format!("create work root {}", cfg.work_root.display()))?;
    let compact = manifest::utc_rfc3339(unix_secs_now()).replace(['-', ':'], "");
    let pid = std::process::id();
    let dir = cfg
        .work_root
        .join(format!("{}-{compact}-{pid}", cfg.scenario));
    std::fs::create_dir(&dir).with_context(|| format!("create session dir {}", dir.display()))?;
    Ok(dir)
}

/// Writes the session's shared gc-hook sitecustomize shim via one
/// `{python} -c ...` call (D-15), importing `rsglang.profiling.hook`
/// through `PYTHONPATH=<repo>/python`.
fn write_session_shim(cfg: &SessionConfig, shim_dir: &Path) -> anyhow::Result<()> {
    let python_dir = cfg.repo_root.join("python");
    let mut cmd = Command::new(&cfg.python);
    cmd.args([
        "-c",
        "import sys; from rsglang.profiling.hook import write_shim; write_shim(sys.argv[1])",
        &shim_dir.to_string_lossy(),
    ]);
    cmd.env("PYTHONPATH", &python_dir);
    let out = cmd
        .output()
        .with_context(|| format!("spawn {} to write gc-hook shim", cfg.python))?;
    if !out.status.success() {
        anyhow::bail!(
            "writing gc-hook shim failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}

fn render_arm_argv(cfg: &SessionConfig, arm: &Arm) -> anyhow::Result<Vec<String>> {
    let mut values: BTreeMap<&str, String> = BTreeMap::new();
    values.insert("python", cfg.python.clone());
    values.insert("model", cfg.model_arg.clone());
    values.insert("port", cfg.port.to_string());
    if let Some(nt) = arm.num_tokenizer {
        values.insert("num_tokenizer", nt.to_string());
    }
    cmdline::render(&arm.template, &values)
}

/// Python's `repr(float)` for whole numbers (`1.0`, not `1`); for every
/// other value, Rust's own shortest round-trip `Display`.
fn py_repr_float(v: f64) -> String {
    if v.is_finite() && v == v.trunc() {
        format!("{v:.1}")
    } else {
        format!("{v}")
    }
}

/// The gc-hook environment for one trial (identical across arms, P1):
/// `RSGLANG_PROFILE_DIR`/`_MODE`/`_INTERVAL_S` plus a `PYTHONPATH` that
/// chain-loads the session's shim, when `cfg.gc_hook` is on; otherwise
/// removes those three vars so a parent shell's own profiling env never
/// leaks into the trial. Shared by [`run_one_trial`] (harness-managed) and
/// [`run_one_trial_runner_managed`] (runner-managed), so both lifecycles
/// build the exact same environment from the exact same `cfg`.
fn build_trial_env(
    cfg: &SessionConfig,
    hook_dir: &Path,
    shim_dir: &Path,
) -> (Vec<(String, String)>, Vec<String>) {
    let mut env_set = Vec::new();
    let mut env_remove = Vec::new();
    if cfg.gc_hook {
        env_set.push((
            "RSGLANG_PROFILE_DIR".to_string(),
            hook_dir.to_string_lossy().into_owned(),
        ));
        env_set.push(("RSGLANG_PROFILE_MODE".to_string(), "gc_only".to_string()));
        env_set.push((
            "RSGLANG_PROFILE_INTERVAL_S".to_string(),
            py_repr_float(cfg.hook_interval_s),
        ));
        env_set.push(("PYTHONPATH".to_string(), build_pythonpath(cfg, shim_dir)));
    } else {
        env_remove.push("RSGLANG_PROFILE_DIR".to_string());
        env_remove.push("RSGLANG_PROFILE_MODE".to_string());
        env_remove.push("RSGLANG_PROFILE_INTERVAL_S".to_string());
    }
    (env_set, env_remove)
}

fn build_pythonpath(cfg: &SessionConfig, shim_dir: &Path) -> String {
    let python_dir = cfg.repo_root.join("python");
    let mut parts = vec![
        shim_dir.to_string_lossy().into_owned(),
        python_dir.to_string_lossy().into_owned(),
    ];
    if let Ok(existing) = std::env::var("PYTHONPATH") {
        parts.push(existing);
    }
    parts.join(":")
}

/// What survives a failed or partially-completed trial attempt, so a
/// [`TrialRecord`](manifest::TrialRecord) can still be built with whatever
/// was actually observed.
struct PartialTrial {
    ready_s: Option<f64>,
    model_id: Option<String>,
    teardown: Option<procs::TeardownReport>,
    samples: Vec<memory::TreeSample>,
}

type AttemptOk = (
    PartialTrial,
    TrialMeasurement,
    gclog::HookLog,
    roles::RoleMap,
);
type AttemptErr = (PartialTrial, String);

#[allow(clippy::too_many_arguments)]
async fn attempt_trial<R: TrialRunner>(
    cfg: &SessionConfig,
    runner: &R,
    client: &reqwest::Client,
    arm: &Arm,
    argv: &[String],
    spec: &procs::LaunchSpec,
    slot: &TrialSlot,
    trial_dir: &Path,
    hook_dir: &Path,
) -> Result<AttemptOk, AttemptErr> {
    let handle = procs::launch(spec, cfg.port).map_err(|e| {
        (
            PartialTrial {
                ready_s: None,
                model_id: None,
                teardown: None,
                samples: Vec::new(),
            },
            format!("launch failed: {e:#}"),
        )
    })?;
    let leader_pid = handle.leader_pid;
    let sampler = memory::MemorySampler::start(leader_pid, cfg.mem_interval);
    let base = client::local_base_url(cfg.port);

    let ready = procs::wait_ready(
        &handle,
        client,
        cfg.port,
        cfg.ready_timeout,
        Duration::from_millis(100),
    )
    .await;
    let ready_s = match ready {
        Ok(d) => d.as_secs_f64(),
        Err(e) => {
            let samples = sampler.stop();
            let teardown = procs::teardown(handle, cfg.teardown_grace).await.ok();
            return Err((
                PartialTrial {
                    ready_s: None,
                    model_id: None,
                    teardown,
                    samples,
                },
                format!("server not ready: {e:#}"),
            ));
        }
    };

    let model_id = match client::fetch_model_id(client, &base).await {
        Ok(id) => id,
        Err(e) => {
            let samples = sampler.stop();
            let teardown = procs::teardown(handle, cfg.teardown_grace).await.ok();
            return Err((
                PartialTrial {
                    ready_s: Some(ready_s),
                    model_id: None,
                    teardown,
                    samples,
                },
                format!("fetch_model_id failed: {e:#}"),
            ));
        }
    };

    for _ in 0..cfg.warmup_requests {
        let req = client::ChatRequest {
            model: model_id.clone(),
            prompt: "Say hello.".to_string(),
            max_tokens: 16,
        };
        let _ = client::stream_chat(client, &base, &req, client::CancelPlan::None).await;
    }

    let ctx = TrialContext {
        client,
        base_url: base.clone(),
        port: cfg.port,
        model_id: Some(model_id.clone()),
        seed: cfg.seed,
        arm,
        argv: argv.to_vec(),
        env_set: spec.env_set.clone(),
        env_remove: spec.env_remove.clone(),
        trial_dir: trial_dir.to_path_buf(),
        index: slot.index,
        cfg,
    };
    let measurement = runner.run_trial(&ctx).await;

    // Let the in-process gc-hook buffers flush this window's GC events
    // before the process is torn down, identically for every arm (D-15).
    if cfg.gc_hook {
        tokio::time::sleep(cfg.hook_interval + Duration::from_millis(500)).await;
    }

    let samples = sampler.stop();
    let teardown = procs::teardown(handle, cfg.teardown_grace).await.ok();

    let measurement = match measurement {
        Ok(m) => m,
        Err(e) => {
            return Err((
                PartialTrial {
                    ready_s: Some(ready_s),
                    model_id: Some(model_id),
                    teardown,
                    samples,
                },
                format!("run_trial failed: {e:#}"),
            ));
        }
    };

    let hook = gclog::read_hook_dir(hook_dir).unwrap_or_default();
    let process_names = memory::process_names(&samples);
    let role_map = roles::RoleMap::build(
        leader_pid,
        arm.kind,
        &hook,
        &process_names,
        &cfg.rust_frontend_process_name,
    );

    Ok((
        PartialTrial {
            ready_s: Some(ready_s),
            model_id: Some(model_id),
            teardown,
            samples,
        },
        measurement,
        hook,
        role_map,
    ))
}

#[allow(clippy::too_many_arguments)]
async fn run_one_trial<R: TrialRunner>(
    cfg: &SessionConfig,
    runner: &R,
    client: &reqwest::Client,
    slot: &TrialSlot,
    arm: &Arm,
    argv: &[String],
    trial_dir: &Path,
    shim_dir: &Path,
    warnings: &mut Vec<String>,
) -> manifest::TrialRecord {
    let hook_dir = trial_dir.join("hook");
    let log_path = trial_dir.join("server.log");
    let launched_utc = Some(manifest::utc_rfc3339(unix_secs_now()));

    let (env_set, env_remove) = build_trial_env(cfg, &hook_dir, shim_dir);

    let spec = procs::LaunchSpec {
        argv: argv.to_vec(),
        env_set,
        env_remove,
        log_path,
    };

    match attempt_trial(
        cfg, runner, client, arm, argv, &spec, slot, trial_dir, &hook_dir,
    )
    .await
    {
        Ok((partial, measurement, hook, role_map)) => {
            if hook.mem_records > 0 {
                warnings.push(format!(
                    "trial {} ({}): tracemalloc records present; hook not in gc_only mode (RESEARCH Pitfall 2)",
                    slot.index, arm.id
                ));
            }
            let no_hook_records =
                cfg.gc_hook && arm.kind == FrontendKind::Python && hook.start_pids.is_empty();
            if no_hook_records {
                warnings.push(format!(
                    "trial {} ({}): no hook records from python arm",
                    slot.index, arm.id
                ));
            }

            let mut windows = Vec::with_capacity(measurement.windows.len());
            let mut histograms = BTreeMap::new();
            for w in &measurement.windows {
                let gc_status = if !cfg.gc_hook {
                    "disabled"
                } else if no_hook_records {
                    "no_hook_records"
                } else {
                    "collected"
                };
                let collected = gc_status == "collected";
                let gc = collected.then(|| {
                    gclog::gc_by_role(&hook, &role_map, arm.kind, w.start_unix_ns, w.end_unix_ns)
                });
                // Co-occurrence correlates GC pauses against TTFT spikes, so
                // it is only meaningful for a window that actually sent
                // requests (D-16). A throughput or crosscheck window has no
                // `client::RequestRecord`s at all; computing it anyway would
                // produce a `CoOccurrenceRow::NoFirstToken` row that falsely
                // implies "requests were sent but none observed a first
                // token" rather than "this window never sent requests".
                let cooccurrence = (collected && !w.requests.is_empty()).then(|| {
                    gclog::cooccurrence_by_group(
                        &w.requests,
                        &hook,
                        &role_map,
                        arm.kind,
                        w.start_unix_ns,
                        w.end_unix_ns,
                    )
                });
                let (by_group, total) = memory::memory_by_group(
                    &partial.samples,
                    &role_map,
                    w.start_unix_ns,
                    w.end_unix_ns,
                );
                windows.push(manifest::WindowObs {
                    label: w.label.clone(),
                    start_unix_ns: w.start_unix_ns,
                    end_unix_ns: w.end_unix_ns,
                    gc_status: gc_status.to_string(),
                    gc,
                    memory: by_group,
                    tree_memory: total,
                    cooccurrence,
                });
                if let Some(h) = &w.histograms
                    && let Ok(enc) = h.encode()
                {
                    histograms.insert(w.label.clone(), enc);
                }
            }

            manifest::TrialRecord {
                index: slot.index,
                round: slot.round,
                arm: arm.id.clone(),
                status: manifest::TrialStatus::Ok,
                error: None,
                model_id: partial.model_id,
                launched_utc,
                ready_s: partial.ready_s,
                result: measurement.result,
                histograms,
                windows,
                roles: role_map.roles.clone(),
                hook: manifest::HookSummary {
                    present: hook.present,
                    files: hook.files,
                    malformed_lines: hook.malformed_lines,
                    mem_records: hook.mem_records,
                },
                teardown_graceful: partial.teardown.as_ref().map(|r| r.graceful),
                teardown_survivors: partial.teardown.map(|r| r.survivors).unwrap_or_default(),
            }
        }
        Err((partial, error)) => manifest::TrialRecord {
            index: slot.index,
            round: slot.round,
            arm: arm.id.clone(),
            status: manifest::TrialStatus::Failed,
            error: Some(error),
            model_id: partial.model_id,
            launched_utc,
            ready_s: partial.ready_s,
            result: serde_json::Value::Null,
            histograms: BTreeMap::new(),
            windows: Vec::new(),
            roles: BTreeMap::new(),
            hook: manifest::HookSummary::default(),
            teardown_graceful: partial.teardown.as_ref().map(|r| r.graceful),
            teardown_survivors: partial.teardown.map(|r| r.survivors).unwrap_or_default(),
        },
    }
}

/// A [`Lifecycle::RunnerManaged`] trial (D-08): the harness only creates
/// the trial dir, renders argv, builds the gc-hook env identically to a
/// harness-managed trial (P1) and checks the port before/after -- the
/// runner itself owns launching, observing and tearing down its own
/// server(s) (e.g. [`crate::scenarios::s3_coldstart::S3Runner`]'s
/// `hyperfine` + `coldstart-once`/`coldstart-stop`). No [`memory::MemorySampler`]
/// runs and no hook directory is read here, because the runner owns
/// observation.
#[allow(clippy::too_many_arguments)]
async fn run_one_trial_runner_managed<R: TrialRunner>(
    cfg: &SessionConfig,
    runner: &R,
    client: &reqwest::Client,
    slot: &TrialSlot,
    arm: &Arm,
    argv: &[String],
    trial_dir: &Path,
    shim_dir: &Path,
    warnings: &mut Vec<String>,
) -> manifest::TrialRecord {
    let hook_dir = trial_dir.join("hook");
    let launched_utc = Some(manifest::utc_rfc3339(unix_secs_now()));
    let (env_set, env_remove) = build_trial_env(cfg, &hook_dir, shim_dir);

    fn failed_record(
        slot: &TrialSlot,
        arm: &Arm,
        launched_utc: Option<String>,
        error: String,
    ) -> manifest::TrialRecord {
        manifest::TrialRecord {
            index: slot.index,
            round: slot.round,
            arm: arm.id.clone(),
            status: manifest::TrialStatus::Failed,
            error: Some(error),
            model_id: None,
            launched_utc,
            ready_s: None,
            result: serde_json::Value::Null,
            histograms: BTreeMap::new(),
            windows: Vec::new(),
            roles: BTreeMap::new(),
            hook: manifest::HookSummary::default(),
            teardown_graceful: None,
            teardown_survivors: Vec::new(),
        }
    }

    if let Err(e) = procs::ensure_port_free(cfg.port) {
        return failed_record(
            slot,
            arm,
            launched_utc,
            format!("port busy before runner-managed trial: {e:#}"),
        );
    }

    let ctx = TrialContext {
        client,
        base_url: client::local_base_url(cfg.port),
        port: cfg.port,
        model_id: None,
        seed: cfg.seed,
        arm,
        argv: argv.to_vec(),
        env_set,
        env_remove,
        trial_dir: trial_dir.to_path_buf(),
        index: slot.index,
        cfg,
    };

    let measurement = runner.run_trial(&ctx).await;

    if let Err(e) = procs::ensure_port_free(cfg.port) {
        warnings.push(format!(
            "trial {} ({}): server still listening after runner-managed trial: {e:#}",
            slot.index, arm.id
        ));
    }

    let measurement = match measurement {
        Ok(m) => m,
        Err(e) => {
            return failed_record(slot, arm, launched_utc, format!("run_trial failed: {e:#}"));
        }
    };

    let mut windows = Vec::with_capacity(measurement.windows.len());
    let mut histograms = BTreeMap::new();
    for w in &measurement.windows {
        windows.push(manifest::WindowObs {
            label: w.label.clone(),
            start_unix_ns: w.start_unix_ns,
            end_unix_ns: w.end_unix_ns,
            gc_status: "runner_managed".to_string(),
            gc: None,
            memory: BTreeMap::new(),
            tree_memory: memory::GroupMemory {
                rss_bytes: memory::MemSummary {
                    start: None,
                    end: None,
                    max: None,
                    growth: None,
                },
                pss_bytes: None,
            },
            cooccurrence: None,
        });
        if let Some(h) = &w.histograms
            && let Ok(enc) = h.encode()
        {
            histograms.insert(w.label.clone(), enc);
        }
    }

    manifest::TrialRecord {
        index: slot.index,
        round: slot.round,
        arm: arm.id.clone(),
        status: manifest::TrialStatus::Ok,
        error: None,
        model_id: None,
        launched_utc,
        ready_s: None,
        result: measurement.result,
        histograms,
        windows,
        roles: BTreeMap::new(),
        hook: manifest::HookSummary::default(),
        teardown_graceful: None,
        teardown_survivors: Vec::new(),
    }
}

/// Dispatches to [`run_one_trial`] or [`run_one_trial_runner_managed`]
/// based on `lifecycle`, as a single `async fn` so both branches of the
/// `match` produce the same concrete `Future` type -- no boxing needed for
/// [`run_session`]'s `tokio::select!`.
#[allow(clippy::too_many_arguments)]
async fn run_one_trial_dispatch<R: TrialRunner>(
    lifecycle: Lifecycle,
    cfg: &SessionConfig,
    runner: &R,
    client: &reqwest::Client,
    slot: &TrialSlot,
    arm: &Arm,
    argv: &[String],
    trial_dir: &Path,
    shim_dir: &Path,
    warnings: &mut Vec<String>,
) -> manifest::TrialRecord {
    match lifecycle {
        Lifecycle::HarnessManaged => {
            run_one_trial(
                cfg, runner, client, slot, arm, argv, trial_dir, shim_dir, warnings,
            )
            .await
        }
        Lifecycle::RunnerManaged => {
            run_one_trial_runner_managed(
                cfg, runner, client, slot, arm, argv, trial_dir, shim_dir, warnings,
            )
            .await
        }
    }
}

/// Runs every scheduled trial for `runner` (D-05/D-06/D-08), writing the
/// manifest atomically after every trial so a crash never loses prior
/// trials. A busy port before the session starts is a setup error
/// (`EXIT_SETUP`); a failed trial is recorded (never dropped or re-run)
/// and the session continues; `Ctrl-C` tears down the in-flight trial,
/// marks the manifest `interrupted: true`, and returns immediately.
pub async fn run_session<R: TrialRunner>(
    cfg: &SessionConfig,
    runner: &R,
) -> anyhow::Result<SessionOutcome> {
    procs::ensure_port_free(cfg.port).context("port already in use before session start")?;

    let session_dir = make_session_dir(cfg)?;
    let shim_dir = session_dir.join("shim");
    if cfg.gc_hook {
        write_session_shim(cfg, &shim_dir)?;
    }

    let client = client::build_client()?;
    let meta = manifest::collect_meta(
        &cfg.repo_root,
        &cfg.python,
        &cfg.model_arg,
        cfg.backend_kind,
    );

    let mut arm_infos = Vec::with_capacity(cfg.arms.len());
    let mut rendered = Vec::with_capacity(cfg.arms.len());
    for arm in &cfg.arms {
        let argv = render_arm_argv(cfg, arm)
            .with_context(|| format!("render launch template for arm {}", arm.id))?;
        arm_infos.push(manifest::ArmInfo {
            id: arm.id.clone(),
            kind: arm.kind,
            num_tokenizer: arm.num_tokenizer,
            also_best: arm.also_best,
            argv: cmdline::redact_argv(&argv),
        });
        rendered.push(argv);
    }

    let slots = schedule(cfg.arms.len(), cfg.runs);
    let schedule_ids: Vec<String> = slots.iter().map(|s| cfg.arms[s.arm].id.clone()).collect();
    let lifecycle = runner.lifecycle();

    let mut doc = manifest::Manifest {
        schema_version: manifest::SCHEMA_VERSION,
        generated_by: manifest::GENERATED_BY.to_string(),
        meta,
        session: manifest::SessionInfo {
            scenario: cfg.scenario.clone(),
            seed: cfg.seed,
            runs: cfg.runs,
            schedule: schedule_ids,
            arms: arm_infos,
            harness_argv: cmdline::redact_argv(&cfg.harness_argv),
            workload: runner.workload(),
        },
        trials: Vec::new(),
        warnings: Vec::new(),
        interrupted: false,
    };

    for slot in &slots {
        let arm = &cfg.arms[slot.arm];
        let argv = &rendered[slot.arm];
        let trial_dir = session_dir.join(format!("trial-{:02}-{}", slot.index, arm.id));
        std::fs::create_dir_all(&trial_dir)
            .with_context(|| format!("create trial dir {}", trial_dir.display()))?;

        let trial_fut = run_one_trial_dispatch(
            lifecycle,
            cfg,
            runner,
            &client,
            slot,
            arm,
            argv,
            &trial_dir,
            &shim_dir,
            &mut doc.warnings,
        );

        tokio::select! {
            record = trial_fut => {
                doc.trials.push(record);
            }
            _ = tokio::signal::ctrl_c() => {
                doc.interrupted = true;
                manifest::write_json_atomic(&cfg.out, &doc)
                    .context("write manifest after Ctrl-C")?;
                return Ok(SessionOutcome {
                    manifest_path: cfg.out.clone(),
                    failed_trials: count_failed(&doc),
                    interrupted: true,
                });
            }
        }

        manifest::write_json_atomic(&cfg.out, &doc).context("write manifest after trial")?;
    }

    Ok(SessionOutcome {
        manifest_path: cfg.out.clone(),
        failed_trials: count_failed(&doc),
        interrupted: false,
    })
}

fn count_failed(doc: &manifest::Manifest) -> usize {
    doc.trials
        .iter()
        .filter(|t| t.status == manifest::TrialStatus::Failed)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_arms_default() {
        let arms = build_arms("py", "rs", 0, None, None).unwrap();
        let ids: Vec<&str> = arms.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec!["python-default", "rust"]);
        assert!(arms[0].also_best);
    }

    #[test]
    fn three_arms_with_distinct_best() {
        let arms = build_arms("py", "rs", 0, Some(2), None).unwrap();
        let ids: Vec<&str> = arms.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec!["python-default", "rust", "python-best"]);
        assert!(!arms[0].also_best);
    }

    #[test]
    fn best_equal_default_merges() {
        let arms = build_arms("py", "rs", 0, Some(0), None).unwrap();
        assert_eq!(arms.len(), 2);
        assert!(arms[0].also_best);
    }

    #[test]
    fn schedule_two_arms_five_runs() {
        let slots = schedule(2, 5);
        assert_eq!(slots.len(), 10);
        assert_eq!(slots[0].arm, 0);
        assert_eq!(slots[1].arm, 1);
        assert_eq!(slots[9].round, 4);
    }

    #[test]
    fn schedule_three_arms_two_runs() {
        let slots = schedule(3, 2);
        let arms: Vec<usize> = slots.iter().map(|s| s.arm).collect();
        assert_eq!(arms, vec![0, 1, 2, 0, 1, 2]);
    }

    #[test]
    fn select_unknown_arm_errors() {
        let err = build_arms(
            "py",
            "rs",
            0,
            None,
            Some(&["rust".to_string(), "bogus".to_string()]),
        )
        .unwrap_err();
        assert!(err.to_string().contains("bogus"));
    }
}
