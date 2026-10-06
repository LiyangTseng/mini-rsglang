---
phase: 02-python-frontend-baseline-profile
plan: 09
subsystem: profiling
tags: [bench-01, radix-01, baseline-report, gpu-measured, docs]
requires:
  - phase: 02-python-frontend-baseline-profile
    plan: 04
    provides: "rsglang.profiling.sidecar: validate_sidecar, SCENARIOS, require_gpu gate"
  - phase: 02-python-frontend-baseline-profile
    plan: 05
    provides: "rsglang.profiling.analysis: exact metric definitions (nearest-rank percentile, GC-overlap rule, GIL%/CPU% formulas) this report narrates"
  - phase: 02-python-frontend-baseline-profile
    plan: 07
    provides: "scripts/gpu_phase2_profile.sh: the GPU-box wrapper this plan's Task 1 ran"
  - phase: 02-python-frontend-baseline-profile
    plan: 08
    provides: "scripts/baseline_profile.py run/validate: the subcommands that produced and schema-checked the committed sidecar"
provides:
  - "docs/benchmarks/baseline-profile.md -- the hand-written narrative BENCH-01 report, every number traced to docs/benchmarks/baseline-profile.json"
  - "docs/benchmarks/baseline-profile.json -- the real GPU-measured sidecar (committed before this plan, re-verified here, unedited)"
  - "python/tests/test_baseline_profile_report.py -- mechanical report-to-JSON consistency proof (radix share and P99 TTFT formatting, required headings, require_gpu re-validation)"
  - "the RADIX-01 input: radix-cache share of scheduler time, measured under 2% in all three scenarios"
  - "the Phase 7 benchmark-design inputs: per-process GC/GIL/IPC metrics and frontend-vs-backend attribution"
affects: [07-frontend-benchmarks]
actuals:
  tokens: 6065
  tasks: 1
  commits: 1
  plan_head_before: b5634a51b9726bd677bc37c54d1b55856300de65
  plan_head_after: b42aa752a7590541b3ceee19d57260d0345b98b3
tech-stack:
  added: []
  patterns:
    - "Report numbers are computed once with a throwaway python3 -c script reading the committed JSON directly (f-string formatting mirroring the sidecar's own f\"{share*100:.2f}%\"/f\"{p99:.1f} ms\" contract), then transcribed into the markdown by hand -- never estimated, never hand-calculated from memory."
key-files:
  created:
    - docs/benchmarks/baseline-profile.md
    - python/tests/test_baseline_profile_report.py
  modified: []
key-decisions:
  - "Task 1 (the real GPU profiling run) was driven interactively by the orchestrator and the human before this worktree agent was dispatched, not via a dispatched executor checkpoint -- the orchestrator installed the 02-01-approved pins on the GPU box, hit and fixed a real bug (py-spy --nonblocking emitting invalid UTF-8 in an unresolvable native frame name, fixed in python/rsglang/profiling/analysis.py's load_speedscope with errors=\"replace\"), fixed a WSL PATH gap in scripts/gpu_phase2_profile.sh, then re-ran to ALL PASS and committed the resulting docs/benchmarks/baseline-profile.json in commit b5634a5 (this worktree's base) before this plan was ever dispatched."
  - "No privilege grant was needed on this specific WSL2 GPU box -- empirically confirmed (py-spy dump attached to a sibling process with exit code 0, no sudo/setcap, and getcap printed nothing both before and after the run), so Task 1's privilege-cleanup step was a no-op rather than an active removal."
  - "The radix-cache share recommendation in docs/benchmarks/baseline-profile.md is stated as a recommendation for the project author's decision (not an automatic verdict), because REQUIREMENTS.md's RADIX-01 does not numerically define 'meaningful share' -- the report states the measured percentages (0.76%-1.58% across all three scenarios) and the reasoning, and leaves the threshold call to the author."
  - "Scenario 3's per-request ipc_zmq/serde ms figures are reported with an explicit caveat that they divide by requests_completed=1 and are not comparable to Scenario 1/2's multi-request figures, rather than omitting them (the plan's action requires reporting them for api_server and the tokenizer in every scenario) or silently presenting them as directly comparable."
