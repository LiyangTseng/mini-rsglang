# Phase 2: Python Frontend Baseline Profile - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-10-05
**Phase:** 02-python-frontend-baseline-profile
**Areas discussed:** Profiling method, Workload driver for the 3 scenarios, Radix cache time attribution, Report format & location

---

## Profiling method

*(Captured in an earlier session, resumed from checkpoint on 2026-10-05.)*

| Question | Selected |
|----------|----------|
| Primary tool for CPU time / GIL contention profiling | py-spy (over cProfile/yappi, scalene) |
| Memory profiling (allocation + resident growth) approach | tracemalloc + periodic RSS sampling (over memory_profiler, scalene's built-in) |
| GC pause measurement | gc.callbacks hook (over PYTHONDEVMODE/gc.set_debug + log parsing) |
| Serialization and IPC cost measurement | py-spy sampling across all frontend processes (over wrapper-level timers around mp.py calls) |

---

## Workload driver for the 3 scenarios

*(Captured in an earlier session, resumed from checkpoint on 2026-10-05.)*

| Question | Selected |
|----------|----------|
| Overall driver approach | Small throwaway Python script, reusing minisgl.benchmark.client helpers (over extending bench_simple.py in place, or a third-party tool) |
| Scenario 1 (128 concurrent, cancellations) driver | Minimal asyncio/aiohttp script with early-cancel fraction (over steady-state only) |
| Scenario 2 (32-token saturation, RPS) driver | Adapt bench_simple.py's client helpers, MAX_INPUT pinned near 32 tokens (over vllm bench serve / sglang benchmark.serving) |
| Scenario 3 (cold start + RAM) driver | hyperfine + /v1/models polling, PSS sampled across process tree (over deferring scenario 3 to Phase 7) |

---

## Radix cache time attribution

| Option | Description | Selected |
|--------|-------------|----------|
| py-spy sampling, filtered to radix frames | Run py-spy against the scheduler process, bucket frames under scheduler/cache.py + kvcache/radix_cache.py as "radix time". Zero changes to vendored backend code. | ✓ |
| Manual time.perf_counter() wrapper | Insert timing wraps around CacheManager.match_req/cache_req in the vendored scheduler. More precise, but touches vendored code. | |
| torch.profiler around the scheduler loop | Heavier, GPU-kernel-oriented. Overkill for a CPU-side call-time attribution question. | |

**User's choice:** py-spy sampling, filtered to radix frames
**Notes:** None of the three options required modifying vendored backend code to be viable, but py-spy was clearly preferred as the least invasive given this phase is observation-only.

| Option | Description | Selected |
|--------|-------------|----------|
| Radix time / total scheduler-process sampled time | Matches BENCH-01's exact wording; isolates backend-internal time. | ✓ |
| Radix time / total end-to-end request latency | Mixes in frontend/network latency, muddying the v2 radix decision. | |

**User's choice:** Radix time / total scheduler-process sampled time

| Option | Description | Selected |
|--------|-------------|----------|
| All 3 scenarios | Piggybacks on the existing py-spy pass already sampling the scheduler process. | ✓ |
| Scenario 2 only (32-token saturation) | Cleanest signal, but drops coverage of scenarios 1 and 3. | |

**User's choice:** All 3 scenarios

---

## Report format & location

| Option | Description | Selected |
|--------|-------------|----------|
| docs/benchmarks/baseline-profile.md | Extends the existing docs/ convention (docs/mini-sglang-reading-guide.md) for durable reference material, outside .planning/'s eventual milestone archival. | ✓ |
| Phase artifact: .planning/phases/02-.../02-PROFILE-REPORT.md | Consistent with other phase docs, but gets buried when .planning/ is archived at milestone completion. | |

**User's choice:** docs/benchmarks/baseline-profile.md

| Option | Description | Selected |
|--------|-------------|----------|
| Markdown doc + a numbers JSON sidecar | docs/benchmarks/baseline-profile.md (narrative) + baseline-profile.json (raw numbers) — Phase 7 can diff the JSON without scraping prose. | ✓ |
| Markdown only | Simpler, but numbers must be scraped from markdown tables later. | |

**User's choice:** Markdown doc + a numbers JSON sidecar

| Option | Description | Selected |
|--------|-------------|----------|
| Script emits the JSON sidecar directly | A re-run on a later GPU session reproduces the numbers with zero manual transcription risk. | ✓ |
| Hand-written report, script output is just raw logs | Simpler script, but risks transcription drift between runs. | |

**User's choice:** Script emits the JSON sidecar directly; markdown narrative stays hand-written, reading from the JSON.

---

## Claude's Discretion

- Exact py-spy invocation flags (sampling rate, `--native` or not)
- The precise `gc.callbacks` wiring
- Internal script/module layout for the Phase 2 profiling scripts

## Deferred Ideas

None — discussion stayed within phase scope.
