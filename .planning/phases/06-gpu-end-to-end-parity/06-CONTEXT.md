# Phase 6: GPU End-to-End Parity - Context

**Gathered:** 2026-10-06
**Status:** Ready for planning

<domain>
## Phase Boundary

On the GPU machine, `--frontend rust` serves a real model through the real backend, and its output is compared against the Python frontend on the same backend. The hard gate is greedy-decoding (temperature 0), one-request-at-a-time output identity on at least 100 prompts for Qwen3-0.6B. The same comparison runs and is reported (not gated) for one Llama-3.x model. Under concurrent load, the match rate is measured and reported as informational only, because GPU batch composition affects results. The 128-request cancellation stress test (built in Phase 5 against the mock) is re-run against the real backend, and this run is where the project finds out whether the suspected abort-during-prefill double-free actually reproduces — settling the abort-timing default used for the Phase 7 benchmarks.

Requirements: PAR-01, PAR-02.

Not in this phase:
- The request-lifecycle FSM, HTTP API, and lifecycle code itself (LIFE-*, API-*, Phase 5) — Phase 6 *consumes* Phase 5's frontend unmodified, swapping only the backend it talks to.
- Tokenizer/detokenizer implementation (TOK-*, Phase 4) — Phase 6 exercises it against a real model, it does not rebuild it.
- The instrumented benchmark harness proper (BENCH-02..08, Phase 7) — Phase 6's parity report and abort-timing decision are inputs Phase 7 consumes, not Phase 7's own measurement tooling.
- Any attempt to fix a deep/structural backend bug if one is found (see D-09) — that is explicitly out of scope by design, not an oversight.

**Dependency note (as of this discussion):** Phase 6 depends on Phase 5, which itself is only at the context-gathered stage (unplanned, no code) as of 2026-10-06. This discussion captures decisions ahead of that code landing — the same pattern Phase 5's own discussion used against then-unplanned Phase 3/4 work. The planner/executor should treat 05-CONTEXT.md as the authoritative interface contract (the `--abort-timing` flag, the per-uid channel API, the stress-test tool) until Phase 5's code actually exists.

</domain>

<decisions>
## Implementation Decisions

### Parity corpus & models
- **D-01:** The ~100 PAR-01 prompts are a **curated, category-covering corpus** (short/long, multi-turn chat, code, CJK/emoji edge cases), reusing Phase 4's tokenizer test corpus where it overlaps — not a random sample from a public chat dataset, not a generic fixed Q&A set. Chosen because this project already cares about tokenizer/detokenizer edge cases (TOK-03/04), and a curated set proves exactly those cases survive a real model.
- **D-02:** The Llama-3.x comparison target is **Llama-3.2-1B-Instruct** — smallest gated checkpoint, not 3B or 8B — chosen to minimize GPU iteration cost during parity debugging, not for production representativeness.
- **D-03:** The **same curated prompt corpus (D-01) is reused for both Qwen3-0.6B and Llama-3.2-1B-Instruct**, rather than maintaining two separate sets, so category coverage is directly comparable across models.

### Diff / mismatch protocol (PAR-01 hard gate)
- **D-04:** **Zero tolerance** — any single-token mismatch on any of the 100 prompts fails the gate. Rationale: greedy decoding + temperature 0 + one request at a time removes GPU batching, so the run should be fully deterministic end to end; any divergence is treated as a real bug, not acceptable noise.
- **D-05:** On a failure, the immediate next step is to **bisect to the first diverging token** for that prompt and trace whether the root cause sits in tokenization/chat-template (Phase 4), FSM/API (Phase 5), or the backend itself — before deciding whether it's fixable in-phase or a hard blocker.
- **D-06:** The diff compares **token-id sequences first** (isolates whether the backend itself is nondeterministic, since ids are the shared ground truth for both frontends), with **detokenized text as a secondary check** specifically for Rust-vs-Python detokenization discrepancies against a real model — TOK-03 already covers detokenization parity on the Mac against the mock; this is the real-model sanity recheck.
- **D-07:** Results are recorded as **`docs/benchmarks/parity-report.{md,json}`**, following Phase 2's `baseline-profile.{md,json}` precedent — narrative findings and any bisection results in `.md`, per-prompt token-id/text diff data in `.json` for reproducibility.

