---
phase: 01-vendored-base-wire-codec
plan: 10
subsystem: scheduler-watchdog
tags: [prctl, PDEATHSIG, error-envelope, tdd, gap-closure, code-review]
requires:
  - phase: 01-vendored-base-wire-codec
    provides: "01-08's launcher-pid parent watchdog (start_parent_watchdog, run_scheduler), re-verified by the 01-09/01-10 code review"
provides:
  - "start_parent_watchdog degrades to the polling watchdog on a prctl failure (OSError from CDLL load or a non-zero prctl return), logging one 'PDEATHSIG unavailable' line instead of raising"
  - "libc.prctl.argtypes declared full-width ([c_int, c_ulong x4]) before the call"
  - "run_scheduler starts the watchdog as the first statement inside its error-envelope try, so a watchdog startup failure reaches the launcher as {kind: error, traceback: ...} instead of a silent exit 1"
  - "test_exits_at_once_when_parent_is_not_the_launcher and test_stays_alive_while_parent_is_the_launcher now assert marker-based proof that the watchdog itself caused the exit/survival, not merely an exit code and timing"
  - "WR-06 and WR-09 recorded as fixed in 01-REVIEW-DISPOSITION.md"
affects: [phase-06-gpu-backend-integration, phase-07-benchmark-harness]
actuals:
  tokens: 2344
  tasks: 2
  commits: 4
tech-stack:
  added: []
  patterns:
    - "Backstop features (D-12 defence-in-depth) degrade-and-log on failure rather than raising, keeping the degraded safety net (polling) alive"
    - "Mutation-check evidence (scratch edit -> run -> revert) proving a test change actually closes the gap it claims to close, recorded in the SUMMARY rather than committed"
key-files:
  created: []
  modified:
    - python/rsglang/backend.py
    - python/tests/test_parent_watchdog.py
    - .planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md
key-decisions:
  - "Reverses 01-08's original design ('a failing prctl raises OSError') per the user's UAT test 7 triage of WR-06 as fix-now: the polling thread alone is now treated as sufficient defence-in-depth, with the prctl failure demoted to a logged degradation rather than a hard failure"
  - "The marker-based exit-test hardening (WR-09) distinguishes 'the watchdog exited the child' from 'the child died for any other reason within 5s' using 'calling'/'returned'/'armed' print markers with flush=True, rather than file-based signalling"
patterns-established:
  - "Degrade-and-log pattern for optional OS-level hardening: wrap in try/except OSError, print one line to stderr with flush=True, fall through to the next layer of defence"
requirements-completed: [BASE-02]
coverage:
  - id: D1
    description: "A failing prctl (either CDLL load raising OSError, or prctl() returning non-zero) degrades to the polling watchdog with one 'PDEATHSIG unavailable' stderr line, full-width argtypes declared, and the process keeps running"
    requirement: "BASE-02"
    verification:
      - kind: unit
        ref: "python/tests/test_parent_watchdog.py::test_prctl_failure_degrades_to_polling[prctl-fails]"
        status: pass
      - kind: unit
        ref: "python/tests/test_parent_watchdog.py::test_prctl_failure_degrades_to_polling[cdll-fails]"
        status: pass
    human_judgment: false
  - id: D2
    description: "A watchdog startup failure (any exception from start_parent_watchdog) reaches the launcher as an {kind: error, traceback} envelope instead of a silent exit"
    requirement: "BASE-02"
    verification:
      - kind: unit
        ref: "python/tests/test_parent_watchdog.py::test_watchdog_startup_failure_reaches_launcher_as_error_envelope"
        status: pass
    human_judgment: false
  - id: D3
    description: "test_exits_at_once_when_parent_is_not_the_launcher proves the watchdog itself caused the exit (requires 'calling', rejects 'returned' and any Traceback), confirmed by two mutation checks"
    requirement: "BASE-02"
    verification:
      - kind: unit
        ref: "python/tests/test_parent_watchdog.py::test_exits_at_once_when_parent_is_not_the_launcher"
        status: pass
      - kind: manual
        ref: "Mutation (a) misspelled import and mutation (b) RuntimeError-at-top-of-start_parent_watchdog, both run on scratch edits reverted before committing (see Deviations / Mutation Checks below)"
        status: pass
    human_judgment: false
  - id: D4
    description: "WR-06 and WR-09 recorded as fixed in 01-REVIEW-DISPOSITION.md"
    requirement: "BASE-02"
    verification:
      - kind: unit
        ref: "grep '| WR-06 | warning | fixed |' and '| WR-09 | warning | fixed |' in 01-REVIEW-DISPOSITION.md"
        status: pass
    human_judgment: false
