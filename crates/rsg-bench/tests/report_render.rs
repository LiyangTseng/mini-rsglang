//! Task 2: `report::{build_report, render_markdown}` cover every scenario
//! (S1, the S2 curve, S3, the sweep, and cross-checks), each with GC,
//! memory and co-occurrence tables, plus the edge and provenance rules
//! (BENCH-07, BENCH-08). These tests build `Manifest` values in code and
//! call `build_report`/`render_markdown` directly -- no subprocess, no
//! fixtures on disk.

use std::collections::BTreeMap;

use rsg_bench::gclog::{
    CoOccurrence, CoOccurrenceRow, GcRow, GcStats, PausePercentiles, RUST_FRONTEND_GC_REASON,
};
use rsg_bench::manifest::{
    ArmInfo, BackendKind, Manifest, Meta, SessionInfo, TrialRecord, TrialStatus, WindowObs,
};
use rsg_bench::memory::{GroupMemory, MemSummary};
use rsg_bench::report::{build_report, render_markdown};
use rsg_bench::roles::{FrontendKind, Group, Role};

// --- Fixture builders --------------------------------------------------------

fn meta(backend_kind: BackendKind, gpu: Option<&str>) -> Meta {
    Meta {
        created_utc: "2026-01-01T00:00:00Z".to_string(),
        platform: "linux".to_string(),
        python: Some("Python 3.12.3".to_string()),
        git_commit: Some("a".repeat(40)),
        git_dirty: Some(false),
        upstream_sha: Some("b".repeat(40)),
        model: "Qwen/Qwen3-0.6B".to_string(),
        gpu: gpu.map(str::to_string),
        arch: "x86_64".to_string(),
        os_long: None,
        kernel: None,
        cpu_brand: None,
        cpu_count: None,
        total_memory_bytes: None,
        gpu_driver: None,
        torch_cuda: None,
        rustc: None,
        harness_profile: "debug".to_string(),
        env: BTreeMap::new(),
        backend_kind,
    }
}

fn arm_info(id: &str, kind: FrontendKind, num_tokenizer: Option<u32>, also_best: bool) -> ArmInfo {
    ArmInfo {
        id: id.to_string(),
        kind,
        num_tokenizer,
        also_best,
        argv: vec![id.to_string()],
    }
}

fn mem_summary(max: u64) -> MemSummary {
    MemSummary {
        start: Some(max),
        end: Some(max),
        max: Some(max),
        growth: Some(0),
    }
}

fn group_memory(rss_max: u64, pss_max: Option<u64>) -> GroupMemory {
    GroupMemory {
        rss_bytes: mem_summary(rss_max),
        pss_bytes: pss_max.map(mem_summary),
    }
}

fn gc_stats_row(count: u64, total_pause_ms: f64, p99: f64, max: f64) -> GcRow {
    GcRow::Stats(GcStats {
        count,
        by_generation: BTreeMap::new(),
        total_pause_ms,
        pause_ms: PausePercentiles {
            p50: Some(total_pause_ms / count.max(1) as f64),
            p99: Some(p99),
            max: Some(max),
        },
    })
}

fn gc_na_row() -> GcRow {
    GcRow::NotApplicable {
        reason: RUST_FRONTEND_GC_REASON.to_string(),
    }
}

fn cooc_row(spike_overlap: f64, nonspike_overlap: f64) -> CoOccurrenceRow {
    CoOccurrenceRow::Computed(CoOccurrence {
        p99_ttft_ms: 100.0,
        spike_requests: 1,
        spike_with_gc: 1,
        nonspike_requests: 9,
        nonspike_with_gc: 8,
        spike_overlap_rate: Some(spike_overlap),
        nonspike_overlap_rate: Some(nonspike_overlap),
    })
}

fn cooc_na_row() -> CoOccurrenceRow {
    CoOccurrenceRow::NotApplicable {
        reason: RUST_FRONTEND_GC_REASON.to_string(),
    }
}

