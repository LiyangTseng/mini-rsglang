---
phase: 07-frontend-benchmarks
plan: 05
subsystem: benchmarking
tags: [rust, sysinfo, proc, gc-attribution, memory-sampling, hdrhistogram-adjacent, tdd]

requires:
  - phase: 07-frontend-benchmarks
    provides: "07-02's RSGLANG_PROFILE_MODE=gc_only hook.py extension (start/gc/proc record shapes, write_shim/hook_env) and 07-04's rsg_bench::client::RequestRecord contract this plan's cooccurrence() consumes"
provides:
  - "rsg_bench::roles::{FrontendKind, Role, Group, group_of, MOCK_SCHEDULER_PROCESS_NAME, classify, RoleMap} -- role attribution without py-spy (D-15)"
  - "rsg_bench::gclog::{GcEvent, HookLog, read_hook_dir, nearest_rank, GcStats, PausePercentiles, gc_stats, events_in_window, GcRow, gc_by_role, CoOccurrence, CoOccurrenceRow, cooccurrence, cooccurrence_by_group} -- gc_only hook-file reader plus Phase 2's analysis.py percentile/gc_stats/gc_ttft_correlation semantics ported to Rust"
  - "rsg_bench::memory::{ProcSample, TreeSample, parse_smaps_rollup_pss, read_pss, sample_tree, process_names, MemorySampler, MemSummary, GroupMemory, memory_by_group, MemPoint, memory_at} -- whole-process-tree RSS everywhere, PSS Linux-only and never faked (D-14)"
  - "Env var RSG_BENCH_PYTHON (test-time interpreter override for the cross-language tracer)"
affects: [07-06-ab-orchestrator, 07-08-s3-coldstart-runner, 07-09-standard-throughput-regression]

actuals:
  tokens: 15000
  tasks: 3
  commits: 3
plan_head_before: 691f5393a89384f8c997fa2973ed730282163099
plan_head_after: ec4ab8c9822d98f3d38e4333405e6a2de0124585

tech-stack:
  added: []
  patterns:
    - "Cross-language tracer test: spawns a real `.venv` Python interpreter through 07-02's real sitecustomize-shim/gc_only mechanism (never a mocked hook file), proving the Rust reader against the exact bytes Python's hook.py actually writes, not a synthetic fixture alone"
    - "Three-way PSS gate ported verbatim from procs.py::tree_memory: RSS via sysinfo always, PSS only `#[cfg(target_os = \"linux\")]` by hand-parsing /proc/<pid>/smaps_rollup's exact `Pss:` line, and a single failed read flips the *whole tree's* PSS to None (never a partial sum, never a faked Some(0))"
    - "Untagged serde enums (GcRow, CoOccurrenceRow) with a `#[serde(rename = \"not_applicable\")]` field give {\"not_applicable\": \"<reason>\"} directly, instead of a discriminant tag wrapping the real Stats/Computed payload"
    - "classify()'s precedence order mirrors its own doc comment exactly: explicit process-name match (RustFrontend/mock-scheduler) > hook-reported mp name (-scheduler suffix / contains \"tokenizer\") > pgid-leader fallback (ApiServer for Python, Launcher for Rust) > Other"

key-files:
  created:
    - crates/rsg-bench/src/gclog.rs
    - crates/rsg-bench/src/roles.rs
    - crates/rsg-bench/src/memory.rs
    - crates/rsg-bench/tests/gclog.rs
    - crates/rsg-bench/tests/memory_pss_gate.rs
  modified:
    - crates/rsg-bench/src/lib.rs

key-decisions:
  - "classify() checks process_name before hook_name before the pid==leader_pid fallback, matching the plan's prose order exactly (RustFrontend/mock-scheduler name match wins first; -scheduler suffix or \"tokenizer\" substring in the hook-reported mp name next; the pgid leader is the last resort, not a priority override) -- this keeps a named process's signal stronger than the generic leader-pid heuristic"
  - "gc_by_role only emits a Stats row for roles with at least one pid in hook.start_pids (i.e. a pid a Python hook actually wrote a start record for), so a role with genuinely zero Python processes never appears as a fabricated all-zero row; FrontendKind::Rust's RustFrontend->NotApplicable row is added unconditionally on top of that set"
  - "MemorySampler.stop() takes one additional sample itself (on the caller's thread, after joining the background thread) rather than relying on the background loop's last iteration, so the returned series always ends with a sample taken at-or-after the stop() call, not up to one full interval stale"
  - "memory_by_group/memory_at treat a group with zero member pids in a given sample as a real, accurate RSS/PSS sum of 0 for that sample (not a sentinel for \"no data\") -- the never-fake rule from RESEARCH Pitfall 1 applies specifically to a failed PSS *read*, not to an empty group"

