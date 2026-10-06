---
phase: 02-python-frontend-baseline-profile
plan: 01
subsystem: dependency-management
tags: [psutil, aiohttp, uv, package-gate, mac-env, requirements-lock]
requires:
  - phase: 01-vendored-base-wire-codec
    provides: "Hashed Mac uv lock (requirements-mac.txt), bootstrap_mac_env.sh sync/relock flow, and the human package-gate convention"
provides:
  - "Human approval of psutil, aiohttp, py-spy, and hyperfine package legitimacy"
  - "psutil==7.2.2 and aiohttp==3.14.4 pinned into requirements-mac.in"
  - "Relocked, sha256-hashed requirements-mac.txt with aiohttp's transitive deps (aiohappyeyeballs, aiosignal, attrs, frozenlist, multidict, propcache, yarl)"
  - ".venv synced with the new lock; smoke import and fast pytest suite green"
  - "Approved GPU-box-only pins recorded for plan 02-09: py-spy==0.4.2, hyperfine 1.20.0"
affects: [02-03-tracer, 02-09-gpu-box-install]
actuals:
  tokens: 17500
  tasks: 2
  commits: 1
commits: 1
plan_head_before: ac2cdf87bf4ee3366c2e60d3662937522fd42a58
plan_head_after: 844392b38a9075ad77c3eae5d30964b5c5a66c0c
tech-stack:
  added: [psutil==7.2.2, aiohttp==3.14.4]
  patterns:
    - "Blocking-human package-legitimacy gate precedes any new dependency pin (Phase 1 convention repeated)"
    - "uv pip compile without --upgrade so existing pins act as preferences and only new packages are added"
key-files:
  created: []
  modified:
    - requirements-mac.in
    - requirements-mac.txt
key-decisions:
  - "Human approved all four new Phase 2 dependencies (psutil, aiohttp, py-spy, hyperfine) at the Task 1 blocking-human checkpoint on 2026-10-05, after reviewing PyPI/crates.io registry metadata matching each expected upstream source (github.com/giampaolo/psutil, github.com/aio-libs/aiohttp + sub-deps, github.com/benfred/py-spy, github.com/sharkdp/hyperfine) with no typosquat found"
  - "Only psutil and aiohttp are pinned into the Mac lock; py-spy and hyperfine stay GPU-box-only (Mac tests use stand-ins from rsglang.testing, per plan 02-03)"
patterns-established: []
requirements-completed: [BENCH-01]
coverage:
  - id: D1
    description: "Task 1: human approved psutil/aiohttp/py-spy/hyperfine package legitimacy"
    verification: []
    human_judgment: true
    rationale: "Package legitimacy requires human judgment per project convention (gate=blocking-human); cannot be auto-verified. Resolved by a prior executor run; human responded 'approved' verbatim after reviewing registry metadata for all four packages."
  - id: D2
    description: "Task 2: psutil and aiohttp pinned into Mac lock, .venv synced, fast suite green"
    requirement: "BENCH-01"
    verification:
      - kind: unit
        ref: ".venv/bin/python -c \"import psutil, aiohttp; assert psutil.__version__=='7.2.2'; assert aiohttp.__version__=='3.14.4'\""
        status: pass
      - kind: unit
        ref: "diff of pre-phase lock (git show 064b0d9:requirements-mac.txt) against relocked requirements-mac.txt: all 31 pre-existing pins present, zero missing"
        status: pass
      - kind: unit
        ref: ".venv/bin/python -m pytest python/tests -q -m \"not slow\""
        status: pass
    human_judgment: false
duration: 20min
completed: 2026-10-05
status: complete
---

# Phase 2 Plan 1: Package Legitimacy Gate and Mac Dependency Pin Summary

**Human approved psutil, aiohttp, py-spy, and hyperfine at the package-legitimacy gate; psutil 7.2.2 and aiohttp 3.14.4 are now hash-pinned into the Mac uv lock and synced into .venv with the fast test suite still green.**