fn window(
    label: &str,
    gc: Option<BTreeMap<Role, GcRow>>,
    memory: BTreeMap<Group, GroupMemory>,
    tree_memory: GroupMemory,
    cooccurrence: Option<BTreeMap<Group, CoOccurrenceRow>>,
) -> WindowObs {
    WindowObs {
        label: label.to_string(),
        start_unix_ns: 0,
        end_unix_ns: 1_000_000_000,
        gc_status: if gc.is_some() {
            "collected".to_string()
        } else {
            "disabled".to_string()
        },
        gc,
        memory,
        tree_memory,
        cooccurrence,
    }
}

fn python_gc() -> BTreeMap<Role, GcRow> {
    let mut m = BTreeMap::new();
    m.insert(Role::ApiServer, gc_stats_row(5, 10.0, 2.0, 3.0));
    m.insert(Role::Scheduler, gc_stats_row(2, 4.0, 1.0, 1.5));
    m
}

fn rust_gc() -> BTreeMap<Role, GcRow> {
    let mut m = BTreeMap::new();
    m.insert(Role::RustFrontend, gc_na_row());
    m.insert(Role::Scheduler, gc_stats_row(2, 4.0, 1.0, 1.5));
    m
}

fn python_cooc() -> BTreeMap<Group, CoOccurrenceRow> {
    let mut m = BTreeMap::new();
    m.insert(Group::Frontend, cooc_row(1.0, 0.8));
    m.insert(Group::Scheduler, cooc_row(1.0, 0.1));
    m
}

fn rust_cooc() -> BTreeMap<Group, CoOccurrenceRow> {
    let mut m = BTreeMap::new();
    m.insert(Group::Frontend, cooc_na_row());
    m.insert(Group::Scheduler, cooc_row(1.0, 0.1));
    m
}

fn default_memory_maps(pss: Option<u64>) -> (BTreeMap<Group, GroupMemory>, GroupMemory) {
    let mut by_group = BTreeMap::new();
    by_group.insert(Group::Frontend, group_memory(1_000_000, pss));
    by_group.insert(
        Group::Scheduler,
        group_memory(2_000_000, pss.map(|p| p * 2)),
    );
    let tree = group_memory(3_000_000, pss.map(|p| p * 3));
    (by_group, tree)
}

fn ok_trial(
    index: usize,
    round: u32,
    arm: &str,
    result: serde_json::Value,
    windows: Vec<WindowObs>,
) -> TrialRecord {
    TrialRecord {
        index,
        round,
        arm: arm.to_string(),
        status: TrialStatus::Ok,
        error: None,
        model_id: Some("test-model".to_string()),
        launched_utc: None,
        ready_s: Some(1.0),
        result,
        histograms: BTreeMap::new(),
        windows,
        roles: BTreeMap::new(),
        hook: Default::default(),
        teardown_graceful: Some(true),
        teardown_survivors: Vec::new(),
    }
}

fn failed_trial(index: usize, round: u32, arm: &str, error: &str) -> TrialRecord {
    TrialRecord {
        index,
        round,
        arm: arm.to_string(),
        status: TrialStatus::Failed,
        error: Some(error.to_string()),
        model_id: None,
        launched_utc: None,
        ready_s: None,
        result: serde_json::Value::Null,
        histograms: BTreeMap::new(),
        windows: Vec::new(),
        roles: BTreeMap::new(),
        hook: Default::default(),
        teardown_graceful: None,
        teardown_survivors: Vec::new(),
    }
}

fn manifest(
    scenario: &str,
    backend_kind: BackendKind,
    gpu: Option<&str>,
    arms: Vec<ArmInfo>,
    trials: Vec<TrialRecord>,
    workload: serde_json::Value,
) -> Manifest {
    let schedule = trials.iter().map(|t| t.arm.clone()).collect();
    Manifest {
        schema_version: rsg_bench::manifest::SCHEMA_VERSION,
        generated_by: "test".to_string(),
        meta: meta(backend_kind, gpu),
        session: SessionInfo {
            scenario: scenario.to_string(),
            seed: 1,
            runs: 1,
            schedule,
            arms,
            harness_argv: vec![],
            workload,
        },
        trials,
        warnings: Vec::new(),
        interrupted: false,
    }
}

