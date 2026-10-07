# Phase 7: Frontend Benchmarks - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-10-06
**Phase:** 07-frontend-benchmarks
**Areas discussed:** Load generator & cancellation model, A/B runs/manifest & confidence intervals, Best --num-tokenizer & regression gate, Report format & GC/memory correlation

---

## Load generator & cancellation model

| Option | Description | Selected |
|--------|-------------|----------|
| One driver, mode flag | Single Rust binary/crate with `--mode closed\|open`, shared hdrhistogram recording | ✓ |
| Separate scenario binaries | Distinct binaries/subcommands per scenario, no shared loop abstraction | |

**User's choice:** One driver, mode flag.

| Option | Description | Selected |
|--------|-------------|----------|
| In the harness driver | Seeded RNG / abort-after-N logic in the Rust load generator itself, mock-scheduler stays dumb | ✓ |
| Push timing into mock-scheduler | New mock-scheduler flags for randomized decode timing | |

**User's choice:** In the harness driver.

| Option | Description | Selected |
|--------|-------------|----------|
| Mac dev against mock-scheduler first | Harness correctness proven on Mac before GPU numbers | ✓ |
| GPU-only, no Mac dev pass | Write/debug directly against real backend on GPU machine | |

**User's choice:** Mac dev against mock-scheduler first.

| Option | Description | Selected |
|--------|-------------|----------|
| Run both as cross-check | vllm bench serve + sglang benchmark.serving reported alongside custom harness for Scenario 2 | ✓ |
| Custom harness only | Skip third-party tools in v1 | |

**User's choice:** Run both as cross-check.

---

## A/B runs, manifest & confidence intervals

| Option | Description | Selected |
|--------|-------------|----------|
| Repeat N full runs, CI across runs | Captures real run-to-run variance | ✓ |
| Single long run, CI from hdrhistogram resampling | Cheaper, only within-run noise | |

**User's choice:** Repeat N full runs, CI across runs.

| Option | Description | Selected |
|--------|-------------|----------|
| 5 runs per side | Default alternating P,R,P,R,... | ✓ |
| 3 runs per side | Faster, wider CI | |

**User's choice:** 5 runs per side.

| Option | Description | Selected |
|--------|-------------|----------|
| Full environment snapshot | Git SHAs, model, GPU, driver, OS, versions, flags, seeds, raw histograms — same depth as Phase 2's meta block | ✓ |
| Minimal flags-and-seed manifest | Just enough to re-run the command | |

**User's choice:** Full environment snapshot.

| Option | Description | Selected |
|--------|-------------|----------|
| One shared A/B orchestrator | Shared alternation/manifest logic, per-scenario single-trial runners plug in | ✓ |
| Per-scenario alternation logic | Each scenario handles its own alternation/manifest | |

**User's choice:** One shared A/B orchestrator.

---

## Best --num-tokenizer & regression gate

| Option | Description | Selected |
|--------|-------------|----------|
| Sweep a small fixed set, pick max-RPS | Measured, bounded GPU time | ✓ |
| Reuse Phase 2's baseline-profile findings | Inference from a single data point, no fresh sweep | |

**User's choice:** Sweep a small fixed set, pick max-RPS.

| Option | Description | Selected |
|--------|-------------|----------|
| Rust reported at one config only | No tunable multi-process split; asymmetry is part of the result | ✓ |
| Sweep Rust's internal tokenizer-pool size too | Symmetric but adds scope | |

**User's choice:** Rust reported at one config only.

| Option | Description | Selected |
|--------|-------------|----------|
| Report the delta, flag if negative | Informational only, no automated gate, matches PROJECT.md's "not a hard gate" stance | ✓ |
| Hard pass/fail exit code at ±2% | Contradicts PROJECT.md's explicit framing | |

**User's choice:** Report the delta, flag if negative.

| Option | Description | Selected |
|--------|-------------|----------|
| Reuse bench_simple.py workload | Keeps "standard inference" consistently defined project-wide | ✓ |
| Define a separate standard-throughput workload | New workload shape to pin down now | |

**User's choice:** Reuse bench_simple.py workload.

---

## Report format & GC/memory correlation

| Option | Description | Selected |
|--------|-------------|----------|
| docs/benchmarks/, same md+json pattern | Identical to Phase 2's convention | ✓ |
| New location/format for Phase 7 | Only if Phase 7's reports don't fit Phase 2's shape | |

**User's choice:** docs/benchmarks/, same md+json pattern.

| Option | Description | Selected |
|--------|-------------|----------|
| Separate lightweight sampling pass | Timed runs stay uninstrumented; RSS/PSS + GC counter run alongside | ✓ |
| Reuse Phase 2's full instrumentation in timed runs | Contradicts the goal of measuring real uninstrumented performance | |

**User's choice:** Separate lightweight sampling pass.

| Option | Description | Selected |
|--------|-------------|----------|
| Keep gc.callbacks, drop tracemalloc+py-spy | gc.callbacks is near-zero overhead; the other two caused Phase 2's measured 3.5x slowdown | ✓ |
| Drop all three, estimate GC separately | Can't correlate GC with the same run's P99 timeline | |

**User's choice:** Keep gc.callbacks, drop tracemalloc+py-spy.

| Option | Description | Selected |
|--------|-------------|----------|
| Timeline overlay + simple co-occurrence stat | GC-pause table + fraction of P99-or-worse requests with a co-occurring GC pause | ✓ |
| Full timestamp-correlated chart | New visualization tooling, beyond what the roadmap criterion requires | |

**User's choice:** Timeline overlay + simple co-occurrence stat.

---

## Claude's Discretion

- Exact crate/module layout for the harness orchestrator and per-scenario runners.
- Exact candidate values and sweep methodology details for the `--num-tokenizer` pre-pass beyond "small fixed set, pick max RPS."
- Exact RSS/PSS sampling interval and `sysinfo` invocation details for the lightweight sampling pass.
- Whether one combined `docs/benchmarks/phase7-benchmarks.{md,json}` or per-scenario files best fits the report-format decision.
- CI/CLI override surface for the default run count of 5.

## Deferred Ideas

None — discussion stayed within phase scope.
