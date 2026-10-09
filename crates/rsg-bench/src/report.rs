//! `rsg-bench report` (D-13): turns the per-scenario manifests in a
//! directory into the combined `docs/benchmarks/` JSON and markdown pair.
//! The JSON is the single source of truth -- `render_markdown` derives
//! every table and sentence from it, so the two can never drift apart
//! (BENCH-07/BENCH-08).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::Serialize;
use serde_json::Value;

use crate::gclog::GcRow;
use crate::manifest::{self, BackendKind, Manifest, TrialRecord, TrialStatus};
use crate::memory::MemPoint;
use crate::roles::{Group, Role};
use crate::scenarios::sweep;
use crate::stats::{mean_ci95, pct_delta, welch_diff_ci95};

/// `rsg-bench report` flags.
#[derive(Debug, Clone, clap::Args)]
pub struct ReportArgs {
    #[arg(long)]
    pub manifests_dir: PathBuf,
    #[arg(long)]
    pub out_json: PathBuf,
    #[arg(long)]
    pub out_md: PathBuf,
}

/// The combined report (D-13): provenance across every input manifest, one
/// [`ScenarioReport`] per `session.scenario` found, and a flat list of
/// human-readable summary sentences (the banner, when present, is always
/// `summary[0]`).
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub generated_by: String,
    pub sources: Vec<String>,
    pub provenance: Provenance,
    pub scenarios: BTreeMap<String, ScenarioReport>,
    pub summary: Vec<String>,
}

/// Whether every input manifest was measured against a `real` backend with
/// a recorded GPU (PROJECT: projections stay projections until measured).
#[derive(Debug, Clone, Serialize)]
pub struct Provenance {
    pub backend_kinds: Vec<BackendKind>,
    pub gpus: Vec<Option<String>>,
    pub comparison_valid: bool,
}

/// One scenario's aggregated arms, Rust-vs-Python deltas, and any
/// scenario-specific extras (the S2 curve, the sweep's candidates/best).
#[derive(Debug, Clone, Serialize)]
pub struct ScenarioReport {
    pub arms: BTreeMap<String, ArmReport>,
    pub deltas: BTreeMap<String, BTreeMap<String, DeltaReport>>,
    pub extra: Value,
}

/// One arm's aggregation: how many trials succeeded/failed, the failures
/// themselves (P2: never dropped), every headline metric's run-to-run
/// mean/CI (BENCH-07), and the GC/memory/co-occurrence tables aggregated
/// over every ok trial's observed windows (BENCH-08).
#[derive(Debug, Clone, Serialize)]
pub struct ArmReport {
    pub n_ok: usize,
    pub n_failed: usize,
    pub failed_trials: Vec<FailedTrialReport>,
    pub metrics: BTreeMap<String, MetricReport>,
    pub gc: BTreeMap<Role, GcAggRow>,
    pub memory: MemoryAggReport,
    pub cooccurrence: BTreeMap<Group, CoOccAggRow>,
}

/// One failed trial, named by its manifest index (P2: never dropped).
#[derive(Debug, Clone, Serialize)]
pub struct FailedTrialReport {
    pub index: usize,
    pub error: String,
}

/// One headline metric's run-to-run mean and 95% CI (D-05). `n < 2` never
/// invents an interval: `ci95_lo`/`ci95_hi` stay `None` and `ci_note` is
/// `Some("n<2")` instead (BENCH-07's empty edge).
#[derive(Debug, Clone, Serialize)]
pub struct MetricReport {
    pub mean: Option<f64>,
    pub ci95_lo: Option<f64>,
    pub ci95_hi: Option<f64>,
    pub n: usize,
    pub ci_note: Option<String>,
}

/// One Rust-minus-Python headline-metric delta: the raw difference with
/// its Welch 95% CI, and the percent delta with its own CI (BENCH-07).
#[derive(Debug, Clone, Serialize)]
pub struct DeltaReport {
    pub diff: f64,
    pub ci95_lo: f64,
    pub ci95_hi: f64,
    pub pct: f64,
    pub pct_lo: f64,
    pub pct_hi: f64,
}

/// One role's GC-pause table row, aggregated over every window/run an arm
/// produced: mean count, mean total pause ms, mean P99 pause ms, max of
/// max pause ms -- or, for the Rust frontend, the fixed N/A reason
/// ([`crate::gclog::RUST_FRONTEND_GC_REASON`]).
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum GcAggRow {
    Stats {
        count: f64,
        total_pause_ms: f64,
        p99_pause_ms: Option<f64>,
        max_pause_ms: Option<f64>,
    },
    NotApplicable {
        not_applicable: String,
    },
}

/// One group's mean RSS/PSS max, aggregated over every window/run an arm
/// produced. `pss_bytes_mean` is `None` whenever any contributing sample's
/// PSS was unavailable -- never a partial/faked PSS mean.
#[derive(Debug, Clone, Serialize)]
pub struct MemAggRow {
    pub rss_bytes_mean: Option<f64>,
    pub pss_bytes_mean: Option<f64>,
}

/// Per-group and whole-tree memory, aggregated over every window/run an
/// arm produced.
#[derive(Debug, Clone, Serialize)]
pub struct MemoryAggReport {
    pub by_group: BTreeMap<Group, MemAggRow>,
    pub tree: MemAggRow,
}

/// One group's GC-pause/P99-TTFT-spike co-occurrence table row, aggregated
/// over every window an arm produced.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum CoOccAggRow {
    Computed {
        spike_overlap_rate_mean: Option<f64>,
        nonspike_overlap_rate_mean: Option<f64>,
    },
    NotApplicable {
        not_applicable: String,
    },
    NoFirstToken {
        no_first_token: bool,
    },
}

