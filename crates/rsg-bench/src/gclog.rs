//! Reader for phase 07-02's `gc_only`-mode hook files
//! (`python/rsglang/profiling/hook.py`), plus a direct port of Phase 2's
//! `analysis.py` GC-pause and GC-to-TTFT-correlation semantics (D-16),
//! so Phase 7's per-role tables are directly comparable to
//! `baseline-profile.json`.
//!
//! Every line is parsed defensively (T-07-10): malformed JSON, a missing
//! `kind`, a non-integer `pid`, and a `gc`/`proc` record seen before its
//! file's own `start` record are all counted in `malformed_lines`, never
//! `unwrap`/panic.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client::RequestRecord;
use crate::roles::{FrontendKind, Group, Role, RoleMap, group_of};

/// One GC pause, converted to the harness's Unix-nanosecond clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GcEvent {
    pub pid: i32,
    pub end_unix_ns: u64,
    pub duration: Duration,
    pub generation: i64,
}

/// Every `hook-*.jsonl` file read back from one `RSGLANG_PROFILE_DIR`
/// (`gc_only` mode, plan 07-02).
#[derive(Debug, Clone, Default)]
pub struct HookLog {
    pub present: bool,
    pub files: usize,
    pub gc_events: Vec<GcEvent>,
    pub proc_names: BTreeMap<i32, String>,
    pub start_pids: BTreeSet<i32>,
    pub mem_records: u64,
    pub malformed_lines: u64,
}

/// Per-file parse state: the first parseable `start` record's
/// (`t`, `wall`) pair, used to convert every later `t` in this file to a
/// Unix-nanosecond timestamp.
struct FileClock {
    t0: f64,
    wall0: f64,
}

/// Reads every `hook-*.jsonl` file in `dir`, in sorted file-name order. A
/// missing directory gives `present: false` with everything else empty.
pub fn read_hook_dir(dir: &Path) -> anyhow::Result<HookLog> {
    let mut log = HookLog {
        present: false,
        ..Default::default()
    };
    if !dir.is_dir() {
        return Ok(log);
    }
    log.present = true;

    let mut paths: Vec<_> = fs::read_dir(dir)
        .with_context(|| format!("read_dir {}", dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            name.starts_with("hook-") && name.ends_with(".jsonl")
        })
        .collect();
    paths.sort_by(|a, b| a.file_name().cmp(&b.file_name()));

    // The record with the greatest `wall` seen so far, per pid (across every
    // file): (wall, name).
    let mut best_proc: BTreeMap<i32, (f64, String)> = BTreeMap::new();

    for path in &paths {
        log.files += 1;
        let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        let mut clock: Option<FileClock> = None;

        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let value: Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => {
                    log.malformed_lines += 1;
                    continue;
                }
            };
            let Some(obj) = value.as_object() else {
                log.malformed_lines += 1;
                continue;
            };
            let Some(kind) = obj.get("kind").and_then(Value::as_str) else {
                log.malformed_lines += 1;
                continue;
            };
            let Some(pid) = obj.get("pid").and_then(Value::as_i64) else {
                log.malformed_lines += 1;
                continue;
            };
            let pid = pid as i32;

            match kind {
                "start" => {
                    if clock.is_some() {
                        continue; // only the first parseable start record counts
                    }
                    let (Some(t), Some(wall)) = (
                        obj.get("t").and_then(Value::as_f64),
                        obj.get("wall").and_then(Value::as_f64),
                    ) else {
                        log.malformed_lines += 1;
                        continue;
                    };
                    clock = Some(FileClock { t0: t, wall0: wall });
                    log.start_pids.insert(pid);
                }
                "gc" => {
                    let Some(FileClock { t0, wall0 }) = clock else {
                        log.malformed_lines += 1; // gc record before this file's start
                        continue;
                    };
                    let (Some(t), Some(duration_s), Some(generation)) = (
                        obj.get("t").and_then(Value::as_f64),
                        obj.get("duration_s").and_then(Value::as_f64),
                        obj.get("generation").and_then(Value::as_i64),
                    ) else {
                        log.malformed_lines += 1;
                        continue;
                    };
                    let end_unix_ns = ((wall0 + (t - t0)) * 1e9).round().max(0.0) as u64;
                    log.gc_events.push(GcEvent {
                        pid,
                        end_unix_ns,
                        duration: Duration::from_secs_f64(duration_s.max(0.0)),
                        generation,
                    });
                }
                "proc" => {
                    if clock.is_none() {
                        log.malformed_lines += 1; // proc record before this file's start
                        continue;
                    }
                    let (Some(name), Some(wall)) = (
                        obj.get("name").and_then(Value::as_str),
                        obj.get("wall").and_then(Value::as_f64),
                    ) else {
                        log.malformed_lines += 1;
                        continue;
                    };
                    let better = best_proc
                        .get(&pid)
                        .map(|(existing_wall, _)| wall > *existing_wall)
                        .unwrap_or(true);
                    if better {
                        best_proc.insert(pid, (wall, name.to_string()));
                    }
                }
                "mem" => {
                    log.mem_records += 1;
                }
                _ => {
                    // Unrecognized kind (e.g. "alloc_top" from full mode):
                    // ignored, never malformed -- "kind" was present and a
                    // valid string, this reader just has no use for it.
                }
            }
        }
    }

    for (pid, (_, name)) in best_proc {
        log.proc_names.insert(pid, name);
    }

    Ok(log)
}

