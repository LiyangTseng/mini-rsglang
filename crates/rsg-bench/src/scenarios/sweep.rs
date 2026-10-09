//! D-09 `--num-tokenizer` sweep pre-pass: one Scenario 2 trial per
//! candidate via the shared orchestrator, picking the candidate with the
//! highest peak RPS (ties toward the smallest candidate). Only the Python
//! frontend is swept -- D-10 fixes the Rust frontend at one configuration,
//! with no tuning knob to sweep.

use crate::orchestrator::Arm;
use crate::roles::FrontendKind;

/// Sweep's own CLI flags (flattened alongside
/// [`crate::orchestrator::SessionArgs`] and
/// [`crate::scenarios::s2_saturation::S2Args`]).
#[derive(Debug, Clone, clap::Args)]
pub struct SweepArgs {
    #[arg(long, value_delimiter = ',', default_value = "0,1,2,4")]
    pub candidates: Vec<u32>,
}

/// Rejects an empty or duplicate-containing candidate list, then sorts
/// ascending.
pub fn validate_candidates(candidates: &[u32]) -> anyhow::Result<Vec<u32>> {
    if candidates.is_empty() {
        anyhow::bail!("candidate list must not be empty");
    }
    let mut sorted = candidates.to_vec();
    sorted.sort_unstable();
    for i in 1..sorted.len() {
        if sorted[i] == sorted[i - 1] {
            anyhow::bail!("duplicate candidate: {}", sorted[i]);
        }
    }
    Ok(sorted)
}

/// Picks the `--num-tokenizer` candidate with the highest `Some` peak RPS,
/// breaking ties toward the smallest candidate (BENCH-07 ordering edge),
/// and ignoring `None` (failed) candidates. Errors when every candidate
/// failed.
pub fn pick_best(results: &[(u32, Option<f64>)]) -> anyhow::Result<u32> {
    let mut best: Option<(u32, f64)> = None;
    for &(candidate, peak) in results {
        let Some(peak) = peak else { continue };
        best = match best {
            None => Some((candidate, peak)),
            Some((best_candidate, best_peak)) => {
                if peak > best_peak || (peak == best_peak && candidate < best_candidate) {
                    Some((candidate, peak))
                } else {
                    Some((best_candidate, best_peak))
                }
            }
        };
    }
    best.map(|(candidate, _)| candidate)
        .ok_or_else(|| anyhow::anyhow!("no --num-tokenizer candidate succeeded"))
}

/// Builds one Python arm per validated candidate, in ascending order
/// (D-10: the Rust template is never used -- there is no Rust sweep).
pub fn sweep_arms(python_template: &str, candidates: &[u32]) -> Vec<Arm> {
    candidates
        .iter()
        .map(|&k| Arm {
            id: format!("python-nt{k}"),
            kind: FrontendKind::Python,
            num_tokenizer: Some(k),
            also_best: false,
            template: python_template.to_string(),
        })
        .collect()
}
