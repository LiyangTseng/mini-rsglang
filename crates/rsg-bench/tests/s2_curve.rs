//! End-to-end and unit-level tests for `rsg-bench s2` (BENCH-04): the
//! open/closed-loop RPS-vs-latency saturation curve, per-level observation
//! windows, and the `validate_levels`/`build_curve` edge rules.

#[allow(dead_code)]
mod common;

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use rsg_bench::manifest::{TrialStatus, read_manifest};
use rsg_bench::metrics::LatencyHistograms;
use rsg_bench::scenarios::s2_saturation::{
    CurvePoint, LoopMode, build_curve, peak_rps, validate_levels,
};

use common::{bench_bin, free_port, repo_root, stub_bin, unique_manifest_path, unique_work_root};

fn run_rsg_bench(args: &[String]) -> std::process::Output {
    Command::new(bench_bin())
        .args(args)
        .output()
        .expect("spawn rsg-bench")
}

/// A common `rsg-bench s2` invocation against two `bench-stub` arms, with
/// `mode_args` appended for the mode-specific level flags.
fn s2_args(
    mode: &str,
    mode_args: &[(&str, &str)],
    work_root: &Path,
    out: &Path,
    port: u16,
) -> Vec<String> {
    let python_cmd = format!("{} --port {{port}} --ttft-ms 20 --itl-ms 1", stub_bin());
    let rust_cmd = format!("{} --port {{port}} --ttft-ms 10 --itl-ms 1", stub_bin());
    let mut args: Vec<String> = vec![
        "s2".to_string(),
        "--mode".to_string(),
        mode.to_string(),
        "--python-cmd".to_string(),
        python_cmd,
        "--rust-cmd".to_string(),
        rust_cmd,
        "--requests-per-level".to_string(),
        "30".to_string(),
        "--max-tokens".to_string(),
        "4".to_string(),
        "--level-pause-ms".to_string(),
        "100".to_string(),
        "--runs".to_string(),
        "1".to_string(),
        "--gc-hook".to_string(),
        "off".to_string(),
        "--backend-kind".to_string(),
        "stub".to_string(),
        "--rust-frontend-process-name".to_string(),
        "bench-stub".to_string(),
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
    for (k, v) in mode_args {
        args.push((*k).to_string());
        args.push((*v).to_string());
    }
    args
}

#[test]
fn s2_open_loop_curve_with_stub_arms() {
    let port = free_port();
    let work_root = unique_work_root();
    let out = unique_manifest_path();

    // Deliberately unsorted: validate_levels must sort ascending.
    let args = s2_args("open", &[("--rates", "40,20")], &work_root, &out, port);

    let output = run_rsg_bench(&args);
    assert!(
        output.status.success(),
        "rsg-bench s2 exited {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
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

        let curve = trial
            .result
            .get("curve")
            .and_then(|c| c.as_array())
            .unwrap_or_else(|| panic!("trial {} has no curve array", trial.arm));
        let labels: Vec<&str> = curve
            .iter()
            .map(|p| p["label"].as_str().expect("label"))
            .collect();
        assert_eq!(
            labels,
            vec!["rate=20", "rate=40"],
            "trial {} curve labels not ascending",
            trial.arm
        );

        for point in curve {
            let completed = point["counts"]["completed"]
                .as_u64()
                .expect("counts.completed");
            assert_eq!(
                completed, 30,
                "trial {} point {:?} completed != 30",
                trial.arm, point["label"]
            );
            assert!(
                point["latency"]["ttft_ms"]["p99"].is_number(),
                "trial {} point {:?} missing ttft_ms.p99",
                trial.arm,
                point["label"]
            );
        }

        assert!(
            trial
                .result
                .get("peak_rps")
                .and_then(|v| v.as_f64())
                .is_some(),
            "trial {} result.peak_rps missing",
            trial.arm
        );

        let hist_keys: BTreeSet<&str> = trial.histograms.keys().map(String::as_str).collect();
        let expected_keys: BTreeSet<&str> = ["rate=20", "rate=40"].into_iter().collect();
        assert_eq!(
            hist_keys, expected_keys,
            "trial {} histogram keys mismatch",
            trial.arm
        );

        let window_labels: Vec<&str> = trial.windows.iter().map(|w| w.label.as_str()).collect();
        assert_eq!(
            window_labels,
            vec!["rate=20", "rate=40"],
            "trial {} window labels mismatch",
            trial.arm
        );
    }
}

#[test]
fn closed_mode_curve() {
    let port = free_port();
    let work_root = unique_work_root();
    let out = unique_manifest_path();

    // Deliberately unsorted: should yield ["concurrency=1", "concurrency=4"].
    let args = s2_args(
        "closed",
        &[("--concurrency", "4,1")],
        &work_root,
        &out,
        port,
    );

    let output = run_rsg_bench(&args);
    assert!(
        output.status.success(),
        "rsg-bench s2 exited {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
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
        let curve = trial
            .result
            .get("curve")
            .and_then(|c| c.as_array())
            .expect("curve array");
        let labels: Vec<&str> = curve
            .iter()
            .map(|p| p["label"].as_str().expect("label"))
            .collect();
        assert_eq!(
            labels,
            vec!["concurrency=1", "concurrency=4"],
            "trial {} curve labels mismatch",
            trial.arm
        );
    }
}

#[test]
fn validate_levels_rules() {
    assert!(
        validate_levels(&[]).is_err(),
        "empty list should be rejected"
    );
    assert!(
        validate_levels(&[0.0]).is_err(),
        "zero level should be rejected"
    );
    assert!(
        validate_levels(&[-1.0]).is_err(),
        "negative level should be rejected"
    );
    assert!(
        validate_levels(&[f64::NAN]).is_err(),
        "NaN level should be rejected"
    );
    assert!(
        validate_levels(&[1.0, 1.0]).is_err(),
        "duplicate levels should be rejected"
    );
    assert_eq!(
        validate_levels(&[5.0, 1.0, 3.0]).expect("valid levels"),
        vec![1.0, 3.0, 5.0],
        "levels should sort ascending"
    );
}

fn curve_point(label: &str, offered: f64, achieved_rps: Option<f64>) -> CurvePoint {
    CurvePoint {
        label: label.to_string(),
        offered,
        mode: LoopMode::Open,
        counts: Default::default(),
        achieved_rps,
        latency: LatencyHistograms::new().summary(),
    }
}

#[test]
fn build_curve_sorted_stable() {
    let points = vec![
        curve_point("b", 20.0, Some(10.0)),
        curve_point("a", 5.0, Some(50.0)),
        curve_point("c", 20.0, Some(30.0)),
    ];
    let sorted = build_curve(points);
    let labels: Vec<&str> = sorted.iter().map(|p| p.label.as_str()).collect();
    // Ascending by offered; stable among ties (b before c, as in the input).
    assert_eq!(labels, vec!["a", "b", "c"]);
    assert_eq!(peak_rps(&sorted), Some(50.0));
}

#[test]
fn peak_rps_ignores_none() {
    let points = vec![curve_point("a", 1.0, None), curve_point("b", 2.0, None)];
    assert_eq!(peak_rps(&points), None);
}