requirements-completed: [BENCH-01]
coverage:
  - id: D1
    description: "Task 1: real GPU profiling run via gpu_phase2_profile.sh on a Linux GPU machine with one NVIDIA GeForce RTX 3050, ALL PASS across preflight/discover/run/validate --require-gpu/check_upstream.py, no privilege grant needed"
    verification: []
    human_judgment: true
    rationale: "Already resolved via orchestrator+human interactive session before this worktree agent was dispatched; documented above as normal flow, not a deviation. The resulting docs/benchmarks/baseline-profile.json was this worktree's base commit (b5634a5) and was re-verified (not re-run) by this plan's Task 2."
  - id: D2
    description: "Task 2: docs/benchmarks/baseline-profile.md hand-written from the committed JSON with all 10 required headings, every radix share and P99 TTFT traced to the JSON in the sidecar's own format, and python/tests/test_baseline_profile_report.py proving that mechanically plus re-validating the sidecar with require_gpu=True"
    requirement: "BENCH-01"
    verification:
      - {kind: integration, ref: "python/tests/test_baseline_profile_report.py::test_sidecar_is_a_valid_gpu_run", status: pass}
      - {kind: integration, ref: "python/tests/test_baseline_profile_report.py::test_all_required_headings_present", status: pass}
      - {kind: integration, ref: "python/tests/test_baseline_profile_report.py::test_every_radix_share_appears_formatted", status: pass}
      - {kind: integration, ref: "python/tests/test_baseline_profile_report.py::test_every_p99_ttft_appears_formatted", status: pass}
    human_judgment: false
  - id: D3
    description: "ROADMAP Phase 2 success criteria 1-4 sign-off on the report's content (GC/memory quantification, honest GIL framing with per-request IPC/serde cost, radix share plus agreement with the RADIX-01 recommendation, concrete Phase 7 benchmark-design inputs)"
    verification: []
    human_judgment: true
    rationale: "Deferred to end-of-phase UAT per workflow.human_verify_mode=end-of-phase (default, unconfigured in this project) -- the phase verifier harvests this <human-check> item into the UAT flow the human reviews after all of Phase 2's plans are done."
duration: 30min
completed: 2026-10-06
status: complete
---

# Phase 2 Plan 9: GPU Baseline Profile Report Summary

**The real GPU-measured baseline for BENCH-01 is now a committed, narrated report: radix-cache time is under 2% of scheduler sampled time in every one of the three benchmark scenarios (the RADIX-01 input), the GIL framing is honest about the 3-process topology with no invented cross-process contention number, and Phase 7's benchmark design gets concrete per-process metrics plus a frontend-vs-backend attribution split, all traced mechanically back to `docs/benchmarks/baseline-profile.json` by a dedicated consistency test.**

## Performance
- **Duration:** ~30min (Task 2 only -- Task 1's GPU run was already complete at dispatch) | **Started:** 2026-10-06 | **Completed:** 2026-10-06T03:36:52Z | **Tasks:** 1/1 (Task 2; Task 1 pre-completed by the orchestrator) | **Files modified:** 2 (both newly created)

## Accomplishments
- Read every scenario block of the 805 KB `docs/benchmarks/baseline-profile.json` sidecar (via targeted `python3 -c` extraction rather than a full file read, since it exceeds the read-tool size cap) and `python/rsglang/profiling/analysis.py`'s exact metric definitions (nearest-rank percentile, the GC-TTFT overlap rule, the GIL%/CPU% formulas) before writing a single line of the report.
- Wrote `docs/benchmarks/baseline-profile.md` by hand with all 10 required headings in order: Run, Topology and GIL framing, the three scenarios (requests/GC/GC-TTFT-overlap/memory/CPU/IPC-serde per role, plus hyperfine cold-start stats for Scenario 3), Radix cache share (input to RADIX-01) with a recommendation left to the author's decision, Inputs to Phase 7 benchmark design (per-process metrics tied to specific measured findings, plus frontend-vs-backend attribution), Known blind spots (including the project-specific missing-allocation-snapshot warnings from the JSON's own `warnings` field), and Reproduce.
- Wrote `python/tests/test_baseline_profile_report.py`: re-validates the sidecar with `require_gpu=True`, asserts every required heading is present as an exact line, and asserts every scenario's radix share and P99 TTFT appear in the markdown formatted exactly as `f"{share*100:.2f}%"` and `f"{p99:.1f} ms"` -- the mechanical proof that the narrative didn't drift from the JSON.
- All verification commands pass: `scripts/baseline_profile.py validate --require-gpu` exits 0, `pytest python/tests/test_baseline_profile_report.py -q` is 4/4, and the full suite `pytest python/tests -q -m "not slow"` shows 110 passed, 36 skipped, no regression. The committed JSON is confirmed byte-unedited (`git diff --quiet HEAD -- docs/benchmarks/baseline-profile.json` exits 0).

## Task Commits
1. **Task 2: Write the narrative report from the JSON and the test that ties them together** - `b42aa75` docs(02-09): write BENCH-01 narrative baseline report from the GPU sidecar