### Abort-during-prefill bug response (criterion 4)
- **D-08:** The response to the suspected abort-during-prefill double-free is **not predetermined before the stress test runs**. First, empirically establish (a) whether it reproduces and under what trigger conditions (timing window of abort landing during prefill), and (b) its failure mode — full scheduler crash/restart vs. an isolated corrupted request.
- **D-09:** Only once that reproduction data exists, triage the response by the **scope of the fix**, not by severity alone: a small, localized fix (e.g. an ordering/refcount bug in the abort-handling path) is attempted as a **shared backend fix** per `PROJECT.md`'s allowed-shared-fixes rule, recorded in `UPSTREAM.md`. A deep/structural CUDA-memory issue is **documented and routed around** by locking the project-wide `--abort-timing` default (Phase 5 D-01) to `deferred`, recorded in `STATE.md`/`UPSTREAM.md` — no attempt to debug deep into CUDA memory management, since that is disproportionate to this project's scope as a frontend-migration learning project. — **Reversibility:** costly — this sets the project-wide `--abort-timing` default that Phase 5 D-01 built specifically for this fairness question; Phase 7's A/B benchmark comparisons depend on the same setting applying equally to both frontends, so changing it after Phase 7 begins means rerunning those comparisons.

### Concurrent-load & stress scope
- **D-10:** Criterion 3's concurrent-load match-rate measurement (PAR-02, informational only) uses **one fixed concurrency level** (matching the 128-agent scenario) and the same curated prompt set (D-01), reported once — not a multi-level concurrency curve. Minimal GPU time spent on a metric that is explicitly not a gate.
- **D-11:** Criterion 4's 128-request cancellation stress test against the real backend **reuses Phase 5's throwaway stress-test tool as-is** (05-CONTEXT.md D-04), pointed at the real backend instead of `mock-scheduler` through the same `Transport` abstraction — no changes to the tool itself.
- **D-12:** Detection of whether the abort-during-prefill bug reproduced during that stress run is **not built into the stress tool**. Instead, a separate, thin process-health watcher follows the existing `scripts/gpu_phase1_check.sh` convention (`ps`/`nvidia-smi` checks around the run) to catch scheduler crash/restart/zombie states — keeping Phase 5's tool untouched and putting detection logic where the GPU-verification-script layer already lives.

### Claude's Discretion
- Exact composition of the curated 100-prompt corpus (how many prompts per category: short/long/multi-turn/code/CJK/emoji) — D-01 fixes the sourcing method, not the exact per-category breakdown.
- The exact fixed concurrency level and sample size for D-10's single-point concurrent-load measurement.
- The precise fix-scope threshold for D-09's "small/localized vs. deep/structural" triage — decided case-by-case once the actual bug (if it reproduces) is read, not fixed in advance.
- Internal module/script layout for `docs/benchmarks/parity-report` generation and D-12's process-health watcher script (new script vs. extending `scripts/gpu_phase1_check.sh`'s pattern).

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Project scope and requirements
- `.planning/ROADMAP.md` §Phase 6 — goal, dependency (Phase 5), 4 success criteria
- `.planning/REQUIREMENTS.md` — PAR-01, PAR-02 full text; traceability row confirming Phase 6 ownership
- `.planning/PROJECT.md` — Constraints (fair comparison; backend changes must apply to both modes; verification claims measured on Linux, projections stay projections until measured; MIT license/attribution)
- `.planning/STATE.md` — Phase 6 blocker note ("the upstream abort-during-prefill double free comes from code reading only" — this phase is where it gets tested empirically for the first time)

### Stack and pattern guidance
- `.claude/CLAUDE.md` §Benchmark Harness (stack for the three RFC scenarios) — "Parity (standard inference)" row: upstream's own `benchmark/online/bench_simple.py` plus a greedy-decoding output-diff script; ±2% throughput reference and byte-identical greedy output
- `.claude/CLAUDE.md` §Constraints — frozen-frontend and shared-backend-fix rules (already summarized in `PROJECT.md` Constraints)

