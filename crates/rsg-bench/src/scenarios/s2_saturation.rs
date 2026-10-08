//! Scenario 2 (BENCH-04): the 32-token short-prompt RPS-vs-latency
//! saturation curve, in open-loop (Poisson) or closed-loop (fixed
//! concurrency) mode (D-01), plugged into the shared orchestrator via
//! [`S2Runner`]/[`S2Args`].

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::loadgen::{self, ClosedParams, OpenLoopParams, OutcomeCounts};
use crate::metrics::LatencySummary;
use crate::orchestrator::{Lifecycle, MeasuredWindow, TrialContext, TrialMeasurement, TrialRunner};

/// `--mode open|closed` (D-01).
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopMode {
    Open,
    Closed,
}

/// Scenario 2's own CLI flags (flattened alongside
/// [`crate::orchestrator::SessionArgs`]). Defaults bracket Phase 2's
/// measured 55.18 rps on a 512-request burst
/// (`docs/benchmarks/baseline-profile.json`).
#[derive(Debug, Clone, clap::Args)]
pub struct S2Args {
    #[arg(long, value_enum, default_value_t = LoopMode::Open)]
    pub mode: LoopMode,
    #[arg(long, value_delimiter = ',', default_value = "5,10,20,40,60,80")]
    pub rates: Vec<f64>,
    #[arg(long, value_delimiter = ',', default_value = "1,8,32,64,128")]
    pub concurrency: Vec<u32>,
    #[arg(long, default_value_t = 512)]
    pub requests_per_level: u32,
    #[arg(long, default_value_t = 32)]
    pub max_tokens: u32,
    #[arg(long, default_value_t = 32)]
    pub prompt_words_max: u32,
    #[arg(long, default_value_t = 500)]
    pub level_pause_ms: u64,
}

/// One load level's curve point: offered load, outcome counts, achieved
/// RPS and latency percentiles. `Serialize`/`Deserialize` so a manifest's
/// `result.curve` round-trips for downstream report/wrapper plans
/// (07-09/07-10).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurvePoint {
    pub label: String,
    pub offered: f64,
    pub mode: LoopMode,
    pub counts: OutcomeCounts,
    pub achieved_rps: Option<f64>,
    pub latency: LatencySummary,
}

/// Rejects an empty list, any value `<= 0`, any `NaN`, and exact-equality
/// duplicates, then sorts ascending. A duplicate level would otherwise
/// collide into one curve point.
pub fn validate_levels(levels: &[f64]) -> anyhow::Result<Vec<f64>> {
    if levels.is_empty() {
        anyhow::bail!("level list must not be empty");
    }
    for &level in levels {
        if level.is_nan() {
            anyhow::bail!("level must not be NaN");
        }
        if level <= 0.0 {
            anyhow::bail!("level must be > 0, got {level}");
        }
    }
    let mut sorted = levels.to_vec();
    sorted.sort_by(|a, b| {
        a.partial_cmp(b)
            .expect("non-NaN levels are totally ordered")
    });
    for i in 1..sorted.len() {
        if sorted[i] == sorted[i - 1] {
            anyhow::bail!("duplicate level: {}", sorted[i]);
        }
    }
    Ok(sorted)
}

/// Sorts `points` ascending by `offered` (stable: ties keep their original
/// relative order).
pub fn build_curve(mut points: Vec<CurvePoint>) -> Vec<CurvePoint> {
    points.sort_by(|a, b| {
        a.offered
            .partial_cmp(&b.offered)
            .expect("offered levels are non-NaN")
    });
    points
}

/// The maximum `achieved_rps` across `curve`, ignoring points with no
/// achieved RPS (e.g. a level where every request failed). `None` for an
/// empty curve or a curve with no achieved RPS anywhere.
pub fn peak_rps(curve: &[CurvePoint]) -> Option<f64> {
    curve
        .iter()
        .filter_map(|p| p.achieved_rps)
        .fold(None, |acc, rps| match acc {
            None => Some(rps),
            Some(best) if rps > best => Some(rps),
            Some(best) => Some(best),
        })
}

/// A [`TrialRunner`] over [`loadgen::run_open_loop`]/[`loadgen::run_closed`],
/// one call per validated load level, walked in ascending order.
pub struct S2Runner {
    pub args: S2Args,
}

impl TrialRunner for S2Runner {
    fn lifecycle(&self) -> Lifecycle {
        Lifecycle::HarnessManaged
    }

    fn workload(&self) -> serde_json::Value {
        serde_json::json!({
            "mode": self.args.mode,
            "rates": self.args.rates,
            "concurrency": self.args.concurrency,
            "requests_per_level": self.args.requests_per_level,
            "max_tokens": self.args.max_tokens,
            "prompt_words_max": self.args.prompt_words_max,
            "level_pause_ms": self.args.level_pause_ms,
        })
    }

    async fn run_trial(&self, ctx: &TrialContext<'_>) -> anyhow::Result<TrialMeasurement> {
        let model = ctx
            .model_id
            .clone()
            .ok_or_else(|| anyhow::anyhow!("TrialContext has no model_id"))?;

        let levels: Vec<f64> = match self.args.mode {
            LoopMode::Open => validate_levels(&self.args.rates)?,
            LoopMode::Closed => {
                let as_f64: Vec<f64> = self
                    .args
                    .concurrency
                    .iter()
                    .map(|&c| f64::from(c))
                    .collect();
                validate_levels(&as_f64)?
            }
        };

        let prompt_words = (1, self.args.prompt_words_max);
        let mut windows = Vec::with_capacity(levels.len());
        let mut points = Vec::with_capacity(levels.len());

        for (i, &level) in levels.iter().enumerate() {
            // P1: every arm's level `i` reuses the same seed offset, so a
            // given level draws identical prompts across arms for a fair
            // comparison.
            let seed = ctx.seed.wrapping_add(i as u64);

            let (label, result) = match self.args.mode {
                LoopMode::Open => {
                    let params = OpenLoopParams {
                        rate_rps: level,
                        requests: self.args.requests_per_level,
                        max_tokens: self.args.max_tokens,
                        prompt_words,
                        seed,
                    };
                    let result =
                        loadgen::run_open_loop(ctx.client, &ctx.base_url, &model, &params).await;
                    (format!("rate={level}"), result)
                }
                LoopMode::Closed => {
                    let concurrency = level as u32;
                    let params = ClosedParams {
                        concurrency,
                        requests: self.args.requests_per_level,
                        max_tokens: self.args.max_tokens,
                        prompt_words,
                        seed,
                    };
                    let result =
                        loadgen::run_closed(ctx.client, &ctx.base_url, &model, &params).await;
                    (format!("concurrency={concurrency}"), result)
                }
            };

            let histograms = loadgen::histograms(&result);
            let summary = loadgen::summarize(&result);

            windows.push(MeasuredWindow {
                label: label.clone(),
                start_unix_ns: result.window_start_unix_ns,
                end_unix_ns: result.window_end_unix_ns,
                requests: result.records,
                histograms: Some(histograms),
            });
            points.push(CurvePoint {
                label,
                offered: level,
                mode: self.args.mode,
                counts: summary.counts,
                achieved_rps: summary.rps,
                latency: summary.latency,
            });

            if i + 1 < levels.len() {
                tokio::time::sleep(Duration::from_millis(self.args.level_pause_ms)).await;
            }
        }

        let curve = build_curve(points);
        let peak = peak_rps(&curve);
        let result = serde_json::json!({
            "mode": self.args.mode,
            "curve": curve,
            "peak_rps": peak,
        });

        Ok(TrialMeasurement { windows, result })
    }
}