/// Nearest-rank percentile (Phase 2's `analysis.py::percentile`): sort, take
/// index `ceil(p/100 * n) - 1`, clamped. `None` for an empty input.
pub fn nearest_rank(values: &[f64], p: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("GC/TTFT durations are never NaN"));
    let n = sorted.len();
    let idx = ((p / 100.0 * n as f64).ceil() as i64 - 1).clamp(0, n as i64 - 1) as usize;
    Some(sorted[idx])
}

/// `p50`/`p99`/`max` pause-duration percentiles, in ms. `None` for an empty
/// event set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PausePercentiles {
    pub p50: Option<f64>,
    pub p99: Option<f64>,
    pub max: Option<f64>,
}

/// GC-pause-per-role block, a direct port of `analysis.py::gc_stats` (minus
/// the raw `events` list, which this crate's callers don't need).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GcStats {
    pub count: u64,
    pub by_generation: BTreeMap<String, u64>,
    pub total_pause_ms: f64,
    pub pause_ms: PausePercentiles,
}

/// `count: 0`, `total_pause_ms: 0.0`, `by_generation` all zero, every
/// percentile `None` for an empty `events`.
pub fn gc_stats(events: &[&GcEvent]) -> GcStats {
    let mut by_generation = BTreeMap::new();
    by_generation.insert("0".to_string(), 0u64);
    by_generation.insert("1".to_string(), 0u64);
    by_generation.insert("2".to_string(), 0u64);

    let mut durations_ms = Vec::with_capacity(events.len());
    for e in events {
        let duration_ms = e.duration.as_secs_f64() * 1000.0;
        durations_ms.push(duration_ms);
        if let Some(c) = by_generation.get_mut(&e.generation.to_string()) {
            *c += 1;
        }
    }

    GcStats {
        count: events.len() as u64,
        by_generation,
        total_pause_ms: durations_ms.iter().sum(),
        pause_ms: PausePercentiles {
            p50: nearest_rank(&durations_ms, 50.0),
            p99: nearest_rank(&durations_ms, 99.0),
            max: nearest_rank(&durations_ms, 100.0),
        },
    }
}

/// Events whose `end_unix_ns` falls in the closed interval `[start_ns,
/// end_ns]`.
pub fn events_in_window(hook: &HookLog, start_ns: u64, end_ns: u64) -> Vec<&GcEvent> {
    hook.gc_events
        .iter()
        .filter(|e| e.end_unix_ns >= start_ns && e.end_unix_ns <= end_ns)
        .collect()
}

