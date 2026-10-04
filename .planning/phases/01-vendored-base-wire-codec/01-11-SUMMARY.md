---
phase: 01-vendored-base-wire-codec
plan: 11
subsystem: handshake-and-launcher
tags: [serde, deserialize_with, argparse, allow_abbrev, gap-closure]
requires:
  - phase: 01-vendored-base-wire-codec
    provides: rsg-server handshake parser (D-10, D-11) and python/rsglang/launch.py rust-mode launcher (D-05, D-06)
provides:
  - "eos_token_id is a required-but-nullable handshake key: missing the key is Malformed/exit 2, an explicit null is still valid"
  - "rust mode rejects --shell-mode however abbreviated (--shell, --shell-m), using the parsed run_shell flag returned by upstream parse_args, before any side effect"
  - "WR-01 and WR-04 recorded as fixed in 01-REVIEW-DISPOSITION.md"
affects: [phase-01-remaining-gap-closure-plans, phase-02-benchmark-harness]
actuals:
  tokens: 1662
  tasks: 2
  commits: 4
  plan_head_before: 351fe652d1769aaa69c306dd35d8300685607056
  plan_head_after: db028e375fa592c16b7fd8a3b6b44fff5863e68e
tech-stack:
  added: []
  patterns:
    - "deserialize_with = \"Option::deserialize\" on a struct field makes an Option<T> key required while keeping null a valid value for that key (serde's implicit missing-means-None for Option is bypassed)"
    - "Keep both return values of a tuple-returning parser (parse_args -> (ServerArgs, run_shell)) and check the discarded one immediately, before any side effect, rather than trusting a separate literal pre-check alone"
key-files:
  created: []
  modified:
    - crates/rsg-server/src/handshake.rs
    - crates/rsg-server/tests/cli.rs
    - python/rsglang/launch.py
    - python/tests/test_launch_args.py
    - .planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md
key-decisions:
  - "Kept the literal \"--shell-mode\" in rest pre-check in run_rust_mode alongside the new parsed run_shell check in _run_rust_mode: the literal check fails fast before resolving the binary and importing torch, while the parsed flag is the authoritative, abbreviation-proof guard (WR-01's fix text explicitly allows keeping both)"
  - "No Python change for WR-04: python/rsglang/handshake.py encode_handshake_line already refuses any payload whose keys are not exactly HANDSHAKE_KEYS, so the launcher always sends eos_token_id; only the Rust parser needed to stop treating the missing key as None"
patterns-established: []
requirements-completed: [BASE-02, BASE-03]
coverage:
  - id: D1
    description: "rsg-server rejects a handshake line with no eos_token_id key (Malformed naming eos_token_id in the unit test; exit 2 with 'handshake rejected' in the CLI test), while an explicit null still parses to None"
    requirement: "BASE-02"
    verification:
      - kind: unit
        ref: "crates/rsg-server/src/handshake.rs#missing_eos_key_is_malformed"
        status: pass
      - kind: integration
        ref: "crates/rsg-server/tests/cli.rs#missing_eos_key_exits_2"
        status: pass
      - kind: unit
        ref: "cargo test -p rsg-server (22 tests, all pass, including null_eos_parses_to_none and null_eos_logs_null_then_eof_exits_3)"
        status: pass
    human_judgment: false
  - id: D2
    description: "rust mode rejects --shell and --shell-m with exit 2 and the standard rejection message, before any subprocess.Popen, multiprocessing.Process, os.setpgid or signal.signal call, using the run_shell flag from upstream parse_args"
    requirement: "BASE-03"
    verification:
      - kind: unit
        ref: "python/tests/test_launch_args.py#test_rust_mode_rejects_abbreviated_shell_mode_without_spawning[--shell|--shell-m]"
        status: pass
      - kind: unit
        ref: ".venv/bin/python -m pytest python/tests/test_launch_args.py -q (9 tests, all pass, including the literal test_rust_mode_rejects_shell_mode_without_spawning and every python-mode test)"
        status: pass
    human_judgment: false
  - id: D3
    description: "WR-01 and WR-04 recorded as fixed in 01-REVIEW-DISPOSITION.md with the open count dropped from 15 to 13"
    requirement: "BASE-02, BASE-03"
    verification:
      - kind: other
        ref: "grep '| WR-01 | warning | fixed |' and '| WR-04 | warning | fixed |' both match; frontmatter open: 13"
        status: pass
    human_judgment: false
duration: 35min
completed: 2026-10-04
status: complete
---

# Phase 01 Plan 11: Required eos_token_id and Abbreviation-Proof Shell-Mode Guard Summary

**eos_token_id required via deserialize_with = "Option::deserialize" (missing key now exits 2, explicit null still accepted); rust mode rejects --shell-mode however abbreviated by checking the parsed run_shell flag before any side effect.**

