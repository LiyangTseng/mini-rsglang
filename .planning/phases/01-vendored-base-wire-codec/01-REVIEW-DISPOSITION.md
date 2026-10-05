---
phase: 01
review: 01-REVIEW.md
titles: json
findings:
  - id: WR-01
    severity: warning
    disposition: open
    title: "`rust_tail` deque is appended to from the stderr-pump thread and read via `list()` in `shutdown()` without synchronization"
  - id: IN-01
    severity: info
    disposition: open
    title: "`extract_handshake`'s dict literal hand-duplicates `HANDSHAKE_KEYS` instead of building from the constant"
  - id: IN-02
    severity: info
    disposition: open
    title: "Statements after the self-directed SIGKILL in `launch.py`'s shutdown path are dead/racy code"
  - id: IN-03
    severity: info
    disposition: deferred
    title: "Duplicated pass/report logic in `wait_no_orphans`"
  - id: WR-06
    severity: warning
    disposition: fixed
    title: "A prctl failure kills the scheduler before the error envelope and for a mere backstop"
  - id: WR-07
    severity: warning
    disposition: fixed
    title: "`gpu_pids` failures and SIGPIPE turn the GPU-orphan check into a false PASS"
  - id: WR-08
    severity: warning
    disposition: fixed
    title: "`start_session` has a 0.5 s startup race that can fail a healthy run"
  - id: WR-09
    severity: warning
    disposition: fixed
    title: "`test_exits_at_once_when_parent_is_not_the_launcher` cannot tell a watchdog exit from any other exit 1"
  - id: WR-10
    severity: warning
    disposition: deferred
    title: "`setpgid(0, 0)` moves the launcher out of the terminal's foreground group when run under a wrapper"
  - id: IN-10
    severity: info
    disposition: open
    title: "Hard-coded Qwen3-specific assertions in a script that accepts `--model`"
  - id: IN-11
    severity: info
    disposition: open
    title: "Repeated stop_requested boilerplate in the launch loops"
  - id: IN-12
    severity: info
    disposition: open
    title: "Test cleanup can signal a reused pid or process group"
  - id: IN-13
    severity: info
    disposition: open
    title: "The slow-boot test's `sitecustomize` can shadow an existing one and relies on `sys.orig_argv`"
  - id: CR-01
    severity: critical
    disposition: fixed
    title: "No top-level exception/finally guard around the rust-mode spawn loop can leave already-spawned processes unkilled"
  - id: WR-02
    severity: warning
    disposition: fixed
    title: "The parent watchdog records its parent pid too late and can miss a launcher that has already died, leaving a GPU scheduler orphaned"
  - id: WR-03
    severity: warning
    disposition: deferred
    title: "`setpgid(0, 0)` takes the launcher out of the terminal's foreground process group whenever a wrapper starts it, so Ctrl-C never reaches it"
  - id: WR-04
    severity: warning
    disposition: fixed
    title: "The Rust handshake accepts a line with no `eos_token_id` key, which the documented contract forbids"
  - id: WR-05
    severity: warning
    disposition: deferred
    title: "`shutdown()` always re-sends SIGINT to the group, which interrupts upstream's graceful `scheduler.shutdown()` after an external group SIGINT"
  - id: IN-04
    severity: info
    disposition: open
    title: "Socket-suffix validation and unlinking have small gaps"
  - id: IN-05
    severity: info
    disposition: open
    title: "The `--rust-log` default always overrides the user's `RUST_LOG`"
  - id: IN-06
    severity: info
    disposition: open
    title: "`resolve_rust_bin` prefers a possibly stale release build, and its error message is misleading"
  - id: IN-07
    severity: info
    disposition: open
    title: "The rsg-server CLI tests wait a fixed 100 ms for the stderr drain thread"
  - id: IN-08
    severity: info
    disposition: open
    title: "The e2e cleanup signals pids and pgids after they may have been reaped"
  - id: IN-09
    severity: info
    disposition: open
    title: "`bootstrap_mac_env.sh --relock` hardcodes Apple Silicon"
open: 13
total: 24
recorded: 2026-10-05T05:02:51.000Z
---

# Phase 01: Code Review Disposition

**ID-reuse notice (2026-10-05):** the 2026-10-05 incremental review (re-scoped to the 4 files
`01-13-PLAN.md` touched) reused finding IDs CR-01, WR-01, IN-01 and IN-02 for entirely new
findings. Per this file's own rule ("when a finding id is REUSED by a different finding, the
earlier decision cannot keep a row — the id is taken — and it is dropped"), four RECORDED
decisions were dropped and are preserved here for the record rather than silently lost:
- CR-01 (critical, was `fixed`): "Ctrl-C (SIGINT to the whole process group) makes the launcher exit 1 and report a failure" — fixed by 01-07-PLAN.md (gap G-01-2)
- WR-01 (warning, was `fixed`): "Abbreviated `--shell-mode` rejection can be masked by an unrelated \"binary not found\" error" — fixed by 01-13-PLAN.md (gap G-01-8), this run
- IN-01 (info, was `fixed`): "`prctl` failure handling only catches `OSError`, not a missing symbol" — fixed by 01-13-PLAN.md (gap G-01-9), this run
- IN-02 (info, was `deferred`): "`gpu_phase1_check.sh`'s safety-net cleanup can signal an unrelated process group" — deferred to Phase 6, UAT test 9

