---
phase: 06-gpu-end-to-end-parity
plan: 08
subsystem: testing
tags: [parity-report, abort-timing, D-09, D-05, harness-bug, PAR-01, PAR-02]

requires:
  - phase: 06-07
    provides: "docs/benchmarks/parity-report.json (the real GPU-measured sidecar) and docs/benchmarks/parity-report.md's D-05 bisection / D-08 findings"
provides:
  - "crates/rsg-server/src/main.rs: --abort-timing default changed from immediate to deferred (D-09 branch C)"
  - "python/rsglang/parity/sweep.py: _bounded_detoks() fixes a real parity-harness bug that had been miscounted as a PAR-01 mismatch"
  - "docs/benchmarks/parity-report.{md,json}: final, corrected GPU parity report — Criterion 2 PASSES 128/128 for both models, zero divergence"
  - "UPSTREAM.md: Known upstream issues entry for the abort-during-prefill crash and the D-09 branch-C disposition"
  - ".planning/STATE.md: abort-timing default for Phase 7, and the harness-bug correction superseding the earlier apparent PAR-01 FAIL"
affects: [07]

actuals:
  tokens: 253289
  tasks: 4
  commits: 12
  plan_head_before: 001b266c2228a965080467f5472b2c853494ea43
  plan_head_after: PENDING_FINAL_COMMIT

tech-stack:
  added: []
  patterns:
    - "A measurement harness that unconditionally joins every tap record for a uid must bound the join at the first terminal (finished=true) record, matching what any real consumer (the frontend's own response-finalization logic) actually sees — an unbounded join can double-count backend-internal bookkeeping that never reaches a client"
    - "When a GPU-measured gate appears to fail, check the real HTTP response text against the derived comparison fields before reaching for a nondeterminism explanation — compare.py's output_ids/ids_match is itself derived data, not a direct observation, and can itself be wrong"

key-files:
  created:
    - crates/rsg-server/tests/backend_finish_boundary.rs
    - python/tests/test_scheduler_abort_fix.py (not created — branch C was chosen, not branch B; see Decisions Made)
  modified:
    - crates/rsg-server/src/main.rs
    - crates/rsg-server/src/engine.rs
    - python/rsglang/parity/sweep.py
    - python/tests/test_parity_check.py
    - python/tests/test_parity_report.py
    - docs/benchmarks/parity-report.md
    - docs/benchmarks/parity-report.json
    - UPSTREAM.md
    - .planning/REQUIREMENTS.md
    - .planning/STATE.md
    - .planning/phases/06-gpu-end-to-end-parity/06-07-SUMMARY.md
    - .planning/phases/06-gpu-end-to-end-parity/06-08-PLAN.md

