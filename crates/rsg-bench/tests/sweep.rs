//! D-09 `--num-tokenizer` sweep pre-pass tests: `pick_best`'s tie-break and
//! failure rules, `validate_candidates`'s edge rules, and an end-to-end
//! `sweep-num-tokenizer` session against two stub Python arms.

#[allow(dead_code)]
mod common;

use std::process::Command;

use rsg_bench::manifest::read_manifest;
use rsg_bench::scenarios::sweep::{pick_best, validate_candidates};

use common::{bench_bin, free_port, repo_root, stub_bin, unique_manifest_path, unique_work_root};

#[test]
fn pick_best_ties_to_smallest() {
    let results = [(0u32, Some(50.0)), (2, Some(55.0)), (4, Some(55.0))];
    assert_eq!(pick_best(&results).expect("a best candidate"), 2);
}

#[test]
fn pick_best_ignores_failed() {
    let results = [(0u32, None), (1, Some(10.0))];
    assert_eq!(pick_best(&results).expect("a best candidate"), 1);
}

#[test]
fn pick_best_all_failed_errors() {
    let results: [(u32, Option<f64>); 2] = [(0, None), (2, None)];
    assert!(pick_best(&results).is_err());
}

#[test]
fn validate_candidates_rules() {
    assert!(
        validate_candidates(&[]).is_err(),
        "empty list should be rejected"
    );
    assert!(
        validate_candidates(&[2, 0, 2]).is_err(),
        "duplicate candidate should be rejected"
    );
    assert_eq!(
        validate_candidates(&[4, 0, 1]).expect("valid candidates"),
        vec![0, 1, 4],
        "candidates should sort ascending"
    );
}

#[test]
fn sweep_end_to_end_prints_best() {
    let port = free_port();
    let work_root = unique_work_root();
    let out = unique_manifest_path();

    // itl = "1" + "{num_tokenizer}": 10ms for K=0 (faster), 14ms for K=4
    // (slower), so K=0 genuinely achieves the higher peak RPS.
    let python_cmd = format!(
        "{} --port {{port}} --ttft-ms 5 --itl-ms 1{{num_tokenizer}}",
        stub_bin()
    );

    let args: Vec<String> = vec![
        "sweep-num-tokenizer".to_string(),
        "--candidates".to_string(),
        "4,0".to_string(),
        "--mode".to_string(),
        "closed".to_string(),
        "--concurrency".to_string(),
        "2".to_string(),
        "--requests-per-level".to_string(),
        "20".to_string(),
        "--max-tokens".to_string(),
        "8".to_string(),
        "--python-cmd".to_string(),
        python_cmd,
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

    let output = Command::new(bench_bin())
        .args(&args)
        .output()
        .expect("spawn rsg-bench");
    assert!(
        output.status.success(),
        "sweep-num-tokenizer exited {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let last_line = stdout.lines().last().unwrap_or("");
    assert_eq!(last_line, "best_num_tokenizer=0", "full stdout:\n{stdout}");

    let manifest = read_manifest(&out).expect("parse manifest");
    assert_eq!(manifest.session.schedule, vec!["python-nt0", "python-nt4"]);
    assert_eq!(manifest.trials.len(), 2);
}