/// A per-role GC-pause table row: either computed stats, or (for the Rust
/// frontend, which has no GC) an explicit N/A reason. Serializes untagged,
/// so a [`GcRow::NotApplicable`] is `{"not_applicable": "<reason>"}` and a
/// [`GcRow::Stats`] is [`GcStats`]'s own fields directly.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum GcRow {
    Stats(GcStats),
    NotApplicable {
        #[serde(rename = "not_applicable")]
        reason: String,
    },
}

/// `RustFrontend` never has a garbage collector; every [`GcRow`] for it is
/// this fixed [`GcRow::NotApplicable`] reason (RESEARCH Open Question 4).
pub const RUST_FRONTEND_GC_REASON: &str = "Rust frontend has no garbage collector";

/// Per-role GC-pause stats over `[start_ns, end_ns]`. Emits a
/// [`GcRow::Stats`] row for every role that has at least one known Python
/// pid (from `hook.start_pids`), even with zero events in the window. For
/// [`FrontendKind::Rust`] it always also adds
/// `RustFrontend -> NotApplicable`.
pub fn gc_by_role(
    hook: &HookLog,
    role_map: &RoleMap,
    kind: FrontendKind,
    start_ns: u64,
    end_ns: u64,
) -> BTreeMap<Role, GcRow> {
    let windowed = events_in_window(hook, start_ns, end_ns);

    let mut roles_present: BTreeSet<Role> = BTreeSet::new();
    for &pid in &hook.start_pids {
        roles_present.insert(role_map.role_of(pid));
    }

    let mut out = BTreeMap::new();
    for role in roles_present {
        let events_for_role: Vec<&GcEvent> = windowed
            .iter()
            .copied()
            .filter(|e| role_map.role_of(e.pid) == role)
            .collect();
        out.insert(role, GcRow::Stats(gc_stats(&events_for_role)));
    }

    if kind == FrontendKind::Rust {
        out.insert(
            Role::RustFrontend,
            GcRow::NotApplicable {
                reason: RUST_FRONTEND_GC_REASON.to_string(),
            },
        );
    }

    out
}

/// GC-pause/P99-TTFT-spike co-occurrence (D-16), a direct port of
/// `analysis.py::gc_ttft_correlation`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CoOccurrence {
    pub p99_ttft_ms: f64,
    pub spike_requests: u64,
    pub spike_with_gc: u64,
    pub nonspike_requests: u64,
    pub nonspike_with_gc: u64,
    pub spike_overlap_rate: Option<f64>,
    pub nonspike_overlap_rate: Option<f64>,
}

/// A per-group co-occurrence table row: computed, "no request observed a
/// first token" (so there is nothing to correlate), or (the Rust frontend)
/// not applicable because it has no GC at all.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CoOccurrenceRow {
    Computed(CoOccurrence),
    NoFirstToken,
    NotApplicable {
        #[serde(rename = "not_applicable")]
        reason: String,
    },
}