### Prior-phase context (carried forward — contract to build against)
- `.planning/phases/05-request-lifecycle-http-api/05-CONTEXT.md` — D-01 (the `--abort-timing immediate|deferred` server-wide CLI flag, built specifically for this phase's fairness question), D-04 (the minimal Phase-5-only stress test this phase reuses as-is per D-11), D-02 (the golden-fixture API-parity pattern this phase's diff methodology mirrors against the real backend instead of the mock)
- `.planning/phases/03-zmq-transport-mock-scheduler/03-CONTEXT.md` — D-08 (`mock-scheduler` as a standalone subprocess behind the `Transport` abstraction) — the layer this phase swaps for the real backend without touching the FSM/HTTP code built on top of it
- `.planning/phases/02-python-frontend-baseline-profile/02-CONTEXT.md` — D-12/D-13 (`docs/benchmarks/{report}.{md,json}` narrative+sidecar convention), reused by D-07 for the parity report

### Upstream code this phase mirrors / runs unchanged (read, modify only if D-09's shared-fix branch is taken)
- `vendor/mini-sglang/benchmark/online/bench_simple.py` — the `AsyncOpenAI`-based client this phase's parity-diff driver is built on, per `.claude/CLAUDE.md`'s Benchmark Harness table
- `vendor/mini-sglang/python/minisgl/scheduler/{scheduler,cache}.py` — suspected location of the abort-during-prefill double-free (D-08/D-09); read first during bisection (D-05); modify only if D-09's "small, localized fix" branch is taken, recorded in `UPSTREAM.md`

### Existing code/scripts this phase builds on
- `scripts/gpu_phase1_check.sh`, `scripts/gpu_phase2_profile.sh` — the human-run, PASS/FAIL-per-criterion GPU verification script convention this phase's own GPU script (and D-12's process-health watcher) should follow
- `docs/benchmarks/baseline-profile.{md,json}` — the direct structural precedent for D-07's `parity-report.{md,json}`
- `crates/rsg-server/src/transport.rs` — the `Transport` trait/abstraction that lets D-11 point Phase 5's stress tool at the real backend without modifying the tool itself

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `scripts/gpu_phase1_check.sh` / `scripts/gpu_phase2_profile.sh` — `mktemp`-dir logging, `--model`/`--timeout` CLI flags, PASS/FAIL-per-step output; the direct template for Phase 6's own GPU script and D-12's process-health watcher
- `docs/benchmarks/` — existing durable-report location (Phase 2 precedent), extended by D-07's `parity-report.{md,json}`
- `scripts/gen_wire_fixtures.py` (Phase 1) — the byte-diff/golden-fixture methodology this phase's token-id diff (D-06) reuses conceptually, against the real backend instead of a Python encoder

### Established Patterns
- Human-run, PASS/FAIL-per-criterion GPU scripts are the established verification convention for every GPU-only phase so far (1, 2) — Phase 6 should follow the same shape, not invent a new one
- No HTTP/FSM/API code exists yet anywhere in the workspace as of this discussion (Phase 5 is still at context-gathered, unplanned) — Phase 6 cannot be executed until Phase 5's code lands; this discussion captures decisions ahead of that, mirroring how Phase 5's own discussion proceeded against then-unplanned Phase 3/4 work

### Integration Points
- Phase 6 is purely a "swap the backend" operation at the `Transport` layer (`crates/rsg-server/src/transport.rs`) — `mock-scheduler` (Phase 3) is replaced by the real GPU backend behind the same trait, so Phase 5's FSM/HTTP/stress-test code needs no changes, only new CLI wiring to point at the real `ipc://` addresses
- Phase 7's benchmark harness and the v2 radix-cache decision both depend on this phase's abort-timing lock-in (D-09) and parity findings (D-04..D-07)

</code_context>

<specifics>
## Specific Ideas

- The Llama checkpoint pick (D-02: 1B-Instruct) is specifically about minimizing GPU iteration cost during parity debugging, not about production representativeness — consistent with this project's framing as a learning-and-proof project, not a production-sizing exercise.
- D-08/D-09's two-step evaluation (reproduce first, triage by fix-scope second) was chosen deliberately over pre-committing to either "always attempt a fix" or "never touch it" — it came directly out of the user asking "你建議怎麼評估這個問題?" (how do you suggest evaluating this) mid-discussion and agreeing with the reproduce-then-triage-by-scope approach.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

### Reviewed Todos (not folded)
None — no pending todos matched this phase.

</deferred>

---

*Phase: 06-gpu-end-to-end-parity*
*Context gathered: 2026-10-06*
