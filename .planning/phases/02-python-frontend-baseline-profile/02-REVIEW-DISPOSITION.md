---
phase: "02"
review: 02-REVIEW.md
titles: json
findings:
  - id: CR-01
    severity: critical
    disposition: fixed
    title: "procs.teardown() doesn't handle the documented EPERM-on-reused-pgid race, unlike its sibling in scenarios.py"
  - id: CR-02
    severity: critical
    disposition: fixed
    title: "scripts/baseline_profile.py run silently drops its own documented py-spy-permission exit code"
  - id: CR-03
    severity: critical
    disposition: fixed
    title: "analysis.cpu_metrics's per_request_ms divides by rate_hz with no zero-guard"
  - id: CR-04
    severity: critical
    disposition: fixed
    title: "subprocess.run(..., timeout=N) calls only catch OSError, letting a hang crash the script after measurement data is already captured but not yet persisted"
  - id: WR-01
    severity: warning
    disposition: open
    title: "scenarios.coldstart_once's self-heal abandons a stale process group it cannot parse, instead of killing it"
  - id: WR-02
    severity: warning
    disposition: open
    title: "analysis.load_speedscope/bucket_samples don't validate that sample frame indices are in range"
  - id: WR-03
    severity: warning
    disposition: open
    title: "procs.py_spy_dump silently swallows non-permission py-spy failures"
  - id: WR-04
    severity: warning
    disposition: open
    title: "procs.wait_ready doesn't verify the HTTP endpoint it probes belongs to the process it just launched"
  - id: WR-05
    severity: warning
    disposition: open
    title: "PySpyRecorder.stop_all() doesn't detect py-spy recorder subprocesses that survive SIGKILL"
  - id: WR-06
    severity: warning
    disposition: open
    title: "session.run_session's teardown finally can mask an original in-flight exception"
  - id: WR-07
    severity: warning
    disposition: open
    title: "No synchronization between the hook's daemon flush thread and its atexit final flush"
  - id: IN-01
    severity: info
    disposition: open
    title: "procs.ROLES is assigned but never used"
open: 8
total: 12
recorded: "2026-10-05T00:00:00Z"
---

# Phase 02: Code Review Disposition

| Finding | Severity | Disposition | Source |
|---------|----------|-------------|--------|
| CR-01 | critical | fixed | commit 0c78fe6 + test_teardown_survives_eperm_on_reused_pgid |
| CR-02 | critical | fixed | commit 0c78fe6 + test_run_permission_denied_exits_2 |
| CR-03 | critical | fixed | commit 0c78fe6 + test_cpu_metrics_rate_hz_zero_does_not_raise |
| CR-04 | critical | fixed | commit 0c78fe6 + test_build_meta_helpers_tolerate_subprocess_timeout |
| WR-01 | warning | open | - |
| WR-02 | warning | open | - |
| WR-03 | warning | open | - |
| WR-04 | warning | open | - |
| WR-05 | warning | open | - |
| WR-06 | warning | open | - |
| WR-07 | warning | open | - |
| IN-01 | info | open | - |

Dispositions: `open` (recorded, not yet triaged), `fixed`, `skipped`, `deferred`.
Set `deferred` by hand and put the reason in the Source cell; both are preserved. A `|` in the reason is kept as prose and escaped on the next run.
Re-running the gate keeps every row it can. A row the current review no longer reports is kept and its Source cell flagged, so a finding does not leave this record silently.
