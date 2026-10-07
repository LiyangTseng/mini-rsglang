//! `hyperfine_argv`/`parse_hyperfine_json` (RESEARCH Pattern 2) and the
//! POSIX shell-quoting round trip they depend on (T-07-19). None of these
//! tests need `hyperfine` installed.

use std::path::Path;
use std::process::Command;

use rsg_bench::cmdline::{shell_join, shell_quote};
use rsg_bench::scenarios::s3_coldstart::{hyperfine_argv, parse_hyperfine_json};

const FIXTURE: &str = include_str!("fixtures/hyperfine_export.json");

/// `serde_json`'s default (non-`float_roundtrip`) float parser is not
/// always bit-exact with Rust's own literal parser for a 17-significant-
/// digit value -- both land within ~1e-12 relative of the true value,
/// which is far finer than any wall-clock timing this type carries, so an
/// epsilon comparison is the correct check here, not bit-exact equality.
fn approx_eq(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn parse_phase2_shape() {
    let stats = parse_hyperfine_json(FIXTURE, "fixture").expect("parse fixture");
    assert!(approx_eq(stats.mean_s, 16.15580571078667), "mean_s: {}", stats.mean_s);
    assert!(
        approx_eq(stats.stddev_s.expect("stddev_s present"), 1.1798471426963029),
        "stddev_s: {:?}",
        stats.stddev_s
    );
    assert!(approx_eq(stats.median_s, 16.244755787120003), "median_s: {}", stats.median_s);
    assert!(approx_eq(stats.min_s, 14.934000985119999), "min_s: {}", stats.min_s);
    assert!(approx_eq(stats.max_s, 17.28866036012), "max_s: {}", stats.max_s);
    let expected_times = [16.244755787120003, 17.28866036012, 14.934000985119999];
    assert_eq!(stats.times_s.len(), expected_times.len(), "times_s: {:?}", stats.times_s);
    for (got, want) in stats.times_s.iter().zip(expected_times.iter()) {
        assert!(approx_eq(*got, *want), "times_s entry {got} != {want}");
    }
    assert_eq!(stats.runs, 3);
}

#[test]
fn parse_missing_results_errors() {
    let err = parse_hyperfine_json("{}", "empty.json").unwrap_err();
    assert!(err.to_string().contains("results"), "error: {err}");

    let err = parse_hyperfine_json(r#"{"results": []}"#, "empty-array.json").unwrap_err();
    assert!(err.to_string().contains("results"), "error: {err}");
}

#[test]
fn parse_null_stddev_ok() {
    let text =
        r#"{"results":[{"command":"x","mean":1.0,"stddev":null,"median":1.0,"min":1.0,"max":1.0,"times":[1.0]}]}"#;
    let stats = parse_hyperfine_json(text, "null-stddev.json").expect("parse");
    assert_eq!(stats.stddev_s, None);
    assert_eq!(stats.runs, 1);
}

#[test]
fn shell_quote_round_trip() {
    let tokens: Vec<String> = vec![
        "plain".to_string(),
        "a b".to_string(),
        "it's".to_string(),
        "$HOME".to_string(),
        ";rm -rf /".to_string(),
        "".to_string(),
        "--flag=v".to_string(),
    ];
    let joined = shell_join(&tokens);

    let output = Command::new("sh")
        .arg("-c")
        .arg(format!("printf '%s\\n' {joined}"))
        .output()
        .expect("spawn sh");
    assert!(
        output.status.success(),
        "sh exited {:?}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    let got: Vec<&str> = std::str::from_utf8(&output.stdout).expect("utf8 stdout").lines().collect();
    assert_eq!(got, tokens.iter().map(String::as_str).collect::<Vec<_>>());

    // shell_quote itself (not just via shell_join) must round-trip the
    // injection token literally, wrapped in single quotes.
    let quoted = shell_quote(";rm -rf /");
    assert!(quoted.starts_with('\'') && quoted.ends_with('\''), "quoted: {quoted}");
}

#[test]
fn hyperfine_argv_shape() {
    let once: Vec<String> = vec!["once-bin".to_string(), "--flag".to_string()];
    let stop: Vec<String> = vec!["stop-bin".to_string(), "--flag".to_string()];
    let export = Path::new("/t/h.json");

    let argv = hyperfine_argv("hyperfine", 3, 1, export, &once, &stop);
    assert_eq!(
        argv,
        vec![
            "hyperfine".to_string(),
            "--runs".to_string(),
            "3".to_string(),
            "--warmup".to_string(),
            "1".to_string(),
            "--export-json".to_string(),
            "/t/h.json".to_string(),
            "--conclude".to_string(),
            shell_join(&stop),
            shell_join(&once),
        ]
    );
}
