//! Scenario 1 (BENCH-03): 128 concurrent agents with random
//! cancellations, plugged into the shared orchestrator via
//! [`S1Runner`]/[`S1Args`].

use std::time::Duration;

use crate::loadgen::{self, AgentParams};
use crate::orchestrator::{Lifecycle, MeasuredWindow, TrialContext, TrialMeasurement, TrialRunner};

/// Scenario 1's own CLI flags (flattened alongside [`crate::orchestrator::SessionArgs`]).
/// Defaults mirror Phase 2's `scenarios.py::run_s1` exactly (D-02/D-06).
#[derive(Debug, Clone, clap::Args)]
pub struct S1Args {
    #[arg(long, default_value_t = 128)]
    pub agents: u32,
    #[arg(long, default_value_t = 120.0)]
    pub duration_s: f64,
    #[arg(long, default_value_t = 0.25)]
    pub cancel_fraction: f64,
    #[arg(long, default_value_t = 256)]
    pub max_tokens: u32,
    #[arg(long, default_value_t = 0.5)]
    pub think_max_s: f64,
    /// Also write one JSON line per request (send offset, TTFT, outcome) to
    /// `<trial_dir>/requests.jsonl`, for locating the tail within a run.
    #[arg(long)]
    pub requests_jsonl: bool,
}

/// A [`TrialRunner`] over [`loadgen::run_agents`].
pub struct S1Runner {
    pub args: S1Args,
}

impl TrialRunner for S1Runner {
    fn lifecycle(&self) -> Lifecycle {
        Lifecycle::HarnessManaged
    }

    fn workload(&self) -> serde_json::Value {
        serde_json::json!({
            "agents": self.args.agents,
            "duration_s": self.args.duration_s,
            "cancel_fraction": self.args.cancel_fraction,
            "max_tokens": self.args.max_tokens,
            "think_max_s": self.args.think_max_s,
        })
    }

    async fn run_trial(&self, ctx: &TrialContext<'_>) -> anyhow::Result<TrialMeasurement> {
        let model = ctx
            .model_id
            .clone()
            .ok_or_else(|| anyhow::anyhow!("TrialContext has no model_id"))?;
        let params = AgentParams {
            agents: self.args.agents,
            duration: Duration::from_secs_f64(self.args.duration_s),
            cancel_fraction: self.args.cancel_fraction,
            max_tokens: self.args.max_tokens,
            think_max: Duration::from_secs_f64(self.args.think_max_s),
            seed: ctx.seed,
            prompt_words: (16, 128),
        };
        let result = loadgen::run_agents(ctx.client, &ctx.base_url, &model, &params).await;
        let histograms = loadgen::histograms(&result);
        let summary = loadgen::summarize(&result);
        if self.args.requests_jsonl {
            write_requests_jsonl(&ctx.trial_dir.join("requests.jsonl"), &result)?;
        }

        Ok(TrialMeasurement {
            windows: vec![MeasuredWindow {
                label: "s1".to_string(),
                start_unix_ns: result.window_start_unix_ns,
                end_unix_ns: result.window_end_unix_ns,
                requests: result.records,
                histograms: Some(histograms),
            }],
            result: serde_json::to_value(summary)?,
        })
    }
}

/// One line per request: `{"send_ms", "ttft_ms", "outcome"}`, where
/// `send_ms` is the send time relative to the window start and `ttft_ms` is
/// `null` when no token arrived.
fn write_requests_jsonl(
    path: &std::path::Path,
    result: &loadgen::LoadResult,
) -> anyhow::Result<()> {
    use std::io::Write;
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
    for r in &result.records {
        let line = serde_json::json!({
            "send_ms": r.t_send_unix_ns.saturating_sub(result.window_start_unix_ns) as f64 / 1e6,
            "ttft_ms": r.ttft.map(|d| d.as_secs_f64() * 1e3),
            "outcome": r.outcome,
        });
        writeln!(out, "{line}")?;
    }
    out.flush()?;
    Ok(())
}