/// Reads every `*.manifest.json` in `dir`, in sorted file-name order.
/// Rejects a manifest whose `schema_version` is not
/// [`manifest::SCHEMA_VERSION`], and rejects two manifests that share the
/// same `session.scenario`, naming both files -- the report never merges
/// or silently picks between sessions.
fn load_manifests(dir: &Path) -> anyhow::Result<Vec<Manifest>> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("read_dir {}", dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.ends_with(".manifest.json"))
                .unwrap_or(false)
        })
        .collect();
    paths.sort();

    let mut out = Vec::with_capacity(paths.len());
    let mut seen_scenarios: BTreeMap<String, PathBuf> = BTreeMap::new();
    for path in paths {
        let m = manifest::read_manifest(&path)
            .with_context(|| format!("read manifest {}", path.display()))?;
        if m.schema_version != manifest::SCHEMA_VERSION {
            anyhow::bail!(
                "{}: unsupported schema_version {} (report expects {})",
                path.display(),
                m.schema_version,
                manifest::SCHEMA_VERSION
            );
        }
        if let Some(prior) = seen_scenarios.get(&m.session.scenario) {
            anyhow::bail!(
                "duplicate scenario {:?} across manifests {} and {}: the report never merges or silently picks between sessions",
                m.session.scenario,
                prior.display(),
                path.display()
            );
        }
        seen_scenarios.insert(m.session.scenario.clone(), path.clone());
        out.push(m);
    }
    Ok(out)
}

/// Writes `text` to `path` atomically (T-07-13: refuses a symlink target,
/// writes a sibling temp file, `sync_all`s it, then renames it over
/// `path`) -- the same scheme [`manifest::write_json_atomic`] uses, for
/// plain markdown text instead of JSON.
fn write_text_atomic(path: &Path, text: &str) -> anyhow::Result<()> {
    if let Ok(meta) = std::fs::symlink_metadata(path)
        && meta.file_type().is_symlink()
    {
        anyhow::bail!("refusing to write through a symlink: {}", path.display());
    }

    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .with_context(|| format!("create parent dir {}", parent.display()))?;

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .context("report markdown path has no file name")?;
    let pid = std::process::id();
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let tmp_path = parent.join(format!(".{file_name}.tmp-{pid}-{counter}"));

    {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)
            .with_context(|| format!("create temp file {}", tmp_path.display()))?;
        use std::io::Write;
        file.write_all(text.as_bytes())
            .with_context(|| format!("write temp file {}", tmp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("sync temp file {}", tmp_path.display()))?;
    }
    std::fs::rename(&tmp_path, path)
        .with_context(|| format!("rename {} -> {}", tmp_path.display(), path.display()))?;
    Ok(())
}

/// Reads every manifest in `args.manifests_dir`, builds the [`Report`], and
/// writes both the JSON ([`manifest::write_json_atomic`]) and the markdown
/// ([`render_markdown`]) atomically.
pub fn run_report(args: ReportArgs) -> anyhow::Result<()> {
    let manifests = load_manifests(&args.manifests_dir)?;
    let report = build_report(&manifests)?;
    manifest::write_json_atomic(&args.out_json, &report).context("write report JSON")?;
    let md = render_markdown(&report);
    write_text_atomic(&args.out_md, &md).context("write report markdown")?;
    Ok(())
}

fn backend_kind_str(k: BackendKind) -> &'static str {
    match k {
        BackendKind::Real => "real",
        BackendKind::Mock => "mock",
        BackendKind::Stub => "stub",
    }
}

/// One memory observation point: `(rss_bytes, pss_bytes)`.
type MemPointF64 = (f64, Option<f64>);
/// One window/run's memory observation: per-group points plus the whole
/// tree's point.
type MemObsPoint = (BTreeMap<Group, MemPointF64>, MemPointF64);
/// Per-level, per-arm S2-curve accumulators: `(achieved_rps, ttft_p99_ms,
/// e2e_p99_ms)` samples across every matching ok trial.
type CurveAccum = (Vec<f64>, Vec<f64>, Vec<f64>);

fn mean_opt(xs: &[f64]) -> Option<f64> {
    if xs.is_empty() {
        None
    } else {
        Some(xs.iter().sum::<f64>() / xs.len() as f64)
    }
}

/// Maps a scenario name onto the result-shape family it shares headline
/// metrics with: every `crosscheck_*` scenario shares `crosscheck`'s
/// dynamic tool-metric shape, and `num_tokenizer_sweep` shares
/// `s2_saturation`'s `{mode, curve, peak_rps}` shape (D-09's arms are all
/// Python, at different `--num-tokenizer` values, same result shape).
fn scenario_family(scenario: &str) -> &str {
    if scenario.starts_with("crosscheck_") {
        "crosscheck"
    } else if scenario == "num_tokenizer_sweep" {
        "s2_saturation"
    } else {
        scenario
    }
}