fn s1_result(ttft_p50: f64, ttft_p99: f64, rps: f64, cancelled: u64) -> serde_json::Value {
    serde_json::json!({
        "counts": {"sent": 10, "completed": 8, "cancelled": cancelled, "failed": 0},
        "latency": {
            "ttft_ms": {"count": 8, "p50": ttft_p50, "p90": ttft_p99, "p99": ttft_p99, "max": ttft_p99},
            "itl_ms": {"count": 8, "p50": 1.0, "p90": 1.0, "p99": 1.0, "max": 1.0},
            "e2e_ms": {"count": 8, "p50": 1.0, "p90": 1.0, "p99": 1.0, "max": 1.0},
        },
        "rps": rps,
        "output_chunks_per_s": 1.0,
    })
}

fn curve_point(
    label: &str,
    offered: f64,
    achieved_rps: f64,
    ttft_p99: f64,
    e2e_p99: f64,
) -> serde_json::Value {
    serde_json::json!({
        "label": label, "offered": offered, "mode": "open",
        "counts": {"sent": 10, "completed": 10, "cancelled": 0, "failed": 0},
        "achieved_rps": achieved_rps,
        "latency": {
            "ttft_ms": {"count": 10, "p50": 1.0, "p90": 1.0, "p99": ttft_p99, "max": ttft_p99},
            "itl_ms": {"count": 10, "p50": 1.0, "p90": 1.0, "p99": 1.0, "max": 1.0},
            "e2e_ms": {"count": 10, "p50": 1.0, "p90": 1.0, "p99": e2e_p99, "max": e2e_p99},
        },
    })
}

fn s2_result(points: Vec<serde_json::Value>, peak: f64) -> serde_json::Value {
    serde_json::json!({"mode": "open", "curve": points, "peak_rps": peak})
}

fn mem_point_json(rss: u64, pss: Option<u64>) -> serde_json::Value {
    serde_json::json!({"rss_bytes": rss, "pss_bytes": pss})
}

#[allow(clippy::too_many_arguments)]
fn s3_run(
    run: u32,
    e2e: f64,
    backend: f64,
    tail: f64,
    frontend_rss: u64,
    frontend_pss: Option<u64>,
    tree_rss: u64,
    tree_pss: Option<u64>,
) -> serde_json::Value {
    serde_json::json!({
        "run": run, "e2e_ready_s": e2e, "backend_ready_s": backend, "frontend_tail_s": tail,
        "memory_at_ready": {
            "groups": {"frontend": mem_point_json(frontend_rss, frontend_pss)},
            "tree": mem_point_json(tree_rss, tree_pss),
        },
        "gc_boot": serde_json::Value::Null,
    })
}

#[allow(clippy::too_many_arguments)]
fn s3_run_with_gc(
    run: u32,
    e2e: f64,
    backend: f64,
    tail: f64,
    frontend_rss: u64,
    frontend_pss: Option<u64>,
    tree_rss: u64,
    tree_pss: Option<u64>,
) -> serde_json::Value {
    let mut gc_boot = BTreeMap::new();
    gc_boot.insert(Role::ApiServer, gc_stats_row(3, 6.0, 2.0, 2.5));
    let gc_boot_json = serde_json::to_value(gc_boot).expect("serialize gc_boot");
    serde_json::json!({
        "run": run, "e2e_ready_s": e2e, "backend_ready_s": backend, "frontend_tail_s": tail,
        "memory_at_ready": {
            "groups": {"frontend": mem_point_json(frontend_rss, frontend_pss)},
            "tree": mem_point_json(tree_rss, tree_pss),
        },
        "gc_boot": gc_boot_json,
    })
}

fn s3_result(hyperfine_mean: f64, runs: Vec<serde_json::Value>) -> serde_json::Value {
    let e2e_vals: Vec<f64> = runs
        .iter()
        .filter_map(|r| r["e2e_ready_s"].as_f64())
        .collect();
    let e2e_mean = e2e_vals.iter().sum::<f64>() / e2e_vals.len().max(1) as f64;
    serde_json::json!({
        "hyperfine": {"mean_s": hyperfine_mean, "stddev_s": null, "median_s": hyperfine_mean, "min_s": hyperfine_mean, "max_s": hyperfine_mean, "times_s": [hyperfine_mean], "runs": 1},
        "runs": runs,
        "means": {
            "e2e_ready_s": e2e_mean,
            "frontend_tail_s": 0.1,
            "frontend_rss_bytes": 1_000_000,
            "frontend_pss_bytes": null,
            "tree_rss_bytes": 3_000_000,
            "tree_pss_bytes": null,
        },
    })
}

