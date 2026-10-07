//! `rsg-bench`: the Phase 7 Python-vs-Rust frontend benchmark harness CLI.

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use rsg_bench::manifest;
use rsg_bench::orchestrator::{self, SessionArgs, SessionConfig};
use rsg_bench::report::{self, ReportArgs};
use rsg_bench::scenarios::crosscheck::{self, CrossArgs, CrossRunner};
use rsg_bench::scenarios::s1_cancel::{S1Args, S1Runner};
use rsg_bench::scenarios::s2_saturation::{S2Args, S2Runner};
use rsg_bench::scenarios::s3_coldstart::{self, OnceArgs, S3Args, S3Runner, StopArgs};
use rsg_bench::scenarios::standard_throughput::{ThroughputArgs, ThroughputRunner};
use rsg_bench::scenarios::sweep::{self, SweepArgs};

/// The session completed with no failed trials.
const EXIT_OK: i32 = 0;
/// A setup error: bad CLI usage caught before any trial (busy port,
/// failed shim write, bad template), a render error, or a manifest-write
/// failure.
const EXIT_SETUP: i32 = 1;
/// At least one trial was recorded with `status: failed`.
const EXIT_TRIAL_FAILED: i32 = 3;
/// `Ctrl-C`: the manifest was written with `interrupted: true`.
const EXIT_INTERRUPTED: i32 = 130;
/// D-09 sweep: no `--num-tokenizer` candidate succeeded (`pick_best` found
/// nothing to pick).
const EXIT_SWEEP_NO_CANDIDATE: i32 = 1;
/// Bad CLI usage caught by cross-check's own validation (missing
/// `--vllm-bin`/`--sglang-python`/`--tool-cmd` for the chosen tool).
const EXIT_BAD_USAGE: i32 = 2;

#[derive(Parser, Debug)]
#[command(name = "rsg-bench", about = "Phase 7 Python-vs-Rust frontend benchmark harness")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Scenario 1 (BENCH-03): 128 concurrent agents with random cancellations.
    S1 {
        #[command(flatten)]
        session: SessionArgs,
        #[command(flatten)]
        s1: S1Args,
    },
    /// Scenario 2 (BENCH-04): 32-token short-prompt saturation curve.
    S2 {
        #[command(flatten)]
        session: SessionArgs,
        #[command(flatten)]
        s2: S2Args,
    },
    /// D-09: a Scenario 2 pre-pass sweeping Python's `--num-tokenizer`
    /// candidates to find the best one.
    SweepNumTokenizer {
        #[command(flatten)]
        session: SessionArgs,
        #[command(flatten)]
        s2: S2Args,
        #[command(flatten)]
        sweep: SweepArgs,
    },
    /// D-04: cross-checks Scenario 2 against a third-party benchmark client
    /// (`vllm bench serve` or `python -m sglang.benchmark.serving`).
    Crosscheck {
        #[command(flatten)]
        session: SessionArgs,
        #[command(flatten)]
        cross: CrossArgs,
    },
    /// Scenario 3 (BENCH-05): hyperfine-timed end-to-end cold start, plus
    /// the frontend's own critical-path tail and memory at ready, reported
    /// separately.
    S3 {
        #[command(flatten)]
        session: SessionArgs,
        #[command(flatten)]
        s3: S3Args,
    },
    /// Internal: one hyperfine-timed cold-start attempt. Spawned by `s3`'s
    /// own `hyperfine` invocation as the timed command; never run directly
    /// by a human.
    #[command(hide = true)]
    ColdstartOnce {
        #[command(flatten)]
        once: OnceArgs,
    },
    /// Internal: hyperfine's `--conclude` command for one cold-start
    /// attempt (samples memory, tears the detached group down).
    #[command(hide = true)]
    ColdstartStop {
        #[command(flatten)]
        stop: StopArgs,
    },
    /// BENCH-06: standard-inference throughput, run as alternating A/B
    /// trials through 07-03's own `python -m rsglang.bench.standard_throughput`
    /// driver (D-12).
    Throughput {
        #[command(flatten)]
        session: SessionArgs,
        #[command(flatten)]
        throughput: ThroughputArgs,
    },
    /// D-13: turns every `*.manifest.json` in a directory into the combined
    /// `docs/benchmarks/` JSON and markdown pair (BENCH-07/BENCH-08).
    Report {
        #[command(flatten)]
        report: ReportArgs,
    },
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let exit_code = match cli.cmd {
        Cmd::S1 { session, s1 } => run_s1(session, s1).await,
        Cmd::S2 { session, s2 } => run_s2(session, s2).await,
        Cmd::SweepNumTokenizer { session, s2, sweep } => run_sweep(session, s2, sweep).await,
        Cmd::Crosscheck { session, cross } => run_crosscheck(session, cross).await,
        Cmd::S3 { session, s3 } => run_s3(session, s3).await,
        Cmd::ColdstartOnce { once } => match s3_coldstart::coldstart_once(once).await {
            Ok(()) => EXIT_OK,
            Err(e) => {
                tracing::error!("coldstart-once failed: {e:#}");
                EXIT_SETUP
            }
        },
        Cmd::ColdstartStop { stop } => match s3_coldstart::coldstart_stop(stop).await {
            Ok(true) => EXIT_OK,
            Ok(false) => {
                tracing::error!("coldstart-stop: group did not fully quiesce");
                EXIT_SETUP
            }
            Err(e) => {
                tracing::error!("coldstart-stop failed: {e:#}");
                EXIT_SETUP
            }
        },
        Cmd::Throughput { session, throughput } => run_throughput(session, throughput).await,
        Cmd::Report { report: report_args } => match report::run_report(report_args) {
            Ok(()) => EXIT_OK,
            Err(e) => {
                tracing::error!("report failed: {e:#}");
                EXIT_SETUP
            }
        },
    };
    std::process::exit(exit_code);
}

