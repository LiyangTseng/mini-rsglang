//! D-04 third-party cross-check tests: `parse_tool_result`'s key-tolerant
//! extraction and defensive rejection rules (T-07-17), plus an end-to-end
//! `crosscheck` session against a fake bench tool script.

#[allow(dead_code)]
mod common;

use std::path::Path;
use std::process::Command;

use rsg_bench::manifest::{TrialStatus, read_manifest};
use rsg_bench::scenarios::crosscheck::{Tool, parse_tool_result};

use common::{bench_bin, free_port, repo_root, stub_bin, unique_manifest_path, unique_work_root};

const VLLM_FIXTURE: &str = include_str!("fixtures/vllm_result.json");
const SGLANG_FIXTURE: &str = include_str!("fixtures/sglang_result.jsonl");

fn fixtures_dir() -> String {
    format!("{}/tests/fixtures", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn parse_vllm_fixture() {
    let result = parse_tool_result(Tool::Vllm, VLLM_FIXTURE).expect("parse vllm fixture");
    assert_eq!(result.tool, Tool::Vllm);
    assert_eq!(result.metrics.get("mean_ttft_ms"), Some(&45.2));
    assert_eq!(result.metrics.get("median_ttft_ms"), Some(&40.1));
    assert_eq!(result.metrics.get("p99_ttft_ms"), Some(&120.3));
    assert_eq!(result.metrics.get("mean_itl_ms"), Some(&8.4));
    assert_eq!(result.metrics.get("p99_itl_ms"), Some(&22.7));
    assert_eq!(result.metrics.get("mean_e2el_ms"), Some(&980.6));
    assert_eq!(result.metrics.get("p99_e2el_ms"), Some(&1500.9));
    assert_eq!(result.metrics.get("request_throughput"), Some(&42.7));
    assert_eq!(result.metrics.get("output_throughput"), Some(&1366.4));
    assert_eq!(result.metrics.get("completed"), Some(&512.0));
    assert!(
        !result.metrics.contains_key("backend"),
        "non-numeric fields must be absent"
    );
    assert!(
        !result.metrics.contains_key("duration"),
        "unrelated numeric fields must be absent"
    );
    assert_eq!(result.metrics.len(), 10, "metrics: {:?}", result.metrics);
}

#[test]
fn parse_sglang_jsonl_last_line() {
    let result = parse_tool_result(Tool::Sglang, SGLANG_FIXTURE).expect("parse sglang fixture");
    assert_eq!(result.metrics.get("mean_ttft_ms"), Some(&38.9));
    assert_eq!(result.metrics.get("p99_ttft_ms"), Some(&110.2));
    assert_eq!(result.metrics.get("median_e2e_latency_ms"), Some(&845.3));
    assert_eq!(result.metrics.get("request_throughput"), Some(&39.4));
    assert!(!result.metrics.contains_key("backend"));
    assert!(
        !result.metrics.contains_key("completed"),
        "only the last line should be parsed"
    );
    assert!(
        !result.metrics.contains_key("total"),
        "only the last line should be parsed"
    );
    assert_eq!(result.metrics.len(), 4, "metrics: {:?}", result.metrics);
}

#[test]
fn parse_rejects_no_ttft() {
    let err = parse_tool_result(Tool::Vllm, r#"{"request_throughput": 5.0}"#).unwrap_err();
    assert!(
        err.to_string().contains("TTFT"),
        "error should mention TTFT: {err}"
    );
}

#[test]
fn parse_rejects_non_object() {
    assert!(
        parse_tool_result(Tool::Vllm, "[1,2]").is_err(),
        "a JSON array should be rejected"
    );
    assert!(
        parse_tool_result(Tool::Vllm, "not json").is_err(),
        "non-JSON text should be rejected"
    );
}

fn run_rsg_bench(args: &[String]) -> std::process::Output {
    Command::new(bench_bin())
        .args(args)
        .output()
        .expect("spawn rsg-bench")
}

/// A single-arm `crosscheck` session (python-default only, to avoid
/// spawning the tool twice per test).
fn base_cross_args(extra: &[String], work_root: &Path, out: &Path, port: u16) -> Vec<String> {
    let python_cmd = format!("{} --port {{port}} --ttft-ms 5 --itl-ms 1", stub_bin());
    let rust_cmd = format!("{} --port {{port}} --ttft-ms 5 --itl-ms 1", stub_bin());
    let mut args: Vec<String> = vec![
        "crosscheck".to_string(),
        "--python-cmd".to_string(),
        python_cmd,
        "--rust-cmd".to_string(),
        rust_cmd,
        "--runs".to_string(),
        "1".to_string(),
        "--arms".to_string(),
        "python-default".to_string(),
        "--gc-hook".to_string(),
        "off".to_string(),
        "--backend-kind".to_string(),
        "stub".to_string(),
        "--rust-frontend-process-name".to_string(),
        "bench-stub".to_string(),
        "--warmup-requests".to_string(),
        "0".to_string(),
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
    args.extend(extra.iter().cloned());
    args
}

#[test]
fn crosscheck_session_with_fake_tool() {
    let port = free_port();
    let work_root = unique_work_root();
    let out = unique_manifest_path();
    let fixtures = fixtures_dir();
    let tool_cmd = format!(
        "sh {fixtures}/fake_bench_tool.sh {{out_dir}} {{out_file}} {fixtures}/vllm_result.json"
    );
    let args = base_cross_args(
        &[
            "--tool".to_string(),
            "vllm".to_string(),
            "--tool-cmd".to_string(),
            tool_cmd,
        ],
        &work_root,
        &out,
        port,
    );

    let output = run_rsg_bench(&args);
    assert!(
        output.status.success(),
        "crosscheck exited {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let manifest = read_manifest(&out).expect("parse manifest");
    assert_eq!(manifest.trials.len(), 1);
    for trial in &manifest.trials {
        assert_eq!(
            trial.status,
            TrialStatus::Ok,
            "trial {} failed: {:?}",
            trial.arm,
            trial.error
        );
        let mean_ttft = trial
            .result
            .get("metrics")
            .and_then(|m| m.get("mean_ttft_ms"))
            .and_then(serde_json::Value::as_f64);
        assert_eq!(mean_ttft, Some(45.2), "trial result: {:?}", trial.result);
    }
}

#[test]
fn crosscheck_requires_tool_path() {
    let port = free_port();
    let work_root = unique_work_root();
    let out = unique_manifest_path();
    let args = base_cross_args(
        &["--tool".to_string(), "vllm".to_string()],
        &work_root,
        &out,
        port,
    );

    let output = run_rsg_bench(&args);
    assert_eq!(
        output.status.code(),
        Some(2),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn crosscheck_tool_failure_recorded() {
    let port = free_port();
    let work_root = unique_work_root();
    let out = unique_manifest_path();
    let args = base_cross_args(
        &[
            "--tool".to_string(),
            "vllm".to_string(),
            "--tool-cmd".to_string(),
            "/usr/bin/false".to_string(),
        ],
        &work_root,
        &out,
        port,
    );

    let output = run_rsg_bench(&args);
    assert_eq!(
        output.status.code(),
        Some(3),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let manifest = read_manifest(&out).expect("parse manifest");
    assert_eq!(manifest.trials.len(), 1);
    assert_eq!(manifest.trials[0].status, TrialStatus::Failed);
}