fn throughput_result(
    model: &str,
    throughput_tok_s: f64,
    throughput_req_s: f64,
    ttft_p99: f64,
) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "summary": {
            "num_requests": 8, "num_tokens": 800, "duration_s": 1.0,
            "throughput_tok_s": throughput_tok_s, "throughput_req_s": throughput_req_s,
            "ttft_ms": {"avg": 1.0, "p50": 1.0, "p90": 1.0, "p99": ttft_p99, "max": 1.0},
            "tpot_ms": {"avg": 1.0, "p50": 1.0, "p90": 1.0, "p99": 1.0, "max": 1.0},
            "e2e_s": {"avg": 1.0, "p50": 1.0, "p90": 1.0, "p99": 1.0, "max": 1.0},
        },
    })
}

fn crosscheck_result(ttft_ms_mean: f64, request_throughput: f64) -> serde_json::Value {
    serde_json::json!({
        "tool": "vllm",
        "metrics": {"ttft_ms_mean": ttft_ms_mean, "request_throughput": request_throughput},
    })
}

/// Pulls out the markdown text for one `## {scenario}` section (up to the
/// next `## ` heading, or end of string).
fn section_text<'a>(md: &'a str, scenario: &str) -> &'a str {
    let heading = format!("## {scenario}\n");
    let start = md
        .find(&heading)
        .unwrap_or_else(|| panic!("section {scenario:?} not found in:\n{md}"));
    let rest = &md[start + heading.len()..];
    let end = rest.find("\n## ").map(|i| i + 1).unwrap_or(rest.len());
    &rest[..end]
}

// --- Tests -------------------------------------------------------------------

