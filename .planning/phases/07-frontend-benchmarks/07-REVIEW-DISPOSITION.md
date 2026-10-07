---
phase: 07
review: 07-REVIEW.md
titles: json
findings:
  - id: WR-01
    severity: warning
    disposition: open
    title: "Unvalidated `--prompt-words-max` can panic or balloon into a near-u32::MAX random range"
  - id: WR-02
    severity: warning
    disposition: open
    title: "A clean EOF with no `[DONE]` is always `Outcome::Completed`, even when the connection closed because the backend died"
  - id: WR-03
    severity: warning
    disposition: open
    title: "`probe_upstream_sha` rejects an otherwise-valid uppercase-hex SHA"
  - id: IN-01
    severity: info
    disposition: open
    title: "`redact_argv` normalizes internal whitespace in multi-word tokens"
  - id: IN-02
    severity: info
    disposition: open
    title: "`also_best` is `true` by default even when no sweep ever ran"
  - id: IN-03
    severity: info
    disposition: open
    title: "`is_secret_name`/redaction allowlist is a fixed substring list"
open: 6
total: 6
recorded: 2026-10-07T20:18:53.719Z
---

# Phase 07: Code Review Disposition

| Finding | Severity | Disposition | Source |
|---------|----------|-------------|--------|
| WR-01 | warning | open | - |
| WR-02 | warning | open | - |
| WR-03 | warning | open | - |
| IN-01 | info | open | - |
| IN-02 | info | open | - |
| IN-03 | info | open | - |

Dispositions: `open` (recorded, not yet triaged), `fixed`, `skipped`, `deferred`.
Set `deferred` by hand and put the reason in the Source cell; both are preserved. A `|` in the reason is kept as prose and escaped on the next run.
Re-running the gate keeps every row it can. A row the current review no longer reports is kept and its Source cell flagged, so a finding does not leave this record silently. ONE exception: when a finding id is REUSED by a different finding, the earlier decision cannot keep a row — the id is taken — and it is dropped. A RECORDED decision (anything but `open`) is named on the console when that happens; a row still at `open` is replaced silently, because `open` records no decision to lose.
