//! The configurable load generator (BENCH-02, D-01): Scenario 1's
//! closed-loop seeded agents with think time and client-side cancellation
//! (D-02, BENCH-03), sharing a hdrhistogram recording path.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::client::{CancelPlan, ChatRequest, Outcome, RequestRecord, stream_chat};
use crate::metrics::{LatencyHistograms, LatencySummary};
use crate::rng::SplitMix64;

fn unix_ns_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// Phase 2's 64-word prompt vocabulary (`python/rsglang/profiling/scenarios.py`),
/// in its exact order. Both frontends receive identical prompt text for a
/// given seed, which is what a fair comparison needs.
pub const WORDS: [&str; 64] = [
    "the", "quick", "brown", "fox", "jumps", "over", "lazy", "dog", "a", "an", "and", "or", "but",
    "if", "then", "else", "cat", "sparrow", "whale", "fish", "run", "jump", "walk", "talk", "red",
    "blue", "green", "yellow", "big", "small", "fast", "slow", "happy", "sad", "angry", "calm",
    "bright", "dark", "light", "heavy", "water", "fire", "earth", "air", "tree", "flower",
    "river", "mountain", "book", "pen", "paper", "table", "chair", "door", "window", "wall",
    "time", "space", "mind", "body", "soul", "heart", "hand", "eye",
];

/// Joins `range_inclusive_u32(min_words, max_words)` words, each chosen
/// uniformly from [`WORDS`].
pub fn make_prompt(rng: &mut SplitMix64, min_words: u32, max_words: u32) -> String {
    let n = rng.range_inclusive_u32(min_words, max_words);
    (0..n)
        .map(|_| WORDS[rng.range_inclusive_u32(0, (WORDS.len() - 1) as u32) as usize])
        .collect::<Vec<_>>()
        .join(" ")
}

/// Scenario 1's closed-loop agent workload (D-02/D-06). Defaults mirror
/// Phase 2's `scenarios.py::run_s1` exactly.
#[derive(Debug, Clone, Copy)]
pub struct AgentParams {
    pub agents: u32,
    pub duration: Duration,
    pub cancel_fraction: f64,
    pub max_tokens: u32,
    pub think_max: Duration,
    pub seed: u64,
    /// Inclusive (min, max) word-count range for a generated prompt.
    pub prompt_words: (u32, u32),
}

impl Default for AgentParams {
    fn default() -> Self {
        Self {
            agents: 128,
            duration: Duration::from_secs_f64(120.0),
            cancel_fraction: 0.25,
            max_tokens: 256,
            think_max: Duration::from_secs_f64(0.5),
            seed: 42,
            prompt_words: (16, 128),
        }
    }
}

/// One agent's planned request: how long it thinks before sending, whether
/// (and when) it cancels, and the prompt it sends.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedRequest {
    pub think: Duration,
    pub cancel: CancelPlan,
    pub prompt: String,
}

/// Draws one planned request in a fixed order (think, then the cancel coin,
/// then — only when cancelling — `cancel_after`, then the prompt), matching
/// Phase 2's `plan_agent_request` draw order exactly so a seed reproduces
/// the same workload. `cancel_after == 0` models a headers-only/prefill
/// abort; `cancel_after == k > 0` models a mid-stream abort after `k`
/// chunks (D-02).
pub fn plan_agent_request(rng: &mut SplitMix64, params: &AgentParams) -> PlannedRequest {
    let think_s = rng.uniform_f64(0.0, params.think_max.as_secs_f64());
    let think = Duration::from_secs_f64(think_s.max(0.0));

    let cancel = rng.next_f64() < params.cancel_fraction;
    let cancel_plan = if cancel {
        let half = params.max_tokens / 2;
        let cancel_after = rng.range_inclusive_u32(0, half);
        if cancel_after == 0 {
            CancelPlan::AfterHeaders
        } else {
            CancelPlan::AfterChunks(cancel_after)
        }
    } else {
        CancelPlan::None
    };

    let prompt = make_prompt(rng, params.prompt_words.0, params.prompt_words.1);

    PlannedRequest {
        think,
        cancel: cancel_plan,
        prompt,
    }
}

/// Everything recorded about one load-generator run.
#[derive(Debug, Clone)]
pub struct LoadResult {
    pub records: Vec<RequestRecord>,
    pub window_start_unix_ns: u64,
    pub window_end_unix_ns: u64,
    pub window: Duration,
}