patterns-established:
  - "Synthetic hook-*.jsonl fixtures built by hand (write_hook_file + start_line/gc_line string builders under std::env::temp_dir(), no tempfile crate) for edge-rule tests that don't need a real Python process, reserving the real-interpreter tracer for the one test that specifically proves cross-language parity"

requirements-completed: []  # BENCH-05 shared with 07-08/07-10 (0/2 ready, both unmet); BENCH-08 shared with 07-02/07-03/07-06/07-07/07-09/07-10 (0/2 ready) -- both stay open in REQUIREMENTS.md per requirements.ready-ids

coverage:
  - id: D1
    description: "gclog::read_hook_dir reads real Python gc_only-mode hook-*.jsonl files byte-for-byte as written by 07-02's hook.py, converts GC pause end times to Unix nanoseconds via each file's own start record, and defensively counts malformed lines (bad JSON, missing kind, non-integer pid, gc/proc before start) instead of panicking"
    requirement: BENCH-08
    verification:
      - kind: integration
        ref: "crates/rsg-bench/tests/gclog.rs#gclog_reads_real_gc_only_hook_files"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/tests/gclog.rs#hook_lines_malformed_counted"
        status: pass
    human_judgment: false
  - id: D2
    description: "roles::classify/RoleMap attribute every pid to a Role (ApiServer/Tokenizer/Scheduler/Launcher/RustFrontend/Other) without py-spy, from process names and hook.py's proc records; FrontendKind::Rust's RustFrontend always rolls into Group::Frontend"
    requirement: BENCH-08
    verification:
      - kind: unit
        ref: "crates/rsg-bench/tests/gclog.rs#classify_rules"
        status: pass
      - kind: integration
        ref: "crates/rsg-bench/tests/gclog.rs#gclog_reads_real_gc_only_hook_files (RoleMap::build assertions)"
        status: pass
    human_judgment: false
  - id: D3
    description: "gc_by_role/cooccurrence/cooccurrence_by_group port Phase 2's analysis.py percentile/gc_stats/gc_ttft_correlation semantics exactly: strict-touch overlap rule, nearest-rank P99 ties count as spikes, closed-interval window boundaries, zero-event roles report real zeros (not omitted rows), and the Rust frontend's GC row is an explicit NotApplicable, never a silently-omitted or fabricated zero"
    requirement: BENCH-08
    verification:
      - kind: unit
        ref: "crates/rsg-bench/tests/gclog.rs#cooccurrence_strict_touch_not_counted"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/tests/gclog.rs#cooccurrence_ties_are_spikes"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/tests/gclog.rs#gc_window_closed_interval"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/tests/gclog.rs#zero_events_role_stats"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/tests/gclog.rs#rust_frontend_gc_not_applicable"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/tests/gclog.rs#cooccurrence_none_without_first_token"
        status: pass
    human_judgment: false
  - id: D4
    description: "memory::{parse_smaps_rollup_pss, read_pss, sample_tree} implement procs.py::tree_memory's three-way PSS gate: RSS via sysinfo always, PSS only on Linux via the exact /proc/<pid>/smaps_rollup Pss: line, and the whole tree's PSS flips to None (never a partial sum or a faked Some(0)) the moment any included pid's read fails"
    requirement: BENCH-05
    verification:
      - kind: unit
        ref: "crates/rsg-bench/tests/memory_pss_gate.rs#parse_smaps_rollup_pss_fixture"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/tests/memory_pss_gate.rs#parse_rejects_missing_or_bad"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/tests/memory_pss_gate.rs#pss_gate_platform"
        status: pass
      - kind: integration
        ref: "crates/rsg-bench/tests/memory_pss_gate.rs#sample_tree_includes_grandchild"
        status: pass
    human_judgment: false
  - id: D5
    description: "MemorySampler samples the whole process tree on its own thread at a fixed interval until stopped; memory_by_group/memory_at summarize RSS/PSS per Group and for the whole tree over a closed time window or at one instant, with an empty window giving all-None summaries rather than zeros"
    requirement: BENCH-05
    verification:
      - kind: integration
        ref: "crates/rsg-bench/tests/memory_pss_gate.rs#sampler_window_summary"
        status: pass
    human_judgment: false

duration: 15min
completed: 2026-10-06
status: complete
---

# Phase 07 Plan 05: Out-of-Band GC-Pause and Memory Observation Layer Summary

**Rust reader for real Python `gc_only` GC-hook files plus py-spy-free role attribution (D-15), a never-faked Linux-only whole-process-tree PSS sampler mirroring `procs.py::tree_memory` (D-14), and Phase 2's exact percentile/GC-stats/co-occurrence semantics (D-16) ported so Phase 7's per-role tables are directly comparable to `baseline-profile.json`.**

## Performance
- **Duration:** ~15min
- **Started:** 2026-10-06 (session start)
- **Completed:** 2026-10-06
- **Tasks:** 3
- **Files modified:** 6 (5 created, 1 modified)

