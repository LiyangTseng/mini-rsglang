//! Whole-process-tree RSS/PSS sampling, the same way for both frontends
//! (D-14). A direct port of `python/rsglang/profiling/procs.py::tree_memory`'s
//! three-way PSS gate (RESEARCH Pitfall 1): RSS everywhere via `sysinfo`;
//! PSS only on Linux, by hand-parsing `/proc/<pid>/smaps_rollup`'s `Pss:`
//! line (`sysinfo` has no PSS API at all); PSS is `None` for the *whole*
//! tree the moment any pid's read fails, never a partial sum, and never
//! `Some(0)` standing in for "unavailable" (T-07-11).

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sysinfo::{Pid as SysPid, ProcessesToUpdate, System, ThreadKind};

use crate::roles::{Group, RoleMap, group_of};

fn unix_ns_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// Parses `/proc/<pid>/smaps_rollup`'s `Pss:` line (format: `Pss:  1234 kB`,
/// in kB) into bytes. Reads only the exact `Pss:` key -- never
/// `Pss_Anon:`/`Pss_File:`/etc, which also start with `Pss` but are not the
/// aggregate. Returns `None` if the line is missing or its value doesn't
/// parse as an integer.
pub fn parse_smaps_rollup_pss(contents: &str) -> Option<u64> {
    for line in contents.lines() {
        let mut parts = line.split_whitespace();
        let Some(key) = parts.next() else { continue };
        if key != "Pss:" {
            continue;
        }
        let value = parts.next()?;
        return match value.parse::<u64>() {
            Ok(kb) => Some(kb * 1024),
            Err(_) => None,
        };
    }
    None
}

/// Reads one pid's PSS, in bytes. Linux-only (`sysinfo` has no PSS API);
/// `None` on a missing/unreadable file or a permission error -- never a
/// crash, and never a value for a pid outside the harness-spawned tree
/// (T-07-11: callers only ever pass pids from [`sample_tree`]).
#[cfg(target_os = "linux")]
pub fn read_pss(pid: i32) -> Option<u64> {
    let path = format!("/proc/{pid}/smaps_rollup");
    let contents = std::fs::read_to_string(path).ok()?;
    parse_smaps_rollup_pss(&contents)
}

/// Non-Linux twin: always `None` (there is no `/proc` to read).
#[cfg(not(target_os = "linux"))]
pub fn read_pss(_pid: i32) -> Option<u64> {
    None
}

/// One process's memory at one sample instant.
#[derive(Debug, Clone, Serialize)]
pub struct ProcSample {
    pub pid: i32,
    pub parent: Option<i32>,
    pub name: String,
    pub rss_bytes: u64,
    pub pss_bytes: Option<u64>,
}

/// One whole-tree sample.
#[derive(Debug, Clone, Serialize)]
pub struct TreeSample {
    pub t_unix_ns: u64,
    pub procs: Vec<ProcSample>,
    /// `true` only on Linux, and only when every included pid's
    /// [`read_pss`] returned `Some`. When `false`, every [`ProcSample`] in
    /// `procs` has `pss_bytes: None` -- the three-way gate applied to the
    /// whole tree at once, matching `procs.py::tree_memory`'s
    /// `pss_bytes = pss_total if (is_linux and pss_ok) else None`.
    pub pss_available: bool,
}

/// Walks the process tree from `root_pid` (inclusive), following parent
/// links breadth-first via one `sysinfo` refresh. A vanished pid (gone
/// between discovery and lookup) is skipped, not an error. RSS is summed
/// for every included pid; PSS is attempted only on Linux and only
/// reported if it succeeded for every included pid.
pub fn sample_tree(sys: &mut System, root_pid: i32) -> TreeSample {
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let is_linux = cfg!(target_os = "linux");

    let root_sys_pid = SysPid::from_u32(root_pid as u32);
    let mut included: Vec<i32> = Vec::new();
    if sys.process(root_sys_pid).is_some() {
        let mut queue = std::collections::VecDeque::new();
        let mut seen = std::collections::HashSet::new();
        queue.push_back(root_pid);
        seen.insert(root_pid);
        while let Some(pid) = queue.pop_front() {
            included.push(pid);
            for (sys_pid, proc) in sys.processes() {
                let candidate = sys_pid.as_u32() as i32;
                if seen.contains(&candidate) {
                    continue;
                }
                // On Linux, sysinfo surfaces each userland OS thread of a
                // multi-threaded process (e.g. tokio worker threads) as its
                // own pid-like entry with `parent()` pointing at the real
                // process. Skip them, or a tokio multi-threaded binary's
                // thread count inflates both the tree and the summed RSS
                // (every thread reports the whole process's RSS again).
                if matches!(proc.thread_kind(), Some(ThreadKind::Userland)) {
                    continue;
                }
                if proc.parent().map(|p| p.as_u32() as i32) == Some(pid) {
                    seen.insert(candidate);
                    queue.push_back(candidate);
                }
            }
        }
    }

    let mut procs = Vec::with_capacity(included.len());
    let mut all_pss_known = true;
    for pid in &included {
        let Some(proc) = sys.process(SysPid::from_u32(*pid as u32)) else {
            continue; // vanished between discovery and lookup
        };
        let rss_bytes = proc.memory();
        let name = proc.name().to_string_lossy().into_owned();
        let parent = proc.parent().map(|p| p.as_u32() as i32);
        let pss_bytes = if is_linux { read_pss(*pid) } else { None };
        if is_linux && pss_bytes.is_none() {
            all_pss_known = false;
        }
        procs.push(ProcSample {
            pid: *pid,
            parent,
            name,
            rss_bytes,
            pss_bytes,
        });
    }

    let pss_available = is_linux && all_pss_known && !procs.is_empty();
    if !pss_available {
        for p in &mut procs {
            p.pss_bytes = None;
        }
    }

    TreeSample {
        t_unix_ns: unix_ns_now(),
        procs,
        pss_available,
    }
}

