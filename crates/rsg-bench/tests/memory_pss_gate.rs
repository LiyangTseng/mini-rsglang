//! Whole-process-tree memory sampling: the never-faked PSS gate (D-14,
//! RESEARCH Pitfall 1), grandchild inclusion via `bench-stub --spawn-child`,
//! and the background sampler's window summaries.

#[allow(dead_code)]
mod common;

use std::collections::BTreeMap;
use std::time::Duration;

use rsg_bench::client;
use rsg_bench::gclog::HookLog;
use rsg_bench::memory::{self, MemorySampler};
use rsg_bench::procs;
use rsg_bench::roles::{FrontendKind, Group, RoleMap};

/// A realistic `/proc/<pid>/smaps_rollup` fixture: a header line, then
/// `Rss`/`Pss`/`Pss_Anon`/`Pss_File`/`Pss_Shmem`/`Pss_Dirty`. `Pss:` itself
/// is `2048 kB` -> `2_097_152` bytes.
const FIXTURE: &str = "\
55f1a2b3c000-55f1a2cde000 r--p 00000000 00:00 0                  [rollup]
Rss:               12345 kB
Pss:                2048 kB
Pss_Anon:           1234 kB
Pss_File:            800 kB
Pss_Shmem:            14 kB
Pss_Dirty:             0 kB
";

#[test]
fn parse_smaps_rollup_pss_fixture() {
    let pss = memory::parse_smaps_rollup_pss(FIXTURE).expect("Pss line present");
    assert_eq!(pss, 2_097_152);
}