/// All six scenario families render a section, with the scenario-specific
/// tables/columns the plan's interface contract names, and no provenance
/// banner (every manifest here is `real`/has a GPU).
#[test]
fn report_has_all_scenarios_and_tables() {
    let s1 = manifest(
        "s1_cancel",
        BackendKind::Real,
        Some("Test GPU"),
        vec![
            arm_info("python-default", FrontendKind::Python, Some(0), false),
            arm_info("rust", FrontendKind::Rust, None, false),
        ],
        vec![
            ok_trial(
                0,
                0,
                "python-default",
                s1_result(50.0, 200.0, 12.0, 2),
                vec![window(
                    "s1",
                    Some(python_gc()),
                    default_memory_maps(Some(500_000)).0,
                    default_memory_maps(Some(500_000)).1,
                    Some(python_cooc()),
                )],
            ),
            ok_trial(
                1,
                0,
                "rust",
                s1_result(30.0, 100.0, 20.0, 1),
                vec![window(
                    "s1",
                    Some(rust_gc()),
                    default_memory_maps(Some(500_000)).0,
                    default_memory_maps(Some(500_000)).1,
                    Some(rust_cooc()),
                )],
            ),
        ],
        serde_json::json!({}),
    );

    let s2 = manifest(
        "s2_saturation",
        BackendKind::Real,
        Some("Test GPU"),
        vec![
            arm_info("python-default", FrontendKind::Python, Some(0), false),
            arm_info("rust", FrontendKind::Rust, None, false),
        ],
        vec![
            ok_trial(
                0,
                0,
                "python-default",
                s2_result(
                    vec![
                        curve_point("rate=5", 5.0, 5.0, 100.0, 120.0),
                        curve_point("rate=10", 10.0, 9.0, 150.0, 170.0),
                    ],
                    9.0,
                ),
                vec![window(
                    "rate=5",
                    Some(python_gc()),
                    default_memory_maps(Some(500_000)).0,
                    default_memory_maps(Some(500_000)).1,
                    Some(python_cooc()),
                )],
            ),
            ok_trial(
                1,
                0,
                "rust",
                s2_result(
                    vec![
                        curve_point("rate=5", 5.0, 5.0, 60.0, 70.0),
                        curve_point("rate=10", 10.0, 10.0, 80.0, 90.0),
                    ],
                    10.0,
                ),
                vec![window(
                    "rate=5",
                    Some(rust_gc()),
                    default_memory_maps(Some(500_000)).0,
                    default_memory_maps(Some(500_000)).1,
                    Some(rust_cooc()),
                )],
            ),
        ],
        serde_json::json!({}),
    );

    let s3 = manifest(
        "s3_coldstart",
        BackendKind::Real,
        Some("Test GPU"),
        vec![
            arm_info("python-default", FrontendKind::Python, Some(0), false),
            arm_info("rust", FrontendKind::Rust, None, false),
        ],
        vec![
            ok_trial(
                0,
                0,
                "python-default",
                s3_result(
                    16.0,
                    vec![s3_run_with_gc(
                        1,
                        16.0,
                        15.0,
                        1.0,
                        1_000_000,
                        Some(500_000),
                        3_000_000,
                        Some(1_500_000),
                    )],
                ),
                vec![],
            ),
            ok_trial(
                1,
                0,
                "rust",
                s3_result(
                    5.0,
                    vec![s3_run(
                        1,
                        5.0,
                        4.5,
                        0.5,
                        400_000,
                        Some(200_000),
                        1_200_000,
                        Some(600_000),
                    )],
                ),
                vec![],
            ),
        ],
        serde_json::json!({}),
    );

    let throughput = manifest(
        "standard_throughput",
        BackendKind::Real,
        Some("Test GPU"),
        vec![
            arm_info("python-default", FrontendKind::Python, Some(0), false),
            arm_info("rust", FrontendKind::Rust, None, false),
        ],
        vec![
            ok_trial(
                0,
                0,
                "python-default",
                throughput_result("fast", 100.0, 10.0, 50.0),
                vec![window(
                    "throughput",
                    Some(python_gc()),
                    default_memory_maps(Some(500_000)).0,
                    default_memory_maps(Some(500_000)).1,
                    None,
                )],
            ),
            ok_trial(
                1,
                0,
                "rust",
                throughput_result("slow", 80.0, 8.0, 60.0),
                vec![window(
                    "throughput",
                    Some(rust_gc()),
                    default_memory_maps(Some(500_000)).0,
                    default_memory_maps(Some(500_000)).1,
                    None,
                )],
            ),
        ],
        serde_json::json!({}),
    );

    let sweep = manifest(
        "num_tokenizer_sweep",
        BackendKind::Real,
        Some("Test GPU"),
        vec![
            arm_info("python-nt0", FrontendKind::Python, Some(0), false),
            arm_info("python-nt2", FrontendKind::Python, Some(2), false),
            arm_info("python-nt4", FrontendKind::Python, Some(4), false),
        ],
        vec![
            ok_trial(
                0,
                0,
                "python-nt0",
                s2_result(vec![curve_point("rate=5", 5.0, 50.0, 100.0, 120.0)], 50.0),
                vec![],
            ),
            ok_trial(
                1,
                0,
                "python-nt2",
                s2_result(vec![curve_point("rate=5", 5.0, 55.0, 90.0, 110.0)], 55.0),
                vec![],
            ),
            ok_trial(
                2,
                0,
                "python-nt4",
                s2_result(vec![curve_point("rate=5", 5.0, 55.0, 85.0, 100.0)], 55.0),
                vec![],
            ),
        ],
        serde_json::json!({}),
    );

    let crosscheck = manifest(
        "crosscheck_vllm",
        BackendKind::Real,
        Some("Test GPU"),
        vec![
            arm_info("python-default", FrontendKind::Python, Some(0), false),
            arm_info("rust", FrontendKind::Rust, None, false),
        ],
        vec![
            ok_trial(
                0,
                0,
                "python-default",
                crosscheck_result(100.0, 10.0),
                vec![],
            ),
            ok_trial(1, 0, "rust", crosscheck_result(80.0, 12.0), vec![]),
        ],
        serde_json::json!({}),
    );

    let manifests = vec![s1, s2, s3, throughput, sweep, crosscheck];
    let report = build_report(&manifests).expect("build_report");
    let md = render_markdown(&report);

    for scenario in [
        "s1_cancel",
        "s2_saturation",
        "s3_coldstart",
        "standard_throughput",
        "num_tokenizer_sweep",
        "crosscheck_vllm",
    ] {
        assert!(
            md.contains(&format!("## {scenario}")),
            "missing section for {scenario}:\n{md}"
        );
    }

    for scenario in [
        "s1_cancel",
        "s2_saturation",
        "s3_coldstart",
        "standard_throughput",
    ] {
        let section = section_text(&md, scenario);
        assert!(
            section.contains("GC pauses"),
            "{scenario} section missing GC pauses table:\n{section}"
        );
        assert!(
            section.contains("Memory"),
            "{scenario} section missing Memory table:\n{section}"
        );
    }

    let s1_section = section_text(&md, "s1_cancel");
    assert!(
        s1_section.contains("P99 TTFT"),
        "s1 section missing P99 TTFT:\n{s1_section}"
    );
    assert!(
        s1_section.contains("RPS"),
        "s1 section missing RPS:\n{s1_section}"
    );

    let s2_section = section_text(&md, "s2_saturation");
    let level_rows = s2_section.matches("| rate=").count();
    assert_eq!(
        level_rows, 2,
        "expected one curve row per level:\n{s2_section}"
    );

    let s3_section = section_text(&md, "s3_coldstart");
    assert!(
        s3_section.contains("End-to-end startup"),
        "s3 section missing End-to-end startup:\n{s3_section}"
    );
    assert!(
        s3_section.contains("Frontend cold start"),
        "s3 section missing Frontend cold start:\n{s3_section}"
    );

    assert!(
        !md.contains("NOT A FRONTEND COMPARISON"),
        "no banner expected when every manifest is real+gpu:\n{md}"
    );
}

