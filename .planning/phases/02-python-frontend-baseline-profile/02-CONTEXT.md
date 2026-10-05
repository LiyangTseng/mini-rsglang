# Phase 2: Python Frontend Baseline Profile - Context

**Gathered:** 2026-10-05
**Status:** Ready for planning

<domain>
## Phase Boundary

Measure where the frozen Python frontend (and, for the radix-cache question specifically, the scheduler/backend it talks to) spends host-side time and memory across the three benchmark scenarios — 128-concurrent-agent cancellations, 32-token saturation RPS, and cold-start/RAM. The output is a profiling report that informs Phase 7's benchmark design and feeds the v2 radix-cache go/no-go decision (RADIX-01/02). This phase is observation-only: no backend or frontend behavior changes, and the measurement is scripted so it can be re-run on the GPU machine.

</domain>

<decisions>
## Implementation Decisions

### Profiling method
- **D-01:** Primary CPU time / GIL contention profiler is **py-spy**, run against the frontend processes (and, per D-05, also the scheduler process).
- **D-02:** Memory profiling (allocation + resident growth) uses **tracemalloc** plus periodic RSS sampling, not `memory_profiler` or scalene's built-in tracking.
- **D-03:** GC pause measurement uses a **`gc.callbacks` hook** (count, duration, timestamp for P99-spike correlation), not `PYTHONDEVMODE`/`gc.set_debug` log parsing.
- **D-04:** Serialization and IPC cost is measured via **py-spy sampling across all frontend processes** (tokenizer/detokenizer/api_server hops in `mp.py`), not hand-added wrapper timers.

### Workload driver for the 3 scenarios
- **D-05:** Overall driver is a **small throwaway Python script**, reusing `minisgl.benchmark.client` / `bench_simple.py` helpers where they fit — not a third-party tool, not an in-place extension of `bench_simple.py`.
- **D-06:** Scenario 1 (128 concurrent agents, dynamic cancellations) uses a **minimal asyncio/aiohttp script** with a configurable early-cancel fraction.
- **D-07:** Scenario 2 (32-token saturation, RPS) **adapts `bench_simple.py`'s client helpers**, with `MAX_INPUT` pinned near 32 tokens.
- **D-08:** Scenario 3 (cold start + RAM) uses **`hyperfine` + `/v1/models` polling**, with PSS sampled across the whole process tree.

### Radix cache time attribution
- **D-09:** Measure the scheduler's radix-cache time share via **py-spy sampling against the scheduler process**, bucketing sampled stack frames under `scheduler/cache.py` (`match_req`, `cache_req`) and `kvcache/radix_cache.py` (`match_prefix`, `insert_prefix`, `evict`, `_tree_walk`) as "radix time". — **Reversibility:** reversible — purely observational; no vendored code is touched, so the method can be swapped later without any migration.
- **D-10:** The radix share's denominator is **radix-attributed sampled time / total scheduler-process sampled time** (not end-to-end request latency, which would mix in frontend time and muddy the v2 decision).
- **D-11:** Radix-cache sampling runs across **all 3 benchmark scenarios** (piggybacking on the same py-spy pass that already samples the scheduler process for GC/memory/GIL in each scenario run), not just scenario 2.

### Report format & location
- **D-12:** The profiling report lives at **`docs/benchmarks/baseline-profile.md`** (narrative findings — what the numbers mean for benchmark design, the radix verdict) — not a `.planning/phases/...` phase artifact, since Phase 7 and the v2 radix decision need to cite it after this phase's planning docs may be archived.
- **D-13:** The report pairs the markdown with a **machine-readable sidecar, `docs/benchmarks/baseline-profile.json`** (GC pause counts/durations, RSS curve, GIL contention %, radix share %, per scenario) — not markdown-only tables.
- **D-14:** The **profiling script itself writes `baseline-profile.json` directly** (so a re-run on a later GPU session reproduces the numbers with no manual transcription). The markdown narrative is hand-written afterward, reading from the JSON.

### Claude's Discretion
- Exact py-spy invocation flags (sampling rate, `--native` or not), the precise `gc.callbacks` wiring, and the internal script/module layout under wherever the Phase 2 scripts live are left to the planner/executor — the decisions above fix the *what* and *where*, not every CLI flag.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Project scope and requirements
- `.planning/ROADMAP.md` — Phase 2 goal, success criteria, BENCH-01 scope
- `.planning/REQUIREMENTS.md` — BENCH-01 full text; v2 RADIX-01/02 (what this phase's radix number feeds); Out of Scope table (no backend/frontend behavior changes)
- `.planning/PROJECT.md` — "Defer the Rust radix cache to v2; only record radix's share of scheduler time during baseline profiling" key decision; constraint that performance claims are measured on Linux, projections stay projections until measured
- `.claude/CLAUDE.md` — project stack/architecture conventions

### Upstream code this phase observes (read, do not modify)
- `vendor/mini-sglang/benchmark/online/bench_simple.py` — existing client helpers to reuse for the workload driver
- `vendor/mini-sglang/python/minisgl/scheduler/cache.py` — `CacheManager.match_req`/`cache_req`, the frames to attribute as radix time
- `vendor/mini-sglang/python/minisgl/kvcache/radix_cache.py` — `RadixPrefixCache.match_prefix`/`insert_prefix`/`evict`, `_tree_walk` — the other frames to attribute as radix time
- `vendor/mini-sglang/python/minisgl/kernel/radix.py` — native radix kernel entry point (py-spy may not resolve into `csrc/src/radix.cpp` without `--native`; planner should note this as a known blind spot rather than solve it here)

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `vendor/mini-sglang/benchmark/online/bench_simple.py` — `AsyncOpenAI`-based client helpers, reused for scenario 2's driver and as a base for scenario 1's asyncio/aiohttp script
- `scripts/gpu_phase1_check.sh` (Phase 1) — existing convention for a GPU-machine verification script with `--help`/`--offline` flags; the Phase 2 profiling script should follow the same CLI shape for consistency

### Established Patterns
- No existing timing/profiling instrumentation exists anywhere in `scheduler/cache.py` or `kvcache/radix_cache.py` — confirmed by inspection. Radix time attribution is a pure external-sampling problem, not something to toggle on in the vendored code.
- `docs/` already holds durable reference material (`docs/mini-sglang-reading-guide.md`) distinct from `.planning/` (process state) and `scripts/` (tooling) — `docs/benchmarks/` extends that existing convention, it is not a new top-level concept.

### Integration Points
- Scheduler process (backend, GPU machine) — radix-cache py-spy sampling target (D-09)
- Frontend processes (api_server / tokenizer / detokenizer, per `mp.py` hops) — py-spy sampling target for IPC/serialization cost (D-04)
- `docs/benchmarks/baseline-profile.{md,json}` — the single artifact Phase 7 (benchmark design) and the v2 radix decision both read

</code_context>

<specifics>
## Specific Ideas

No further specific ideas beyond the decisions above — the profiling method, driver shape, radix attribution method, and report location/format are all locked.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>

---

*Phase: 02-python-frontend-baseline-profile*
*Context gathered: 2026-10-05*