## Performance
- **Duration:** 20min
- **Started:** 2026-10-05T23:40:00Z (continuation agent spawn)
- **Completed:** 2026-10-05T23:57:44Z
- **Tasks:** 2 completed (Task 1 resolved by prior executor + human; Task 2 executed this session)
- **Files modified:** 2

## Accomplishments
- Task 1's blocking-human package-legitimacy checkpoint was resolved: the human reviewed a prior executor's read-only registry verification of all four new Phase 2 dependencies (psutil 7.2.2, aiohttp 3.14.4, py-spy 0.4.2, hyperfine 1.20.0) against their expected upstream sources and replied "approved" verbatim, with no rejections.
- Appended `psutil==7.2.2` and `aiohttp==3.14.4` to `requirements-mac.in` (now 8 pins, up from 6).
- Relocked `requirements-mac.txt` via `uv pip compile requirements-mac.in --python-version 3.12 --python-platform aarch64-apple-darwin --generate-hashes -o requirements-mac.txt` (no `--upgrade`). All 31 pre-existing `name==version` pins (verified against the pre-phase lock at commit `064b0d9`) are unchanged; 9 new hashed entries were added: `psutil`, `aiohttp`, `aiohappyeyeballs`, `aiosignal`, `attrs`, `frozenlist`, `multidict`, `propcache`, `yarl`.
- Ran `bash scripts/bootstrap_mac_env.sh` (fresh `.venv` in this worktree, since `.venv` is gitignored and not present in a new worktree): it created the venv, synced the 40-package lock, reran the two `--no-deps -e` editable installs, and the smoke import passed.
- Verified `psutil.__version__ == '7.2.2'` and `aiohttp.__version__ == '3.14.4'` import cleanly, and the fast pytest suite (`python/tests -m "not slow"`) still passes: 65 passed, 36 skipped.

## Task Commits
1. **Task 2: Pin psutil and aiohttp into the Mac lock and sync .venv** - `844392b` (feat)

**Plan metadata:** (pending — SUMMARY commit follows this file)

## Files Created/Modified
- `requirements-mac.in` - appended `psutil==7.2.2` and `aiohttp==3.14.4` after the six existing Phase 1 pins
- `requirements-mac.txt` - relocked with uv; added psutil, aiohttp and aiohttp's transitive dependencies with sha256 hashes; all pre-existing pins untouched

## Decisions Made
- Human approved all four packages (psutil, aiohttp, py-spy, hyperfine) at the Task 1 blocking-human checkpoint on 2026-10-05, responding "approved" after reviewing PyPI/crates.io metadata confirming each package's maintainer/organization and GitHub source matched the expected upstream project, with no typosquat found. This is documented here as normal flow, not a deviation — the checkpoint existed precisely to gate this decision.
- Only psutil and aiohttp were added to the Mac lock. py-spy and hyperfine are intentionally excluded here: py-spy needs root/ptrace to attach on macOS and every Mac test uses a stand-in from `rsglang.testing`; hyperfine is a Rust binary installed via `cargo install`, not a PyPI package. Both remain approved for the GPU-box install in plan 02-09.

## Deviations from Plan
None - plan executed exactly as written. (Task 1's resolution was handled by a prior executor + human response before this continuation agent was spawned; this agent executed only Task 2, as instructed in the checkpoint_resolution context.)

## Issues Encountered
None. The relock, bootstrap sync, and fast test suite all succeeded on the first attempt with no pin conflicts.

## User Setup Required
None - no external service configuration required. The human's "approved" response at the Task 1 checkpoint was the only manual input needed, and it was already captured before this session started.

## Next Phase Readiness
py-spy==0.4.2 and hyperfine 1.20.0 are approved for the GPU-box install in plan 02-09 (not installed here — GPU-box only). Plan 02-03 (the phase tracer) can now safely import psutil, since the Mac lock carries it.

---
*Phase: 02-python-frontend-baseline-profile*
*Completed: 2026-10-05*

## Self-Check: PASSED

- FOUND: commit 844392b
- FOUND: requirements-mac.in
- FOUND: .planning/phases/02-python-frontend-baseline-profile/02-01-SUMMARY.md
- FOUND: psutil==7.2.2 pin in requirements-mac.in
