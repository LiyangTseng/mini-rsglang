//! End-to-end tests of `rsg-bench s1` against two `bench-stub` arms: the
//! Task 1 tracer (alternation + manifest schema) and Task 3's BENCH-08
//! wiring (identical gc-hook env, memory/GC observation per window).

#[allow(dead_code)]
mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use rsg_bench::gclog::GcRow;
use rsg_bench::manifest::{TrialStatus, read_manifest};
use rsg_bench::metrics::decode_histogram;
use rsg_bench::roles::{Group, Role};

use common::{
    bench_bin, free_port, repo_root, stub_bin, unique_manifest_path, unique_work_root,
    wait_for_event,
};

/// Runs `rsg-bench s1` with `args` appended to a common prefix (binary
/// path only -- callers supply every flag), returning the process
/// `Output`.
fn run_rsg_bench(args: &[String]) -> std::process::Output {
    Command::new(bench_bin())
        .args(args)
        .output()
        .expect("spawn rsg-bench")
}

/// The single subdirectory of `dir` (one session dir per `work_root`).
fn only_subdir(dir: &Path) -> PathBuf {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("read work_root")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    assert_eq!(
        entries.len(),
        1,
        "expected exactly one session dir in {dir:?}: {entries:?}"
    );
    entries.remove(0)
}