#[test]
fn parse_rejects_missing_or_bad() {
    let without_pss: String = FIXTURE
        .lines()
        .filter(|l| !l.trim_start().starts_with("Pss:"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        memory::parse_smaps_rollup_pss(&without_pss),
        None,
        "missing Pss: line must give None"
    );

    let bad_value = "Pss:          not-a-number kB\n";
    assert_eq!(
        memory::parse_smaps_rollup_pss(bad_value),
        None,
        "non-numeric Pss: value must give None"
    );
}

/// Proves the gate on whichever platform runs this (Mac: `None`
/// everywhere; Linux: a real `Some(> 0)` value for this test's own pid).
#[test]
fn pss_gate_platform() {
    let own_pid = std::process::id() as i32;

    if cfg!(target_os = "linux") {
        let pss = memory::read_pss(own_pid);
        assert!(
            matches!(pss, Some(v) if v > 0),
            "Linux read_pss(own pid) must be Some(>0), got {pss:?}"
        );
    } else {
        assert_eq!(
            memory::read_pss(own_pid),
            None,
            "non-Linux read_pss must be None"
        );
    }

    let mut sys = sysinfo::System::new();
    let sample = memory::sample_tree(&mut sys, own_pid);
    if !cfg!(target_os = "linux") {
        assert!(
            !sample.pss_available,
            "non-Linux sample_tree pss_available must be false"
        );
        for proc in &sample.procs {
            assert_eq!(
                proc.pss_bytes, None,
                "non-Linux pss_bytes must be None, got {proc:?}"
            );
        }
    }
}

/// `sample_tree(stub leader started with --spawn-child)` lists 2 pids, both
/// with `rss_bytes > 0`. The leader's parent link is this test process, and
/// the child's parent is the leader.
#[tokio::test(flavor = "multi_thread")]
async fn sample_tree_includes_grandchild() {
    let port = common::free_port();
    let log_path = common::unique_log_path();
    let spec = common::stub_spec(port, &log_path, &["--spawn-child"]);

    let handle = procs::launch(&spec, port).expect("launch bench-stub");
    let client = client::build_client().expect("build client");
    procs::wait_ready(
        &handle,
        &client,
        port,
        Duration::from_secs(10),
        Duration::from_millis(50),
    )
    .await
    .expect("bench-stub became ready");

    // Give the spawned child a moment to actually appear.
    tokio::time::sleep(Duration::from_millis(200)).await;

    let mut sys = sysinfo::System::new();
    let sample = memory::sample_tree(&mut sys, handle.leader_pid);
    assert_eq!(
        sample.procs.len(),
        2,
        "expected leader + spawned child, got {:?}",
        sample.procs
    );
    for proc in &sample.procs {
        assert!(
            proc.rss_bytes > 0,
            "{proc:?} must have rss_bytes > 0"
        );
    }

    let leader = sample
        .procs
        .iter()
        .find(|p| p.pid == handle.leader_pid)
        .expect("leader present in sample");
    let test_pid = std::process::id() as i32;
    assert_eq!(
        leader.parent,
        Some(test_pid),
        "leader's parent must be this test process"
    );

    let child = sample
        .procs
        .iter()
        .find(|p| p.pid != handle.leader_pid)
        .expect("child present in sample");
    assert_eq!(
        child.parent,
        Some(handle.leader_pid),
        "child's parent must be the leader"
    );

    let _ = procs::teardown(handle, Duration::from_secs(5)).await;
}

/// `MemorySampler` at a 50ms interval for 300ms gives >= 4 samples.
/// `memory_by_group` over `[first.t, last.t]` with every pid mapped to
/// `Group::Frontend` gives `Some` values for start/end/max, and
/// `growth = end - start`. The same call over a window with no samples
/// gives all-`None` summaries, never zeros.
#[tokio::test(flavor = "multi_thread")]
async fn sampler_window_summary() {
    let port = common::free_port();
    let log_path = common::unique_log_path();
    let spec = common::stub_spec(port, &log_path, &[]);

    let handle = procs::launch(&spec, port).expect("launch bench-stub");
    let client = client::build_client().expect("build client");
    procs::wait_ready(
        &handle,
        &client,
        port,
        Duration::from_secs(10),
        Duration::from_millis(50),
    )
    .await
    .expect("bench-stub became ready");

    let leader_pid = handle.leader_pid;
    let sampler = MemorySampler::start(leader_pid, Duration::from_millis(50));
    tokio::time::sleep(Duration::from_millis(300)).await;
    let samples = sampler.stop();
    assert!(
        samples.len() >= 4,
        "expected >= 4 samples in 300ms at a 50ms interval, got {}",
        samples.len()
    );

    // bench-stub's own process name is its binary name, "bench-stub";
    // classify it as the Rust frontend so every pid maps to Group::Frontend.
    let mut process_names: BTreeMap<i32, String> = BTreeMap::new();
    process_names.insert(leader_pid, "bench-stub".to_string());
    let hook = HookLog::default();
    let role_map = RoleMap::build(
        leader_pid,
        FrontendKind::Rust,
        &hook,
        &process_names,
        "bench-stub",
    );

    let first_t = samples.first().expect("non-empty samples").t_unix_ns;
    let last_t = samples.last().expect("non-empty samples").t_unix_ns;
    let (by_group, total) = memory::memory_by_group(&samples, &role_map, first_t, last_t);

    let frontend = by_group
        .get(&Group::Frontend)
        .expect("Frontend row present");
    assert!(frontend.rss_bytes.start.is_some());
    assert!(frontend.rss_bytes.end.is_some());
    assert!(frontend.rss_bytes.max.is_some());
    let growth = frontend.rss_bytes.growth.expect("growth present");
    assert_eq!(
        growth,
        frontend.rss_bytes.end.unwrap() as i64 - frontend.rss_bytes.start.unwrap() as i64
    );
    assert!(total.rss_bytes.start.is_some());

    let (empty_by_group, empty_total) =
        memory::memory_by_group(&samples, &role_map, last_t + 1, last_t + 2);
    for group_mem in empty_by_group.values() {
        assert_eq!(group_mem.rss_bytes.start, None);
        assert_eq!(group_mem.rss_bytes.end, None);
        assert_eq!(group_mem.rss_bytes.max, None);
        assert_eq!(group_mem.rss_bytes.growth, None);
        assert_eq!(group_mem.pss_bytes, None, "empty window must never fake PSS");
    }
    assert_eq!(empty_total.rss_bytes.start, None);
    assert_eq!(empty_total.pss_bytes, None);

    let _ = procs::teardown(handle, Duration::from_secs(5)).await;
}