duration: 35min
completed: 2026-10-04
status: complete
---

# Phase 01 Plan 10: Prctl-Failure Degradation and Watchdog-Exit Test Hardening Summary

**A failing prctl(PR_SET_PDEATHSIG) now logs and degrades to the polling watchdog with full-width argtypes, watchdog startup failures reach the launcher as error envelopes, and the watchdog exit test now requires marker-based proof that the watchdog itself caused the exit — closing code-review gaps WR-06 and WR-09.**

## Performance
- **Duration:** ~35min
- **Started:** 2026-10-04 (session start)
- **Completed:** 2026-10-04
- **Tasks:** 2
- **Files modified:** 3 (`python/rsglang/backend.py`, `python/tests/test_parent_watchdog.py`, `.planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md`)

## Accomplishments
- `start_parent_watchdog`'s Linux branch wraps the CDLL load, `argtypes` declaration, and the `prctl` call in one `try`/`except OSError`, printing `rsglang: PDEATHSIG unavailable (...); using the polling watchdog only` to stderr and falling through to the getppid re-check and polling thread, rather than raising and aborting scheduler startup.
- `libc.prctl.argtypes` is now declared as `[c_int, c_ulong, c_ulong, c_ulong, c_ulong]` before the call, so the variadic arguments are passed at full width.
- `run_scheduler` moves `start_parent_watchdog(launcher_pid)` to be the first statement inside its existing error-envelope `try`, so any exception the watchdog setup raises is reported to the launcher as `{kind: error, rank, traceback}` instead of a bare "exited with code 1" with no cause. The immediate-exit path (`os._exit(1)` when the launcher already died) is unaffected — it still exits with no envelope, correctly, since the launcher is gone.
- Two new tests prove this: `test_prctl_failure_degrades_to_polling` (parametrized over `prctl-fails` and `cdll-fails`) and `test_watchdog_startup_failure_reaches_launcher_as_error_envelope`.
- The two existing child-program tests are hardened with print markers (`calling`/`returned`/`armed`, all `flush=True`) so they can no longer pass vacuously on an ImportError or any other traceback that happens to exit 1 within 5s.
- WR-06 and WR-09 recorded as `fixed` in `01-REVIEW-DISPOSITION.md`; `open` count drops from 17 to 15.

## Task Commits
1. **Task 1 (tracer, TDD): prctl-failure degradation + error-envelope startup reporting**
   - RED: `c54c756` (test) — added `test_prctl_failure_degrades_to_polling[prctl-fails|cdll-fails]` and `test_watchdog_startup_failure_reaches_launcher_as_error_envelope`; confirmed failing on unmodified `backend.py`
   - GREEN: `05ffff3` (feat) — degrade-and-log prctl path with declared argtypes; `start_parent_watchdog` moved inside `run_scheduler`'s error-envelope try
2. **Task 2 (auto, TDD): hardened exit test + mutation checks + disposition update**
   - `d523d34` (test) — hardened `test_exits_at_once_when_parent_is_not_the_launcher` and `test_stays_alive_while_parent_is_the_launcher` with marker-based assertions
   - `3f35d56` (docs) — WR-06 and WR-09 marked `fixed` in `01-REVIEW-DISPOSITION.md`

**Plan metadata commit:** pending (this SUMMARY + REQUIREMENTS, committed immediately after this file is written — see note below)