/// Every pid's most-recently-observed name across `samples`.
pub fn process_names(samples: &[TreeSample]) -> BTreeMap<i32, String> {
    let mut out = BTreeMap::new();
    for sample in samples {
        for proc in &sample.procs {
            out.insert(proc.pid, proc.name.clone());
        }
    }
    out
}

/// Samples [`sample_tree`] on its own thread, immediately and then every
/// `interval`, until [`MemorySampler::stop`] is called.
pub struct MemorySampler {
    stop_flag: Arc<AtomicBool>,
    handle: Option<JoinHandle<Vec<TreeSample>>>,
    root_pid: i32,
}

impl MemorySampler {
    pub fn start(root_pid: i32, interval: Duration) -> MemorySampler {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let flag = stop_flag.clone();
        let handle = thread::spawn(move || {
            let mut sys = System::new();
            let mut samples = vec![sample_tree(&mut sys, root_pid)];
            while !flag.load(Ordering::Relaxed) {
                thread::sleep(interval);
                if flag.load(Ordering::Relaxed) {
                    break;
                }
                samples.push(sample_tree(&mut sys, root_pid));
            }
            samples
        });
        MemorySampler {
            stop_flag,
            handle: Some(handle),
            root_pid,
        }
    }

    /// Signals the sampling thread to stop, joins it, then takes one final
    /// sample before returning every sample collected.
    pub fn stop(mut self) -> Vec<TreeSample> {
        self.stop_flag.store(true, Ordering::Relaxed);
        let mut samples = self
            .handle
            .take()
            .expect("sampler thread handle present")
            .join()
            .expect("sampler thread did not panic");
        let mut sys = System::new();
        samples.push(sample_tree(&mut sys, self.root_pid));
        samples
    }
}

impl Drop for MemorySampler {
    fn drop(&mut self) {
        // Best-effort: let a sampler that was never explicitly stopped wind
        // down on its own, without blocking this drop on a join.
        self.stop_flag.store(true, Ordering::Relaxed);
    }
}

/// `start`/`end`/`max` over a closed time window, plus `growth = end -
/// start`. Every field is `None` for an empty window -- never a fabricated
/// zero.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MemSummary {
    pub start: Option<u64>,
    pub end: Option<u64>,
    pub max: Option<u64>,
    pub growth: Option<i64>,
}

fn summarize_u64(values: &[u64]) -> MemSummary {
    if values.is_empty() {
        return MemSummary {
            start: None,
            end: None,
            max: None,
            growth: None,
        };
    }
    let start = values[0];
    let end = *values.last().expect("non-empty");
    let max = *values.iter().max().expect("non-empty");
    MemSummary {
        start: Some(start),
        end: Some(end),
        max: Some(max),
        growth: Some(end as i64 - start as i64),
    }
}

/// `None` when `values` is empty, or when any entry is `None` (PSS is never
/// faked by summarizing only the entries that happened to succeed).
fn summarize_opt_u64(values: &[Option<u64>]) -> Option<MemSummary> {
    if values.is_empty() {
        return None;
    }
    let mut collected = Vec::with_capacity(values.len());
    for v in values {
        collected.push((*v)?);
    }
    Some(summarize_u64(&collected))
}

/// RSS (always) and PSS (`None` unless every window sample had PSS
/// available) window summaries for one [`Group`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupMemory {
    pub rss_bytes: MemSummary,
    pub pss_bytes: Option<MemSummary>,
}