key-decisions:
  - "Task 0 (user-approved scope expansion): investigated both 06-07 PAR-01 mismatches (edge-08, both models) with a deterministic Mac-side reproduction before accepting the apparent FAIL. Found the Rust frontend's engine/dispatch pair has no independent stopping logic and never second-guesses the backend's finished flag -- this finding is unaffected by the later correction. The investigation's own explanation for the mismatch (GPU backend nondeterminism) was wrong and is corrected below."
  - "Task 1/D-09: chose branch C (deep/structural) over branch B (localized scheduler fix). abort_stress.reproduced=yes and conclusive=yes, but zero double_free_uids, zero dup_free_slot_events and zero collisions were recorded in either run or the 72-trial probe -- there is no double-free or collision evidence to localize a <=30-line scheduler.py fix around. The immediate-timing run's own failure mode is a full scheduler process crash (KeyboardInterrupt traceback, watcher verdict unhealthy), not RESEARCH.md's predicted silent corruption. The project-wide --abort-timing default is changed to deferred (crates/rsg-server/src/main.rs); the vendored scheduler stays pristine."
  - "Task 2 (checkpoint:human-action, gate=blocking-human): resolved as 'no-rerun: branch C', per the plan's own instruction that branches A/A'/C require no GPU re-run (nothing changed in the measured system). A GPU re-run was nonetheless performed independently -- not because of D-09, but because of the harness-bug investigation below -- and that re-run incidentally reconfirms D-09's evidence unchanged in kind (immediate=crash, deferred=none, reproduced=yes, conclusive=yes, 1 prefill_window probe hit, 0 double frees)."
  - "The real root cause of the apparent Criterion 2 FAIL was found AFTER Task 0's investigation closed: not GPU nondeterminism (Task 0's conclusion, corrected here) and not a frontend defect, but a bug in the parity test harness's own token-joining code. python/rsglang/parity/sweep.py's join_sequential/join_concurrent collected every backend-tap detok record for a uid unconditionally; the scheduler's pipelined execution can emit a straggler detok record after it already sent finished=true (observed for the corpus's last request in each sequential session, raced against that session's own teardown SIGINT). That straggler inflated the Python side's output_ids by one token even though the real HTTP response text both frontends sent was already byte-identical. Fixed by _bounded_detoks() (commit ae8feec), which truncates each uid's detok records at the first finished=true record, with two regression tests in test_parity_check.py. No production code change was needed in either frontend."
  - "A full GPU re-run with this fix in place (on top of the D-09 branch-C deferred default) produced the final docs/benchmarks/parity-report.json: 128/128 for both models, zero divergence anywhere. PAR-01's hard gate genuinely passes outright, so no '## PAR-01 disposition (D-05)' section was added to parity-report.md -- Task 3's own instruction only requires that section when the gate actually fails on the final JSON, and it does not."
  - "docs/benchmarks/parity-report.md was fully regenerated against the final JSON rather than patched incrementally, per the original 06-07-PLAN.md Task 2 heading contract. The superseded 'PAR-01 off-by-one investigation' section is kept (not deleted) with an explicit, dated correction note explaining what was wrong and pointing at the fix commit, consistent with how 06-07-SUMMARY.md and 06-08-PLAN.md's Task 0 conclusion are also corrected (not silently rewritten)."
  - "python/tests/test_parity_report.py gained two new tests (not in the original 06-07 tie-test set): one ties the new '## Abort-timing decision (D-09)' section's abort-timing default line to STATE.md's own recorded decision (parsed, not hardcoded), and one re-derives Criterion 2's pass/fail from the summary fields (status ok, n>=100, matched==n) to decide whether a PAR-01 disposition section is required, rather than trusting a cached verdict string."

requirements-completed: [PAR-01, PAR-02]

coverage:
  - id: D1
    description: "The Rust frontend's engine/dispatch pair is proven, via a deterministic Mac-side reproduction, to have no independent stopping logic and to never second-guess the backend's finished flag"
    requirement: "PAR-01"
    verification:
      - kind: unit
        ref: "cargo test -p rsg-server --test backend_finish_boundary --test http_chat -q"
        status: pass
    human_judgment: false
  - id: D2
    description: "D-09's abort-timing default for Phase 7 is chosen from the recorded evidence (reproduced/conclusive/failure_mode/probe double-frees), not assumed in advance, and the default is changed project-wide to deferred (branch C)"
    requirement: "PAR-02"
    verification:
      - kind: unit
        ref: "cargo test --workspace -- --test-threads=1"
        status: pass
      - kind: unit
        ref: "command: .venv/bin/python scripts/check_upstream.py --offline"
        status: pass
    human_judgment: false
  - id: D3
    description: "The real root cause of the apparent PAR-01 mismatch (a parity-harness join bug, not a frontend defect or GPU nondeterminism) is found, fixed with regression tests, and a GPU re-run confirms 128/128 for both models with zero divergence"
    requirement: "PAR-01"
    verification:
      - kind: unit
        ref: "python/tests/test_parity_check.py::test_bounded_detoks_discards_straggler_after_finished"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_check.py::test_join_sequential_discards_straggler_detok_for_par01"
        status: pass
      - kind: unit
        ref: "command: .venv/bin/python scripts/parity_check.py verdict docs/benchmarks/parity-report.json --criterion 2"
        status: pass
    human_judgment: false
  - id: D4
    description: "STATE.md and parity-report.md both record the D-09 decision with its evidence, and the harness-bug correction supersedes the earlier apparent PAR-01 FAIL rather than leaving it standing"
    requirement: "PAR-01"
    verification:
      - kind: unit
        ref: "python/tests/test_parity_report.py::test_abort_timing_decision_section_present_and_agrees_with_state"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_report.py::test_par01_disposition_section_matches_gate_outcome"
        status: pass
    human_judgment: false
  - id: D5
    description: "A human reads the final, corrected docs/benchmarks/parity-report.md and the D-09 decision against ROADMAP Phase 6 criteria 1-4 and replies approved, or names a decision to revisit"
    requirement: "PAR-01"
    verification: []
    human_judgment: true
    rationale: "The plan's Task 3 human-check is a judgment call on narrative accuracy, fairness of the D-09 decision, and whether the correction note is credible -- no automated assertion can stand in for a human's 'approved' reply, same rationale as 06-07's own human-check item"