**Plan metadata:** commit pending (this SUMMARY, committed next by the orchestrator per the parallel_execution briefing -- STATE.md/ROADMAP.md are not touched by this worktree agent)

## Files Created/Modified
- `docs/benchmarks/baseline-profile.md` - the hand-written narrative report; every number traced to `docs/benchmarks/baseline-profile.json`
- `python/tests/test_baseline_profile_report.py` - report-to-JSON consistency test (require_gpu re-validation, heading presence, radix-share/P99-TTFT format checks)

## Decisions Made
- **Task 1's GPU run was already complete when this worktree agent was dispatched.** The orchestrator drove it interactively with the human: installed the 02-01-approved pins on the GPU box, hit a real bug (py-spy `--nonblocking` emitting invalid UTF-8 inside an unresolvable native frame name for api_server's asyncio/uvloop event loop), fixed it in `python/rsglang/profiling/analysis.py`'s `load_speedscope` (decode with `errors="replace"` before `json.loads`, with its own regression test), fixed a WSL PATH gap in `scripts/gpu_phase2_profile.sh`, then re-ran to ALL PASS and committed `docs/benchmarks/baseline-profile.json` in commit `b5634a5` -- this worktree's base commit. This plan's Task 2 treated that JSON as the real, final artifact: read it, never regenerated or hand-edited it.
- **No privilege grant was needed on this WSL2 GPU box.** Confirmed empirically rather than assumed: `py-spy dump` attached to a sibling process with exit code 0 and no `sudo`/`setcap`, and `getcap` on the py-spy binary printed nothing both before and after the run -- this specific box does not enforce `ptrace_scope` restrictions.
- **The radix-cache share recommendation is stated as a recommendation, not a verdict.** `REQUIREMENTS.md`'s RADIX-01 does not numerically define "meaningful share"; the report states the measured percentages (0.76%-1.58% across all three scenarios, roughly 50-100x less than the scheduler's non-radix time) and the reasoning behind reading that as not clearing a "meaningful" bar, then explicitly leaves the threshold call to the project author.
- **Scenario 3's per-request IPC/serde figures are reported with an explicit caveat rather than omitted.** They divide by `requests_completed=1` (only one request is sent in the cold-start scenario), so they are not comparable to Scenario 1/2's multi-request figures -- the plan's action requires reporting them for every scenario, so they are included with the caveat stated directly next to them instead of silently presented as comparable or quietly dropped.

## Deviations from Plan

None - plan executed exactly as written for Task 2. Task 1 was already resolved before dispatch (documented above as normal flow, per the dispatch briefing, not a deviation of this plan's own execution).

## Issues Encountered
- `docs/benchmarks/baseline-profile.json` is 805 KB, which exceeds the Read tool's 256 KB cap. Worked around this by extracting every needed field with targeted `python3 -c` scripts (`json.load` plus dict/list indexing) instead of reading the raw file, which also had the side benefit of computing every formatted number (`f"{share*100:.2f}%"`, `f"{p99:.1f} ms"`, MB conversions) in the same interpreter that would later be asserted against by the test, eliminating transcription risk.
- `.venv` did not exist in this fresh worktree (expected, gitignored). Ran `bash scripts/bootstrap_mac_env.sh`, which completed cleanly and installed `rsglang`/`minisgl` in editable mode with all previously-approved pins.

## User Setup Required
None. The real GPU run (Task 1) is already complete and committed; no further external service configuration is needed from this plan.

## Next Phase Readiness
- Phase 2 is now content-complete: all 9 plans (02-01 through 02-09) have a SUMMARY.md. BENCH-01 is satisfied by a real, measured GPU run plus the narrative report derived from it.
- Phase 7 (Frontend Benchmarks) can now read `docs/benchmarks/baseline-profile.md`'s "Inputs to Phase 7 benchmark design" section directly for which per-process metrics to capture and how to attribute frontend vs. backend effects.
- The v2 RADIX-01 decision (not in scope for v1) has its input: radix-cache share measured under 2% of scheduler time in all three scenarios, with the report's reasoning and an explicit recommendation left to the author.
- No blockers identified. The end-of-phase human check on ROADMAP Phase 2 criteria 1-4 (this plan's Task 2 `<human-check>` item) remains open per `workflow.human_verify_mode=end-of-phase` and will be harvested into the phase's UAT flow by the orchestrator/phase verifier, not by this worktree agent.

---
*Phase: 02-python-frontend-baseline-profile*
*Completed: 2026-10-06*

## Self-Check: PASSED
- FOUND: docs/benchmarks/baseline-profile.md
- FOUND: python/tests/test_baseline_profile_report.py
- FOUND commit: b42aa75 (git log --oneline -3)
