//! Task 1 tracer: `rsg-bench throughput` alternates arms through the real
//! 07-03 driver writer, and `rsg-bench report` states the throughput delta,
//! its CI and the regression call-out (D-11, D-12, D-13). Plus
//! `parse_throughput_output`'s defensive-parsing edge tests.

#[allow(dead_code)]
mod common;

use std::path::PathBuf;
use std::process::Command;

use rsg_bench::manifest::read_manifest;
use rsg_bench::scenarios::standard_throughput::{THROUGHPUT_SCHEMA, parse_throughput_output};

use common::{bench_bin, free_port, repo_root, stub_bin, unique_work_root};

/// `RSG_BENCH_PYTHON` override, else `.venv/bin/python` -- the Phase 1 gate
/// precedent (fails, never skips, when no interpreter is found).
fn python_interpreter() -> PathBuf {
    if let Ok(p) = std::env::var("RSG_BENCH_PYTHON") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return path;
        }
    }
    let default = repo_root().join(".venv").join("bin").join("python");
    if default.is_file() {
        return default;
    }
    panic!("no Python interpreter found (set RSG_BENCH_PYTHON, or run scripts/bootstrap_mac_env.sh)");
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_standard_throughput.py")
}

fn run_rsg_bench(args: &[String]) -> std::process::Output {
    Command::new(bench_bin()).args(args).output().expect("spawn rsg-bench")
}