## Performance
- **Duration:** 35min
- **Started:** 2026-10-04T08:00:00Z (approx, first RED test run)
- **Completed:** 2026-10-04T08:35:00Z (approx)
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments
- Closed gap G-01-7-WR04: `crates/rsg-server/src/handshake.rs`'s `eos_token_id` field now carries `#[serde(deserialize_with = "Option::deserialize")]`, which bypasses serde's implicit missing-means-None handling for `Option<T>` so the key itself is required while an explicit `null` still parses to `None`. Proven at both the parser level (`missing_eos_key_is_malformed`) and the binary level (`missing_eos_key_exits_2`, exit 2 + "handshake rejected").
- Closed gap G-01-7-WR01: `python/rsglang/launch.py`'s `_run_rust_mode` now keeps the second return value of `parse_args(rest)` (`run_shell`) and checks it immediately — before `_unique_suffix` replace, socket unlink, `setpgid`, signal handlers, or any spawn — returning 2 with the standard rejection message if upstream's abbreviation-tolerant parser (`allow_abbrev=True`) resolved `--shell` or `--shell-m` to shell mode. The literal `"--shell-mode" in rest` pre-check in `run_rust_mode` is kept as a fast-fail ahead of binary resolution and the torch import.
- `.planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md` now records WR-01 and WR-04 as `fixed`, with the frontmatter `open` count corrected from 15 to 13.

## Task Commits
1. **Task 1 RED: add failing tests for missing eos_token_id key** - `89c125d` (test)
2. **Task 1 GREEN: require eos_token_id key, keep explicit null valid** - `4783d49` (feat)
3. **Task 2 RED: add failing test for abbreviated --shell-mode rejection** - `152e5bd` (test)
4. **Task 2 GREEN: rust mode rejects abbreviated --shell-mode via parsed run_shell** - `db028e3` (feat; also updates 01-REVIEW-DISPOSITION.md)

_Note: both tasks used the full RED-GREEN TDD cycle; neither needed a REFACTOR commit (no cleanup was needed after either GREEN step)._

## Files Created/Modified
- `crates/rsg-server/src/handshake.rs` - `eos_token_id` field gets `deserialize_with = "Option::deserialize"` plus an explanatory comment; new unit test `missing_eos_key_is_malformed`
- `crates/rsg-server/tests/cli.rs` - new CLI test `missing_eos_key_exits_2`
- `python/rsglang/launch.py` - `_run_rust_mode` keeps `run_shell` from `parse_args` and returns 2 immediately if set, before any side effect
- `python/tests/test_launch_args.py` - new parametrized test `test_rust_mode_rejects_abbreviated_shell_mode_without_spawning[--shell|--shell-m]`
- `.planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md` - WR-01 and WR-04 rows and frontmatter entries changed to `fixed`; `open` count 15 -> 13

## RED Evidence

**Task 1** (`cargo test -p rsg-server missing_eos` on unmodified handshake.rs/cli.rs):
- `missing_eos_key_is_malformed`: `called Result::unwrap_err() on an Ok value: Handshake { ..., eos_token_id: None, ... }` — the parser silently accepted the missing key as null.
- `missing_eos_key_exits_2` (run separately as `cargo test -p rsg-server --test cli missing_eos`): panicked at the 20s `wait_exit` deadline — "rsg-server did not exit" — with stderr showing `handshake received ... eos_token_id=null ...` then `idle until SIGINT/SIGTERM or stdin EOF`, confirming the server accepted and idled instead of rejecting.

**Task 2** (`.venv/bin/python -m pytest python/tests/test_launch_args.py -q -k abbreviated` on unmodified launch.py):
- Both `[--shell]` and `[--shell-m]` cases failed with `AssertionError: nothing may be spawned or changed before the shell-mode check`, raised from the monkeypatched `os.setpgid` call inside `_run_rust_mode` — i.e. the abbreviated flag passed the literal pre-check and the binary-resolution step, then hit the process-group guard with no exit 2.
- Captured stdout for both cases showed upstream's parsed `ServerArgs` with `max_running_req=1, cuda_graph_max_bs=1, silent_output=True` — concrete confirmation that `argparse`'s `allow_abbrev=True` resolved `--shell`/`--shell-m` to shell mode and upstream had already applied its limits, exactly as WR-01 described.

## Decisions Made
- Kept the literal `"--shell-mode" in rest` pre-check in `run_rust_mode` alongside the new parsed `run_shell` check in `_run_rust_mode`: the literal check fails fast before resolving the binary and importing torch, while the parsed flag is the authoritative, abbreviation-proof guard. WR-01's fix text explicitly permits keeping both.
- No Python change was needed for WR-04 beyond the tests: `python/rsglang/handshake.py`'s `encode_handshake_line` already refuses any payload whose keys are not exactly `HANDSHAKE_KEYS`, so the launcher always sends `eos_token_id`; only the Rust parser needed to stop silently treating a missing key as `None`.

## Deviations from Plan

None - plan executed exactly as written. Both tasks' `<action>` blocks specified exact code changes and test content, which were followed verbatim. A `<precondition>` on Task 2 required `.venv/bin/python` to exist in this checkout and import `rsglang` from it; it was unmet at the start of execution, so the precondition's own prescribed idempotent remedy (`bash scripts/bootstrap_mac_env.sh`) was run first — not a plan deviation, but documented here for continuity since it was not itself a plan task.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required. The one setup action taken (running `scripts/bootstrap_mac_env.sh` to create this worktree's local `.venv`) was Claude-automated per the task's own `<precondition>` instructions, not a manual step.

## Next Phase Readiness

G-01-7-WR04 and G-01-7-WR01 are closed. `cargo test -p rsg-server` (22 tests), `.venv/bin/python -m pytest python/tests/test_launch_args.py -q` (9 tests), and `bash scripts/check_all.sh --offline` (full Phase 1 Mac gate) all pass. `git status --porcelain vendor/` is clean. Ready for 01-12 (next gap closure plan in the chain).

---
*Phase: 01-vendored-base-wire-codec*
*Completed: 2026-10-04*