## Accomplishments
- `roles::classify`/`RoleMap`: attributes every pid to a `Role` (ApiServer/Tokenizer/Scheduler/Launcher/RustFrontend/Other) purely from process names and 07-02's hook-reported multiprocessing names -- no py-spy. `group_of` rolls ApiServer/Tokenizer/RustFrontend up into `Group::Frontend`.
- `gclog::read_hook_dir`: parses `hook-*.jsonl` files from 07-02's `gc_only` mode in sorted order, converting each file's `gc` records to Unix-nanosecond `GcEvent`s via that file's own `start` record, tracking the last-written `proc` name per pid, and counting (never panicking on) malformed lines -- bad JSON, missing `kind`, a non-integer `pid`, or a `gc`/`proc` record seen before its file's `start` record.
- `gclog_reads_real_gc_only_hook_files`: a cross-language tracer that runs a real `.venv` Python interpreter through 07-02's actual sitecustomize-shim mechanism, spawns a named `mp.Process`, and proves end to end: zero `mem` records, the child classified `Role::Scheduler`, the parent classified `Role::ApiServer`, and every GC event's converted timestamp falling inside the test's own before/after window.
- `gclog::{nearest_rank, gc_stats, events_in_window, gc_by_role}`: a direct Rust port of `analysis.py`'s nearest-rank `percentile`/`gc_stats`, with `FrontendKind::Rust` always adding an explicit `RustFrontend -> NotApplicable("Rust frontend has no garbage collector")` row rather than omitting it.
- `gclog::{cooccurrence, cooccurrence_by_group}`: a direct port of `gc_ttft_correlation`'s strict-touch overlap rule, nearest-rank-P99 spike threshold (ties always count), and `Frontend`/`Scheduler`-keyed group rows (Frontend is `NotApplicable` for a Rust arm; Scheduler is always computed, reporting the shared-backend floor for both arms).
- `memory::{parse_smaps_rollup_pss, read_pss, sample_tree}`: ports `procs.py::tree_memory`'s three-way PSS gate -- RSS via `sysinfo` always, PSS only `#[cfg(target_os = "linux")]` by parsing the exact `Pss:` line of `/proc/<pid>/smaps_rollup`, and the whole tree's PSS flips to `None` (never a partial sum, never `Some(0)`) the instant any included pid's read fails.
- `memory::MemorySampler`: samples the whole tree on its own thread, immediately and then every fixed interval, until `stop()` signals it, joins, and takes one final sample.
- `memory::{memory_by_group, memory_at}`: per-`Group` and whole-tree RSS/PSS summaries (`start`/`end`/`max`/`growth`) over a closed window or at one instant; an empty window gives all-`None` summaries, never fabricated zeros.
- All 14 new tests pass (9 in `tests/gclog.rs`, 5 in `tests/memory_pss_gate.rs`); the full `cargo test -p rsg-bench` (33 tests across lib + 6 integration targets) and `cargo clippy -p rsg-bench --all-targets -- -D warnings` are both clean; `cargo build --workspace` is unaffected.

## Task Commits
1. **Task 1: Tracer -- real Python gc_only hook files through read_hook_dir, RoleMap and per-role GC stats** - `b12433f` (feat)
2. **Task 2: Whole-tree RSS sampling with a Linux-only, never-faked PSS gate; background sampler; per-group window summaries** - `6714c59` (feat)
3. **Task 3: D-16 co-occurrence statistic and the BENCH-08 edge rules** - `ec4ab8c` (test)

**Plan metadata:** commit recorded below (docs: complete plan)

## Files Created/Modified
- `crates/rsg-bench/src/roles.rs` - `FrontendKind`, `Role`, `Group`, `group_of`, `MOCK_SCHEDULER_PROCESS_NAME`, `classify`, `RoleMap::{build, role_of, pids_in}`
- `crates/rsg-bench/src/gclog.rs` - `GcEvent`, `HookLog`, `read_hook_dir`, `nearest_rank`, `PausePercentiles`, `GcStats`, `gc_stats`, `events_in_window`, `GcRow`, `RUST_FRONTEND_GC_REASON`, `gc_by_role`, `CoOccurrence`, `CoOccurrenceRow`, `cooccurrence`, `cooccurrence_by_group`
- `crates/rsg-bench/src/memory.rs` - `parse_smaps_rollup_pss`, `read_pss` (Linux/non-Linux twins), `ProcSample`, `TreeSample`, `sample_tree`, `process_names`, `MemorySampler`, `MemSummary`, `GroupMemory`, `memory_by_group`, `MemPoint`, `memory_at`
- `crates/rsg-bench/tests/gclog.rs` - `gclog_reads_real_gc_only_hook_files` plus the 8 edge-rule tests: `cooccurrence_strict_touch_not_counted`, `cooccurrence_ties_are_spikes`, `gc_window_closed_interval`, `zero_events_role_stats`, `rust_frontend_gc_not_applicable`, `cooccurrence_none_without_first_token`, `hook_lines_malformed_counted`, `classify_rules`
- `crates/rsg-bench/tests/memory_pss_gate.rs` - `parse_smaps_rollup_pss_fixture`, `parse_rejects_missing_or_bad`, `pss_gate_platform`, `sample_tree_includes_grandchild`, `sampler_window_summary`
- `crates/rsg-bench/src/lib.rs` - added `pub mod gclog; pub mod memory; pub mod roles;`

