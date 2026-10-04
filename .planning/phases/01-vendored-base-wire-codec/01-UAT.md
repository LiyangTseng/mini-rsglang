---
status: partial
phase: 01-vendored-base-wire-codec
source: [01-VERIFICATION.md]
started: 2026-10-04T04:55:00Z
updated: 2026-10-04T05:40:00Z
---

## Current Test

[testing paused — 1 items outstanding]

## Tests

### 1. Run `bash scripts/gpu_phase1_check.sh` on the Linux GPU box
expected: ALL PASS on steps 1-5 (covers the GPU halves of ROADMAP criteria 2 and 3; WINDOWS.md entry 1)
result: blocked
blocked_by: physical-device
reason: "手上沒有linux gpu怎麼辦? 我現在在mac 我應該要先commit 然後讓其他人在Linux machine上面測試？"

### 2. Decide the disposition of code-review finding CR-01 (group SIGINT / Ctrl-C makes the rust-mode launcher exit 1 with a failure report)
expected: Either fix now (re-check stop_requested right after each ready_queue.get and before scanning children, plus an e2e test that SIGINTs the launcher's process group and asserts exit 0), or mark it deferred in 01-REVIEW-DISPOSITION.md with a target phase
result: issue
reported: "修"
severity: major

### 3. Decide the disposition of WR-02 (parent watchdog records getppid() only after the scheduler child has booted)
expected: Either pass the launcher pid explicitly (plus PR_SET_PDEATHSIG on Linux), or accept/defer it in 01-REVIEW-DISPOSITION.md
result: issue
reported: "修"
severity: major

### 4. Review the judgment-tier prohibition from 01-03: rust mode runs the byte-identical upstream Scheduler and the handshake is not produced by patching vendored code
expected: Agree with the verifier's non-authoritative verdict that it holds (see the Prohibitions table in 01-VERIFICATION.md)
result: pass

### 5. Confirm the 01-01 process truth: no Python package was installed before you approved the PyPI names and pins
expected: Confirmed. The session record shows the 01-01 package gate was approved ("approve (use ... uv ...)") before the lock was installed into the project .venv
result: pass

## Summary

total: 5
passed: 2
issues: 2
pending: 0
skipped: 0
blocked: 1

## Gaps

- gap_id: G-01-2
  truth: "Group SIGINT (Ctrl-C) to the rust-mode launcher's process group exits 0 without a failure report: stop_requested is re-checked right after each ready_queue.get and before scanning children, and an e2e test SIGINTs the launcher's process group and asserts exit 0 (CR-01 in 01-REVIEW.md)"
  status: failed
  reason: "User reported: 修 (fix CR-01 now rather than defer)"
  severity: major
  test: 2
  artifacts: []
  missing: []

- gap_id: G-01-3
  truth: "The parent-death watchdog cannot miss a launcher that dies early: the launcher pid is passed explicitly to the scheduler child (not read via getppid() after boot), plus PR_SET_PDEATHSIG on Linux, so kill -9 of the launcher never leaves an orphaned scheduler (WR-02 in 01-REVIEW.md)"
  status: failed
  reason: "User reported: 修 (fix WR-02 now rather than defer)"
  severity: major
  test: 3
  artifacts: []
  missing: []