async fn run_s1(session: SessionArgs, s1: S1Args) -> i32 {
    let cfg = match SessionConfig::from_args(&session, "s1_cancel") {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("setup failed: {e:#}");
            return EXIT_SETUP;
        }
    };
    let runner = S1Runner { args: s1 };

    match orchestrator::run_session(&cfg, &runner).await {
        Ok(outcome) => {
            tracing::info!(
                manifest = %outcome.manifest_path.display(),
                failed_trials = outcome.failed_trials,
                interrupted = outcome.interrupted,
                "session finished"
            );
            if outcome.interrupted {
                EXIT_INTERRUPTED
            } else if outcome.failed_trials > 0 {
                EXIT_TRIAL_FAILED
            } else {
                EXIT_OK
            }
        }
        Err(e) => {
            tracing::error!("session setup failed: {e:#}");
            EXIT_SETUP
        }
    }
}

/// D-09's sweep pre-pass: runs one S2 trial per `--num-tokenizer` candidate
/// (ascending, via [`sweep::sweep_arms`]), then picks the candidate with the
/// highest `peak_rps` from the written manifest and prints
/// `best_num_tokenizer=<K>` as the final stdout line.
async fn run_sweep(session: SessionArgs, s2: S2Args, sweep_args: SweepArgs) -> i32 {
    let candidates = match sweep::validate_candidates(&sweep_args.candidates) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("setup failed: {e:#}");
            return EXIT_SETUP;
        }
    };

    let mut cfg = match SessionConfig::from_args(&session, "num_tokenizer_sweep") {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("setup failed: {e:#}");
            return EXIT_SETUP;
        }
    };
    cfg.arms = sweep::sweep_arms(&session.python_cmd, &candidates);

    let runner = S2Runner { args: s2 };

    let outcome = match orchestrator::run_session(&cfg, &runner).await {
        Ok(o) => o,
        Err(e) => {
            tracing::error!("session setup failed: {e:#}");
            return EXIT_SETUP;
        }
    };

    let doc = match manifest::read_manifest(&outcome.manifest_path) {
        Ok(m) => m,
        Err(e) => {
            tracing::error!("reading manifest after sweep failed: {e:#}");
            return EXIT_SETUP;
        }
    };

    let mut pairs = Vec::with_capacity(doc.trials.len());
    eprintln!("candidate  peak_rps");
    for trial in &doc.trials {
        let Some(num_tokenizer) = doc
            .session
            .arms
            .iter()
            .find(|a| a.id == trial.arm)
            .and_then(|a| a.num_tokenizer)
        else {
            continue;
        };
        let peak = trial.result.get("peak_rps").and_then(serde_json::Value::as_f64);
        eprintln!("{num_tokenizer:<10} {peak:?}");
        pairs.push((num_tokenizer, peak));
    }

    match sweep::pick_best(&pairs) {
        Ok(best) => {
            println!("best_num_tokenizer={best}");
            if outcome.failed_trials > 0 {
                EXIT_TRIAL_FAILED
            } else {
                EXIT_OK
            }
        }
        Err(e) => {
            tracing::error!("sweep has no successful candidate: {e:#}");
            EXIT_SWEEP_NO_CANDIDATE
        }
    }
}

