---
phase: 05-request-lifecycle-http-api
plan: 02
subsystem: infra
tags: [python, uv, fastapi, uvicorn, prompt_toolkit, requirements-lock, mac-dev-env]

# Dependency graph
requires:
  - phase: 01-foundation
    provides: project-local uv-managed .venv synced from sha256-hashed requirements-mac.txt
provides:
  - Mac lock with approved fastapi, uvicorn and prompt_toolkit pins (plus uv-resolved transitives) so upstream's frozen Python frontend module imports on the Mac
affects: [05-05, Phase 6 API fixture comparison]

# Actuals (#2632)
actuals:
  tokens: 5075
  tasks: 2
  commits: 1
  plan_head_before: 89faad947e1f8ac845dd7444350ac4f940983698
  plan_head_after: 308dd9300f704b7a1bc13b6d80830a31e314faac

# Tech tracking
tech-stack:
  added: [fastapi==0.142.2, uvicorn==0.54.0, prompt_toolkit==3.0.53, starlette, pydantic, pydantic-core, anyio, h11, click, annotated-doc, annotated-types, typing-inspection, wcwidth, opentelemetry-api==1.45.1]
  patterns: ["Human package-legitimacy gate before any uv relock that adds PyPI packages outside RESEARCH's crate-only audit (precedent: Phase 2 plan 02-01)"]

key-files:
  created: []
  modified: [requirements-mac.in, requirements-mac.txt]

key-decisions:
  - "Human approved fastapi==0.142.2, uvicorn==0.54.0 and prompt_toolkit==3.0.53 (Task 1 checkpoint) after verifying PyPI project links to their canonical GitHub repos"
  - "Relock surfaced opentelemetry-api==1.45.1 as an additional transitive dependency pulled in directly by fastapi, not foreseen in the Task 1 package list; human separately approved it after confirming it resolves to github.com/open-telemetry/opentelemetry-python (CNCF) with the name spelled correctly"
  - "No --upgrade passed to uv pip compile; uv treated the existing requirements-mac.txt as preferences so all 40 pre-existing pins stayed byte-for-byte identical (verified by the pins-kept diff)"

patterns-established:
  - "Pattern: relock verification always diffs name==version pins before/after against the pre-plan committed lock (via git show HEAD:requirements-mac.txt), not just a visual diff, to catch silent upgrades"

requirements-completed: [API-01]

coverage:
  - id: D1
    description: "requirements-mac.in pins fastapi==0.142.2, uvicorn==0.54.0 and prompt_toolkit==3.0.53; requirements-mac.txt is the uv-compiled, sha256-hashed relock of that input with no pre-existing pin changed"
    requirement: "API-01"
    verification:
      - kind: other
        ref: "python/tests full fast suite (pytest -m 'not slow'): 113 passed, 36 skipped, 60 deselected"
        status: pass
      - kind: other
        ref: "grep acceptance checks on requirements-mac.in/.txt (fastapi/uvicorn/prompt_toolkit/prompt-toolkit/starlette/pydantic lines present)"
        status: pass
    human_judgment: false
  - id: D2
    description: "Upstream's frozen Python frontend module (minisgl.server.api_server) imports unmodified on the Mac from the new lock, with no file under vendor/ touched"
    requirement: "API-01"
    verification:
      - kind: other
        ref: ".venv/bin/python -c \"import minisgl.server.api_server\" (exit 0, printed api_server-import-ok)"
        status: pass
      - kind: other
        ref: "git status --porcelain vendor/ (empty output)"
        status: pass
    human_judgment: false

