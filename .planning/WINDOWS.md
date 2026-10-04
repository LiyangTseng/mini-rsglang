---
schema_version: 1
open_count: 1
waived_count: 0
fixed_count: 0
total_count: 1
last_updated: 2026-10-04T03:57:00.618Z
---

# Broken Windows Ledger

> Cross-phase defect register. With `workflow.windows_enforce` enabled, `/gsd-ship` blocks while `open_count > 0`.
> Waive with `gsd-tools windows waive <id> "<reason>"` (reason required).
> Mark fixed with `gsd-tools windows fixed <id>`.

| id | phase | kind | file | line | description | status | reason | recorded_at | resolved_at |
|----|-------|------|------|------|-------------|--------|--------|-------------|-------------|
| 1 | 01 | unrun-verify | scripts/gpu_phase1_check.sh |  | GPU end-of-phase check (criteria 2/3, no-orphan, frozen frontend) not run: needs the Linux GPU box; human sign-off pending | open |  | 2026-10-04T03:57:00.618Z |  |

````json
[
  {
    "id": 1,
    "kind": "unrun-verify",
    "phase": "01",
    "file": "scripts/gpu_phase1_check.sh",
    "line": null,
    "description": "GPU end-of-phase check (criteria 2/3, no-orphan, frozen frontend) not run: needs the Linux GPU box; human sign-off pending",
    "status": "open",
    "reason": "",
    "recorded_at": "2026-10-04T03:57:00.618Z",
    "resolved_at": null,
    "milestone": null
  }
]
````
