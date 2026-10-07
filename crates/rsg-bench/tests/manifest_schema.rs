//! Behavior tests for the manifest's D-07 environment snapshot, secret
//! redaction (T-07-12), atomic/symlink-safe writes (T-07-13), and
//! failed-trial recording.

#[allow(dead_code)]
mod common;

use std::path::Path;
use std::process::Command;

use rsg_bench::manifest::{collect_meta, read_manifest, utc_rfc3339, write_json_atomic, BackendKind, TrialStatus};

use common::{bench_bin, free_port, repo_root, stub_bin, unique_manifest_path, unique_work_root};

#[test]
fn manifest_meta_has_d07_fields() {
    let meta = collect_meta(&repo_root(), "python3", "Qwen/Qwen3-0.6B", BackendKind::Stub);

    // created_utc matches YYYY-MM-DDTHH:MM:SSZ.
    assert_eq!(meta.created_utc.len(), 20, "{:?}", meta.created_utc);
    assert!(meta.created_utc.ends_with('Z'));
    assert_eq!(meta.created_utc.as_bytes()[4], b'-');
    assert_eq!(meta.created_utc.as_bytes()[10], b'T');

    assert_eq!(meta.platform, "macos");

    let git_commit = meta.git_commit.expect("git_commit present in a git checkout");
    assert_eq!(git_commit.len(), 40);
    assert!(git_commit.bytes().all(|b| b.is_ascii_hexdigit()));

    let upstream_sha = meta.upstream_sha.expect("upstream_sha present (vendor/UPSTREAM_SHA)");
    let expected = std::fs::read_to_string(repo_root().join("vendor").join("UPSTREAM_SHA"))
        .expect("read vendor/UPSTREAM_SHA")
        .trim()
        .to_string();
    assert_eq!(upstream_sha, expected);

    let rustc = meta.rustc.expect("rustc version present");
    assert!(rustc.starts_with("rustc 1.99"), "{rustc:?}");

    assert!(meta.gpu.is_none(), "no GPU on this Mac dev session");
    assert_eq!(meta.backend_kind, BackendKind::Stub);
}

#[test]
fn utc_rfc3339_known_values() {
    assert_eq!(utc_rfc3339(0), "1970-01-01T00:00:00Z");
    assert_eq!(utc_rfc3339(951_868_799), "2000-02-29T23:59:59Z");
    assert_eq!(utc_rfc3339(1_791_256_634), "2026-10-06T03:17:14Z");
}

#[test]
fn secrets_never_recorded() {
    let port = free_port();
    let work_root = unique_work_root();
    let out = unique_manifest_path();

    // This arm fails: bench-stub rejects --api-key as an unknown flag, so
    // the python trial is recorded `status: failed` -- the manifest is
    // still written, and must still contain no secret.
    let python_cmd = format!(
        "{} --port {{port}} --api-key sekrit-value HF_TOKEN=hf_sekrit123",
        stub_bin()
    );
    let rust_cmd = format!("{} --port {{port}}", stub_bin());

    let output = Command::new(bench_bin())
        .args([
            "s1",
            "--python-cmd",
            &python_cmd,
            "--rust-cmd",
            &rust_cmd,
            "--runs",
            "1",
            "--gc-hook",
            "off",
            "--backend-kind",
            "stub",
            "--agents",
            "1",
            "--duration-s",
            "1",
            "--max-tokens",
            "4",
            "--warmup-requests",
            "0",
            "--teardown-grace-s",
            "5",
            "--port",
            &port.to_string(),
            "--work-root",
            work_root.to_string_lossy().as_ref(),
            "--repo-root",
            repo_root().to_string_lossy().as_ref(),
            "--out",
            out.to_string_lossy().as_ref(),
        ])
        .env("HF_TOKEN", "hf_envsecret")
        .output()
        .expect("spawn rsg-bench");
    let _ = output; // exit code (likely 3, a failed trial) is not the point here

    let raw = std::fs::read_to_string(&out).expect("manifest written even with a failed trial");
    assert!(!raw.contains("sekrit-value"), "raw manifest leaked the --api-key value");
    assert!(!raw.contains("hf_sekrit123"), "raw manifest leaked the HF_TOKEN=... argv value");
    assert!(!raw.contains("hf_envsecret"), "raw manifest leaked the harness's own HF_TOKEN env var");
    assert!(raw.contains("<redacted>"), "raw manifest should contain the redaction marker");
}

#[test]
fn failed_trial_recorded() {
    let port = free_port();
    let work_root = unique_work_root();
    let out = unique_manifest_path();
    let rust_cmd = format!("{} --port {{port}}", stub_bin());

    let output = Command::new(bench_bin())
        .args([
            "s1",
            "--python-cmd",
            "/usr/bin/false",
            "--rust-cmd",
            &rust_cmd,
            "--runs",
            "1",
            "--gc-hook",
            "off",
            "--backend-kind",
            "stub",
            "--agents",
            "1",
            "--duration-s",
            "1",
            "--max-tokens",
            "4",
            "--warmup-requests",
            "0",
            "--teardown-grace-s",
            "5",
            "--port",
            &port.to_string(),
            "--work-root",
            work_root.to_string_lossy().as_ref(),
            "--repo-root",
            repo_root().to_string_lossy().as_ref(),
            "--out",
            out.to_string_lossy().as_ref(),
        ])
        .output()
        .expect("spawn rsg-bench");

    assert_eq!(output.status.code(), Some(3), "a failed trial should give EXIT_TRIAL_FAILED (3)");

    let manifest = read_manifest(&out).expect("parse manifest");
    assert_eq!(manifest.trials.len(), 2);
    let python_trial = manifest
        .trials
        .iter()
        .find(|t| t.arm == "python-default")
        .expect("python-default trial present");
    assert_eq!(python_trial.status, TrialStatus::Failed);
    assert!(
        !python_trial.error.as_deref().unwrap_or("").is_empty(),
        "failed trial should carry a non-empty error"
    );
}

#[test]
fn write_json_atomic_refuses_symlink() {
    let dir = unique_work_root();
    std::fs::create_dir_all(&dir).expect("create dir");
    let target = dir.join("real.json");
    std::fs::write(&target, b"{\"original\":true}").expect("write original file");
    let link = dir.join("link.json");
    symlink(&target, &link).expect("create symlink");

    let err = write_json_atomic(&link, &serde_json::json!({"should_not_land": true}));
    assert!(err.is_err(), "write_json_atomic must refuse a symlink target");

    let contents = std::fs::read_to_string(&target).expect("read target through the symlink");
    assert_eq!(contents, "{\"original\":true}", "the symlink's destination file must be unchanged");
}

#[cfg(unix)]
fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}
