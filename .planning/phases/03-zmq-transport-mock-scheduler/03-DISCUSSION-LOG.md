# Phase 3: ZMQ Transport & Mock Scheduler - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-10-05
**Phase:** 3-zmq-transport-mock-scheduler
**Areas discussed:** Mock scheduler shape, Slow-consumer backpressure, Ordering-guarantee proof, Mock timing & reuse horizon

---

## Mock scheduler shape

| Option | Description | Selected |
|--------|-------------|----------|
| Subprocess only | Standalone binary over real ipc:// sockets, matching how Phase 1's launcher spawns rsg-server | ✓ |
| In-process library only | Rust struct/trait driven directly in test code, no real sockets | |
| Both: library core + thin subprocess wrapper | Reusable crate + thin binary wrapper | |

**User's choice:** Subprocess only.

| Option | Description | Selected |
|--------|-------------|----------|
| CLI flags at spawn | e.g. --abort-late-tokens, --overlong-threshold, --batch-size | ✓ |
| Scenario config file (JSON/TOML) | Declarative per-uid/per-phase behavior file | |
| Per-request signal via sampling_params | Piggyback scenario tag on existing wire fields | |

**User's choice:** CLI flags at spawn.

| Option | Description | Selected |
|--------|-------------|----------|
| uid-range flags | e.g. --misbehave-uids 3,7 --behavior late-abort-token | ✓ |
| One behavior per process, one test per behavior | Each mock instance has exactly one behavior for all uids | |

**User's choice:** uid-range flags, so multiple behaviors can be active in one run.

| Option | Description | Selected |
|--------|-------------|----------|
| Fixed batch size flag | --batch-size N, deterministic accumulate-then-flush | ✓ |
| Randomized/jittered batching | Seeded random batch size/timing | |

**User's choice:** Fixed batch size flag.

**Notes:** None.

---

## Slow-consumer backpressure

| Option | Description | Selected |
|--------|-------------|----------|
| Bounded per-uid channel, drop oldest on overflow | Each uid gets its own bounded mpsc; oldest token dropped on overflow, dispatcher never blocks | ✓ |
| Unbounded per-uid channel | Never drops, but can leak memory for a truly stuck consumer | |
| One dispatcher task per uid | Isolate blocking by giving each uid its own task | |

**User's choice:** Bounded per-uid channel, drop oldest on overflow.

| Option | Description | Selected |
|--------|-------------|----------|
| Surface a dropped-count signal | Per-uid dropped-token counter + tracing warning | ✓ |
| Silent drop, no signal | Dropped tokens vanish with no trace | |

**User's choice:** Surface a dropped-count signal.

| Option | Description | Selected |
|--------|-------------|----------|
| Small fixed constant, e.g. 16 | One constant, no new config surface | ✓ |
| Configurable via CLI/constructor param | Expose the bound so later phases can tune it | |

**User's choice:** Small fixed constant, e.g. 16.

**Notes:** The dropped-count signal was chosen specifically so Phase 6 parity debugging can distinguish an intentional drop from a real backend bug.

---

## Ordering-guarantee proof

| Option | Description | Selected |
|--------|-------------|----------|
| Property-based random interleavings (proptest) | Random submit/abort sequences across many callers, asserts order preserved every run | ✓ |
| Targeted deterministic stress tests | Fixed hand-written scenarios | |
| Both: proptest + deterministic regression tests | Property test for the invariant, deterministic tests for found regressions | |

**User's choice:** Property-based random interleavings (proptest).

| Option | Description | Selected |
|--------|-------------|----------|
| End-to-end through mock-scheduler | Real transport + real mock-scheduler subprocess as the receiver | ✓ |
| Isolated writer-logic test | In-memory stand-in, no real sockets | |

**User's choice:** End-to-end through mock-scheduler.

**Notes:** None.

---

## Mock timing & reuse horizon

| Option | Description | Selected |
|--------|-------------|----------|
| Minimal now, extend later | Just enough delay configurability for Phase 3's own criteria; Phase 5/7 extend when their plans exist | ✓ |
| Build full configurability now | Seeded RNG delays, replayable scenario profiles, anticipating Phase 5/7's needs | |

**User's choice:** Minimal now, extend later.

| Option | Description | Selected |
|--------|-------------|----------|
| Fixed per-token delay flags | --prefill-delay-ms / --decode-delay-ms, uniform across requests | ✓ |
| No configurable delay | Emit tokens as fast as possible | |

**User's choice:** Fixed per-token delay flags.

**Notes:** Deliberate anti-speculation choice — Phase 5 and Phase 7 don't have locked plans yet, so their exact timing needs aren't known.

---

## Claude's Discretion

- The exact single-writer mechanism (dedicated OS thread + channel vs. async task owning the socket).
- Internal module/crate layout for `mock-scheduler` and the per-uid dispatch table.
- Exact proptest case count, shrinking configuration, and simulated-caller concurrency.
- The precise `--behavior` flag vocabulary/syntax for `mock-scheduler`.

## Deferred Ideas

- Richer mock-scheduler timing (seeded random delays, per-uid overrides, replayable scenarios) — revisit when Phase 5/7 plans specify what they need.
- Configurable per-uid channel bound — add a knob only if a later phase's scenario needs to tune it.