/// `None` when no request in `requests` observed a first token (`ttft`).
/// Otherwise: TTFT in ms is `ttft` for every request with `Some(ttft)`; the
/// spike threshold is the nearest-rank P99 of those TTFTs; a request is a
/// spike when its TTFT is at or above that threshold (ties count, never
/// fewer than the nearest-rank index). A pause `(pause_start, pause_end)`
/// overlaps a request when `pause_start < t_first && pause_end > t_send`,
/// both strict: a pause that only touches a window endpoint does not count.
pub fn cooccurrence(requests: &[RequestRecord], gc_events: &[&GcEvent]) -> Option<CoOccurrence> {
    let pairs: Vec<(&RequestRecord, u64, f64)> = requests
        .iter()
        .filter_map(|r| {
            r.ttft.map(|ttft| {
                let t_first_ns = r.t_send_unix_ns + ttft.as_nanos() as u64;
                let ttft_ms = ttft.as_secs_f64() * 1000.0;
                (r, t_first_ns, ttft_ms)
            })
        })
        .collect();
    if pairs.is_empty() {
        return None;
    }

    let ttfts: Vec<f64> = pairs.iter().map(|(_, _, ms)| *ms).collect();
    let threshold = nearest_rank(&ttfts, 99.0)?;

    let pauses: Vec<(u64, u64)> = gc_events
        .iter()
        .map(|e| {
            let pause_start = e.end_unix_ns.saturating_sub(e.duration.as_nanos() as u64);
            (pause_start, e.end_unix_ns)
        })
        .collect();

    let mut spike_requests = 0u64;
    let mut spike_with_gc = 0u64;
    let mut nonspike_requests = 0u64;
    let mut nonspike_with_gc = 0u64;

    for (r, t_first_ns, ttft_ms) in &pairs {
        let overlaps = pauses
            .iter()
            .any(|&(start, end)| start < *t_first_ns && end > r.t_send_unix_ns);
        if *ttft_ms >= threshold {
            spike_requests += 1;
            if overlaps {
                spike_with_gc += 1;
            }
        } else {
            nonspike_requests += 1;
            if overlaps {
                nonspike_with_gc += 1;
            }
        }
    }

    Some(CoOccurrence {
        p99_ttft_ms: threshold,
        spike_requests,
        spike_with_gc,
        nonspike_requests,
        nonspike_with_gc,
        spike_overlap_rate: if spike_requests > 0 {
            Some(spike_with_gc as f64 / spike_requests as f64)
        } else {
            None
        },
        nonspike_overlap_rate: if nonspike_requests > 0 {
            Some(nonspike_with_gc as f64 / nonspike_requests as f64)
        } else {
            None
        },
    })
}

fn row_from(co: Option<CoOccurrence>) -> CoOccurrenceRow {
    match co {
        Some(c) => CoOccurrenceRow::Computed(c),
        None => CoOccurrenceRow::NoFirstToken,
    }
}

/// Co-occurrence by [`Group`] (`Frontend` and `Scheduler` keys only -- the
/// shared-backend floor Phase 2 asked to report separately).
///
/// `Frontend`: for [`FrontendKind::Rust`] the row is always
/// [`CoOccurrenceRow::NotApplicable`] (no GC in the Rust frontend). For
/// [`FrontendKind::Python`] it is computed over GC events from
/// `ApiServer`/`Tokenizer` pids in `[start_ns, end_ns]`.
///
/// `Scheduler`: computed over `Scheduler` pids, for both kinds.
pub fn cooccurrence_by_group(
    requests: &[RequestRecord],
    hook: &HookLog,
    role_map: &RoleMap,
    kind: FrontendKind,
    start_ns: u64,
    end_ns: u64,
) -> BTreeMap<Group, CoOccurrenceRow> {
    let windowed = events_in_window(hook, start_ns, end_ns);
    let mut out = BTreeMap::new();

    if kind == FrontendKind::Rust {
        out.insert(
            Group::Frontend,
            CoOccurrenceRow::NotApplicable {
                reason: RUST_FRONTEND_GC_REASON.to_string(),
            },
        );
    } else {
        let events: Vec<&GcEvent> = windowed
            .iter()
            .copied()
            .filter(|e| group_of(role_map.role_of(e.pid)) == Group::Frontend)
            .collect();
        out.insert(Group::Frontend, row_from(cooccurrence(requests, &events)));
    }

    let scheduler_events: Vec<&GcEvent> = windowed
        .iter()
        .copied()
        .filter(|e| role_map.role_of(e.pid) == Role::Scheduler)
        .collect();
    out.insert(
        Group::Scheduler,
        row_from(cooccurrence(requests, &scheduler_events)),
    );

    out
}