## Decisions Made
- `classify()`'s precedence order follows the plan's prose exactly: an explicit process-name match (the Rust frontend binary's own name, or `mock-scheduler`) wins before a hook-reported multiprocessing name (`-scheduler` suffix, or containing `tokenizer`), which wins before the pgid-leader fallback (`ApiServer` for Python, `Launcher` for Rust), which wins before `Other`.
- `gc_by_role` only emits a `Stats` row for roles that have at least one pid in `hook.start_pids` (a pid a Python hook actually wrote a `start` record for) -- so a role with zero real Python processes never appears as a fabricated all-zero row. `FrontendKind::Rust`'s `RustFrontend -> NotApplicable` row is unconditional on top of that set.
- `MemorySampler::stop()` takes one additional sample itself (on the caller's thread, after joining the background thread), so the returned series always ends with a sample taken at-or-after the `stop()` call rather than up to one full interval stale.
- `memory_by_group`/`memory_at` treat a group with zero member pids in a sample as an accurate RSS/PSS sum of `0` for that sample (a real measurement, not a sentinel) -- RESEARCH Pitfall 1's "never fake" rule applies specifically to a failed PSS *read*, not to an empty group.

## Deviations from Plan

None - plan executed exactly as written. All three tasks' acceptance criteria were verified directly:
- `grep -n "fn read_hook_dir" crates/rsg-bench/src/gclog.rs` and `grep -n "fn classify" crates/rsg-bench/src/roles.rs` both match.
- `grep -n "Rust frontend has no garbage collector" crates/rsg-bench/src/gclog.rs` matches (the `RUST_FRONTEND_GC_REASON` constant).
- `grep -n 'cfg(target_os = "linux")' crates/rsg-bench/src/memory.rs` matches the `read_pss` implementation.
- No code path in `memory.rs` builds `Some(0)` for PSS when a read fails; `pss_gate_platform` asserts `None` on this Mac session.
- An equivalent strict overlap comparison (`start < *t_first_ns && end > r.t_send_unix_ns`) is present in `gclog.rs`, and `cooccurrence_strict_touch_not_counted` passes.
- All nine `gclog` tests and all five `memory_pss_gate` tests pass; `cargo clippy -p rsg-bench --all-targets -- -D warnings` is clean.

## Issues Encountered
None. `.venv` already existed from an earlier plan in this phase, so the cross-language tracer ran against the real, already-bootstrapped Mac Python environment without needing `scripts/bootstrap_mac_env.sh`.

## User Setup Required
None - no external service configuration required. No new crates were added (threat T-07-SC: `sysinfo` was already pinned and audited in 07-01; this plan only added new modules inside the existing `rsg-bench` crate).

## Next Phase Readiness
`rsg_bench::{roles, gclog, memory}` are ready for 07-06 (A/B orchestrator, BENCH-03/07/08) to attach per-role GC tables, per-group memory, and the co-occurrence statistic to every timed trial, and for 07-08 (S3 cold-start runner, BENCH-05) to call `memory_at` for frontend memory at ready. `BENCH-05` and `BENCH-08` both stay open in `REQUIREMENTS.md` per the shared-ID gate (`requirements.ready-ids` reports 0/2 ready for both) -- `BENCH-05` is also claimed by 07-08/07-10, and `BENCH-08` by 07-02/07-03/07-06/07-07/07-09/07-10. No blockers.

---
*Phase: 07-frontend-benchmarks*
*Completed: 2026-10-06*

## Self-Check: PASSED

- FOUND: crates/rsg-bench/src/roles.rs
- FOUND: crates/rsg-bench/src/gclog.rs
- FOUND: crates/rsg-bench/src/memory.rs
- FOUND: crates/rsg-bench/tests/gclog.rs
- FOUND: crates/rsg-bench/tests/memory_pss_gate.rs
- FOUND: b12433f (feat: cross-language GC-only hook reader and role attribution)
- FOUND: 6714c59 (feat: whole-tree RSS sampling with never-faked PSS gate)
- FOUND: ec4ab8c (test: D-16 co-occurrence edge rules and classify table test)
- No unexpected file deletions in any of the three task commits (`git diff --diff-filter=D` empty for each)
