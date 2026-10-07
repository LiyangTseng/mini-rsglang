//! End-to-end tests for `rsg-bench s3` (BENCH-05): a hyperfine-backed,
//! runner-managed trial in the shared A/B orchestrator (D-08), plus the
//! orchestrator's generic `RunnerManaged` lifecycle branch.

#[allow(dead_code)]
mod common;

use std::process::Command;

use rsg_bench::manifest::{BackendKind, TrialStatus, read_manifest};
use rsg_bench::orchestrator::{
    GcHook, Lifecycle, MeasuredWindow, SessionArgs, SessionConfig, TrialContext, TrialMeasurement, TrialRunner,
    run_session,
};

use common::{bench_bin, free_port, repo_root, stub_bin, unique_manifest_path, unique_work_root};

fn run_rsg_bench(args: &[String]) -> std::process::Output {
    Command::new(bench_bin()).args(args).output().expect("spawn rsg-bench")
}

/// `rsg-bench s3` against two `bench-stub` arms, with hyperfine itself
/// wrapping `coldstart-once`/`coldstart-stop`. The python arm's marker
/// fires at 100ms while readiness lands at ~300ms (frontend_tail_s ~0.2s);
/// the rust arm's marker also fires at 100ms with readiness at ~200ms
/// (frontend_tail_s ~0.1s).
#[test]
#[ignore = "needs hyperfine 1.20.0 on PATH"]
fn s3_session_with_hyperfine_and_stub_arms() {
    if Command::new("hyperfine").arg("--version").output().is_err() {
        panic!("install hyperfine 1.20.0 (cargo install hyperfine --version 1.20.0 --locked) before running this test");
    }

    let port = free_port();
    let work_root = unique_work_root();
    let out = unique_manifest_path();

    let python_cmd = format!(
        "{} --port {{port}} --ready-delay-ms 300 --marker-after-ms 100",
        stub_bin()
    );
    let rust_cmd = format!(
        "{} --port {{port}} --ready-delay-ms 200 --marker-after-ms 100",
        stub_bin()
    );

    let args: Vec<String> = vec![
        "s3".to_string(),
        "--python-cmd".to_string(),
        python_cmd,
        "--rust-cmd".to_string(),
        rust_cmd,
        "--hyperfine-runs".to_string(),
        "2".to_string(),
        "--hyperfine-warmup".to_string(),
        "1".to_string(),
        "--python-backend-ready-marker".to_string(),
        "stub backend ready".to_string(),
        "--rust-backend-ready-marker".to_string(),
        "stub backend ready".to_string(),
        "--runs".to_string(),
        "1".to_string(),
        "--gc-hook".to_string(),
        "off".to_string(),
        "--backend-kind".to_string(),
        "stub".to_string(),
        "--rust-frontend-process-name".to_string(),
        "bench-stub".to_string(),
        "--ready-timeout-s".to_string(),
        "20".to_string(),
        "--teardown-grace-s".to_string(),
        "5".to_string(),
        "--port".to_string(),
        port.to_string(),
        "--work-root".to_string(),
        work_root.to_string_lossy().into_owned(),
        "--repo-root".to_string(),
        repo_root().to_string_lossy().into_owned(),
        "--out".to_string(),
        out.to_string_lossy().into_owned(),
    ];

    let output = run_rsg_bench(&args);
    assert!(
        output.status.success(),
        "rsg-bench s3 exited {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let manifest = read_manifest(&out).expect("parse manifest");
    assert_eq!(manifest.trials.len(), 2);
    assert_eq!(manifest.session.scenario, "s3_coldstart");

    for trial in &manifest.trials {
        assert_eq!(trial.status, TrialStatus::Ok, "trial {} failed: {:?}", trial.arm, trial.error);

        let hyperfine_runs = trial.result["hyperfine"]["runs"].as_u64().expect("hyperfine.runs");
        assert_eq!(hyperfine_runs, 2);

        let runs = trial.result["runs"].as_array().expect("runs array");
        assert_eq!(runs.len(), 2, "trial {} runs: {:?}", trial.arm, runs);
        let run_indices: Vec<u64> = runs.iter().map(|r| r["run"].as_u64().expect("run")).collect();
        assert_eq!(run_indices, vec![1, 2], "warm-up run 0 should be excluded");

        let expected_tail = if trial.arm == "rust" { 0.1 } else { 0.2 };
        for r in runs {
            let tail = r["frontend_tail_s"].as_f64().expect("frontend_tail_s");
            assert!(
                (tail - expected_tail).abs() <= 0.15,
                "trial {} run {:?} frontend_tail_s {tail} not within 0.15 of {expected_tail}",
                trial.arm,
                r["run"]
            );
        }

        let frontend_rss = trial.result["means"]["frontend_rss_bytes"].as_u64();
        assert!(
            frontend_rss.unwrap_or(0) > 0,
            "trial {} means.frontend_rss_bytes missing/zero",
            trial.arm
        );
    }

    rsg_bench::procs::ensure_port_free(port).expect("port free after s3 session");
}

/// A fake [`TrialRunner`] with [`Lifecycle::RunnerManaged`] that asserts
/// the orchestrator rendered `ctx.argv`/built `ctx.env_remove` for it
/// (P1 env parity) without ever launching a server itself -- the fake's
/// own `python_cmd`/`rust_cmd` templates point at a nonexistent binary, so
/// if the orchestrator ever harness-launched this arm the trial would
/// come back `Failed`, not `Ok`.
struct FakeRunnerManaged;

impl TrialRunner for FakeRunnerManaged {
    fn lifecycle(&self) -> Lifecycle {
        Lifecycle::RunnerManaged
    }

    fn workload(&self) -> serde_json::Value {
        serde_json::json!({})
    }

    async fn run_trial(&self, ctx: &TrialContext<'_>) -> anyhow::Result<TrialMeasurement> {
        assert!(!ctx.argv.is_empty(), "ctx.argv should be rendered");
        assert!(
            !ctx.env_remove.is_empty(),
            "ctx.env_remove should mirror harness-managed cfg.gc_hook=off handling (P1)"
        );
        Ok(TrialMeasurement {
            windows: Vec::<MeasuredWindow>::new(),
            result: serde_json::json!({ "fake": true }),
        })
    }
}

#[tokio::test]
async fn runner_managed_skips_harness_launch() {
    let port = free_port();
    let work_root = unique_work_root();
    let out = unique_manifest_path();

    // A template that would fail to launch as a real server (a
    // nonexistent binary) -- if the orchestrator ever harness-launched
    // this arm, the trial would come back Failed, proving the branch was
    // only actually skipped when it instead comes back Ok.
    let session_args = SessionArgs {
        python_cmd: "/nonexistent/bogus-binary --port {port}".to_string(),
        rust_cmd: "/nonexistent/bogus-binary --port {port}".to_string(),
        python_default_num_tokenizer: 0,
        python_best_num_tokenizer: None,
        arms: Some(vec!["python-default".to_string()]),
        runs: 1,
        seed: 1,
        port,
        model_arg: "fake-model".to_string(),
        python: None,
        gc_hook: GcHook::Off,
        hook_interval_s: 1.0,
        warmup_requests: 0,
        ready_timeout_s: 5.0,
        teardown_grace_s: 5.0,
        mem_interval_ms: 1000,
        backend_kind: BackendKind::Stub,
        rust_frontend_process_name: "bench-stub".to_string(),
        work_root: work_root.clone(),
        repo_root: Some(repo_root()),
        out: out.clone(),
    };

    let cfg = SessionConfig::from_args(&session_args, "fake_runner_managed").expect("build SessionConfig");
    let runner = FakeRunnerManaged;

    let outcome = run_session(&cfg, &runner).await.expect("run_session");
    assert_eq!(outcome.failed_trials, 0, "trial should not have been harness-launched");

    let manifest = read_manifest(&outcome.manifest_path).expect("parse manifest");
    assert_eq!(manifest.trials.len(), 1);
    assert_eq!(manifest.trials[0].status, TrialStatus::Ok, "trial: {:?}", manifest.trials[0]);
    assert_eq!(manifest.trials[0].result, serde_json::json!({ "fake": true }));

    rsg_bench::procs::ensure_port_free(port).expect("port never touched by a runner-managed trial");
}