/// The Rust frontend's GC row is always the fixed N/A reason -- never a
/// fabricated zero-event stats row.
#[test]
fn rust_gc_row_not_applicable() {
    let m = manifest(
        "s1_cancel",
        BackendKind::Real,
        Some("Test GPU"),
        vec![
            arm_info("python-default", FrontendKind::Python, Some(0), false),
            arm_info("rust", FrontendKind::Rust, None, false),
        ],
        vec![
            ok_trial(
                0,
                0,
                "python-default",
                s1_result(50.0, 200.0, 12.0, 2),
                vec![window(
                    "s1",
                    Some(python_gc()),
                    BTreeMap::new(),
                    group_memory(0, None),
                    None,
                )],
            ),
            ok_trial(
                1,
                0,
                "rust",
                s1_result(30.0, 100.0, 20.0, 1),
                vec![window(
                    "s1",
                    Some(rust_gc()),
                    BTreeMap::new(),
                    group_memory(0, None),
                    None,
                )],
            ),
        ],
        serde_json::json!({}),
    );

    let report = build_report(&[m]).expect("build_report");
    let md = render_markdown(&report);
    assert!(
        md.contains("N/A (Rust frontend has no garbage collector)"),
        "markdown missing the fixed Rust GC N/A reason:\n{md}"
    );
}

/// A single trial per arm gives `n/a (n<2)` in the markdown, and
/// `ci_note: "n<2"` with no numeric interval in the JSON.
#[test]
fn ci_n_lt_2_note() {
    let m = manifest(
        "standard_throughput",
        BackendKind::Real,
        Some("Test GPU"),
        vec![
            arm_info("python-default", FrontendKind::Python, Some(0), false),
            arm_info("rust", FrontendKind::Rust, None, false),
        ],
        vec![
            ok_trial(
                0,
                0,
                "python-default",
                throughput_result("fast", 100.0, 10.0, 50.0),
                vec![],
            ),
            ok_trial(
                1,
                0,
                "rust",
                throughput_result("slow", 80.0, 8.0, 60.0),
                vec![],
            ),
        ],
        serde_json::json!({}),
    );

    let report = build_report(&[m]).expect("build_report");
    let json = serde_json::to_value(&report).expect("serialize report");
    let ci_note = json
        .pointer(
            "/scenarios/standard_throughput/arms/python-default/metrics/throughput_tok_s/ci_note",
        )
        .and_then(serde_json::Value::as_str);
    assert_eq!(
        ci_note,
        Some("n<2"),
        "ci_note should be n<2 for a single trial: {json}"
    );
    assert!(
        json.pointer(
            "/scenarios/standard_throughput/arms/python-default/metrics/throughput_tok_s/ci95_lo"
        )
        .map(serde_json::Value::is_null)
        .unwrap_or(true),
        "ci95_lo should be null/absent for n<2: {json}"
    );

    let md = render_markdown(&report);
    assert!(
        md.contains("n/a (n<2)"),
        "markdown missing n/a (n<2):\n{md}"
    );
}