/// Drives `params.agents` closed-loop agents for `params.duration`. Each
/// agent owns its own `SplitMix64` stream (forked from `params.seed` by
/// agent index) and repeats: think, then one `stream_chat`. A failed
/// request never aborts an agent (Phase 2 rule: failures are recorded
/// outcomes, not errors that stop the loop).
pub async fn run_agents(
    client: &reqwest::Client,
    base_url: &str,
    model: &str,
    params: &AgentParams,
) -> LoadResult {
    let window_start_unix_ns = unix_ns_now();
    let t_start = std::time::Instant::now();
    let deadline = t_start + params.duration;

    let mut tasks = Vec::with_capacity(params.agents as usize);
    for i in 0..params.agents {
        let mut rng = SplitMix64::new(params.seed).fork(u64::from(i));
        let client = client.clone();
        let base_url = base_url.to_string();
        let model = model.to_string();
        let params = *params;
        tasks.push(tokio::spawn(async move {
            let mut records = Vec::new();
            while std::time::Instant::now() < deadline {
                let planned = plan_agent_request(&mut rng, &params);
                tokio::time::sleep(planned.think).await;
                let req = ChatRequest {
                    model: model.clone(),
                    prompt: planned.prompt,
                    max_tokens: params.max_tokens,
                };
                let record = stream_chat(&client, &base_url, &req, planned.cancel).await;
                records.push(record);
            }
            records
        }));
    }

    let mut records = Vec::new();
    for task in tasks {
        if let Ok(mut agent_records) = task.await {
            records.append(&mut agent_records);
        }
    }

    let window_end_unix_ns = unix_ns_now();
    let window = t_start.elapsed();
    LoadResult {
        records,
        window_start_unix_ns,
        window_end_unix_ns,
        window,
    }
}

/// Client-observed outcome counts for one [`LoadResult`].
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct OutcomeCounts {
    pub sent: u64,
    pub completed: u64,
    pub cancelled: u64,
    pub failed: u64,
}

/// A summarized [`LoadResult`]: outcome counts, latency percentiles, and
/// throughput.
#[derive(Debug, Clone, Serialize)]
pub struct LoadSummary {
    pub counts: OutcomeCounts,
    pub latency: LatencySummary,
    /// `completed / window_s`. `None` for a zero-length window.
    pub rps: Option<f64>,
    /// `sum(chunks) / window_s`. `None` for a zero-length window.
    pub output_chunks_per_s: Option<f64>,
}

/// Builds the TTFT/ITL/E2E histograms for every record in `result`.
pub fn histograms(result: &LoadResult) -> LatencyHistograms {
    let mut hist = LatencyHistograms::new();
    for record in &result.records {
        hist.record(record);
    }
    hist
}

/// Summarizes a [`LoadResult`]: outcome counts, latency percentiles
/// (via [`histograms`]), RPS, and output chunks/s.
pub fn summarize(result: &LoadResult) -> LoadSummary {
    let mut counts = OutcomeCounts::default();
    let mut chunks_sum: u64 = 0;
    for record in &result.records {
        counts.sent += 1;
        chunks_sum += u64::from(record.chunks);
        match record.outcome {
            Outcome::Completed => counts.completed += 1,
            Outcome::Cancelled => counts.cancelled += 1,
            Outcome::Failed => counts.failed += 1,
        }
    }

    let window_s = result.window.as_secs_f64();
    let (rps, output_chunks_per_s) = if window_s > 0.0 {
        (
            Some(counts.completed as f64 / window_s),
            Some(chunks_sum as f64 / window_s),
        )
    } else {
        (None, None)
    };

    LoadSummary {
        counts,
        latency: histograms(result).summary(),
        rps,
        output_chunks_per_s,
    }
}

/// Scenario 2's open-loop Poisson arrival workload.
#[derive(Debug, Clone, Copy)]
pub struct OpenLoopParams {
    pub rate_rps: f64,
    pub requests: u32,
    pub max_tokens: u32,
    pub prompt_words: (u32, u32),
    pub seed: u64,
}

/// Scenario 2's fixed-concurrency closed workload.
#[derive(Debug, Clone, Copy)]
pub struct ClosedParams {
    pub concurrency: u32,
    pub requests: u32,
    pub max_tokens: u32,
    pub prompt_words: (u32, u32),
    pub seed: u64,
}

/// Accumulates `n` exponential inter-arrival gaps (rate `rate_rps`/s) from a
/// fresh `SplitMix64::new(seed)` stream into absolute offsets from a common
/// start time. Offsets are non-decreasing by construction (each gap is
/// `>= 0`).
pub fn poisson_offsets(rate_rps: f64, n: usize, seed: u64) -> Vec<Duration> {
    let mut rng = SplitMix64::new(seed);
    let mut offsets = Vec::with_capacity(n);
    let mut acc_s = 0.0;
    for _ in 0..n {
        acc_s += rng.exp(rate_rps);
        offsets.push(Duration::from_secs_f64(acc_s));
    }
    offsets
}