duration: ~2h55m (first 06-08 commit 2026-10-07T13:47:13-07:00 through this finalization session's close)
completed: 2026-10-07
status: complete
---

# Phase 06 Plan 08: Abort-Timing Decision, PAR-01 Root-Cause Correction, and Final Parity Report Summary

**Chose D-09 branch C (`--abort-timing` default changed to `deferred`, no vendored scheduler fix) from the abort-stress evidence; then found and fixed the real cause of the apparent 06-07 PAR-01 FAIL — not GPU nondeterminism as first concluded, but a parity-harness bug (`sweep.py` double-counting a post-finish straggler `detok` record) — and the final GPU re-run shows 128/128 for both models with zero divergence, so PAR-01's hard gate genuinely passes and no disposition was needed.**

## Performance

- **Duration:** ~2h55m across the full plan (Task 0/1 committed earlier in this session; this execution finalizes Task 3 and closes out the plan)
- **Completed:** 2026-10-07
- **Tasks:** 4 of 4 (Task 0, Task 1, Task 2, Task 3)
- **Files modified:** 13 (see `key-files` above)

## Accomplishments

- **Task 0 (user-approved scope expansion):** Added `crates/rsg-server/tests/backend_finish_boundary.rs` (2 new tests) proving, via direct injection into the dispatcher's bound detok socket, that `engine.rs`/`dispatch.rs` only ever branch on the wire's `finished` bit — never an independent "looks like EOS" call, and never second-guessing an immediate `finished=true` on the first message for a uid. This finding is unaffected by the correction below; it remains the reason no Rust-side fix was ever warranted.
- **Task 1 (D-09 triage):** Chose branch C from the abort-stress evidence (`reproduced=yes`, `conclusive=yes`, `immediate` failure_mode `crash`, `deferred` failure_mode `none`, zero double-frees/collisions anywhere) and changed the project-wide `--abort-timing` default to `deferred` in `crates/rsg-server/src/main.rs`, recording the decision in `UPSTREAM.md`'s new `## Known upstream issues` section and `.planning/REQUIREMENTS.md`'s LIFE-05 line.
- **Task 2 (checkpoint:human-action):** Resolved as "no-rerun: branch C" per the plan's own instruction — branch C requires no GPU re-run because nothing changed in the measured system. A GPU re-run happened anyway, independently, as part of the harness-bug investigation below, and it reconfirms D-09's evidence in kind.
- **The real investigation (beyond this plan's original scope, closing out what Task 0 should have found):** A repeat GPU run reproduced the exact same 127/128 result byte-for-byte, ruling out random nondeterminism. Direct inspection of the raw backend tap showed the scheduler's pipelined execution emits a straggler `detok` record for a uid *after* it already sent `finished: true`, for the corpus's last request in each sequential session. The real HTTP response text both frontends sent was already byte-identical for both mismatched prompts in both models — the discrepancy existed only in the harness's own derived `output_ids`/`ids_match` fields. Fixed with `_bounded_detoks()` in `python/rsglang/parity/sweep.py` (commit `ae8feec`), which truncates each uid's detok records at the first `finished=true` record, matching what any real client actually receives. Two regression tests added to `python/tests/test_parity_check.py`.
- **Task 3 (finalize):** Ran the full GPU verification one more time with the harness fix in place, on top of the branch-C `deferred` default: **128/128 for both Qwen/Qwen3-0.6B and Llama-3.2-1B-Instruct, zero divergence anywhere** (`docs/benchmarks/parity-report.json`, committed unedited). Fully regenerated `docs/benchmarks/parity-report.md` against this final JSON per the original 06-07-PLAN.md Task 2 heading contract, added `## Abort-timing decision (D-09)` with the Python-baseline fairness note for Phase 7, and corrected (not silently rewrote) the superseded "PAR-01 off-by-one investigation" section with a dated note explaining the real root cause. No `## PAR-01 disposition (D-05)` section was needed, since Criterion 2's hard gate passes outright on the final JSON. Added two tests to `python/tests/test_parity_report.py` tying the D-09 section to STATE.md's decision line and re-deriving the gate's pass/fail from the summary fields. Recorded the decision and the correction in `.planning/STATE.md`, replacing the "code reading only" blocker with the empirical result and removing the false PAR-01-failed claim. Added dated correction notes (not silent rewrites) to `06-07-SUMMARY.md` and `06-08-PLAN.md`'s Task 0 conclusion.

## Task Commits

1. **Task 0: Investigate the PAR-01 one-token-short divergences (Mac-side reproduction)** — `6558c46` (test)
2. **Task 0 (continued): Record the then-current investigation finding** — `224fcb6` (docs) — *superseded by the correction below, kept with a dated note, not deleted*
3. **Task 1: Route around the abort-during-prefill crash with `--abort-timing deferred` (D-09 branch C)** — `de520b9` (fix)
4. **Task 2:** resolved as "no-rerun: branch C" — no code commit of its own (per the plan's own instruction for branches A/A'/C)
5. **Harness-bug fix (found while finalizing Task 3's own verification, not a new task): stop the parity test harness from double-counting a post-finish straggler detok record** — `ae8feec` (fix)
6. **Task 3: Commit the final GPU parity sidecar (128/128, zero divergence)** — `95f819f` (docs)
7. **Task 3: Regenerate parity-report.md against the final JSON with the harness-bug correction** — `19f4ce4` (docs)
8. **Task 3: Record the D-09 decision and the harness-bug correction in STATE.md** — `feef0ad` (docs)
9. **Task 3: Add dated correction notes to 06-07-SUMMARY.md and 06-08-PLAN.md Task 0** — `25b084e` (docs)

**Plan metadata:** this commit (pending, see below — includes this SUMMARY, STATE.md, ROADMAP.md, REQUIREMENTS.md).

## Files Created/Modified

- `crates/rsg-server/tests/backend_finish_boundary.rs` - new Mac-only tests proving engine/dispatch pass through the backend's `finished` flag (Task 0)
- `crates/rsg-server/src/main.rs` - `--abort-timing` default changed to `Deferred` (Task 1, D-09 branch C)
- `crates/rsg-server/src/engine.rs` - supporting change for the default-timing plumbing (Task 1)
- `UPSTREAM.md` - `## Known upstream issues` entry recording D-09's evidence and the branch-C disposition (Task 1)
- `.planning/REQUIREMENTS.md` - LIFE-05 line notes the default change (Task 1)
- `python/rsglang/parity/sweep.py` - `_bounded_detoks()` fixes the real parity-harness bug (harness-bug fix)
- `python/tests/test_parity_check.py` - two regression tests proving the straggler-detok truncation (harness-bug fix)
- `docs/benchmarks/parity-report.json` - final, corrected GPU sidecar: 128/128 for both models, zero divergence (Task 3, committed unedited)
- `docs/benchmarks/parity-report.md` - fully regenerated narrative, with the D-09 section and the dated correction note (Task 3)
- `python/tests/test_parity_report.py` - two new tests tying the D-09 section to STATE.md and re-deriving the PAR-01 disposition requirement (Task 3)
- `.planning/STATE.md` - D-09 decision line, harness-bug correction, replaced blocker, Phase 7 fairness concern (Task 3)
- `.planning/phases/06-gpu-end-to-end-parity/06-07-SUMMARY.md` - dated correction note (Task 3)
- `.planning/phases/06-gpu-end-to-end-parity/06-08-PLAN.md` - dated correction note on Task 0's own conclusion (Task 3)

## Decisions Made

See `key-decisions` in the frontmatter above for the full reasoning on: Task 0's Rust-side finding (still true), the D-09 branch-C choice, Task 2's "no-rerun" resolution, the harness-bug root cause and fix, why no PAR-01 disposition section was needed, and why the report was fully regenerated with a correction note rather than silently edited.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Parity test harness double-counted a post-finish straggler detok record as an extra output token**
- **Found during:** Finalizing Task 3's own re-verification of the apparent Criterion 2 FAIL that Task 0 had investigated and attributed to GPU nondeterminism
- **Issue:** `python/rsglang/parity/sweep.py`'s `join_sequential`/`join_concurrent` collected every backend-tap `detok` record for a uid unconditionally. The scheduler's pipelined/overlapped execution can emit one more `detok` record for a uid after it already sent `finished: true` (observed for the corpus's last request in each sequential session, raced against that session's own teardown SIGINT). This inflated `output_ids` by one token on the Python side only, producing a spurious `ids_match=False` for prompt `edge-08` in both models, even though the real HTTP response text both frontends sent was already byte-identical.
- **Fix:** Added `_bounded_detoks()`, which truncates each uid's seq-sorted detok records at the first `finished=True` record (inclusive), applied in both `join_sequential` and `join_concurrent`.
- **Files modified:** `python/rsglang/parity/sweep.py`, `python/tests/test_parity_check.py`
- **Verification:** Two new regression tests in `test_parity_check.py` pass; a full GPU re-run with the fix in place shows 128/128 for both models with zero divergence.
- **Committed in:** `ae8feec` (fix)

**2. [Rule 1 - Bug, documentation] The original 06-08 Task 0 conclusion and 06-07's reported Criterion 2 FAIL were both wrong**
- **Found during:** Task 3's finalization, after the harness-bug fix above was identified
- **Issue:** `06-08-PLAN.md`'s Task 0 conclusion (and the then-current `docs/benchmarks/parity-report.md`) attributed the apparent mismatch to "backend/GPU-session-to-session nondeterminism." That attribution was never correct — see item 1 above.
- **Fix:** Added dated correction notes to `06-08-PLAN.md`'s Task 0 HTML comment and to `06-07-SUMMARY.md`, explicitly explaining what was wrong and pointing at the real cause and fix commit, rather than silently editing the original text. Fully regenerated `docs/benchmarks/parity-report.md` against the final 128/128 JSON, with its own dated correction section replacing the "nondeterminism" narrative.
- **Files modified:** `.planning/phases/06-gpu-end-to-end-parity/06-07-SUMMARY.md`, `.planning/phases/06-gpu-end-to-end-parity/06-08-PLAN.md`, `docs/benchmarks/parity-report.md`
- **Verification:** `python/tests/test_parity_report.py` (11 tests, all passing) ties every heading and the D-09 section to the final JSON and to STATE.md.
- **Committed in:** `19f4ce4` (docs, parity-report.md), `25b084e` (docs, the two correction notes)

---

**Total deviations:** 2 auto-fixed (both Rule 1 bugs — one a real harness bug with production impact on measured results, one a documentation-accuracy correction following from the first).
**Impact on plan:** Both were necessary. The harness-bug fix changes the measured outcome of Criterion 2 from an apparent FAIL to a genuine PASS; the documentation corrections make that change traceable and honest rather than silently erasing the earlier (wrong) conclusion.

## Issues Encountered

- The plan's Task 3 `<verify>` requires a `<human-check>` reply ("approved" or named revisions) on the final report against ROADMAP Phase 6 criteria 1-4. This agent cannot supply that reply itself — flagged below under Next Phase Readiness, same pattern as 06-07-SUMMARY.md's own outstanding item (which this plan's work has now resolved the substance of, but the fresh human-check reply on the corrected report is still a new ask).
- Confirmed, by direct re-run, two pre-existing environmental flakes unrelated to this plan's scope: `cargo test -p rsg-tokenizer`'s `loader::tests::gated_access_unavailable_*` tests race under default parallel test threads (deterministic pass with `--test-threads=1`; already logged in `.planning/phases/04-tokenizer-detokenizer-parity/deferred-items.md` and `STATE.md`'s Blockers/Concerns), and two `hyperfine`-on-`PATH` test assumptions in Phase 2 scripts (`test_run_s3_hyperfine_missing_exits_2`, `test_hyperfine_ok`; already logged in `.planning/phases/06-gpu-end-to-end-parity/deferred-items.md` item 2a). Neither touches any file in this plan's scope. `cargo test --workspace -- --test-threads=1` and `.venv/bin/python scripts/check_upstream.py --offline` both pass cleanly.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- `.planning/STATE.md` records `abort-timing default for Phase 7 = deferred` with its full evidence trail, and the Phase 7 fairness concern (the frozen Python frontend has no abort-timing switch) — Phase 7's benchmark design must state explicitly how its cancellation-stress scenario treats this asymmetry.
- PAR-01 and PAR-02 are both genuinely proven: `docs/benchmarks/parity-report.{md,json}` show 128/128 for both models with zero divergence, and the concurrent-load rate is reported informationally. `.planning/REQUIREMENTS.md` already marks both `Complete`; no change needed there.
- **Outstanding before this plan can be considered fully closed per its own acceptance criteria:** a human needs to read the final, corrected `docs/benchmarks/parity-report.md` (including the `## Abort-timing decision (D-09)` section) against ROADMAP Phase 6 criteria 1-4 and reply "approved" or list sections to revise — the task's `<human-check>`. Everything automatable is confirmed green: `python/tests/test_parity_report.py` (11/11), `cargo test --workspace -- --test-threads=1`, `.venv/bin/python scripts/check_upstream.py --offline`, and the full `.venv/bin/python -m pytest python/tests -q` suite (only the two pre-existing, already-documented flakes noted above).
- No blockers for Phase 7 to start reading this report's Criterion 4 data and the D-09 decision.

---
*Phase: 06-gpu-end-to-end-parity*
*Completed: 2026-10-07*

## Self-Check: PASSED

- `docs/benchmarks/parity-report.md` found on disk; contains the exact lines `## Abort-timing decision (D-09)`, `## Divergence bisection (D-05)` and `## Criterion 4: Cancellation stress and the abort-during-prefill bug (D-08)`.
- `crates/rsg-server/tests/backend_finish_boundary.rs` found on disk.
- `python/rsglang/parity/sweep.py` contains `_bounded_detoks`.
- Commits `6558c46`, `224fcb6`, `de520b9`, `ae8feec`, `95f819f`, `19f4ce4`, `feef0ad`, `25b084e` all found in `git log --oneline`.
- `git diff --quiet HEAD -- docs/benchmarks/parity-report.json` exits 0 (the committed JSON is unedited).
- Re-ran `.venv/bin/python scripts/parity_check.py validate docs/benchmarks/parity-report.json --require-gpu` -> `valid`, exit 0.
- Re-ran `.venv/bin/python scripts/parity_check.py verdict docs/benchmarks/parity-report.json --criterion 1|2|3|4` -> all four print `PASS`.
- Re-ran `.venv/bin/python -m pytest python/tests/test_parity_report.py -q` -> 11 passed.
- Re-ran `cargo test --workspace -- --test-threads=1` -> all passed (0 failed).
- Re-ran `.venv/bin/python scripts/check_upstream.py --offline` -> `check_upstream: OK (offline, tree ... matches pristine 9a91cfa, 0 listed modifications)`.
- Outstanding: the task's `<human-check>` reply ("approved" or revisions) on the final, corrected report — see Next Phase Readiness above.
