---
schema_version: 1
open_count: 0
waived_count: 1
fixed_count: 2
total_count: 3
last_updated: 2026-10-06T08:09:54.818Z
---

# Broken Windows Ledger

> Cross-phase defect register. With `workflow.windows_enforce` enabled, `/gsd-ship` blocks while `open_count > 0`.
> Waive with `gsd-tools windows waive <id> "<reason>"` (reason required).
> Mark fixed with `gsd-tools windows fixed <id>`.

| id | phase | kind | file | line | description | status | reason | recorded_at | resolved_at |
|----|-------|------|------|------|-------------|--------|--------|-------------|-------------|
| 1 | 01 | unrun-verify | scripts/gpu_phase1_check.sh |  | GPU end-of-phase check (criteria 2/3, no-orphan, frozen frontend) not run: needs the Linux GPU box; human sign-off pending | waived | User decision 2026-10-05: Linux+CUDA hardware dependency must not block Phase 1 completion or broader project progress. A GitHub collaborator will run gpu_phase1_check.sh and the Linux-only pdeathsig_sigkill test on their own Linux+GPU machine. Tracked as UAT tests 1 and 6 (both blocked_by: physical-device), unresolved on 01-VERIFICATION.md with 4 explicit, disclosed overrides (accepted_by: LiyangTseng). | 2026-10-04T03:57:00.618Z | 2026-10-05T08:04:03.231Z |
| 2 | 03 | deviation | crates/rsg-server/src/transport.rs |  | Rule 1 fix: scheduler_side_round_trips_with_split_frontend was racy (ZMQ connect-before-first-send drop); fixed with a bounded retry-until-received loop | fixed |  | 2026-10-06T08:09:18.628Z | 2026-10-06T08:09:44.685Z |
| 3 | 03 | deviation | crates/rsg-server/src/bin/mock-scheduler.rs |  | Rule 1 fix: default ZMQ_RECONNECT_IVL (100ms) + cross-thread socket handoff caused ~46-100ms first-message delay; fixed by opening sockets on the engine thread itself and lowering ZMQ_RECONNECT_IVL to 1ms | fixed |  | 2026-10-06T08:09:50.602Z | 2026-10-06T08:09:54.818Z |

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
  },
  {
    "id": 2,
    "kind": "deviation",
    "phase": "03",
    "file": "crates/rsg-server/src/transport.rs",
    "line": null,
    "description": "Rule 1 fix: scheduler_side_round_trips_with_split_frontend was racy (ZMQ connect-before-first-send drop); fixed with a bounded retry-until-received loop",
    "status": "fixed",
    "reason": "",
    "recorded_at": "2026-10-06T08:09:18.628Z",
    "resolved_at": "2026-10-06T08:09:44.685Z",
    "milestone": null
  },
  {
    "id": 3,
    "kind": "deviation",
    "phase": "03",
    "file": "crates/rsg-server/src/bin/mock-scheduler.rs",
    "line": null,
    "description": "Rule 1 fix: default ZMQ_RECONNECT_IVL (100ms) + cross-thread socket handoff caused ~46-100ms first-message delay; fixed by opening sockets on the engine thread itself and lowering ZMQ_RECONNECT_IVL to 1ms",
    "status": "fixed",
    "reason": "",
    "recorded_at": "2026-10-06T08:09:50.602Z",
    "resolved_at": "2026-10-06T08:09:54.818Z",
    "milestone": null
  }
]
````