/// Sends `params.requests` requests at Poisson arrival times and never
/// waits for a response before scheduling the next arrival (D-01). Prompts
/// are precomputed from a stream decorrelated (via `fork`) from the
/// arrival-time stream, so changing `max_tokens` or `prompt_words` never
/// perturbs the schedule.
pub async fn run_open_loop(
    client: &reqwest::Client,
    base_url: &str,
    model: &str,
    params: &OpenLoopParams,
) -> LoadResult {
    let window_start_unix_ns = unix_ns_now();
    let t_start = std::time::Instant::now();
    let base = tokio::time::Instant::now();

    let offsets = poisson_offsets(params.rate_rps, params.requests as usize, params.seed);
    let mut prompt_rng = SplitMix64::new(params.seed).fork(1);

    let mut tasks = Vec::with_capacity(offsets.len());
    for offset in offsets {
        let prompt = make_prompt(&mut prompt_rng, params.prompt_words.0, params.prompt_words.1);
        let client = client.clone();
        let base_url = base_url.to_string();
        let model = model.to_string();
        let max_tokens = params.max_tokens;
        let deadline = base + offset;
        tasks.push(tokio::spawn(async move {
            tokio::time::sleep_until(deadline).await;
            let req = ChatRequest {
                model,
                prompt,
                max_tokens,
            };
            stream_chat(&client, &base_url, &req, CancelPlan::None).await
        }));
    }

    let mut records = Vec::with_capacity(tasks.len());
    for task in tasks {
        if let Ok(record) = task.await {
            records.push(record);
        }
    }

    let window_end_unix_ns = unix_ns_now();
    let window = t_start.elapsed();
    LoadResult {
        records,
        window_start_unix_ns,
        window_end_unix_ns,
        window,
    }
}

/// Runs `params.concurrency` worker tasks that pull request indices from a
/// shared counter until `params.requests` have been claimed. Each index's
/// prompt is derived from `SplitMix64::new(seed).fork(index)`, so the
/// workload is independent of which worker claims which index.
pub async fn run_closed(
    client: &reqwest::Client,
    base_url: &str,
    model: &str,
    params: &ClosedParams,
) -> LoadResult {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    let window_start_unix_ns = unix_ns_now();
    let t_start = std::time::Instant::now();

    let next_index = Arc::new(AtomicU32::new(0));
    let mut workers = Vec::with_capacity(params.concurrency as usize);
    for _ in 0..params.concurrency {
        let client = client.clone();
        let base_url = base_url.to_string();
        let model = model.to_string();
        let next_index = next_index.clone();
        let requests = params.requests;
        let max_tokens = params.max_tokens;
        let seed = params.seed;
        let prompt_words = params.prompt_words;
        workers.push(tokio::spawn(async move {
            let mut records = Vec::new();
            loop {
                let idx = next_index.fetch_add(1, Ordering::SeqCst);
                if idx >= requests {
                    break;
                }
                let mut rng = SplitMix64::new(seed).fork(u64::from(idx));
                let prompt = make_prompt(&mut rng, prompt_words.0, prompt_words.1);
                let req = ChatRequest {
                    model: model.clone(),
                    prompt,
                    max_tokens,
                };
                let record = stream_chat(&client, &base_url, &req, CancelPlan::None).await;
                records.push(record);
            }
            records
        }));
    }

    let mut records = Vec::new();
    for worker in workers {
        if let Ok(mut worker_records) = worker.await {
            records.append(&mut worker_records);
        }
    }

    let window_end_unix_ns = unix_ns_now();
    let window = t_start.elapsed();
    LoadResult {
        records,
        window_start_unix_ns,
        window_end_unix_ns,
        window,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_match_phase_2_list() {
        assert_eq!(WORDS.len(), 64);
        assert_eq!(WORDS[0], "the");
        assert_eq!(WORDS[63], "eye");
    }

    #[test]
    fn poisson_offsets_is_non_decreasing_with_bounded_mean_gap() {
        let offsets = poisson_offsets(50.0, 2000, 1);
        for i in 1..offsets.len() {
            assert!(offsets[i] >= offsets[i - 1], "offsets not non-decreasing at {i}");
        }
        let last = offsets.last().expect("non-empty");
        let mean_gap_ms = last.as_secs_f64() * 1000.0 / offsets.len() as f64;
        assert!(
            (18.0..=22.0).contains(&mean_gap_ms),
            "mean gap {mean_gap_ms}ms not within 10% of 20ms"
        );
    }
}