const ALL_GROUPS: [Group; 4] = [
    Group::Frontend,
    Group::Launcher,
    Group::Scheduler,
    Group::Other,
];

/// Per-[`Group`] and whole-tree RSS/PSS window summaries over the closed
/// interval `[start_ns, end_ns]`. A window with no samples gives
/// all-`None` summaries for every group and the tree total.
pub fn memory_by_group(
    samples: &[TreeSample],
    role_map: &RoleMap,
    start_ns: u64,
    end_ns: u64,
) -> (BTreeMap<Group, GroupMemory>, GroupMemory) {
    let windowed: Vec<&TreeSample> = samples
        .iter()
        .filter(|s| s.t_unix_ns >= start_ns && s.t_unix_ns <= end_ns)
        .collect();

    let mut rss_series: BTreeMap<Group, Vec<u64>> =
        ALL_GROUPS.iter().map(|g| (*g, Vec::new())).collect();
    let mut pss_series: BTreeMap<Group, Vec<Option<u64>>> =
        ALL_GROUPS.iter().map(|g| (*g, Vec::new())).collect();
    let mut total_rss_series: Vec<u64> = Vec::new();
    let mut total_pss_series: Vec<Option<u64>> = Vec::new();

    for sample in &windowed {
        let mut group_rss: BTreeMap<Group, u64> = BTreeMap::new();
        let mut group_pss: BTreeMap<Group, u64> = BTreeMap::new();
        let mut tree_rss = 0u64;
        let mut tree_pss = 0u64;
        for proc in &sample.procs {
            let group = group_of(role_map.role_of(proc.pid));
            *group_rss.entry(group).or_insert(0) += proc.rss_bytes;
            tree_rss += proc.rss_bytes;
            if sample.pss_available {
                let pss = proc.pss_bytes.unwrap_or(0);
                *group_pss.entry(group).or_insert(0) += pss;
                tree_pss += pss;
            }
        }
        for group in ALL_GROUPS {
            rss_series
                .get_mut(&group)
                .expect("all groups pre-populated")
                .push(*group_rss.get(&group).unwrap_or(&0));
            let pss_value = sample
                .pss_available
                .then(|| *group_pss.get(&group).unwrap_or(&0));
            pss_series
                .get_mut(&group)
                .expect("all groups pre-populated")
                .push(pss_value);
        }
        total_rss_series.push(tree_rss);
        total_pss_series.push(sample.pss_available.then_some(tree_pss));
    }

    let mut by_group = BTreeMap::new();
    for group in ALL_GROUPS {
        let rss = summarize_u64(&rss_series[&group]);
        let pss = summarize_opt_u64(&pss_series[&group]);
        by_group.insert(
            group,
            GroupMemory {
                rss_bytes: rss,
                pss_bytes: pss,
            },
        );
    }

    let total = GroupMemory {
        rss_bytes: summarize_u64(&total_rss_series),
        pss_bytes: summarize_opt_u64(&total_pss_series),
    };

    (by_group, total)
}

/// Point-in-time RSS/PSS for one [`Group`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MemPoint {
    pub rss_bytes: u64,
    pub pss_bytes: Option<u64>,
}

/// The per-group and whole-tree memory at exactly one `sample` (e.g. "at
/// ready", for the S3 cold-start runner).
pub fn memory_at(sample: &TreeSample, role_map: &RoleMap) -> (BTreeMap<Group, MemPoint>, MemPoint) {
    let mut group_rss: BTreeMap<Group, u64> = BTreeMap::new();
    let mut group_pss: BTreeMap<Group, u64> = BTreeMap::new();
    let mut tree_rss = 0u64;
    let mut tree_pss = 0u64;
    for proc in &sample.procs {
        let group = group_of(role_map.role_of(proc.pid));
        *group_rss.entry(group).or_insert(0) += proc.rss_bytes;
        tree_rss += proc.rss_bytes;
        if sample.pss_available {
            let pss = proc.pss_bytes.unwrap_or(0);
            *group_pss.entry(group).or_insert(0) += pss;
            tree_pss += pss;
        }
    }

    let mut by_group = BTreeMap::new();
    for group in ALL_GROUPS {
        let rss_bytes = *group_rss.get(&group).unwrap_or(&0);
        let pss_bytes = sample
            .pss_available
            .then(|| *group_pss.get(&group).unwrap_or(&0));
        by_group.insert(
            group,
            MemPoint {
                rss_bytes,
                pss_bytes,
            },
        );
    }

    let total = MemPoint {
        rss_bytes: tree_rss,
        pss_bytes: sample.pss_available.then_some(tree_pss),
    };

    (by_group, total)
}
