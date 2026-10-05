---
phase: 01
review: 01-REVIEW.md
titles: json
findings:
  - id: WR-01
    severity: warning
    disposition: fixed
    title: "Abbreviated `--shell-mode` rejection can be masked by an unrelated \"binary not found\" error"
  - id: IN-01
    severity: info
    disposition: fixed
    title: "`prctl` failure handling only catches `OSError`, not a missing symbol"
  - id: IN-02
    severity: info
    disposition: deferred
    title: "`gpu_phase1_check.sh`'s safety-net cleanup can signal an unrelated process group"
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
    title: "Ctrl-C (SIGINT to the whole process group) makes the launcher exit 1 and report a failure"
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
open: 10
total: 24
recorded: 2026-10-04T09:50:00.000Z
---

# Phase 01: Code Review Disposition

| Finding | Severity | Disposition | Source |
|---------|----------|-------------|--------|
| WR-01 | warning | fixed | 01-13-PLAN.md (gap G-01-8): upstream parse_args and the run_shell check run before resolve_rust_bin; --shell, --shell-m and --shell-mode report the shell-mode rejection even with no rsg-server binary |
| IN-01 | info | fixed | 01-13-PLAN.md (gap G-01-9): the prctl setup catches OSError and AttributeError; a missing prctl symbol logs PDEATHSIG unavailable and degrades to the polling watchdog |
| IN-02 | info | deferred | deferred to Phase 6 (bundle with the GPU end-to-end validation pass over gpu_phase1_check.sh) — UAT test 9, 2026-10-04 |
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
| CR-01 | critical | fixed | 01-07-PLAN.md (gap G-01-2): stop re-checked after every ready_queue.get and before each child-state-driven shutdown(1) (not in the current review) |
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