/// A failed trial is listed with its error under its arm, and named by
/// index in the summary (P2: never dropped).
#[test]
fn failed_trials_listed() {
    let m = manifest(
        "s1_cancel",
        BackendKind::Real,
        Some("Test GPU"),
        vec![
            arm_info("python-default", FrontendKind::Python, Some(0), false),
            arm_info("rust", FrontendKind::Rust, None, false),
        ],
        vec![
            ok_trial(
                0,
                0,
                "python-default",
                s1_result(50.0, 200.0, 12.0, 2),
                vec![],
            ),
            failed_trial(1, 0, "python-default", "server not ready after 900s"),
            ok_trial(2, 0, "rust", s1_result(30.0, 100.0, 20.0, 1), vec![]),
        ],
        serde_json::json!({}),
    );

    let report = build_report(&[m]).expect("build_report");
    let arm = report.scenarios["s1_cancel"]
        .arms
        .get("python-default")
        .expect("python-default arm");
    assert_eq!(arm.failed_trials.len(), 1);
    assert_eq!(arm.failed_trials[0].index, 1);
    assert_eq!(arm.failed_trials[0].error, "server not ready after 900s");

    let summary_text = report.summary.join("\n");
    assert!(
        summary_text.contains("trial 1"),
        "summary should name the failed trial's index: {summary_text}"
    );
    assert!(
        summary_text.contains("server not ready after 900s"),
        "summary: {summary_text}"
    );
}

/// A mock backend_kind makes `summary[0]` the provenance banner, naming
/// "mock".
#[test]
fn banner_for_mock_provenance() {
    let m = manifest(
        "s1_cancel",
        BackendKind::Mock,
        Some("Test GPU"),
        vec![arm_info(
            "python-default",
            FrontendKind::Python,
            Some(0),
            false,
        )],
        vec![ok_trial(
            0,
            0,
            "python-default",
            s1_result(50.0, 200.0, 12.0, 2),
            vec![],
        )],
        serde_json::json!({}),
    );

    let report = build_report(&[m]).expect("build_report");
    assert!(
        !report.summary.is_empty(),
        "summary should have at least the banner"
    );
    assert!(
        report.summary[0].starts_with("NOT A FRONTEND COMPARISON"),
        "summary[0] = {:?}",
        report.summary[0]
    );
    assert!(
        report.summary[0].contains("mock"),
        "summary[0] should name 'mock': {:?}",
        report.summary[0]
    );
}

/// A sweep manifest with `peak_rps {0: 50, 2: 55, 4: 55}` recomputes
/// `extra.best == 2` (ties toward the smallest candidate), and the D-10
/// sentence names default 0 and best 2.
#[test]
fn sweep_best_recomputed() {
    let m = manifest(
        "num_tokenizer_sweep",
        BackendKind::Real,
        Some("Test GPU"),
        vec![
            arm_info("python-nt0", FrontendKind::Python, Some(0), false),
            arm_info("python-nt2", FrontendKind::Python, Some(2), false),
            arm_info("python-nt4", FrontendKind::Python, Some(4), false),
        ],
        vec![
            ok_trial(
                0,
                0,
                "python-nt0",
                s2_result(vec![curve_point("rate=5", 5.0, 50.0, 100.0, 120.0)], 50.0),
                vec![],
            ),
            ok_trial(
                1,
                0,
                "python-nt2",
                s2_result(vec![curve_point("rate=5", 5.0, 55.0, 90.0, 110.0)], 55.0),
                vec![],
            ),
            ok_trial(
                2,
                0,
                "python-nt4",
                s2_result(vec![curve_point("rate=5", 5.0, 55.0, 85.0, 100.0)], 55.0),
                vec![],
            ),
        ],
        serde_json::json!({}),
    );

    let report = build_report(&[m]).expect("build_report");
    let extra = &report.scenarios["num_tokenizer_sweep"].extra;
    assert_eq!(
        extra.pointer("/best").and_then(serde_json::Value::as_u64),
        Some(2)
    );

    let summary_text = report.summary.join("\n");
    assert!(
        summary_text.contains("default --num-tokenizer is 0"),
        "summary: {summary_text}"
    );
    assert!(
        summary_text.contains("best-performing candidate is 2"),
        "summary: {summary_text}"
    );
}