None of the underlying fixes/deferrals are undone — the source code and UAT record are
unaffected. Only this ledger's four rows now point at new findings under the same IDs.

| Finding | Severity | Disposition | Source |
|---------|----------|-------------|--------|
| WR-01 | warning | open | python/rsglang/launch.py:94-100, 158-161, 210-217 — `rust_tail` deque is appended to from `_pump_rsg_stderr` and read via `list(rust_tail)` in `shutdown()` with no lock; if the pump thread is still writing when shutdown's grace period expires, this risks `RuntimeError: deque mutated during iteration`, crashing shutdown() before its own SIGKILL escalation. 2026-10-05 incremental review. |
| IN-01 | info | open | python/rsglang/backend.py:42-55 vs python/rsglang/handshake.py:16-24 — `extract_handshake`'s dict literal hand-duplicates `HANDSHAKE_KEYS`'s order/fields instead of building from the constant; only a runtime check in `encode_handshake_line` would catch future drift. 2026-10-05 incremental review. |
| IN-02 | info | open | python/rsglang/launch.py:219-233 — statements after the self-directed `os.killpg(..., SIGKILL)` (closing rust.stdin, final _log) are effectively racy/dead code since that signal also kills the launcher; harmless but reads as more reliable than it is. 2026-10-05 incremental review. |
| IN-03 | info | deferred | deferred to Phase 6 (bundle with the GPU end-to-end validation pass over gpu_phase1_check.sh) — UAT test 9, 2026-10-04 |
| WR-06 | warning | fixed | 01-10-PLAN.md (gap G-01-7-WR06): prctl failure logs and degrades to the polling watchdog (argtypes declared); watchdog started inside run_scheduler's error-envelope try (not in the current review) |
| WR-07 | warning | fixed | 01-09-PLAN.md (gap G-01-7-WR07): on_gpu captures nvidia-smi output once and fails the step on an nvidia-smi error; steps 4/4b use wait_no_orphans (not in the current review) |
| WR-08 | warning | fixed | 01-09-PLAN.md (gap G-01-7-WR08): start_session polls pgid for up to 5 s and kills the pid it started on failure (not in the current review) |
| WR-09 | warning | fixed | 01-10-PLAN.md (gap G-01-7-WR09): exit test requires the 'calling' marker, no 'returned' marker and no Traceback; mutation-checked (not in the current review) |
| WR-10 | warning | deferred | deferred to Phase 7 (same issue as WR-03) — UAT test 7, 2026-10-03 (not in the current review) |
| IN-10 | info | open | - (not in the current review) |
| IN-11 | info | open | - (not in the current review) |
| IN-12 | info | open | - (not in the current review) |
| IN-13 | info | open | - (not in the current review) |
| CR-01 | critical | fixed | .planning/quick/261004-vqo-fix-cr-01-critical-finding-2026-10-05-in/261004-vqo-PLAN.md (quick task 261004-vqo): everything in _run_rust_mode after the rsg-server spawn runs inside try/except BaseException, which prints the traceback, unlinks the run sockets and SIGKILLs the launcher's process group before re-raising; regression test test_unexpected_error_leaves_no_orphans covers the spawn-loop and handshake-encode triggers |
| WR-02 | warning | fixed | 01-08-PLAN.md (gap G-01-3): launcher pid passed explicitly, immediate getppid re-check, PR_SET_PDEATHSIG on Linux (not in the current review) |
| WR-03 | warning | deferred | deferred to Phase 7 (benchmark harness launches the launcher under wrappers; same issue as WR-10) — UAT test 7, 2026-10-03 (not in the current review) |
| WR-04 | warning | fixed | 01-11-PLAN.md (gap G-01-7-WR04): eos_token_id required via deserialize_with = Option::deserialize; missing key exits 2 (not in the current review) |
| WR-05 | warning | deferred | deferred to Phase 6 (graceful upstream scheduler.shutdown on the real GPU backend; intentionally left unchanged by 01-07) — UAT test 7, 2026-10-03 (not in the current review) |
| IN-04 | info | open | - (not in the current review) |
| IN-05 | info | open | - (not in the current review) |
| IN-06 | info | open | - (not in the current review) |
| IN-07 | info | open | - (not in the current review) |
| IN-08 | info | open | - (not in the current review) |
| IN-09 | info | open | - (not in the current review) |

Dispositions: `open` (recorded, not yet triaged), `fixed`, `skipped`, `deferred`.
Set `deferred` by hand and put the reason in the Source cell; both are preserved. A `|` in the reason is kept as prose and escaped on the next run.
Re-running the gate keeps every row it can. A row the current review no longer reports is kept and its Source cell flagged, so a finding does not leave this record silently. ONE exception: when a finding id is REUSED by a different finding, the earlier decision cannot keep a row — the id is taken — and it is dropped. A RECORDED decision (anything but `open`) is named on the console when that happens; a row still at `open` is replaced silently, because `open` records no decision to lose.
