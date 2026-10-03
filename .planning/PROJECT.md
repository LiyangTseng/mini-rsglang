# mini-rsglang

## What This Is

An original Rust implementation of the non-kernel "front half" of an LLM serving stack, modeled on the spirit of `sgl-project/mini-sglang`. It is not a fork: the design of mini-sglang is studied and the host-side infrastructure (concurrent ingress, request lifecycle FSM, Hugging Face tokenization, radix cache trie) is rewritten in Rust. The Rust front half drives an unmodified upstream mini-sglang Python/CUDA backend over a MessagePack boundary. It is for the author as a learning-and-proof project: demonstrating that GC-free, GIL-free host infrastructure measurably beats Python where host overhead matters.

## Core Value

End-to-end serving through the Rust front half produces output identical to the Python baseline, and a reproducible benchmark harness shows it beating that baseline in the three host-overhead-bound scenarios.

## Requirements

### Validated

(None yet — ship to validate)

### Active

- [ ] Rust concurrent ingress (HTTP/async) accepting requests, streaming responses, handling dynamic cancellations
- [ ] Rust async request-lifecycle FSM (queued, prefill, decode, cancelled, finished) supporting 128 concurrent agents with dynamic requests/cancellations
- [ ] Rust Hugging Face tokenization (string to token ids) matching Python tokenizer output
- [ ] Rust radix cache trie (prefix matching, insertion, eviction) behaviorally equivalent to the Python radix cache
- [ ] MessagePack byte-stream boundary over a lock-free channel between the Rust front half and the unmodified Python/CUDA backend (scheduler loop, weight loading, FlashAttention/FlashInfer stay in Python)
- [ ] Mock backend so the entire Rust front half is developed and tested on macOS without a GPU
- [ ] End-to-end run on a remote GPU machine with real model output identical to the upstream Python baseline
- [ ] Reproducible benchmark harness for three scenarios against the upstream Python baseline: (1) 128 concurrent agents with dynamic requests/cancellations, P99 TTFT; (2) 32-token short-prompt saturation, RPS; (3) serverless cold start latency and host RAM
- [ ] Standard inference throughput does not regress versus the Python baseline (about parity)

### Out of Scope

- GPU kernels, weight loading, continuous batching loop — stay in unmodified upstream Python/CUDA (the "surgical" boundary of the RFC)
- Forking or patching upstream mini-sglang — the project is an original Rust implementation; upstream is a dependency and baseline only
- Structured-output / constrained-decoding FSM (regex, JSON schema) — deferred to v2; the v1 FSM is the request lifecycle FSM
- Running CUDA on the Mac — Apple Silicon has no CUDA; real-backend runs and benchmarks happen on a remote GPU machine

## Context

- Source blueprint: "[RFC] Mini-SGLang Front-Half Rust Migration & Benchmark Verification Blueprint" (draft v0.2, 2026-10-02), targeting sgl-project/mini-sglang. Its numbers (30%+ P99 TTFT reduction, 20-40% RPS gain, millisecond cold start, under 500 MB host RAM, ±2% parity on standard inference) are projections, not measurements; the ±2% figure in particular is an estimate and a reference target, not a hard gate.
- Upstream mini-sglang is used unmodified as a submodule or pip dependency and doubles as the benchmark baseline, so baseline and backend are the same code.
- Development machine is macOS (Darwin, no CUDA). The author will separately try to enable CUDA on the Mac, but the plan assumes GPU work runs on a remote machine.
- Directory started empty; no existing code.

## Constraints

- **Architecture**: Rust owns ingress, lifecycle FSM, tokenization, radix cache; Python/CUDA owns weights, batching loop, kernels — keeps GPU kernels untouched and preserves the educational spirit of mini-sglang
- **IPC**: MessagePack over a lock-free channel — avoids the Python GIL on the boundary
- **Environment**: Must be developable and testable without a GPU (mock backend) — dev machine is a Mac
- **Verification**: Benchmarks compare against unmodified upstream on identical hardware — claims are projections until measured
- **Dependency**: Upstream mini-sglang pinned to a specific revision — baseline reproducibility

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Original Rust implementation, not a fork | Study mini-sglang's spirit and rewrite non-kernel parts; avoids carrying upstream patches | — Pending |
| Upstream used unmodified as backend and baseline | Fair, same-code comparison; no need to rewrite GPU half | — Pending |
| All four Rust modules in v1 (ingress, lifecycle FSM, tokenizer, radix cache) | No blocking reason to defer any; IPC boundary built first to de-risk | — Pending |
| FSM = request lifecycle FSM; constrained decoding deferred to v2 | Matches RFC Scenario 1 (dynamic requests/cancellations) | — Pending |
| Mock backend for Mac development, remote GPU for real runs | Mac has no CUDA | — Pending |
| Done = output parity + benchmark harness showing three-scenario wins; ±2% only a reference | RFC numbers are estimates | — Pending |

## Evolution

This document evolves at phase transitions and milestone boundaries.

**After each phase transition** (via `/gsd-transition`):
1. Requirements invalidated? → Move to Out of Scope with reason
2. Requirements validated? → Move to Validated with phase reference
3. New requirements emerged? → Add to Active
4. Decisions to log? → Add to Key Decisions
5. "What This Is" still accurate? → Update if drifted

**After each milestone** (via `/gsd-complete-milestone`):
1. Full review of all sections
2. Core Value check — still the right priority?
3. Audit Out of Scope — reasons still valid?
4. Update Context with current state

---
*Last updated: 2026-10-02 after initialization*