/// `also_best: true` on `python-default` gives a summary line saying the
/// default setting is also the best.
#[test]
fn also_best_note() {
    let m = manifest(
        "s1_cancel",
        BackendKind::Real,
        Some("Test GPU"),
        vec![
            arm_info("python-default", FrontendKind::Python, Some(0), true),
            arm_info("rust", FrontendKind::Rust, None, false),
        ],
        vec![
            ok_trial(
                0,
                0,
                "python-default",
                s1_result(50.0, 200.0, 12.0, 2),
                vec![],
            ),
            ok_trial(1, 0, "rust", s1_result(30.0, 100.0, 20.0, 1), vec![]),
        ],
        serde_json::json!({}),
    );

    let report = build_report(&[m]).expect("build_report");
    let summary_text = report.summary.join("\n");
    assert!(
        summary_text.contains("also its best-performing one"),
        "summary should note also_best: {summary_text}"
    );
}

/// A `null` PSS renders "n/a (PSS needs Linux)", never a fabricated value.
#[test]
fn pss_unavailable_rendered() {
    let m = manifest(
        "s1_cancel",
        BackendKind::Real,
        Some("Test GPU"),
        vec![
            arm_info("python-default", FrontendKind::Python, Some(0), false),
            arm_info("rust", FrontendKind::Rust, None, false),
        ],
        vec![
            ok_trial(
                0,
                0,
                "python-default",
                s1_result(50.0, 200.0, 12.0, 2),
                vec![window(
                    "s1",
                    Some(python_gc()),
                    default_memory_maps(None).0,
                    default_memory_maps(None).1,
                    Some(python_cooc()),
                )],
            ),
            ok_trial(
                1,
                0,
                "rust",
                s1_result(30.0, 100.0, 20.0, 1),
                vec![window(
                    "s1",
                    Some(rust_gc()),
                    default_memory_maps(None).0,
                    default_memory_maps(None).1,
                    Some(rust_cooc()),
                )],
            ),
        ],
        serde_json::json!({}),
    );

    let report = build_report(&[m]).expect("build_report");
    let md = render_markdown(&report);
    assert!(
        md.contains("n/a (PSS needs Linux)"),
        "markdown missing PSS-unavailable phrase:\n{md}"
    );
}

/// Rust throughput at or above Python's gives the "No throughput
/// regression" sentence, still with the percent and CI.
#[test]
fn no_regression_sentence() {
    let m = manifest(
        "standard_throughput",
        BackendKind::Real,
        Some("Test GPU"),
        vec![
            arm_info("python-default", FrontendKind::Python, Some(0), false),
            arm_info("rust", FrontendKind::Rust, None, false),
        ],
        vec![
            ok_trial(
                0,
                0,
                "python-default",
                throughput_result("fast", 100.0, 10.0, 50.0),
                vec![],
            ),
            ok_trial(
                1,
                0,
                "python-default",
                throughput_result("fast", 102.0, 10.2, 51.0),
                vec![],
            ),
            ok_trial(
                2,
                0,
                "rust",
                throughput_result("slow", 110.0, 11.0, 48.0),
                vec![],
            ),
            ok_trial(
                3,
                0,
                "rust",
                throughput_result("slow", 112.0, 11.2, 49.0),
                vec![],
            ),
        ],
        serde_json::json!({}),
    );

    let report = build_report(&[m]).expect("build_report");
    let summary_text = report.summary.join("\n");
    assert!(
        summary_text.contains("No throughput regression"),
        "summary: {summary_text}"
    );
    assert!(
        summary_text.contains('%'),
        "summary should still carry a percent: {summary_text}"
    );
    assert!(
        summary_text.contains("95% CI"),
        "summary should still carry a CI: {summary_text}"
    );
}
