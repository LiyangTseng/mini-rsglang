---
phase: 01
review: 01-REVIEW.md
titles: json
findings:
  - id: CR-01
    severity: critical
    disposition: open
    title: "Ctrl-C (SIGINT to the whole process group) makes the launcher exit 1 and report a failure"
  - id: WR-01
    severity: warning
    disposition: open
    title: "The `--shell-mode` rejection can be bypassed by an abbreviation, and rust mode then runs silently with shell-mode limits"
  - id: WR-02
    severity: warning
    disposition: open
    title: "The parent watchdog records its parent pid too late and can miss a launcher that has already died, leaving a GPU scheduler orphaned"
  - id: WR-03
    severity: warning
    disposition: open
    title: "`setpgid(0, 0)` takes the launcher out of the terminal's foreground process group whenever a wrapper starts it, so Ctrl-C never reaches it"
  - id: WR-04
    severity: warning
    disposition: open
    title: "The Rust handshake accepts a line with no `eos_token_id` key, which the documented contract forbids"
  - id: WR-05
    severity: warning
    disposition: open
    title: "`shutdown()` always re-sends SIGINT to the group, which interrupts upstream's graceful `scheduler.shutdown()` after an external group SIGINT"
  - id: IN-01
    severity: info
    disposition: open
    title: "Struct-level wire decoders do not validate `__type__`"
  - id: IN-02
    severity: info
    disposition: open
    title: "A stdin read error is reported as \"stdin EOF\" and exits 3, even for a malformed (non-UTF-8) handshake"
  - id: IN-03
    severity: info
    disposition: open
    title: "The upstream SHA is included separately in two crates"
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
open: 15
total: 15
recorded: 2026-10-04T04:46:01.408Z
---

# Phase 01: Code Review Disposition

| Finding | Severity | Disposition | Source |
|---------|----------|-------------|--------|
| CR-01 | critical | open | - |
| WR-01 | warning | open | - |
| WR-02 | warning | open | - |
| WR-03 | warning | open | - |
| WR-04 | warning | open | - |
| WR-05 | warning | open | - |
| IN-01 | info | open | - |
| IN-02 | info | open | - |
| IN-03 | info | open | - |
| IN-04 | info | open | - |
| IN-05 | info | open | - |
| IN-06 | info | open | - |
| IN-07 | info | open | - |
| IN-08 | info | open | - |
| IN-09 | info | open | - |

Dispositions: `open` (recorded, not yet triaged), `fixed`, `skipped`, `deferred`.
Set `deferred` by hand and put the reason in the Source cell; both are preserved. A `|` in the reason is kept as prose and escaped on the next run.
Re-running the gate keeps every row it can. A row the current review no longer reports is kept and its Source cell flagged, so a finding does not leave this record silently. ONE exception: when a finding id is REUSED by a different finding, the earlier decision cannot keep a row — the id is taken — and it is dropped. A RECORDED decision (anything but `open`) is named on the console when that happens; a row still at `open` is replaced silently, because `open` records no decision to lose.