/// `rsg-bench throughput` alternates `python-default`/`rust` through the
/// fixture runner (which goes through 07-03's own `main`/writer, so the
/// output schema is faithful), then `rsg-bench report` must state the
/// Rust-vs-Python throughput delta with its CI, call out the regression in
/// plain words next to the reference-target sentence (D-11), and open with
/// the `NOT A FRONTEND COMPARISON` banner (this session uses
/// `--backend-kind stub`).
#[test]
fn throughput_session_and_report_flag_regression() {
    let port = free_port();
    let work_root = unique_work_root();
    let manifests_dir = unique_work_root();
    std::fs::create_dir_all(&manifests_dir).expect("create manifests dir");
    let out = manifests_dir.join("standard_throughput.manifest.json");

    let python = python_interpreter();
    let throughput_cmd = format!(
        "{} {} --port {{port}} --out {{out}} --seed {{seed}}",
        python.to_string_lossy(),
        fixture_path().to_string_lossy(),
    );
    let python_cmd = format!("{} --port {{port}} --model-id fast", stub_bin());
    let rust_cmd = format!("{} --port {{port}} --model-id slow", stub_bin());

    let args: Vec<String> = vec![
        "throughput".to_string(),
        "--throughput-cmd".to_string(),
        throughput_cmd,
        "--python".to_string(),
        python.to_string_lossy().into_owned(),
        "--python-cmd".to_string(),
        python_cmd,
        "--rust-cmd".to_string(),
        rust_cmd,
        "--runs".to_string(),
        "2".to_string(),
        "--gc-hook".to_string(),
        "off".to_string(),
        "--backend-kind".to_string(),
        "stub".to_string(),
        "--rust-frontend-process-name".to_string(),
        "bench-stub".to_string(),
        "--warmup-requests".to_string(),
        "1".to_string(),
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
        "rsg-bench throughput exited {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let manifest = read_manifest(&out).expect("parse manifest");
    assert_eq!(manifest.trials.len(), 4, "4 trials: 2 runs x 2 arms");
    let schedule: Vec<&str> = manifest.session.schedule.iter().map(String::as_str).collect();
    assert_eq!(schedule, vec!["python-default", "rust", "python-default", "rust"]);
    for t in &manifest.trials {
        assert_eq!(
            t.status,
            rsg_bench::manifest::TrialStatus::Ok,
            "trial {} ({}) failed: {:?}",
            t.index,
            t.arm,
            t.error
        );
    }

    let python_throughputs: Vec<f64> = manifest
        .trials
        .iter()
        .filter(|t| t.arm == "python-default")
        .map(|t| t.result["summary"]["throughput_tok_s"].as_f64().expect("throughput_tok_s"))
        .collect();
    let rust_throughputs: Vec<f64> = manifest
        .trials
        .iter()
        .filter(|t| t.arm == "rust")
        .map(|t| t.result["summary"]["throughput_tok_s"].as_f64().expect("throughput_tok_s"))
        .collect();
    assert_eq!(python_throughputs.len(), 2);
    assert_eq!(rust_throughputs.len(), 2);
    for &r in &rust_throughputs {
        for &p in &python_throughputs {
            assert!(r < p, "rust throughput {r} should be below python throughput {p}");
        }
    }

    let report_json = manifests_dir.join("r.json");
    let report_md = manifests_dir.join("r.md");
    let report_output = run_rsg_bench(&[
        "report".to_string(),
        "--manifests-dir".to_string(),
        manifests_dir.to_string_lossy().into_owned(),
        "--out-json".to_string(),
        report_json.to_string_lossy().into_owned(),
        "--out-md".to_string(),
        report_md.to_string_lossy().into_owned(),
    ]);
    assert!(
        report_output.status.success(),
        "rsg-bench report exited {:?}\nstdout:\n{}\nstderr:\n{}",
        report_output.status.code(),
        String::from_utf8_lossy(&report_output.stdout),
        String::from_utf8_lossy(&report_output.stderr),
    );

    let report_text = std::fs::read_to_string(&report_json).expect("read report json");
    let report: serde_json::Value = serde_json::from_str(&report_text).expect("parse report json");

    let pct = report
        .pointer("/scenarios/standard_throughput/deltas/rust_vs_python-default/throughput_tok_s/pct")
        .and_then(serde_json::Value::as_f64)
        .expect("throughput_tok_s delta pct present");
    assert!(pct < 0.0, "expected a negative (regression) pct delta, got {pct}");
    assert!(
        report
            .pointer("/scenarios/standard_throughput/deltas/rust_vs_python-default/throughput_tok_s/pct_lo")
            .is_some(),
        "pct_lo missing from delta"
    );
    assert!(
        report
            .pointer("/scenarios/standard_throughput/deltas/rust_vs_python-default/throughput_tok_s/pct_hi")
            .is_some(),
        "pct_hi missing from delta"
    );

    let summary = report["summary"].as_array().expect("summary array");
    let summary_first = summary[0].as_str().expect("summary[0] is a string");
    assert!(
        summary_first.starts_with("NOT A FRONTEND COMPARISON"),
        "summary[0] = {summary_first:?}"
    );

    let summary_text = summary.iter().filter_map(serde_json::Value::as_str).collect::<Vec<_>>().join("\n");
    assert!(summary_text.contains("REGRESSION"), "summary: {summary_text}");
    assert!(
        summary_text.contains("\u{b1}2% is a reference target, not a gate"),
        "summary: {summary_text}"
    );

    let md_text = std::fs::read_to_string(&report_md).expect("read report md");
    assert!(md_text.contains("REGRESSION"), "markdown missing REGRESSION:\n{md_text}");
    assert!(
        md_text.contains("\u{b1}2% is a reference target, not a gate"),
        "markdown missing reference-target sentence:\n{md_text}"
    );
    let bullet_lines: Vec<&str> = md_text.lines().filter(|l| l.starts_with("- ")).collect();
    assert!(
        bullet_lines
            .first()
            .map(|l| l.contains("NOT A FRONTEND COMPARISON"))
            .unwrap_or(false),
        "first summary bullet should be the banner: {bullet_lines:?}"
    );

    rsg_bench::procs::ensure_port_free(port).expect("port free after throughput session");
}

fn full_valid_summary() -> serde_json::Value {
    serde_json::json!({
        "num_requests": 1, "num_tokens": 1, "duration_s": 1.0,
        "throughput_tok_s": 1.0, "throughput_req_s": 1.0,
        "ttft_ms": {"avg": 1.0, "p50": 1.0, "p90": 1.0, "p99": 1.0, "max": 1.0},
        "tpot_ms": {"avg": 1.0, "p50": 1.0, "p90": 1.0, "p99": 1.0, "max": 1.0},
        "e2e_s": {"avg": 1.0, "p50": 1.0, "p90": 1.0, "p99": 1.0, "max": 1.0},
    })
}

#[test]
fn parse_throughput_output_rejects_wrong_schema() {
    let text = serde_json::json!({
        "schema": "something/else/1",
        "model": "m",
        "t_start_unix": 1.0,
        "t_end_unix": 2.0,
        "summary": full_valid_summary(),
    })
    .to_string();
    let err = parse_throughput_output(&text).expect_err("wrong schema must be rejected");
    assert!(err.to_string().contains("schema"), "{err}");
}

#[test]
fn parse_throughput_output_rejects_missing_fields() {
    // t_end_unix is missing entirely.
    let text = serde_json::json!({
        "schema": THROUGHPUT_SCHEMA,
        "model": "m",
        "t_start_unix": 1.0,
        "summary": full_valid_summary(),
    })
    .to_string();
    let err = parse_throughput_output(&text).expect_err("missing field must be rejected");
    assert!(err.to_string().contains("t_end_unix"), "{err}");
}
