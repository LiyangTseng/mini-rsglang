//! `gclog_reads_real_gc_only_hook_files` (Task 1): real Python `gc_only`-mode
//! hook files (phase 07-02) read back through `gclog::read_hook_dir`,
//! classified by `roles::RoleMap`, and summarized by `gclog::gc_by_role` --
//! no py-spy, no `sysinfo`, just the files the Python hook actually writes.
//! Fails (never skips) when no Python interpreter is available: the Phase 1
//! gate precedent.
//!
//! Task 3 adds the BENCH-08 edge-rule tests below it, against synthetic
//! `HookLog`/`RequestRecord` fixtures.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rsg_bench::client::{Outcome, RequestRecord};
use rsg_bench::gclog::{self, CoOccurrenceRow, GcEvent, GcRow, HookLog};
use rsg_bench::roles::{FrontendKind, Group, Role, RoleMap};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn repo_root() -> PathBuf {
    // crates/rsg-bench -> crates -> repo root
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/ dir")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn python_interpreter() -> PathBuf {
    if let Ok(p) = std::env::var("RSG_BENCH_PYTHON") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return path;
        }
    }
    let default = repo_root().join(".venv").join("bin").join("python");
    if default.is_file() {
        return default;
    }
    panic!(
        "no Python interpreter found (set RSG_BENCH_PYTHON, or run scripts/bootstrap_mac_env.sh)"
    );
}

fn unique_tmp_dir(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("rsg-bench-gclog-{tag}-{pid}-{n}"));
    fs::create_dir_all(&dir).expect("create unique tmp dir");
    dir
}

fn unix_ns_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after unix epoch")
        .as_nanos() as u64
}

fn line_value<'a>(stdout: &'a str, prefix: &str) -> &'a str {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix(prefix))
        .unwrap_or_else(|| panic!("no {prefix:?} line in stdout: {stdout:?}"))
}