_Note: TDD tasks produced the standard RED -> GREEN commit pair for Task 1. Task 2 is test-hardening plus a docs update; its two mutation checks were run on scratch edits and reverted before committing (never their own commit) — see "Deviations from Plan / Mutation-Check Evidence" below._

## Files Created/Modified
- `python/rsglang/backend.py` — `start_parent_watchdog`'s Linux branch now catches `OSError` around the CDLL load + `argtypes` + `prctl` call, logs one line, and continues; `run_scheduler` starts the watchdog inside its `try`.
- `python/tests/test_parent_watchdog.py` — two new tests (prctl-failure degradation, startup-failure envelope); two existing tests hardened with print markers and `Traceback`-absence assertions.
- `.planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md` — WR-06 and WR-09 rows and frontmatter entries changed `open` -> `fixed`; `open: 17` -> `open: 15`.

## Decisions Made
- This plan deliberately reverses 01-08's original design decision ("a failing prctl raises OSError"), per the user's UAT test 7 triage ("修 WR-06"). The polling thread alone is treated as sufficient backstop (D-12 defence-in-depth), and a prctl failure is now a logged degradation, not a startup failure. Consequence (documented in the plan, re-confirmed here): a GPU run of step 3 passing no longer proves prctl armed — `test_linux_arms_pdeathsig_sigkill` (UAT test 6, Linux-only) remains the proof of that, since a degraded prctl leaves `PR_GET_PDEATHSIG` at 0 and fails that test.
- Markers are printed with `flush=True` and read from captured stdout/stderr rather than using a file-based signal, keeping the tests self-contained in the `-c` program string as the existing tests already did.

## Deviations from Plan

None — plan executed exactly as written. For completeness, the plan's required RED output and mutation-check evidence are recorded below (these are evidence/verification artifacts the plan explicitly asked to be recorded in the SUMMARY, not deviations).

### RED Output (Task 1, before the GREEN fix)

Running `.venv/bin/python -m pytest python/tests/test_parent_watchdog.py -v -rs` on the unmodified `backend.py` (immediately after adding the new tests):

```
test_exits_at_once_when_parent_is_not_the_launcher PASSED
test_stays_alive_while_parent_is_the_launcher PASSED
test_prctl_failure_degrades_to_polling[prctl-fails] FAILED
test_prctl_failure_degrades_to_polling[cdll-fails] FAILED
test_watchdog_startup_failure_reaches_launcher_as_error_envelope FAILED
test_linux_arms_pdeathsig_sigkill SKIPPED

3 failed, 2 passed, 1 skipped in 0.70s
```

- `test_prctl_failure_degrades_to_polling[cdll-fails]` failed with the fake `ctypes.CDLL` raising `FileNotFoundError: [Errno 2] no libc`, propagating unhandled out of `start_parent_watchdog` as the child process's uncaught exception (returncode 1, not 0).
- `test_prctl_failure_degrades_to_polling[prctl-fails]` failed the same way, via the `OSError(ctypes.get_errno(), "prctl(PR_SET_PDEATHSIG) failed")` raise on the fake's `-1` return.
- `test_watchdog_startup_failure_reaches_launcher_as_error_envelope` failed on `q.get_nowait()` raising `queue.Empty`, because the unmodified `run_scheduler` called `start_parent_watchdog` before the `try`, so the `RuntimeError` from the monkeypatch escaped without ever reaching the `except BaseException` that posts the envelope.

After the GREEN fix: `5 passed, 1 skipped in 1.75s`.

### Mutation-Check Evidence (Task 2)

Both mutations were applied as scratch edits, run, observed, and reverted before committing. `git diff --stat python/rsglang/backend.py` against the Task 1 commit was empty after the revert, confirmed.

**Mutation (a):** In the hardened `test_exits_at_once_when_parent_is_not_the_launcher`'s child program, changed the import to `from rsglang.backend import start_parent_watchdog_typo as start_parent_watchdog`.
- Result: `1 failed in 0.05s`. `assert "calling" in result.stdout` failed — stdout was empty; stderr contained `ImportError: cannot import name 'start_parent_watchdog_typo' ... Did you mean: 'start_parent_watchdog'?`. The hardened test correctly fails on a broken import.