/// This scenario's headline-metric names (the interface contract's table).
/// `crosscheck`'s list is dynamic -- every numeric key under `result.metrics`
/// across this scenario's own ok trials.
fn headline_metric_names(scenario: &str, trials: &[TrialRecord]) -> Vec<String> {
    match scenario_family(scenario) {
        "s1_cancel" => ["ttft_p99_ms", "ttft_p50_ms", "rps", "cancelled"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        "s2_saturation" => vec!["peak_rps".to_string()],
        "s3_coldstart" => [
            "e2e_ready_s",
            "frontend_tail_s",
            "frontend_rss_bytes",
            "frontend_pss_bytes",
            "tree_pss_bytes",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
        "standard_throughput" => ["throughput_tok_s", "throughput_req_s", "ttft_p99_ms"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        "crosscheck" => {
            let mut set = BTreeSet::new();
            for t in trials {
                if let Some(obj) = t.result.pointer("/metrics").and_then(Value::as_object) {
                    for k in obj.keys() {
                        set.insert(k.clone());
                    }
                }
            }
            set.into_iter().collect()
        }
        _ => Vec::new(),
    }
}

/// Reads one headline metric out of a trial's `result` (a scenario-specific
/// JSON path). `None` when the scenario/metric pair is not known, or the
/// path is missing/not numeric -- never a panic.
fn extract_metric(scenario: &str, result: &Value, metric: &str) -> Option<f64> {
    match scenario_family(scenario) {
        "s1_cancel" => match metric {
            "ttft_p99_ms" => result
                .pointer("/latency/ttft_ms/p99")
                .and_then(Value::as_f64),
            "ttft_p50_ms" => result
                .pointer("/latency/ttft_ms/p50")
                .and_then(Value::as_f64),
            "rps" => result.pointer("/rps").and_then(Value::as_f64),
            "cancelled" => result.pointer("/counts/cancelled").and_then(Value::as_f64),
            _ => None,
        },
        "s2_saturation" => match metric {
            "peak_rps" => result.pointer("/peak_rps").and_then(Value::as_f64),
            _ => None,
        },
        "s3_coldstart" => result
            .pointer(&format!("/means/{metric}"))
            .and_then(Value::as_f64),
        "standard_throughput" => match metric {
            "throughput_tok_s" => result
                .pointer("/summary/throughput_tok_s")
                .and_then(Value::as_f64),
            "throughput_req_s" => result
                .pointer("/summary/throughput_req_s")
                .and_then(Value::as_f64),
            "ttft_p99_ms" => result
                .pointer("/summary/ttft_ms/p99")
                .and_then(Value::as_f64),
            _ => None,
        },
        "crosscheck" => result
            .pointer(&format!("/metrics/{metric}"))
            .and_then(Value::as_f64),
        _ => None,
    }
}

fn metric_report(values: &[f64]) -> MetricReport {
    match values.len() {
        0 => MetricReport {
            mean: None,
            ci95_lo: None,
            ci95_hi: None,
            n: 0,
            ci_note: None,
        },
        1 => MetricReport {
            mean: Some(values[0]),
            ci95_lo: None,
            ci95_hi: None,
            n: 1,
            ci_note: Some("n<2".to_string()),
        },
        _ => {
            let ci = mean_ci95(values).expect("n >= 2 checked above");
            MetricReport {
                mean: Some(ci.mean),
                ci95_lo: Some(ci.lo),
                ci95_hi: Some(ci.hi),
                n: ci.n,
                ci_note: None,
            }
        }
    }
}

/// Rust-minus-every-Python-arm deltas (BENCH-07), keyed
/// `"rust_vs_<python arm id>"`. Omits a metric entirely when either side
/// has `n < 2` -- never an invented interval.
fn compute_deltas(
    arm_values: &BTreeMap<String, BTreeMap<String, Vec<f64>>>,
) -> BTreeMap<String, BTreeMap<String, DeltaReport>> {
    let mut out = BTreeMap::new();
    let Some(rust_values) = arm_values.get("rust") else {
        return out;
    };
    for (arm_id, python_metrics) in arm_values {
        if arm_id == "rust" || !arm_id.starts_with("python") {
            continue;
        }
        let mut deltas = BTreeMap::new();
        for (metric, python_values) in python_metrics {
            let Some(rust_metric_values) = rust_values.get(metric) else {
                continue;
            };
            if let (Some(w), Some(p)) = (
                welch_diff_ci95(python_values, rust_metric_values),
                pct_delta(python_values, rust_metric_values),
            ) {
                deltas.insert(
                    metric.clone(),
                    DeltaReport {
                        diff: w.diff,
                        ci95_lo: w.lo,
                        ci95_hi: w.hi,
                        pct: p.pct,
                        pct_lo: p.lo_pct,
                        pct_hi: p.hi_pct,
                    },
                );
            }
        }
        if !deltas.is_empty() {
            out.insert(format!("rust_vs_{arm_id}"), deltas);
        }
    }
    out
}

/// D-09's saturation curve, aggregated per level (`CurvePoint.label`) per
/// arm with [`mean_ci95`] over every ok trial's matching curve point.
/// Shared by `s2_saturation` and `num_tokenizer_sweep` (same result shape).
fn build_curve_extra(m: &Manifest) -> Value {
    let mut labels: Vec<String> = Vec::new();
    let mut per_label_arm: BTreeMap<String, BTreeMap<String, CurveAccum>> = BTreeMap::new();

    for t in &m.trials {
        if t.status != TrialStatus::Ok {
            continue;
        }
        let Some(curve) = t.result.get("curve").and_then(Value::as_array) else {
            continue;
        };
        for point in curve {
            let label = point
                .get("label")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if !labels.contains(&label) {
                labels.push(label.clone());
            }
            let entry = per_label_arm
                .entry(label)
                .or_default()
                .entry(t.arm.clone())
                .or_default();
            if let Some(v) = point.get("achieved_rps").and_then(Value::as_f64) {
                entry.0.push(v);
            }
            if let Some(v) = point
                .pointer("/latency/ttft_ms/p99")
                .and_then(Value::as_f64)
            {
                entry.1.push(v);
            }
            if let Some(v) = point.pointer("/latency/e2e_ms/p99").and_then(Value::as_f64) {
                entry.2.push(v);
            }
        }
    }

    let rows: Vec<Value> = labels
        .into_iter()
        .map(|label| {
            let arms_map = per_label_arm.remove(&label).unwrap_or_default();
            let arms_json: BTreeMap<String, Value> = arms_map
                .into_iter()
                .map(|(arm, (rps, ttft, e2e))| {
                    (
                        arm,
                        serde_json::json!({
                            "achieved_rps": serde_json::to_value(metric_report(&rps)).unwrap_or(Value::Null),
                            "ttft_p99_ms": serde_json::to_value(metric_report(&ttft)).unwrap_or(Value::Null),
                            "e2e_p99_ms": serde_json::to_value(metric_report(&e2e)).unwrap_or(Value::Null),
                        }),
                    )
                })
                .collect();
            serde_json::json!({"label": label, "arms": arms_json})
        })
        .collect();

    Value::Array(rows)
}

/// Scenario-specific extras and summary sentences: `standard_throughput`'s
/// regression/no-regression sentence (D-11); `num_tokenizer_sweep`'s
/// recomputed `candidates`/`best` and D-10 asymmetry sentence; and
/// `s2_saturation`'s per-level curve CIs.
fn build_extra(
    scenario: &str,
    m: &Manifest,
    arm_values: &BTreeMap<String, BTreeMap<String, Vec<f64>>>,
    deltas: &BTreeMap<String, BTreeMap<String, DeltaReport>>,
) -> (Value, Vec<String>) {
    let mut summary = Vec::new();
    match scenario {
        "standard_throughput" => {
            for (key, metric_deltas) in deltas {
                if let Some(d) = metric_deltas.get("throughput_tok_s") {
                    let python_arm = key.strip_prefix("rust_vs_").unwrap_or(key);
                    if d.pct < 0.0 {
                        summary.push(format!(
                            "REGRESSION: Rust frontend standard throughput is {:.2}% lower than {python_arm} (95% CI [{:.2}%, {:.2}%]); \u{b1}2% is a reference target, not a gate.",
                            -d.pct, -d.pct_hi, -d.pct_lo
                        ));
                    } else {
                        summary.push(format!(
                            "No throughput regression: Rust is +{:.2}% vs {python_arm} (95% CI [{:.2}%, {:.2}%]); \u{b1}2% is a reference target, not a gate.",
                            d.pct, d.pct_lo, d.pct_hi
                        ));
                    }
                }
            }
            (Value::Null, summary)
        }
        "num_tokenizer_sweep" => {
            let mut candidates: Vec<(u32, Option<f64>)> = m
                .session
                .arms
                .iter()
                .filter_map(|a| {
                    let k = a.num_tokenizer?;
                    let peak = arm_values
                        .get(&a.id)
                        .and_then(|mm| mm.get("peak_rps"))
                        .and_then(|v| mean_opt(v));
                    Some((k, peak))
                })
                .collect();
            candidates.sort_by_key(|(k, _)| *k);
            let best = sweep::pick_best(&candidates).ok();
            let default_k = candidates.iter().map(|(k, _)| *k).min();
            if let (Some(default_k), Some(best)) = (default_k, best) {
                let suffix = if default_k == best {
                    " (default equals best)"
                } else {
                    ""
                };
                summary.push(format!(
                    "D-10: Python's default --num-tokenizer is {default_k}; this sweep's best-performing candidate is {best}{suffix}."
                ));
            }
            let candidates_json: Vec<Value> = candidates
                .iter()
                .map(|(k, p)| serde_json::json!({"k": k, "peak_rps": p}))
                .collect();
            (
                serde_json::json!({"candidates": candidates_json, "best": best}),
                summary,
            )
        }
        "s2_saturation" => (serde_json::json!({"curve": build_curve_extra(m)}), summary),
        _ => (Value::Null, summary),
    }
}

/// Aggregates every ok trial's `WindowObs.gc` maps for one arm (every
/// window-observing scenario except `s3_coldstart`, whose observation
/// lives in `result.runs[].gc_boot` instead -- see
/// [`aggregate_s3_observations`]): mean count, mean total pause ms, mean
/// P99 pause ms, max of max pause ms per role. A role whose rows are ever
/// [`GcRow::NotApplicable`] (the Rust frontend) keeps that reason.
fn aggregate_gc(maps: &[&BTreeMap<Role, GcRow>]) -> BTreeMap<Role, GcAggRow> {
    let mut roles: BTreeSet<Role> = BTreeSet::new();
    for m in maps {
        roles.extend(m.keys().copied());
    }
    let mut out = BTreeMap::new();
    for role in roles {
        let mut na_reason: Option<String> = None;
        let mut counts = Vec::new();
        let mut totals = Vec::new();
        let mut p99s = Vec::new();
        let mut maxs: Vec<f64> = Vec::new();
        for m in maps {
            if let Some(row) = m.get(&role) {
                match row {
                    GcRow::NotApplicable { reason } => na_reason = Some(reason.clone()),
                    GcRow::Stats(s) => {
                        counts.push(s.count as f64);
                        totals.push(s.total_pause_ms);
                        if let Some(p) = s.pause_ms.p99 {
                            p99s.push(p);
                        }
                        if let Some(mx) = s.pause_ms.max {
                            maxs.push(mx);
                        }
                    }
                }
            }
        }
        if let Some(reason) = na_reason {
            out.insert(
                role,
                GcAggRow::NotApplicable {
                    not_applicable: reason,
                },
            );
        } else if !counts.is_empty() {
            out.insert(
                role,
                GcAggRow::Stats {
                    count: mean_opt(&counts).unwrap_or(0.0),
                    total_pause_ms: mean_opt(&totals).unwrap_or(0.0),
                    p99_pause_ms: mean_opt(&p99s),
                    max_pause_ms: maxs.into_iter().fold(None, |acc: Option<f64>, v| {
                        Some(acc.map_or(v, |a| a.max(v)))
                    }),
                },
            );
        }
    }
    out
}

/// Aggregates every memory observation point (RSS always, PSS only when
/// every contributing point had it) per [`Group`] and the whole tree, over
/// every window/run an arm produced.
fn aggregate_memory(points: &[MemObsPoint]) -> MemoryAggReport {
    let mut groups: BTreeSet<Group> = BTreeSet::new();
    for (g, _) in points {
        groups.extend(g.keys().copied());
    }

    let mut by_group = BTreeMap::new();
    for group in groups {
        let present: Vec<&(f64, Option<f64>)> =
            points.iter().filter_map(|(g, _)| g.get(&group)).collect();
        let rss_vals: Vec<f64> = present.iter().map(|(r, _)| *r).collect();
        let pss_ok = !present.is_empty() && present.iter().all(|(_, p)| p.is_some());
        let pss_vals: Vec<f64> = if pss_ok {
            present.iter().filter_map(|(_, p)| *p).collect()
        } else {
            Vec::new()
        };
        by_group.insert(
            group,
            MemAggRow {
                rss_bytes_mean: mean_opt(&rss_vals),
                pss_bytes_mean: if pss_ok { mean_opt(&pss_vals) } else { None },
            },
        );
    }

    let tree_rss: Vec<f64> = points.iter().map(|(_, (r, _))| *r).collect();
    let tree_pss_ok = !points.is_empty() && points.iter().all(|(_, (_, p))| p.is_some());
    let tree_pss: Vec<f64> = if tree_pss_ok {
        points.iter().filter_map(|(_, (_, p))| *p).collect()
    } else {
        Vec::new()
    };
    let tree = MemAggRow {
        rss_bytes_mean: mean_opt(&tree_rss),
        pss_bytes_mean: if tree_pss_ok {
            mean_opt(&tree_pss)
        } else {
            None
        },
    };

    MemoryAggReport { by_group, tree }
}

/// Aggregates every ok trial's `WindowObs.cooccurrence` maps for one arm:
/// mean spike/non-spike overlap rate per [`Group`]. A group that is ever
/// [`crate::gclog::CoOccurrenceRow::NotApplicable`] (the Rust frontend)
/// keeps that reason; a group with rows but none `Computed` (every window
/// observed no first token) reports `NoFirstToken`.
fn aggregate_cooccurrence(
    maps: &[&BTreeMap<Group, crate::gclog::CoOccurrenceRow>],
) -> BTreeMap<Group, CoOccAggRow> {
    use crate::gclog::CoOccurrenceRow;
    let mut groups: BTreeSet<Group> = BTreeSet::new();
    for m in maps {
        groups.extend(m.keys().copied());
    }
    let mut out = BTreeMap::new();
    for group in groups {
        let mut na_reason: Option<String> = None;
        let mut spike_rates = Vec::new();
        let mut nonspike_rates = Vec::new();
        let mut any_computed = false;
        for m in maps {
            if let Some(row) = m.get(&group) {
                match row {
                    CoOccurrenceRow::NotApplicable { reason } => na_reason = Some(reason.clone()),
                    CoOccurrenceRow::NoFirstToken => {}
                    CoOccurrenceRow::Computed(c) => {
                        any_computed = true;
                        if let Some(r) = c.spike_overlap_rate {
                            spike_rates.push(r);
                        }
                        if let Some(r) = c.nonspike_overlap_rate {
                            nonspike_rates.push(r);
                        }
                    }
                }
            }
        }
        if let Some(reason) = na_reason {
            out.insert(
                group,
                CoOccAggRow::NotApplicable {
                    not_applicable: reason,
                },
            );
        } else if any_computed {
            out.insert(
                group,
                CoOccAggRow::Computed {
                    spike_overlap_rate_mean: mean_opt(&spike_rates),
                    nonspike_overlap_rate_mean: mean_opt(&nonspike_rates),
                },
            );
        } else {
            out.insert(
                group,
                CoOccAggRow::NoFirstToken {
                    no_first_token: true,
                },
            );
        }
    }
    out
}

/// Every window-observing scenario except `s3_coldstart`: aggregates
/// `WindowObs.gc`/`memory`/`tree_memory`/`cooccurrence` across every
/// window of every ok trial for one arm.
fn aggregate_window_observations(
    ok_trials: &[&&TrialRecord],
) -> (
    BTreeMap<Role, GcAggRow>,
    MemoryAggReport,
    BTreeMap<Group, CoOccAggRow>,
) {
    let mut gc_maps: Vec<&BTreeMap<Role, GcRow>> = Vec::new();
    let mut mem_points: Vec<MemObsPoint> = Vec::new();
    let mut cooc_maps: Vec<&BTreeMap<Group, crate::gclog::CoOccurrenceRow>> = Vec::new();

    for t in ok_trials {
        for w in &t.windows {
            if let Some(gc) = &w.gc {
                gc_maps.push(gc);
            }
            if let Some(co) = &w.cooccurrence {
                cooc_maps.push(co);
            }

            let mut groups_point: BTreeMap<Group, (f64, Option<f64>)> = BTreeMap::new();
            for (g, gm) in &w.memory {
                if let Some(max) = gm.rss_bytes.max {
                    let pss = gm.pss_bytes.as_ref().and_then(|p| p.max).map(|v| v as f64);
                    groups_point.insert(*g, (max as f64, pss));
                }
            }
            if let Some(max) = w.tree_memory.rss_bytes.max {
                let pss = w
                    .tree_memory
                    .pss_bytes
                    .as_ref()
                    .and_then(|p| p.max)
                    .map(|v| v as f64);
                mem_points.push((groups_point, (max as f64, pss)));
            }
        }
    }

    (
        aggregate_gc(&gc_maps),
        aggregate_memory(&mem_points),
        aggregate_cooccurrence(&cooc_maps),
    )
}

/// `s3_coldstart` only: aggregates from `result.runs[].gc_boot` (the first
/// kept run per trial, per the plan's own spec) and
/// `result.runs[].memory_at_ready` (every kept run) rather than
/// `WindowObs`, because [`crate::scenarios::s3_coldstart::S3Runner`]
/// returns zero `MeasuredWindow`s (it owns its own observation entirely).
/// There is no co-occurrence data for this scenario (no requests are ever
/// sent), so that map is always empty.
fn aggregate_s3_observations(
    ok_trials: &[&&TrialRecord],
) -> (
    BTreeMap<Role, GcAggRow>,
    MemoryAggReport,
    BTreeMap<Group, CoOccAggRow>,
) {
    let mut gc_rows: Vec<BTreeMap<Role, GcRow>> = Vec::new();
    let mut mem_points: Vec<MemObsPoint> = Vec::new();

    for t in ok_trials {
        let Some(runs) = t.result.get("runs").and_then(Value::as_array) else {
            continue;
        };
        if let Some(first_run) = runs.first()
            && let Some(gc_boot) = first_run.get("gc_boot")
            && !gc_boot.is_null()
            && let Ok(parsed) = serde_json::from_value::<BTreeMap<Role, GcRow>>(gc_boot.clone())
        {
            gc_rows.push(parsed);
        }

        for run in runs {
            let Some(mem) = run.get("memory_at_ready") else {
                continue;
            };
            let mut groups_point: BTreeMap<Group, (f64, Option<f64>)> = BTreeMap::new();
            if let Some(groups_val) = mem.get("groups")
                && let Ok(parsed) =
                    serde_json::from_value::<BTreeMap<Group, MemPoint>>(groups_val.clone())
            {
                for (g, mp) in parsed {
                    groups_point.insert(g, (mp.rss_bytes as f64, mp.pss_bytes.map(|p| p as f64)));
                }
            }
            if let Some(tree_val) = mem.get("tree")
                && let Ok(mp) = serde_json::from_value::<MemPoint>(tree_val.clone())
            {
                mem_points.push((
                    groups_point,
                    (mp.rss_bytes as f64, mp.pss_bytes.map(|p| p as f64)),
                ));
            }
        }
    }

    let gc_refs: Vec<&BTreeMap<Role, GcRow>> = gc_rows.iter().collect();
    (
        aggregate_gc(&gc_refs),
        aggregate_memory(&mem_points),
        BTreeMap::new(),
    )
}

fn aggregate_observations(
    scenario: &str,
    ok_trials: &[&&TrialRecord],
) -> (
    BTreeMap<Role, GcAggRow>,
    MemoryAggReport,
    BTreeMap<Group, CoOccAggRow>,
) {
    if scenario == "s3_coldstart" {
        aggregate_s3_observations(ok_trials)
    } else {
        aggregate_window_observations(ok_trials)
    }
}

fn build_scenario_report(m: &Manifest) -> (ScenarioReport, Vec<String>) {
    let scenario = m.session.scenario.as_str();
    let mut by_arm: BTreeMap<String, Vec<&TrialRecord>> = BTreeMap::new();
    for t in &m.trials {
        by_arm.entry(t.arm.clone()).or_default().push(t);
    }

    let metric_names = headline_metric_names(scenario, &m.trials);
    let mut arm_values: BTreeMap<String, BTreeMap<String, Vec<f64>>> = BTreeMap::new();
    let mut arms = BTreeMap::new();
    let mut summary_lines = Vec::new();

    for (arm_id, trials) in &by_arm {
        let ok_trials: Vec<&&TrialRecord> = trials
            .iter()
            .filter(|t| t.status == TrialStatus::Ok)
            .collect();
        let failed_trials: Vec<FailedTrialReport> = trials
            .iter()
            .filter(|t| t.status == TrialStatus::Failed)
            .map(|t| FailedTrialReport {
                index: t.index,
                error: t.error.clone().unwrap_or_default(),
            })
            .collect();
        for f in &failed_trials {
            summary_lines.push(format!(
                "{scenario}: trial {} (arm {arm_id}) failed: {}",
                f.index, f.error
            ));
        }

        let mut metrics = BTreeMap::new();
        let mut values_for_arm: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        for metric in &metric_names {
            let values: Vec<f64> = ok_trials
                .iter()
                .filter_map(|t| extract_metric(scenario, &t.result, metric))
                .collect();
            values_for_arm.insert(metric.clone(), values.clone());
            metrics.insert(metric.clone(), metric_report(&values));
        }
        arm_values.insert(arm_id.clone(), values_for_arm);

        let (gc, memory, cooccurrence) = aggregate_observations(scenario, &ok_trials);

        if let Some(info) = m.session.arms.iter().find(|a| &a.id == arm_id)
            && info.also_best
            && arm_id == "python-default"
        {
            summary_lines.push(format!(
                "{scenario}: Python's default --num-tokenizer setting is also its best-performing one (also_best = true); no separate python-best arm was needed."
            ));
        }

        arms.insert(
            arm_id.clone(),
            ArmReport {
                n_ok: ok_trials.len(),
                n_failed: failed_trials.len(),
                failed_trials,
                metrics,
                gc,
                memory,
                cooccurrence,
            },
        );
    }

    let deltas = compute_deltas(&arm_values);
    let (extra, mut extra_summary) = build_extra(scenario, m, &arm_values, &deltas);
    summary_lines.append(&mut extra_summary);

    (
        ScenarioReport {
            arms,
            deltas,
            extra,
        },
        summary_lines,
    )
}

/// Builds the [`Report`] from already-loaded manifests. Provenance is
/// evaluated first: if any manifest's `backend_kind` is not `real`, or any
/// manifest has no recorded GPU, `summary[0]` is a `NOT A FRONTEND
/// COMPARISON` banner naming every such backend kind/condition found
/// (PROJECT: projections stay projections until measured).
pub fn build_report(manifests: &[Manifest]) -> anyhow::Result<Report> {
    let sources: Vec<String> = manifests
        .iter()
        .map(|m| m.session.scenario.clone())
        .collect();
    let backend_kinds: Vec<BackendKind> = manifests.iter().map(|m| m.meta.backend_kind).collect();
    let gpus: Vec<Option<String>> = manifests.iter().map(|m| m.meta.gpu.clone()).collect();
    let comparison_valid = manifests
        .iter()
        .all(|m| m.meta.backend_kind == BackendKind::Real && m.meta.gpu.is_some());

    let mut summary = Vec::new();
    if !comparison_valid {
        let mut kinds: BTreeSet<String> = BTreeSet::new();
        for m in manifests {
            if m.meta.backend_kind != BackendKind::Real {
                kinds.insert(backend_kind_str(m.meta.backend_kind).to_string());
            }
            if m.meta.gpu.is_none() {
                kinds.insert("no-gpu".to_string());
            }
        }
        let kinds_joined = kinds.into_iter().collect::<Vec<_>>().join(", ");
        summary.push(format!(
            "NOT A FRONTEND COMPARISON: backend kind(s)/condition(s) [{kinds_joined}] present -- the numbers below are not a valid Python-vs-Rust frontend comparison."
        ));
    }

    let mut scenarios = BTreeMap::new();
    for m in manifests {
        let (scenario_report, mut scenario_summary) = build_scenario_report(m);
        scenarios.insert(m.session.scenario.clone(), scenario_report);
        summary.append(&mut scenario_summary);
    }

    Ok(Report {
        schema_version: manifest::SCHEMA_VERSION,
        generated_by: "crates/rsg-bench report".to_string(),
        sources,
        provenance: Provenance {
            backend_kinds,
            gpus,
            comparison_valid,
        },
        scenarios,
        summary,
    })
}

fn render_ci(mr: &MetricReport) -> String {
    if let Some(note) = &mr.ci_note {
        return if note == "n<2" {
            "n/a (n<2)".to_string()
        } else {
            format!("n/a ({note})")
        };
    }
    match (mr.ci95_lo, mr.ci95_hi) {
        (Some(lo), Some(hi)) => format!("[{lo:.4}, {hi:.4}]"),
        _ => "n/a".to_string(),
    }
}

fn fmt_opt(v: Option<f64>, decimals: usize) -> String {
    v.map(|x| format!("{x:.decimals$}"))
        .unwrap_or_else(|| "n/a".to_string())
}

fn fmt_pss(v: Option<f64>) -> String {
    v.map(|x| format!("{x:.0}"))
        .unwrap_or_else(|| "n/a (PSS needs Linux)".to_string())
}

fn render_s1_extras(out: &mut String, sr: &ScenarioReport) {
    out.push_str("**P99 TTFT and RPS per arm:**\n\n");
    out.push_str("| Arm | P99 TTFT (ms) | RPS |\n|---|---|---|\n");
    for (arm, ar) in &sr.arms {
        let ttft = ar.metrics.get("ttft_p99_ms").and_then(|m| m.mean);
        let rps = ar.metrics.get("rps").and_then(|m| m.mean);
        out.push_str(&format!(
            "| {arm} | {} | {} |\n",
            fmt_opt(ttft, 2),
            fmt_opt(rps, 2)
        ));
    }
    out.push('\n');
}

fn render_s3_extras(out: &mut String, sr: &ScenarioReport) {
    out.push_str("**Cold start:**\n\n");
    out.push_str(
        "| Arm | End-to-end startup (s) | Frontend cold start tail (s) | Frontend RSS (bytes) | Frontend PSS (bytes) |\n|---|---|---|---|---|\n",
    );
    for (arm, ar) in &sr.arms {
        let e2e = ar.metrics.get("e2e_ready_s").and_then(|m| m.mean);
        let tail = ar.metrics.get("frontend_tail_s").and_then(|m| m.mean);
        let rss = ar.metrics.get("frontend_rss_bytes").and_then(|m| m.mean);
        let pss = ar.metrics.get("frontend_pss_bytes").and_then(|m| m.mean);
        out.push_str(&format!(
            "| {arm} | {} | {} | {} | {} |\n",
            fmt_opt(e2e, 3),
            fmt_opt(tail, 3),
            fmt_opt(rss, 0),
            fmt_pss(pss),
        ));
    }
    out.push('\n');
}

fn render_sweep_table(out: &mut String, sr: &ScenarioReport) {
    let Some(candidates) = sr.extra.get("candidates").and_then(Value::as_array) else {
        return;
    };
    out.push_str("**Sweep candidates (D-09):**\n\n");
    out.push_str("| --num-tokenizer | Peak RPS |\n|---|---|\n");
    for c in candidates {
        let k = c.get("k").and_then(Value::as_u64).unwrap_or(0);
        let peak = c.get("peak_rps").and_then(Value::as_f64);
        out.push_str(&format!("| {k} | {} |\n", fmt_opt(peak, 2)));
    }
    out.push('\n');
}

fn render_curve_table(out: &mut String, sr: &ScenarioReport) {
    let Some(rows) = sr.extra.get("curve").and_then(Value::as_array) else {
        return;
    };
    if rows.is_empty() {
        return;
    }

    let mut arm_ids: BTreeSet<String> = BTreeSet::new();
    for row in rows {
        if let Some(arms) = row.get("arms").and_then(Value::as_object) {
            for k in arms.keys() {
                arm_ids.insert(k.clone());
            }
        }
    }
    let arm_ids: Vec<String> = arm_ids.into_iter().collect();

    out.push_str("**Saturation curve:**\n\n");
    let mut header = "| Level |".to_string();
    let mut sep = "|---|".to_string();
    for arm in &arm_ids {
        header.push_str(&format!(" {arm} achieved RPS | {arm} P99 TTFT (ms) |"));
        sep.push_str("---|---|");
    }
    out.push_str(&header);
    out.push('\n');
    out.push_str(&sep);
    out.push('\n');

    for row in rows {
        let label = row.get("label").and_then(Value::as_str).unwrap_or("");
        let mut line = format!("| {label} |");
        for arm in &arm_ids {
            let rps = row
                .pointer(&format!("/arms/{arm}/achieved_rps/mean"))
                .and_then(Value::as_f64);
            let ttft = row
                .pointer(&format!("/arms/{arm}/ttft_p99_ms/mean"))
                .and_then(Value::as_f64);
            line.push_str(&format!(" {} | {} |", fmt_opt(rps, 2), fmt_opt(ttft, 2)));
        }
        out.push_str(&line);
        out.push('\n');
    }
    out.push('\n');
}

fn render_deltas_table(out: &mut String, sr: &ScenarioReport) {
    out.push_str("**Rust vs Python deltas:**\n\n");
    out.push_str(
        "| Comparison | Metric | Diff | 95% CI | % | % 95% CI |\n|---|---|---|---|---|---|\n",
    );
    for (key, metrics) in &sr.deltas {
        for (metric, d) in metrics {
            out.push_str(&format!(
                "| {key} | {metric} | {:.4} | [{:.4}, {:.4}] | {:.2}% | [{:.2}%, {:.2}%] |\n",
                d.diff, d.ci95_lo, d.ci95_hi, d.pct, d.pct_lo, d.pct_hi
            ));
        }
    }
    out.push('\n');
}

fn render_gc_table(out: &mut String, sr: &ScenarioReport) {
    let any = sr.arms.values().any(|a| !a.gc.is_empty());
    if !any {
        return;
    }
    out.push_str("**GC pauses:**\n\n");
    out.push_str("| Arm | Role | Count | Total pause (ms) | P99 pause (ms) | Max pause (ms) |\n|---|---|---|---|---|---|\n");
    for (arm, ar) in &sr.arms {
        for (role, row) in &ar.gc {
            match row {
                GcAggRow::NotApplicable { not_applicable } => {
                    out.push_str(&format!(
                        "| {arm} | {role:?} | N/A ({not_applicable}) | | | |\n"
                    ));
                }
                GcAggRow::Stats {
                    count,
                    total_pause_ms,
                    p99_pause_ms,
                    max_pause_ms,
                } => {
                    out.push_str(&format!(
                        "| {arm} | {role:?} | {count:.2} | {total_pause_ms:.2} | {} | {} |\n",
                        fmt_opt(*p99_pause_ms, 2),
                        fmt_opt(*max_pause_ms, 2),
                    ));
                }
            }
        }
    }
    out.push('\n');
}

fn render_memory_table(out: &mut String, sr: &ScenarioReport) {
    let any = sr
        .arms
        .values()
        .any(|a| !a.memory.by_group.is_empty() || a.memory.tree.rss_bytes_mean.is_some());
    if !any {
        return;
    }
    out.push_str("**Memory:**\n\n");
    out.push_str(
        "| Arm | Group | RSS max (mean, bytes) | PSS max (mean, bytes) |\n|---|---|---|---|\n",
    );
    for (arm, ar) in &sr.arms {
        for (group, row) in &ar.memory.by_group {
            out.push_str(&format!(
                "| {arm} | {group:?} | {} | {} |\n",
                fmt_opt(row.rss_bytes_mean, 0),
                fmt_pss(row.pss_bytes_mean),
            ));
        }
        if ar.memory.tree.rss_bytes_mean.is_some() {
            out.push_str(&format!(
                "| {arm} | tree | {} | {} |\n",
                fmt_opt(ar.memory.tree.rss_bytes_mean, 0),
                fmt_pss(ar.memory.tree.pss_bytes_mean),
            ));
        }
    }
    out.push('\n');
}

fn render_cooccurrence_table(out: &mut String, sr: &ScenarioReport) {
    let any = sr.arms.values().any(|a| !a.cooccurrence.is_empty());
    if !any {
        return;
    }
    out.push_str("**GC/P99 co-occurrence:**\n\n");
    out.push_str(
        "| Arm | Group | Spike overlap (mean) | Non-spike overlap (mean) |\n|---|---|---|---|\n",
    );
    for (arm, ar) in &sr.arms {
        for (group, row) in &ar.cooccurrence {
            match row {
                CoOccAggRow::NotApplicable { not_applicable } => {
                    out.push_str(&format!(
                        "| {arm} | {group:?} | N/A ({not_applicable}) | |\n"
                    ));
                }
                CoOccAggRow::NoFirstToken { .. } => {
                    out.push_str(&format!(
                        "| {arm} | {group:?} | no first token observed | |\n"
                    ));
                }
                CoOccAggRow::Computed {
                    spike_overlap_rate_mean,
                    nonspike_overlap_rate_mean,
                } => {
                    let spike = spike_overlap_rate_mean
                        .map(|v| format!("{:.1}%", v * 100.0))
                        .unwrap_or_else(|| "n/a".to_string());
                    let nonspike = nonspike_overlap_rate_mean
                        .map(|v| format!("{:.1}%", v * 100.0))
                        .unwrap_or_else(|| "n/a".to_string());
                    out.push_str(&format!("| {arm} | {group:?} | {spike} | {nonspike} |\n"));
                }
            }
        }
    }
    out.push('\n');
}

fn render_failed_trials(out: &mut String, sr: &ScenarioReport) {
    let mut any_failed = false;
    for (arm, ar) in &sr.arms {
        if !ar.failed_trials.is_empty() {
            if !any_failed {
                out.push_str("**Failed trials:**\n\n");
                any_failed = true;
            }
            for f in &ar.failed_trials {
                out.push_str(&format!("- arm {arm}, trial {}: {}\n", f.index, f.error));
            }
        }
    }
    if any_failed {
        out.push('\n');
    }
}

fn render_scenario_section(out: &mut String, scenario: &str, sr: &ScenarioReport) {
    out.push_str("| Arm | OK | Failed |\n|---|---|---|\n");
    for (arm, ar) in &sr.arms {
        out.push_str(&format!("| {arm} | {} | {} |\n", ar.n_ok, ar.n_failed));
    }
    out.push('\n');

    let any_metrics = sr.arms.values().any(|a| !a.metrics.is_empty());
    if any_metrics {
        out.push_str("**Headline metrics:**\n\n");
        out.push_str("| Arm | Metric | Mean | 95% CI |\n|---|---|---|---|\n");
        for (arm, ar) in &sr.arms {
            for (metric, mr) in &ar.metrics {
                let mean_str = mr
                    .mean
                    .map(|v| format!("{v:.4}"))
                    .unwrap_or_else(|| "n/a".to_string());
                out.push_str(&format!(
                    "| {arm} | {metric} | {mean_str} | {} |\n",
                    render_ci(mr)
                ));
            }
        }
        out.push('\n');
    }

    if scenario == "s1_cancel" {
        render_s1_extras(out, sr);
    }

    if !sr.deltas.is_empty() {
        render_deltas_table(out, sr);
    }

    if scenario == "num_tokenizer_sweep" {
        render_sweep_table(out, sr);
    } else if scenario == "s2_saturation" {
        render_curve_table(out, sr);
    }

    if scenario == "s3_coldstart" {
        render_s3_extras(out, sr);
    }

    render_gc_table(out, sr);
    render_memory_table(out, sr);
    render_cooccurrence_table(out, sr);
    render_failed_trials(out, sr);
}

/// Renders the [`Report`] as markdown. Every number comes from `report`
/// itself, so the markdown can never disagree with the JSON it was derived
/// from.
pub fn render_markdown(report: &Report) -> String {
    let mut out = String::new();
    out.push_str("# Frontend Benchmark Report\n\n");

    out.push_str("## Summary\n\n");
    if report.summary.is_empty() {
        out.push_str("No summary lines.\n\n");
    } else {
        for line in &report.summary {
            out.push_str(&format!("- {line}\n"));
        }
        out.push('\n');
    }

    for (scenario, sr) in &report.scenarios {
        out.push_str(&format!("## {scenario}\n\n"));
        render_scenario_section(&mut out, scenario, sr);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins the exact phrase a reader sees for the Rust frontend's GC row
    /// (RESEARCH Open Question 4): "N/A (" plus
    /// [`crate::gclog::RUST_FRONTEND_GC_REASON`] verbatim, never a
    /// fabricated zero-event stats row.
    #[test]
    fn gc_row_not_applicable_renders_fixed_phrase() {
        let row = GcAggRow::NotApplicable {
            not_applicable: crate::gclog::RUST_FRONTEND_GC_REASON.to_string(),
        };
        let GcAggRow::NotApplicable { not_applicable } = &row else {
            unreachable!()
        };
        let rendered = format!("N/A ({not_applicable})");
        assert_eq!(rendered, "N/A (Rust frontend has no garbage collector)");
    }
}