/// D-04's third-party cross-check: validates `--vllm-bin`/`--sglang-python`/
/// `--tool-cmd` before any server is launched (`EXIT_BAD_USAGE`), then runs
/// one session via [`CrossRunner`].
async fn run_crosscheck(session: SessionArgs, cross: CrossArgs) -> i32 {
    if let Err(e) = crosscheck::validate_cross_args(&cross) {
        eprintln!("{e:#}");
        return EXIT_BAD_USAGE;
    }

    let scenario = format!("crosscheck_{}", cross.tool.as_str());
    let cfg = match SessionConfig::from_args(&session, &scenario) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("setup failed: {e:#}");
            return EXIT_SETUP;
        }
    };
    let runner = CrossRunner { args: cross };

    match orchestrator::run_session(&cfg, &runner).await {
        Ok(outcome) => {
            tracing::info!(
                manifest = %outcome.manifest_path.display(),
                failed_trials = outcome.failed_trials,
                interrupted = outcome.interrupted,
                "session finished"
            );
            if outcome.interrupted {
                EXIT_INTERRUPTED
            } else if outcome.failed_trials > 0 {
                EXIT_TRIAL_FAILED
            } else {
                EXIT_OK
            }
        }
        Err(e) => {
            tracing::error!("session setup failed: {e:#}");
            EXIT_SETUP
        }
    }
}

/// Scenario 3 (BENCH-05): a `rsg-bench s3` session over [`S3Runner`]
/// (`Lifecycle::RunnerManaged`).
async fn run_s3(session: SessionArgs, s3: S3Args) -> i32 {
    let cfg = match SessionConfig::from_args(&session, "s3_coldstart") {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("setup failed: {e:#}");
            return EXIT_SETUP;
        }
    };
    let runner = S3Runner { args: s3 };

    match orchestrator::run_session(&cfg, &runner).await {
        Ok(outcome) => {
            tracing::info!(
                manifest = %outcome.manifest_path.display(),
                failed_trials = outcome.failed_trials,
                interrupted = outcome.interrupted,
                "session finished"
            );
            if outcome.interrupted {
                EXIT_INTERRUPTED
            } else if outcome.failed_trials > 0 {
                EXIT_TRIAL_FAILED
            } else {
                EXIT_OK
            }
        }
        Err(e) => {
            tracing::error!("session setup failed: {e:#}");
            EXIT_SETUP
        }
    }
}

/// BENCH-06 (D-12): a `rsg-bench throughput` session over [`ThroughputRunner`].
async fn run_throughput(session: SessionArgs, throughput: ThroughputArgs) -> i32 {
    let cfg = match SessionConfig::from_args(&session, "standard_throughput") {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("setup failed: {e:#}");
            return EXIT_SETUP;
        }
    };
    let runner = ThroughputRunner { args: throughput };

    match orchestrator::run_session(&cfg, &runner).await {
        Ok(outcome) => {
            tracing::info!(
                manifest = %outcome.manifest_path.display(),
                failed_trials = outcome.failed_trials,
                interrupted = outcome.interrupted,
                "session finished"
            );
            if outcome.interrupted {
                EXIT_INTERRUPTED
            } else if outcome.failed_trials > 0 {
                EXIT_TRIAL_FAILED
            } else {
                EXIT_OK
            }
        }
        Err(e) => {
            tracing::error!("session setup failed: {e:#}");
            EXIT_SETUP
        }
    }
}

async fn run_s2(session: SessionArgs, s2: S2Args) -> i32 {
    let cfg = match SessionConfig::from_args(&session, "s2_saturation") {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("setup failed: {e:#}");
            return EXIT_SETUP;
        }
    };
    let runner = S2Runner { args: s2 };

    match orchestrator::run_session(&cfg, &runner).await {
        Ok(outcome) => {
            tracing::info!(
                manifest = %outcome.manifest_path.display(),
                failed_trials = outcome.failed_trials,
                interrupted = outcome.interrupted,
                "session finished"
            );
            if outcome.interrupted {
                EXIT_INTERRUPTED
            } else if outcome.failed_trials > 0 {
                EXIT_TRIAL_FAILED
            } else {
                EXIT_OK
            }
        }
        Err(e) => {
            tracing::error!("session setup failed: {e:#}");
            EXIT_SETUP
        }
    }
}
