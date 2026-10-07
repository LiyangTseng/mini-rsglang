# Phase 7: Frontend Benchmarks - Context

**Gathered:** 2026-10-06
**Status:** Ready for planning

<domain>
## Phase Boundary

A reproducible harness that drives both frontends (Python and Rust) against the same backend and quantifies how much the Rust frontend improves each of the three host-overhead-bound scenarios, plus a standard-inference throughput check that shows no regression.

Requirements: BENCH-02, BENCH-03, BENCH-04, BENCH-05, BENCH-06, BENCH-07, BENCH-08.

Not in this phase:
- The scenario 1 cancellation *correctness* test (LIFE-03) — Phase 5 already built a throwaway, Phase-5-only stress test for that; Phase 7 builds its own instrumented load generator, unconstrained by that throwaway tool (per 05-CONTEXT.md D-04).
- Profiling methodology itself (py-spy, tracemalloc, gc.callbacks instrumentation design) — Phase 2 (BENCH-01) already built and ran that; Phase 7 reuses its gc.callbacks hook for lightweight GC counting but does not redesign the profiling approach.
- Real GPU backend correctness/parity (PAR-01/02) — that's Phase 6, a hard dependency. Phase 7's harness is developed and verified against `mock-scheduler` on the Mac first; only the final reported numbers require the GPU machine and Phase 6 having landed.
- Rust radix cache (deferred to v2, no phase).

**Dependency note (as of this discussion):** Phase 7 depends on Phase 2 (done — `docs/benchmarks/baseline-profile.{md,json}` already exist) and Phase 6 (not started; other sessions are working Phases 3-6 in parallel worktrees as of this discussion). This CONTEXT.md can be written now because the harness's *design* doesn't require Phase 6's code — only running it for real against the GPU backend does.

</domain>

<decisions>
## Implementation Decisions

### Load generator & cancellation model (BENCH-02, BENCH-03)
- **D-01:** The harness is **one configurable Rust driver** with a `--mode closed|open` flag, not separate per-scenario binaries. Scenario 1 (128 agents, cancellations) always runs closed-loop; Scenario 2 (saturation) can run either closed (at concurrency C) or open-loop (Poisson), per CLAUDE.md's benchmark stack table. Shared TTFT/ITL/E2E recording via hdrhistogram across both modes.
- **D-02:** Cancellation-timing logic (seeded RNG, think-time, abort-after-N-tokens per agent) lives **in the harness driver itself**, driving real client-side disconnects — same placement Phase 5 chose for its throwaway stress test (05-CONTEXT.md D-04). `mock-scheduler` stays a dumb, deterministic token source; its CLI-configured misbehaviors (Phase 3 D-09) are not extended with randomized decode timing for this phase. — **Reversibility:** reversible — client-side cancellation logic can be refactored without touching `mock-scheduler`'s contract, which Phase 5/6 also depend on (03-CONTEXT.md D-08).
- **D-03:** Phase 7 includes a **Mac-side development/verification pass against `mock-scheduler`** before any GPU run — proving loop modes, cancellation injection, hdrhistogram recording, and manifest writing work correctly on hardware without CUDA, matching the pattern Phases 3 and 5 already established and PROJECT.md's GPU-free dev constraint. Only the actual benchmark *numbers* (the headline comparison) require the GPU machine.
- **D-04:** Phase 7 **runs `vllm bench serve --backend openai-chat` and `python -m sglang.benchmark.serving --backend sglang-oai-chat`** as third-party cross-checks for Scenario 2, reporting their TTFT/ITL/E2E percentiles alongside the custom harness's own numbers — per CLAUDE.md's stated rationale ("make results credible to outsiders") and the roadmap's BENCH-04 intent. Neither tool can inject mid-stream cancellations, so they are Scenario-2-only; Scenario 1 and 3 remain custom-harness-only.