**Mutation (b):** Inserted `raise RuntimeError("mutation check WR-09")` as the first statement of `start_parent_watchdog` in `python/rsglang/backend.py`.
- Hardened test: `1 failed in 0.05s`. `assert "Traceback" not in result.stderr` failed — "calling" was present (the call was reached) but the child's stderr contained a `RuntimeError: mutation check WR-09` traceback.
- Pre-change test (saved via `git show HEAD:python/tests/test_parent_watchdog.py` into a scratch path, run with `-k exits_at_once`, before Task 2's edits were committed): `1 passed, 5 deselected, 1 warning in 0.04s`. This is exactly the hole WR-09 described — the old test can't distinguish "the watchdog exited the child" from "the child died from any other exception within 5 s".
- Both `backend.py` edits were reverted immediately after observing each result; the file's diff relative to the Task 1 (GREEN) commit is empty.

**Total deviations:** 0. **Impact:** None — the plan's verification evidence requirements were satisfied exactly as specified, with no additional changes needed.

## Issues Encountered
`.venv/bin/python` did not exist at the start of this plan (fresh worktree checkout). Per the plan's `<precondition>` on Task 1, ran `bash scripts/bootstrap_mac_env.sh` (idempotent, worktree-safe), which created `.venv`, installed pinned dependencies (`torch==2.9.1`, `pyzmq==27.2.0`, `msgpack==1.2.3`, etc.), and installed `minisgl` and `rsglang` in editable mode from this worktree. Confirmed `rsglang.__file__` resolved under this checkout's `python/` before proceeding. This is routine environment setup, not a deviation from the plan's code changes.

## User Setup Required
None — no external service configuration required. (The `.venv` bootstrap above was run automatically as part of satisfying the plan's stated precondition; nothing further is needed from the user.)

## Verification Summary

- `.venv/bin/python -m pytest python/tests/test_parent_watchdog.py -q -rs` → `5 passed, 1 skipped in 1.78s` (skip reason: "PR_SET_PDEATHSIG is Linux-only")
- `cargo build -p rsg-server` → succeeded (`Finished `dev` profile ... in 37.39s`)
- `.venv/bin/python -m pytest python/tests/test_launch_rust_e2e.py -q` → `10 passed in 48.80s`
- `grep -c "PDEATHSIG unavailable" python/rsglang/backend.py` → `1`
- `grep -c "argtypes" python/rsglang/backend.py` → `1`
- `grep -c "calling" python/tests/test_parent_watchdog.py` → `2`
- `grep -c "Traceback" python/tests/test_parent_watchdog.py` → `3`
- `grep -q '| WR-06 | warning | fixed |'` and `'| WR-09 | warning | fixed |'` in `01-REVIEW-DISPOSITION.md` → both found; `| WR-07 | warning | fixed |` still present (unaffected)
- `bash scripts/check_all.sh --offline` → `check_all: OK` (Rust unit/wire tests, 79 passed/37 skipped pytest, fixture freshness, WIRE-02 decode, vendored-tree offline check all green)
- `git status --porcelain vendor/` → empty, both before and after this plan's changes

## Next Phase Readiness
G-01-7-WR06 and G-01-7-WR09 are closed. `01-REVIEW-DISPOSITION.md` now shows `open: 15` (down from 17 after 01-09). Remaining `open` warnings from the 01-09/01-10 review cycle: WR-01, WR-04 (not in the current incremental review — carried from the prior review), plus IN-10..IN-13 (info-level, all `open`). WR-03/WR-05/WR-10 remain `deferred` to later phases as previously recorded. Ready for 01-11 if further gap-closure plans exist in this chain, or for the next phase in ROADMAP.md.

---
*Phase: 01-vendored-base-wire-codec*
*Completed: 2026-10-04*