duration: 45min (includes prior-session Task 1 approval + relock, this session's verify/commit)
completed: 2026-10-06
status: complete
---

# Phase 05 Plan 02: Mac Lock Gate for Upstream's Python Web Stack Summary

**fastapi 0.142.2, uvicorn 0.54.0 and prompt_toolkit 3.0.53 (plus their uv-resolved transitives, including opentelemetry-api 1.45.1) pinned into the hash-locked Mac requirements and synced into .venv, so `minisgl.server.api_server` now imports unmodified on the Mac.**

## Performance

- **Duration:** ~45 min total across two sessions (Task 1 approval + Task 2 relock happened in a prior session; this session verified and committed)
- **Started:** 2026-10-06 (prior session, Task 1)
- **Completed:** 2026-10-06T22:00:15Z
- **Tasks:** 2 (Task 1: checkpoint:human-verify, Task 2: auto)
- **Files modified:** 2 (requirements-mac.in, requirements-mac.txt)

## Accomplishments
- Human approved the PyPI package-legitimacy gate for fastapi, uvicorn and prompt_toolkit (Task 1), confirming each links to its canonical GitHub repo and is spelled correctly
- Relocked requirements-mac.txt with `uv pip compile --generate-hashes` (no `--upgrade`), adding exactly the approved packages plus their resolved dependencies with sha256 hashes, while every one of the 40 pre-existing `name==version` pins stayed unchanged
- Verified `minisgl.server.api_server` now imports standalone in `.venv` on the Mac, with no file under `vendor/` touched
- Ran the full fast pytest suite (`python/tests -m "not slow"`): 113 passed, 36 skipped, 60 deselected, 0 failures

## Task Commits

1. **Task 1: Package legitimacy gate for the upstream frontend's web stack, before any install** - checkpoint:human-verify, no commit (human typed "approved" in a prior session; no side effects before approval)
2. **Task 2: Pin fastapi, uvicorn and prompt_toolkit into the hashed Mac lock and sync .venv** - `308dd93` (feat)

**Plan metadata:** commit pending (this SUMMARY + STATE.md + ROADMAP.md update)

## Files Created/Modified
- `requirements-mac.in` - Added `fastapi==0.142.2`, `uvicorn==0.54.0`, `prompt_toolkit==3.0.53` direct pins
- `requirements-mac.txt` - uv-recompiled hash-pinned lock: added fastapi, uvicorn, prompt-toolkit, starlette, pydantic, pydantic-core, anyio, h11, click, annotated-doc, annotated-types, typing-inspection, wcwidth, and opentelemetry-api (transitive via fastapi), each with sha256 hashes; all 40 pre-existing pins unchanged

## Decisions Made
- Approved fastapi 0.142.2, uvicorn 0.54.0, prompt_toolkit 3.0.53 at the Task 1 blocking-human checkpoint after independent PyPI/GitHub verification
- Separately approved the unforeseen transitive dependency `opentelemetry-api==1.45.1` (pulled in directly by fastapi) after confirming it resolves to the CNCF `open-telemetry/opentelemetry-python` project and the name is spelled correctly — this pin stands as-is, no substitution
- No `--upgrade` flag used during relock, preserving every pre-existing pin exactly (uv's preference-based resolution against the existing lock file)

## Deviations from Plan

None - plan executed exactly as written. The relock pulling in `opentelemetry-api` as an extra transitive dependency was anticipated by the plan's Task 1 language ("uv resolves their dependencies... Each is frozen with sha256 hashes") and handled via the same package-legitimacy gate discipline (human approval before the pin was treated as final), consistent with Rule 3's package-install exclusion in the executor's deviation rules — not an autonomous substitution, but an explicit human sign-off captured in the checkpoint context for this resumed session.

## Issues Encountered

None. The relock and `.venv` sync had already completed successfully in a prior session; this session's job was to verify the on-disk state matched the plan's acceptance criteria (no pin regressions, api_server imports, fast tests green) and commit.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- Plan 05-05 can now run upstream's unmodified Python frontend (`minisgl.server.api_server`) on the Mac against the mock-scheduler to capture API-01 golden fixtures (D-02)
- The resolved FastAPI/Starlette/uvicorn/pydantic versions are available in `requirements-mac.txt` for plan 05-05's fixture manifest generator block, to compare against what the GPU box resolves later
- No blockers introduced; `vendor/mini-sglang` remains untouched, preserving the frozen-Python-frontend constraint

---
*Phase: 05-request-lifecycle-http-api*
*Completed: 2026-10-06*

## Self-Check: PASSED

- FOUND: requirements-mac.in
- FOUND: requirements-mac.txt
- FOUND: .planning/phases/05-request-lifecycle-http-api/05-02-SUMMARY.md
- FOUND: commit 308dd93