### A/B runs, manifest & confidence intervals (BENCH-07)
- **D-05:** Confidence intervals come from **repeating N full runs per frontend, alternating strictly** (P, R, P, R, ...), not from bootstrap-resampling a single long run's hdrhistogram. This captures real run-to-run variance (thermal throttling, GC timing luck, scheduler noise) rather than just within-run sampling noise. — **Reversibility:** costly — changing this later means re-running every already-reported scenario to get comparable CIs; the run manifest schema (D-07) is built around "N trials per side," so switching methods changes what the manifest records.
- **D-06:** Default run count is **5 runs per frontend per scenario** (10 total trials, strictly alternating), a reasonable default for a learning/proof project — not a paper submission. Can be overridden via CLI for scenarios where GPU time is tight.
- **D-07:** The run manifest is a **full environment snapshot**: git commit (Rust frontend + vendored upstream SHA), model name, GPU, CUDA/driver version, OS, rustc/python versions, every CLI flag passed to both the frontend and the harness, RNG seed(s), run count N, timestamp, and the raw per-run hdrhistogram files — same depth as Phase 2's `baseline-profile.json` `meta` block. Stored as JSON alongside the human-readable report, one manifest per scenario per A/B session.
- **D-08:** The A/B alternation (P,R,P,R,...) and manifest writing are **one shared orchestrator** wrapping all 3 scenarios, not reimplemented per scenario. Each scenario plugs in its own single-trial runner (the closed/open-loop driver for scenarios 1/2, a `hyperfine`-based runner for scenario 3) into the shared "run N alternating trials, collect results, write manifest" logic. — **Reversibility:** reversible — the orchestrator/runner split can be refactored internally without changing the manifest schema or reported numbers.

### Best --num-tokenizer & regression gate (BENCH-06, BENCH-07)
- **D-09:** Python's "best" `--num-tokenizer` is determined by a **small fixed-candidate sweep (e.g. 0, 1, 2, 4) on Scenario 2**, run once as a cheap pre-pass; whichever candidate gives the highest RPS becomes "best" and is used for that scenario's full 5-run A/B comparison (and, unless later measurement says otherwise, for scenarios 1/3 and BENCH-06 too). Grounded in an actual measurement, bounded in GPU time since the candidate list is small and fixed.
- **D-10:** The **Rust frontend is reported at one fixed configuration only** — it has no multi-process tokenizer-worker split to tune (CLAUDE.md flags any internal tokenizer-pool sizing as "decide by criterion measurement," an implementation detail, not a user-facing knob). No sweep is run or reported for Rust; this asymmetry itself is part of the value proposition (Rust needs no tuning knob to hit its number).
- **D-11:** BENCH-06's throughput regression is **always reported with its delta and CI**; a negative delta (Rust slower than Python) is explicitly called out in the report's summary text. **No automated pass/fail gate** is wired into the harness's exit code or CI — per PROJECT.md's explicit "±2% is a reference target, not a hard gate" framing. — **Reversibility:** reversible — a hard gate could be added later as a separate CI check without touching how the harness itself measures and reports.
- **D-12:** BENCH-06's "standard inference" workload **reuses the existing `bench_simple.py` client-helper shape** (the same one Phase 2 adapted for its own Scenario 2 driver, per 02-CONTEXT.md D-05/D-07) rather than defining a new workload profile — keeps "standard inference" consistently defined project-wide instead of introducing a second workload shape for one gate.

