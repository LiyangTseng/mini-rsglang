---
schema_version: 1
open_count: 0
waived_count: 1
fixed_count: 0
total_count: 1
last_updated: 2026-10-05T08:04:03.231Z
---

# Broken Windows Ledger

> Cross-phase defect register. With `workflow.windows_enforce` enabled, `/gsd-ship` blocks while `open_count > 0`.
> Waive with `gsd-tools windows waive <id> "<reason>"` (reason required).
> Mark fixed with `gsd-tools windows fixed <id>`.

| id | phase | kind | file | line | description | status | reason | recorded_at | resolved_at |
|----|-------|------|------|------|-------------|--------|--------|-------------|-------------|
| 1 | 01 | unrun-verify | scripts/gpu_phase1_check.sh |  | GPU end-of-phase check (criteria 2/3, no-orphan, frozen frontend) not run: needs the Linux GPU box; human sign-off pending | waived | User decision 2026-10-05: Linux+CUDA hardware dependency must not block Phase 1 completion or broader project progress. A GitHub collaborator will run gpu_phase1_check.sh and the Linux-only pdeathsig_sigkill test on their own Linux+GPU machine. Tracked as UAT tests 1 and 6 (both blocked_by: physical-device), unresolved on 01-VERIFICATION.md with 4 explicit, disclosed overrides (accepted_by: LiyangTseng). | 2026-10-04T03:57:00.618Z | 2026-10-05T08:04:03.231Z |

````json
[
  {
    "id": 1,
    "kind": "unrun-verify",
    "phase": "01",
    "file": "scripts/gpu_phase1_check.sh",
    "line": null,
    "description": "GPU end-of-phase check (criteria 2/3, no-orphan, frozen frontend) not run: needs the Linux GPU box; human sign-off pending",
    "status": "waived",
    "reason": "User decision 2026-10-05: Linux+CUDA hardware dependency must not block Phase 1 completion or broader project progress. A GitHub collaborator will run gpu_phase1_check.sh and the Linux-only pdeathsig_sigkill test on their own Linux+GPU machine. Tracked as UAT tests 1 and 6 (both blocked_by: physical-device), unresolved on 01-VERIFICATION.md with 4 explicit, disclosed overrides (accepted_by: LiyangTseng).",
    "recorded_at": "2026-10-04T03:57:00.618Z",
    "resolved_at": "2026-10-05T08:04:03.231Z",
    "milestone": null
  }
]
````