#[test]
fn gclog_reads_real_gc_only_hook_files() {
    let python = python_interpreter();
    let repo_root = repo_root();
    let python_dir = repo_root.join("python");

    let tmp = unique_tmp_dir("run");
    let shim_dir = tmp.join("shim");
    let mods_dir = tmp.join("mods");
    let hook_dir = tmp.join("hook");
    fs::create_dir_all(&mods_dir).expect("create mods dir");

    // Write the sitecustomize shim via the real hook.py's write_shim().
    let write_shim_output = Command::new(&python)
        .arg("-c")
        .arg("import sys; from rsglang.profiling.hook import write_shim; write_shim(sys.argv[1])")
        .arg(&shim_dir)
        .env("PYTHONPATH", &python_dir)
        .output()
        .expect("run write_shim");
    assert!(
        write_shim_output.status.success(),
        "write_shim failed: {}",
        String::from_utf8_lossy(&write_shim_output.stderr)
    );

    // The spawn-child target, importable by name from PYTHONPATH.
    fs::write(
        mods_dir.join("rsgb_child.py"),
        "def work():\n    import gc, time\n    gc.collect()\n    time.sleep(1.0)\n",
    )
    .expect("write rsgb_child.py");

    let pythonpath = format!(
        "{}:{}:{}",
        shim_dir.display(),
        mods_dir.display(),
        python_dir.display()
    );

    let parent_script = r#"
import multiprocessing as mp
import gc
import os
import time

import rsgb_child

if __name__ == "__main__":
    mp.set_start_method("spawn", force=True)
    p = mp.Process(target=rsgb_child.work, name="rsgbench-TP0-scheduler")
    p.start()
    print(f"PARENT_PID={os.getpid()}")
    print(f"CHILD_PID={p.pid}")
    p.join()
    gc.collect()
    time.sleep(0.3)
"#;

    let before = unix_ns_now();
    let output = Command::new(&python)
        .arg("-c")
        .arg(parent_script)
        .env("RSGLANG_PROFILE_DIR", &hook_dir)
        .env("RSGLANG_PROFILE_MODE", "gc_only")
        .env("RSGLANG_PROFILE_INTERVAL_S", "0.1")
        .env("PYTHONPATH", &pythonpath)
        .output()
        .expect("run parent script");
    let after = unix_ns_now();

    assert!(
        output.status.success(),
        "parent script failed: stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parent_pid: i32 = line_value(&stdout, "PARENT_PID=")
        .parse()
        .expect("PARENT_PID is an integer");
    let child_pid: i32 = line_value(&stdout, "CHILD_PID=")
        .parse()
        .expect("CHILD_PID is an integer");

    let log = gclog::read_hook_dir(&hook_dir).expect("read_hook_dir");
    assert!(log.present, "hook dir must exist after the run");
    assert_eq!(
        log.mem_records, 0,
        "gc_only mode must write zero mem records"
    );
    assert_eq!(
        log.proc_names.get(&child_pid).map(String::as_str),
        Some("rsgbench-TP0-scheduler"),
        "child's last proc record name, proc_names = {:?}",
        log.proc_names
    );

    let empty_names: BTreeMap<i32, String> = BTreeMap::new();
    let role_map = RoleMap::build(
        parent_pid,
        FrontendKind::Python,
        &log,
        &empty_names,
        "rsg-server",
    );
    assert_eq!(role_map.role_of(parent_pid), Role::ApiServer);
    assert_eq!(role_map.role_of(child_pid), Role::Scheduler);

    let by_role = gclog::gc_by_role(&log, &role_map, FrontendKind::Python, before, after);
    for role in [Role::ApiServer, Role::Scheduler] {
        match by_role.get(&role) {
            Some(GcRow::Stats(stats)) => {
                assert!(
                    stats.count >= 1,
                    "{role:?} must have at least one GC event, got {stats:?}"
                );
            }
            other => panic!("{role:?} must be a Stats row, got {other:?}"),
        }
    }

    for event in &log.gc_events {
        assert!(
            event.end_unix_ns >= before && event.end_unix_ns <= after,
            "GC event end_unix_ns={} outside [{before}, {after}]",
            event.end_unix_ns
        );
    }
}

// --- Task 3: BENCH-08 edge rules, against synthetic fixtures ---------------

fn write_hook_file(dir: &std::path::Path, name: &str, lines: &[String]) {
    fs::create_dir_all(dir).expect("create hook dir");
    fs::write(dir.join(name), lines.join("\n") + "\n").expect("write hook file");
}

fn start_line(pid: i32, t: f64, wall: f64) -> String {
    format!(
        r#"{{"kind":"start","pid":{pid},"ppid":1,"orig_argv":[],"t":{t},"wall":{wall},"clock":"mach_absolute_time"}}"#
    )
}

fn gc_line(pid: i32, t: f64, duration_s: f64, generation: i64) -> String {
    format!(
        r#"{{"kind":"gc","pid":{pid},"t":{t},"duration_s":{duration_s},"generation":{generation},"collected":0,"uncollectable":0}}"#
    )
}

fn completed_request(t_send_unix_ns: u64, ttft_ms: f64) -> RequestRecord {
    RequestRecord {
        t_send_unix_ns,
        ttft: Some(Duration::from_secs_f64(ttft_ms / 1000.0)),
        e2e: Duration::from_secs_f64((ttft_ms / 1000.0) + 0.1),
        itl: Vec::new(),
        chunks: 1,
        outcome: Outcome::Completed,
        error: None,
    }
}

/// A pause whose interval only touches a request's `[t_send, t_first]`
/// endpoints does not count as overlap; a pause that strictly straddles
/// `t_send` does.
#[test]
fn cooccurrence_strict_touch_not_counted() {
    // t_send=1_000_000_000, t_first=1_100_000_000 (ttft 100ms). A second
    // request carries the same ttft so the P99 threshold includes both and
    // every request is at-or-above threshold (a spike).
    let r1 = completed_request(1_000_000_000, 100.0);
    let r2 = completed_request(2_000_000_000, 100.0);
    let requests = vec![r1, r2];

    // Pause ending exactly at r1's t_send (1_000_000_000): touches, must not
    // overlap.
    let touch_at_send = GcEvent {
        pid: 1,
        end_unix_ns: 1_000_000_000,
        duration: Duration::from_millis(50),
        generation: 0,
    };
    // Pause starting exactly at r2's t_first (2_100_000_000): touches, must
    // not overlap.
    let touch_at_first = GcEvent {
        pid: 1,
        end_unix_ns: 2_200_000_000,
        duration: Duration::from_millis(100), // start = 2_100_000_000
        generation: 0,
    };
    let touching = [&touch_at_send, &touch_at_first];
    let co_touch = gclog::cooccurrence(&requests, &touching).expect("has ttft");
    assert_eq!(
        co_touch.spike_with_gc, 0,
        "touching pauses must not overlap"
    );

    // A pause that strictly straddles r1's t_send: starts before, ends after.
    let straddles = GcEvent {
        pid: 1,
        end_unix_ns: 1_000_000_100,
        duration: Duration::from_nanos(200), // start = 999_999_900
        generation: 0,
    };
    let straddling = [&straddles];
    let co_straddle = gclog::cooccurrence(&requests, &straddling).expect("has ttft");
    assert!(
        co_straddle.spike_with_gc >= 1,
        "a pause straddling t_send must overlap"
    );
}

/// With 100 TTFTs where 3 tie at the nearest-rank P99 value, spike_requests
/// is at least 3 -- ties are never excluded.
#[test]
fn cooccurrence_ties_are_spikes() {
    // 97 requests at 10ms, 3 requests tied at 50ms. Nearest-rank P99 of 100
    // sorted values (index ceil(0.99*100)-1 = 98, 0-based) lands on one of
    // the tied 50ms values, so all 3 are spikes.
    let mut requests = Vec::new();
    let mut t = 0u64;
    for _ in 0..97 {
        requests.push(completed_request(t, 10.0));
        t += 1_000_000;
    }
    for _ in 0..3 {
        requests.push(completed_request(t, 50.0));
        t += 1_000_000;
    }

    let events: Vec<&GcEvent> = Vec::new();
    let co = gclog::cooccurrence(&requests, &events).expect("has ttft");
    assert_eq!(co.p99_ttft_ms, 50.0);
    assert!(
        co.spike_requests >= 3,
        "tied P99 values must all count as spikes, got {}",
        co.spike_requests
    );
}

/// An event ending exactly at the window end is included; end + 1ns is
/// excluded.
#[test]
fn gc_window_closed_interval() {
    let mut log = HookLog::default();
    log.start_pids.insert(1);
    log.gc_events.push(GcEvent {
        pid: 1,
        end_unix_ns: 1_000,
        duration: Duration::from_nanos(10),
        generation: 0,
    });
    log.gc_events.push(GcEvent {
        pid: 1,
        end_unix_ns: 1_001,
        duration: Duration::from_nanos(10),
        generation: 0,
    });

    let inside = gclog::events_in_window(&log, 0, 1_000);
    assert_eq!(inside.len(), 1, "end == window end must be included");
    assert_eq!(inside[0].end_unix_ns, 1_000);

    let excluded = gclog::events_in_window(&log, 0, 999);
    assert!(
        excluded.is_empty(),
        "end + 1ns past window end must be excluded"
    );
}

/// A Python role with known pids and no events gives count 0, total 0.0,
/// and every percentile None.
#[test]
fn zero_events_role_stats() {
    let hook_dir = unique_tmp_dir("zero-events");
    write_hook_file(
        &hook_dir,
        "hook-100-1.jsonl",
        &[start_line(100, 0.0, 1_000.0)],
    );
    let log = gclog::read_hook_dir(&hook_dir).expect("read_hook_dir");
    assert_eq!(log.gc_events.len(), 0);

    let empty_names: BTreeMap<i32, String> = BTreeMap::new();
    let role_map = RoleMap::build(100, FrontendKind::Python, &log, &empty_names, "rsg-server");
    let by_role = gclog::gc_by_role(&log, &role_map, FrontendKind::Python, 0, u64::MAX);

    match by_role
        .get(&Role::ApiServer)
        .expect("ApiServer row present")
    {
        GcRow::Stats(stats) => {
            assert_eq!(stats.count, 0);
            assert_eq!(stats.total_pause_ms, 0.0);
            assert_eq!(stats.pause_ms.p50, None);
            assert_eq!(stats.pause_ms.p99, None);
            assert_eq!(stats.pause_ms.max, None);
        }
        other => panic!("expected Stats, got {other:?}"),
    }
}

/// `gc_by_role` for `FrontendKind::Rust` always contains
/// `RustFrontend -> NotApplicable`, which serializes as
/// `{"not_applicable":"..."}`.
#[test]
fn rust_frontend_gc_not_applicable() {
    let log = HookLog::default();
    let empty_names: BTreeMap<i32, String> = BTreeMap::new();
    let role_map = RoleMap::build(1, FrontendKind::Rust, &log, &empty_names, "rsg-server");
    let by_role = gclog::gc_by_role(&log, &role_map, FrontendKind::Rust, 0, u64::MAX);

    let row = by_role
        .get(&Role::RustFrontend)
        .expect("RustFrontend row present");
    match row {
        GcRow::NotApplicable { reason } => {
            assert_eq!(reason, "Rust frontend has no garbage collector");
        }
        other => panic!("expected NotApplicable, got {other:?}"),
    }
    let json = serde_json::to_value(row).expect("serialize GcRow");
    assert_eq!(
        json,
        serde_json::json!({"not_applicable": "Rust frontend has no garbage collector"})
    );
}

/// Requests that all lack `ttft` give `CoOccurrenceRow::NoFirstToken`. For a
/// Rust arm, the Frontend group row is `NotApplicable` regardless.
#[test]
fn cooccurrence_none_without_first_token() {
    let requests = vec![RequestRecord {
        t_send_unix_ns: 0,
        ttft: None,
        e2e: Duration::from_millis(10),
        itl: Vec::new(),
        chunks: 0,
        outcome: Outcome::Failed,
        error: Some("boom".to_string()),
    }];

    let log = HookLog::default();
    let empty_names: BTreeMap<i32, String> = BTreeMap::new();

    let python_roles = RoleMap::build(1, FrontendKind::Python, &log, &empty_names, "rsg-server");
    let python_rows = gclog::cooccurrence_by_group(
        &requests,
        &log,
        &python_roles,
        FrontendKind::Python,
        0,
        u64::MAX,
    );
    assert_eq!(
        python_rows.get(&Group::Frontend),
        Some(&CoOccurrenceRow::NoFirstToken)
    );
    assert_eq!(
        python_rows.get(&Group::Scheduler),
        Some(&CoOccurrenceRow::NoFirstToken)
    );

    let rust_roles = RoleMap::build(1, FrontendKind::Rust, &log, &empty_names, "rsg-server");
    let rust_rows = gclog::cooccurrence_by_group(
        &requests,
        &log,
        &rust_roles,
        FrontendKind::Rust,
        0,
        u64::MAX,
    );
    match rust_rows.get(&Group::Frontend) {
        Some(CoOccurrenceRow::NotApplicable { reason }) => {
            assert_eq!(reason, "Rust frontend has no garbage collector");
        }
        other => panic!("expected NotApplicable, got {other:?}"),
    }
    assert_eq!(
        rust_rows.get(&Group::Scheduler),
        Some(&CoOccurrenceRow::NoFirstToken)
    );
}

/// A truncated line, a gc line before any start record, and a string pid
/// each add 1 to malformed_lines; the valid lines still parse.
#[test]
fn hook_lines_malformed_counted() {
    let hook_dir = unique_tmp_dir("malformed");
    let lines = vec![
        "{\"kind\":\"start\",\"pid\":\"not-an-int\"".to_string(), // truncated AND bad JSON
        gc_line(42, 1.0, 0.001, 0), // gc before any start record in this file
        r#"{"kind":"gc","pid":"42","t":1.0,"duration_s":0.001,"generation":0}"#.to_string(), // string pid
        start_line(42, 0.0, 1_000.0),
        gc_line(42, 1.0, 0.002, 1), // valid, after start
    ];
    write_hook_file(&hook_dir, "hook-42-1.jsonl", &lines);

    let log = gclog::read_hook_dir(&hook_dir).expect("read_hook_dir");
    assert_eq!(log.malformed_lines, 3, "log = {log:?}");
    assert_eq!(log.gc_events.len(), 1, "only the post-start gc line parses");
    assert_eq!(log.gc_events[0].generation, 1);
}

/// A table test covering every `classify` branch.
#[test]
fn classify_rules() {
    use rsg_bench::roles::classify;

    // rsg-server name (the Rust frontend's own process name) -> RustFrontend.
    assert_eq!(
        classify(
            5,
            1,
            FrontendKind::Rust,
            None,
            Some("rsg-server"),
            "rsg-server"
        ),
        Role::RustFrontend
    );
    // mock-scheduler name -> Scheduler.
    assert_eq!(
        classify(
            5,
            1,
            FrontendKind::Rust,
            None,
            Some("mock-scheduler"),
            "rsg-server"
        ),
        Role::Scheduler
    );
    // "-scheduler" suffix (hook name) -> Scheduler.
    assert_eq!(
        classify(
            5,
            1,
            FrontendKind::Python,
            Some("minisgl-TP0-scheduler"),
            None,
            "rsg-server"
        ),
        Role::Scheduler
    );
    // "minisgl-detokenizer-0" contains "tokenizer" -> Tokenizer.
    assert_eq!(
        classify(
            5,
            1,
            FrontendKind::Python,
            Some("minisgl-detokenizer-0"),
            None,
            "rsg-server"
        ),
        Role::Tokenizer
    );
    // Leader, Python kind, no name match -> ApiServer.
    assert_eq!(
        classify(1, 1, FrontendKind::Python, None, None, "rsg-server"),
        Role::ApiServer
    );
    // Leader, Rust kind, no name match -> Launcher.
    assert_eq!(
        classify(1, 1, FrontendKind::Rust, None, None, "rsg-server"),
        Role::Launcher
    );
    // Not the leader, no name match -> Other.
    assert_eq!(
        classify(5, 1, FrontendKind::Python, None, None, "rsg-server"),
        Role::Other
    );
}