### Report format & GC/memory correlation (BENCH-05, BENCH-08)
- **D-13:** Phase 7's reports follow **Phase 2's exact `docs/benchmarks/` md+json convention** — narrative markdown (human-readable findings) paired with a JSON sidecar the harness/orchestrator writes directly (for re-run reproducibility), in the same directory as `baseline-profile.{md,json}`. Not a `.planning/` process artifact, since this is durable reference material.
- **D-14:** The timed A/B runs (BENCH-02..07) stay **fully uninstrumented** for honest TTFT/RPS numbers. BENCH-08's memory/GC columns come from a **separate, parallel lightweight sampling pass** running alongside without perturbing timing: periodic RSS/PSS polling (`sysinfo`, per CLAUDE.md's harness stack — Rust side gets equivalent RSS/PSS sampling for symmetry) plus the `gc.callbacks` hook for Python GC pause counts. — **Reversibility:** reversible — sampling cadence/method can change without affecting the timed-run numbers it sits alongside.
- **D-15:** Within that lightweight pass, **`gc.callbacks` stays active during timed runs** (it only timestamps GC events Python already runs — near-zero added overhead), but **`tracemalloc` and `py-spy` are dropped** from timed runs — those are the two instruments Phase 2 itself identified as causing its measured ~3.5x overhead (baseline-profile.md's "instrumented ready time is roughly 3.5x the uninstrumented hyperfine mean"). This is a direct consequence of Phase 2's own finding, not a fresh assumption.
- **D-16:** BENCH-08's P99-vs-GC-pause comparison is presented as a **GC-pause table (count, total/P99 pause duration — same shape Phase 2 already reports) alongside the latency percentiles, plus one derived co-occurrence stat**: the fraction of P99-or-worse requests that had a GC pause on that process within their request window. No full timestamp-correlated visualization/chart is built — the roadmap's success criterion only requires GC pauses shown "alongside" TTFT/P99/RPS, not a joint plot.

### Claude's Discretion
- Exact crate/module layout for the harness orchestrator and per-scenario runners (e.g. a new `crates/rsg-bench` binary crate vs. something else) — D-01/D-08 fix the architecture shape, not file layout.
- Exact candidate values and sweep methodology details for D-09's `--num-tokenizer` pre-pass beyond "small fixed set, pick max RPS."
- Exact RSS/PSS sampling interval and `sysinfo` invocation details for D-14's lightweight pass.
- Whether one combined `docs/benchmarks/phase7-benchmarks.{md,json}` or per-scenario files (`scenario1-cancel.{md,json}`, etc.) best fits D-13 — the convention is locked, the exact file split is not.
- CI/CLI override surface for D-06's default run count of 5.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Project scope and requirements
- `.planning/ROADMAP.md` §Phase 7 — goal, dependencies (Phase 2 + Phase 6), 5 success criteria
- `.planning/REQUIREMENTS.md` — BENCH-02 through BENCH-08 full text; traceability table
- `.planning/PROJECT.md` — Constraints (fair comparison, GPU-free dev environment, "projections stay projections until measured"); Key Decisions table ("±2% figure ... is an estimate and a reference target, not a hard gate")
- `.planning/STATE.md` — current progress and cross-phase blockers (notably Phase 6's abort-during-prefill bug, which settles the abort-timing setting both frontends use symmetrically in these benchmarks)

### Stack and pattern guidance
- `.claude/CLAUDE.md` §Benchmark Harness (stack for the three RFC scenarios) — the scenario-to-tool table this phase implements directly (custom Rust harness, third-party cross-checks for scenario 2, hyperfine for scenario 3)
- `.claude/CLAUDE.md` §Recommended Stack — `reqwest` (streaming load generator), `hdrhistogram` (TTFT/ITL/E2E percentiles), `sysinfo` (RSS/PSS sampling, `/proc/<pid>/smaps_rollup` on Linux), `nix` (process-group kill for the GPU-process-leak-free benchmark teardown)
- `.claude/CLAUDE.md` §Alternatives Considered — "Custom Rust load-generator crate" row (why vllm/sglang bench tools are cross-checks, not the benchmark of record, because they can't inject mid-stream cancellation or measure cold start/RSS)
- `.claude/CLAUDE.md` §What NOT to Use — `oha`/`wrk`/`vegeta` as benchmark of record (can't parse SSE token timing); `tokio-console`/OTel exporters in benchmark builds (measurable overhead skews P99)

### Prior-phase context (carried forward — contract to build against)
- `.planning/phases/02-python-frontend-baseline-profile/02-CONTEXT.md` — D-05/D-07 (the `bench_simple.py`-derived client-helper shape this phase's BENCH-06 workload reuses per D-12); D-12/D-13/D-14 (the `docs/benchmarks/` md+json report convention this phase follows per D-13); the measured ~3.5x instrumentation overhead finding that directly motivates D-14/D-15's "drop tracemalloc+py-spy from timed runs" decision
- `.planning/phases/03-zmq-transport-mock-scheduler/03-CONTEXT.md` — D-08/D-09 (`mock-scheduler`'s CLI-configured-misbehavior contract, which this phase's harness spawns in place of the real backend for its Mac dev pass per D-03, without extending per D-02); D-11 (explicitly named this phase as one of the two consumers whose "exact needs aren't locked yet" — this discussion is what locks them: Phase 7 does NOT need `mock-scheduler` timing changes, per D-02)
- `.planning/phases/05-request-lifecycle-http-api/05-CONTEXT.md` — D-04 (explicitly scoped Phase 5's stress test as "not a shared foundation for Phase 7's benchmark harness" — this phase's harness is built independently, confirmed by D-02); D-01 (the `--abort-timing immediate|deferred` server-wide flag this phase's A/B runs must hold constant/matching between frontends for a fair comparison)
- `docs/benchmarks/baseline-profile.md` and `.json` (Phase 2's output) — the measured baseline this phase's Rust-vs-Python comparison is measured against; also the source of the "~3.5x instrumentation overhead" finding behind D-14/D-15

### Upstream code and tools this phase uses (read, do not modify)
- `vendor/mini-sglang/benchmark/online/bench_simple.py` — the `AsyncOpenAI`-based client-helper workload reused for BENCH-06 (D-12) and already adapted once by Phase 2
- `vendor/mini-sglang/python/minisgl/server/args.py` — `--num-tokenizer` flag definition, the knob D-09's sweep varies

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `docs/benchmarks/baseline-profile.json` / `.md` — Phase 2's `meta` block structure is the direct template for this phase's run-manifest schema (D-07) and report format (D-13)
- `scripts/baseline_profile.py` (Phase 2) — existing convention for a profiling/benchmark driver script that writes its own JSON sidecar directly; this phase's orchestrator follows the same "script writes the machine-readable artifact itself" pattern (02-CONTEXT.md D-14)
- `crates/rsg-server/src/main.rs` — CLI parsing (`clap`) and exit-code convention; the harness binary/orchestrator follows the same CLI shape for consistency

### Established Patterns
- No benchmark/harness crate exists yet anywhere in the workspace (`find` for `*bench*` only turns up `docs/benchmarks/`) — this phase introduces the harness from nothing, likely as a new crate (exact layout is Claude's discretion)
- `mock-scheduler` (Phase 3) is a standalone subprocess binary, spawned the same way by every phase that needs a stand-in backend (Phase 5's stress test, this phase's Mac dev pass) — this phase does not rebuild that spawning convention, it reuses it
- Phase 2 already proved that heavy profiling instrumentation (py-spy + tracemalloc + gc.callbacks together) measurably distorts latency (~3.5x) — this is empirical, not assumed, and is why D-14/D-15 split "timed run" from "memory/GC sampling pass"

### Integration Points
- `mock-scheduler`'s existing CLI flags (Phase 3 D-09/D-10: `--misbehave-uids`, `--behavior`, `--batch-size`, `--prefill-delay-ms`, `--decode-delay-ms`) are what this phase's Mac dev pass (D-03) spawns against — no new flags needed per D-02
- The real backend's `--abort-timing` setting (Phase 5 D-01) and Phase 6's abort-during-prefill bug finding must be held fixed/symmetric across the Python-vs-Rust A/B comparison for Scenario 1 to be a fair comparison
- `vendor/mini-sglang/python/minisgl/server/args.py`'s `--num-tokenizer` and the Python frontend's own launch CLI is what D-09's sweep drives directly

</code_context>

<specifics>
## Specific Ideas

- The project is explicitly a "learning-and-proof project" (PROJECT.md), not a paper submission — this shaped D-06 (5 runs, not 20+) and D-16 (a co-occurrence stat, not a full correlation chart): proportionate rigor, not maximum rigor.
- D-09/D-10's asymmetry (Python gets a sweep, Rust doesn't) is intentional, not an oversight — it's framed as part of the result being demonstrated, not a gap in the comparison's fairness.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

### Reviewed Todos (not folded)
None — no pending todos matched this phase.

</deferred>

---

*Phase: 07-frontend-benchmarks*
*Context gathered: 2026-10-06*
