//! Behavior tests for arm construction, scheduling, CLI edge cases and
//! template rendering (D-05/D-06/D-09/D-10, T-07-14).

#[allow(dead_code)]
mod common;

use std::collections::BTreeMap;
use std::process::Command;

use rsg_bench::cmdline::render;
use rsg_bench::orchestrator::{build_arms, schedule};

use common::{bench_bin, unique_manifest_path};

#[test]
fn two_arms_alternate_p_r() {
    let arms = build_arms("py", "rs", 0, None, None).expect("build_arms");
    let ids: Vec<&str> = arms.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, vec!["python-default", "rust"]);

    let slots = schedule(arms.len(), 5);
    assert_eq!(slots.len(), 10);
    let scheduled_ids: Vec<&str> = slots.iter().map(|s| arms[s.arm].id.as_str()).collect();
    let expected: Vec<&str> = std::iter::repeat_n(["python-default", "rust"], 5)
        .flatten()
        .collect();
    assert_eq!(scheduled_ids, expected);
}

#[test]
fn three_arms_round_robin() {
    let arms = build_arms("py", "rs", 0, Some(2), None).expect("build_arms");
    let ids: Vec<&str> = arms.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, vec!["python-default", "rust", "python-best"]);

    let slots = schedule(arms.len(), 2);
    let scheduled_ids: Vec<&str> = slots.iter().map(|s| arms[s.arm].id.as_str()).collect();
    assert_eq!(
        scheduled_ids,
        vec![
            "python-default",
            "rust",
            "python-best",
            "python-default",
            "rust",
            "python-best",
        ]
    );
}

#[test]
fn best_equal_to_default_merges() {
    let arms = build_arms("py", "rs", 0, Some(0), None).expect("build_arms");
    assert_eq!(arms.len(), 2);
    let python_default = arms
        .iter()
        .find(|a| a.id == "python-default")
        .expect("python-default arm");
    assert!(python_default.also_best);
}

#[test]
fn select_unknown_arm_errors() {
    let output = Command::new(bench_bin())
        .args([
            "s1",
            "--arms",
            "rust,bogus",
            "--out",
            unique_manifest_path().to_string_lossy().as_ref(),
        ])
        .output()
        .expect("spawn rsg-bench");
    assert!(
        !output.status.success(),
        "expected a non-zero exit for an unknown --arms entry"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("bogus"),
        "stderr should name the unknown arm: {stderr}"
    );
}

#[test]
fn runs_zero_rejected_by_cli() {
    let output = Command::new(bench_bin())
        .args([
            "s1",
            "--runs",
            "0",
            "--out",
            unique_manifest_path().to_string_lossy().as_ref(),
        ])
        .output()
        .expect("spawn rsg-bench");
    assert_eq!(
        output.status.code(),
        Some(2),
        "--runs 0 should be a clap usage error (exit 2)"
    );
}

#[test]
fn render_rules() {
    let mut values: BTreeMap<&str, String> = BTreeMap::new();
    values.insert("port", "1919".to_string());

    // An unknown placeholder is an error naming it.
    let err = render("--model {model}", &values).unwrap_err();
    assert!(err.to_string().contains("model"));

    // `{num_tokenizer}` is only in the values map for python arms; for a
    // rust-style template without it, rendering is the same "unknown
    // placeholder" error (D-10's "a {num_tokenizer} placeholder in
    // --rust-cmd is a render error" falls directly out of this).
    let err = render("--frontend rust --nt {num_tokenizer}", &values).unwrap_err();
    assert!(err.to_string().contains("num_tokenizer"));

    // A value containing spaces and `;` stays one argv token when quoted.
    let out = render(r#"--extra "a b;c""#, &values).unwrap();
    assert_eq!(out, vec!["--extra".to_string(), "a b;c".to_string()]);

    // A quoted template segment is preserved verbatim (no placeholder
    // substitution needed to prove it survives split_template intact).
    let out = render("'--literal {{not a placeholder}}'", &values).unwrap();
    assert_eq!(out, vec!["--literal {not a placeholder}".to_string()]);
}