#[test]
fn s1_session_end_to_end_with_stub_arms() {
    let port = free_port();
    let work_root = unique_work_root();
    let out = unique_manifest_path();

    // A wide TTFT gap (not just the plan prose's illustrative 20ms/10ms)
    // so the per-arm routing sanity check is never flaky under CI/system
    // load: this suite spawns many real subprocesses concurrently, and
    // with only ~4 agents x 2s of samples, scheduling jitter of a few ms
    // would otherwise occasionally overwhelm a narrow margin.
    let python_cmd = format!("{} --port {{port}} --ttft-ms 150 --itl-ms 5", stub_bin());
    let rust_cmd = format!("{} --port {{port}} --ttft-ms 5 --itl-ms 5", stub_bin());

    let args: Vec<String> = vec![
        "s1".to_string(),
        "--python-cmd".to_string(),
        python_cmd,
        "--rust-cmd".to_string(),
        rust_cmd,
        "--runs".to_string(),
        "1".to_string(),
        "--gc-hook".to_string(),
        "off".to_string(),
        "--backend-kind".to_string(),
        "stub".to_string(),
        "--rust-frontend-process-name".to_string(),
        "bench-stub".to_string(),
        "--agents".to_string(),
        "4".to_string(),
        "--duration-s".to_string(),
        "2".to_string(),
        "--max-tokens".to_string(),
        "16".to_string(),
        "--warmup-requests".to_string(),
        "1".to_string(),
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
        "rsg-bench s1 exited {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let manifest = read_manifest(&out).expect("parse manifest");
    assert_eq!(manifest.schema_version, 1);
    assert_eq!(manifest.generated_by, "crates/rsg-bench");
    assert_eq!(manifest.session.schedule, vec!["python-default", "rust"]);
    assert_eq!(manifest.trials.len(), 2);

    let mut p99_by_arm = std::collections::BTreeMap::new();
    for trial in &manifest.trials {
        assert_eq!(
            trial.status,
            TrialStatus::Ok,
            "trial {} failed: {:?}",
            trial.arm,
            trial.error
        );

        let sent = trial
            .result
            .get("counts")
            .and_then(|c| c.get("sent"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        assert!(sent > 0, "trial {} sent no requests", trial.arm);

        let p99 = trial
            .result
            .get("latency")
            .and_then(|l| l.get("ttft_ms"))
            .and_then(|t| t.get("p99"))
            .and_then(|v| v.as_f64());
        assert!(p99.is_some(), "trial {} has no ttft_ms.p99", trial.arm);

        let encoded = trial.histograms.get("s1").expect("s1 histogram present");
        let decoded = decode_histogram(&encoded.ttft_us).expect("decode ttft histogram");
        assert!(
            !decoded.is_empty(),
            "trial {} ttft histogram is empty",
            trial.arm
        );

        p99_by_arm.insert(trial.arm.clone(), p99.unwrap());
    }

    assert!(
        p99_by_arm["rust"] < p99_by_arm["python-default"],
        "expected rust p99 ({}) < python-default p99 ({})",
        p99_by_arm["rust"],
        p99_by_arm["python-default"]
    );
}

/// Shared two-stub-arm session args, for the BENCH-08 wiring tests.
fn stub_session_args(work_root: &Path, out: &Path, port: u16, gc_hook: &str) -> Vec<String> {
    let python_cmd = format!("{} --port {{port}} --ttft-ms 5 --itl-ms 1", stub_bin());
    let rust_cmd = format!("{} --port {{port}} --ttft-ms 5 --itl-ms 1", stub_bin());
    vec![
        "s1".to_string(),
        "--python-cmd".to_string(),
        python_cmd,
        "--rust-cmd".to_string(),
        rust_cmd,
        "--runs".to_string(),
        "1".to_string(),
        "--gc-hook".to_string(),
        gc_hook.to_string(),
        "--hook-interval-s".to_string(),
        "1.0".to_string(),
        "--backend-kind".to_string(),
        "stub".to_string(),
        "--rust-frontend-process-name".to_string(),
        "bench-stub".to_string(),
        "--agents".to_string(),
        "2".to_string(),
        "--duration-s".to_string(),
        "1".to_string(),
        "--max-tokens".to_string(),
        "8".to_string(),
        "--warmup-requests".to_string(),
        "0".to_string(),
        "--teardown-grace-s".to_string(),
        "5".to_string(),
        "--mem-interval-ms".to_string(),
        "100".to_string(),
        "--python".to_string(),
        repo_root()
            .join(".venv")
            .join("bin")
            .join("python")
            .to_string_lossy()
            .into_owned(),
        "--port".to_string(),
        port.to_string(),
        "--work-root".to_string(),
        work_root.to_string_lossy().into_owned(),
        "--repo-root".to_string(),
        repo_root().to_string_lossy().into_owned(),
        "--out".to_string(),
        out.to_string_lossy().into_owned(),
    ]
}

#[test]
fn gc_hook_env_identical_across_arms() {
    let port = free_port();
    let work_root = unique_work_root();
    let out = unique_manifest_path();
    let args = stub_session_args(&work_root, &out, port, "on");

    let output = run_rsg_bench(&args);
    assert!(
        output.status.success(),
        "rsg-bench s1 exited {:?}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let session_dir = only_subdir(&work_root);
    let shim_file = session_dir.join("shim").join("sitecustomize.py");
    assert!(shim_file.is_file(), "{shim_file:?} should exist");

    let shim_dir_str = session_dir.join("shim").to_string_lossy().into_owned();

    for trial_dir_name in ["trial-00-python-default", "trial-01-rust"] {
        let log = session_dir.join(trial_dir_name).join("server.log");
        let ev = wait_for_event(&log, |e| e.kind == "env", Duration::from_secs(10));
        assert_eq!(
            ev.fields.get("profile_mode").map(String::as_str),
            Some("gc_only")
        );
        let profile_dir = ev.fields.get("profile_dir").expect("profile_dir field");
        assert!(
            profile_dir.ends_with(&format!("{trial_dir_name}/hook")),
            "profile_dir {profile_dir:?} should end with {trial_dir_name}/hook"
        );
        assert_eq!(ev.fields.get("interval").map(String::as_str), Some("1.0"));
        let pythonpath = ev.fields.get("pythonpath").expect("pythonpath field");
        let first_entry = pythonpath.split(':').next().unwrap_or("");
        assert_eq!(first_entry, shim_dir_str);
    }
}

#[test]
fn gc_hook_off_strips_env() {
    let port = free_port();
    let work_root = unique_work_root();
    let out = unique_manifest_path();
    let args = stub_session_args(&work_root, &out, port, "off");

    let output = Command::new(bench_bin())
        .args(&args)
        .env("RSGLANG_PROFILE_MODE", "full")
        .output()
        .expect("spawn rsg-bench");
    assert!(
        output.status.success(),
        "rsg-bench s1 exited {:?}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let session_dir = only_subdir(&work_root);
    for trial_dir_name in ["trial-00-python-default", "trial-01-rust"] {
        let log = session_dir.join(trial_dir_name).join("server.log");
        let ev = wait_for_event(&log, |e| e.kind == "env", Duration::from_secs(10));
        assert_eq!(
            ev.fields.get("profile_mode").map(String::as_str),
            Some("unset")
        );
        assert_eq!(
            ev.fields.get("profile_dir").map(String::as_str),
            Some("unset")
        );
    }

    let manifest = read_manifest(&out).expect("parse manifest");
    for trial in &manifest.trials {
        assert_eq!(
            trial.status,
            TrialStatus::Ok,
            "trial {} failed: {:?}",
            trial.arm,
            trial.error
        );
        for window in &trial.windows {
            assert_eq!(window.gc_status, "disabled");
        }
    }
}

#[test]
fn windows_carry_memory_and_gc() {
    let port = free_port();
    let work_root = unique_work_root();
    let out = unique_manifest_path();
    let args = stub_session_args(&work_root, &out, port, "on");

    let output = run_rsg_bench(&args);
    assert!(
        output.status.success(),
        "rsg-bench s1 exited {:?}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let manifest = read_manifest(&out).expect("parse manifest");
    assert_eq!(manifest.trials.len(), 2);

    for trial in &manifest.trials {
        assert_eq!(
            trial.status,
            TrialStatus::Ok,
            "trial {} failed: {:?}",
            trial.arm,
            trial.error
        );
        assert!(
            !trial.roles.is_empty(),
            "trial {} has no classified pids",
            trial.arm
        );

        let window = trial.windows.first().expect("one s1 window");
        let frontend = window
            .memory
            .get(&Group::Frontend)
            .expect("frontend memory group");
        assert!(
            frontend.rss_bytes.max.unwrap_or(0) > 0,
            "trial {} frontend rss max should be > 0",
            trial.arm
        );
        assert!(
            window.tree_memory.rss_bytes.max.unwrap_or(0) > 0,
            "trial {} tree rss max should be > 0",
            trial.arm
        );

        if trial.arm == "rust" {
            let gc = window.gc.as_ref().expect("rust arm has a gc map");
            match gc.get(&Role::RustFrontend) {
                Some(GcRow::NotApplicable { .. }) => {}
                other => panic!("expected RustFrontend -> NotApplicable, got {other:?}"),
            }
        }
        if trial.arm == "python-default" {
            assert_eq!(window.gc_status, "no_hook_records");
        }
    }

    assert!(
        manifest
            .warnings
            .iter()
            .any(|w| w.contains("no hook records from python arm")),
        "warnings should name the python-kind trial with no hook records: {:?}",
        manifest.warnings
    );
}
