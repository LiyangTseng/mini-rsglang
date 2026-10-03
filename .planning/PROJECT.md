# mini-rsglang

## What This Is

mini-sglang with its Python frontend replaced by an optimized Rust frontend. The repo is built on top of `sgl-project/mini-sglang` (MIT): its Python code is vendored in at a pinned commit, and the frontend processes — API server, tokenizer, detokenizer — are rewritten in Rust (concurrent ingress, request-lifecycle FSM, Hugging Face tokenization and detokenization). The Python/CUDA backend (scheduler, engine, KV cache, kernels) stays in Python and is shared by both frontends. The original Python frontend is kept frozen as the baseline, so the same repo can launch either `--frontend python` or `--frontend rust` and measure how much the Rust frontend improves each scenario. It is for the author as a learning-and-proof project.

## Core Value

Serving through the Rust frontend produces output identical to the Python frontend on the same backend, and a reproducible benchmark harness quantifies the Rust frontend's improvement in each of the three host-overhead-bound scenarios.

## Requirements

### Validated

(None yet — ship to validate)

### Active

- [ ] Vendor mini-sglang @ `9a91cfa` into the repo with its MIT license and attribution; record the source commit and every modified file in `UPSTREAM.md`
- [ ] One launcher that starts the shared backend with either the frozen Python frontend or the Rust frontend
- [ ] Rust concurrent ingress (HTTP/async) accepting requests, streaming responses, handling client disconnects as cancellations
- [ ] Rust async request-lifecycle FSM (received, tokenizing, submitted, decoding, finished, cancelled, failed) supporting 128 concurrent agents with dynamic requests/cancellations
- [ ] Rust Hugging Face tokenization, chat-template rendering and incremental detokenization matching the Python frontend exactly
- [ ] Rust frontend talks to the backend over the existing ZMQ + MessagePack boundary; lock-free channels are used inside the Rust process
- [ ] Mock backend so the entire Rust frontend is developed and tested on macOS without a GPU
- [ ] End-to-end run on a remote GPU machine with output identical to the Python frontend
- [ ] Reproducible benchmark harness comparing Python vs Rust frontend on the same backend for three scenarios: (1) 128 concurrent agents with dynamic requests/cancellations, P99 TTFT; (2) 32-token short-prompt saturation, RPS; (3) frontend cold start latency and frontend host RAM
- [ ] Standard inference throughput does not regress versus the Python frontend (about parity)

### Out of Scope

- GPU kernels, weight loading, continuous batching loop — stay in Python/CUDA; the Rust work is frontend-only
- Modifying the vendored Python frontend — it is the frozen baseline; any unavoidable change is recorded in `UPSTREAM.md`
- Rust radix cache — deferred to v2. The radix cache lives in the scheduler (backend) and indexes GPU KV pages, so it is not part of the frontend migration; listing it in the original RFC as a front-half module was a misunderstanding. Revisit only if profiling shows radix time matters
- Structured-output / constrained-decoding FSM (regex, JSON schema) — deferred to v2; the v1 FSM is the request lifecycle FSM
- Running the backend on the Mac — upstream backend is Linux/CUDA only; real-backend runs and benchmarks happen on a remote GPU machine

## Context

- Source blueprint: "[RFC] Mini-SGLang Front-Half Rust Migration & Benchmark Verification Blueprint" (draft v0.2, 2026-10-02), targeting sgl-project/mini-sglang. Its numbers (30%+ P99 TTFT reduction, 20-40% RPS gain, millisecond cold start, under 500 MB host RAM, ±2% parity on standard inference) are projections, not measurements; the ±2% figure in particular is an estimate and a reference target, not a hard gate.
- Research (`.planning/research/`, mini-sglang source read at `9a91cfa`): the frontend↔scheduler boundary is pyzmq PUSH/PULL over `ipc://` carrying `__type__`-tagged msgpack maps decoded with `cls(**kwargs)`; the scheduler returns token ids, not text, so detokenization is a frontend job. The radix cache lives inside the scheduler and indexes GPU KV pages, so it is backend code.
- Known backend behaviors the frontend must absorb: no abort acknowledgement (late tokens after abort), overlong prompts dropped silently, no finish reason on the wire, no readiness signal. Because the backend is vendored, small shared backend fixes (e.g. a readiness handshake) are allowed when they apply to both frontends.
- Development machine is macOS (Darwin, no CUDA). GPU work runs on a remote machine.
- Directory started empty; no existing code.

## Constraints

- **Architecture**: Rust owns ingress, lifecycle FSM, tokenization, detokenization; Python/CUDA owns scheduler, weights, batching loop, kernels, KV cache (including the radix cache)
- **Fair comparison**: both frontends run against the same vendored backend; backend changes must apply to both modes; the Python frontend stays frozen
- **IPC**: the existing ZMQ + MessagePack wire format; Rust must match it byte-for-byte (an extra key crashes the scheduler)
- **Environment**: must be developable and testable without a GPU (mock backend) — dev machine is a Mac
- **Verification**: performance claims are measured on Linux on identical hardware; projections stay projections until measured
- **License**: mini-sglang is MIT; keep its copyright notice and LICENSE

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Build on top of mini-sglang by vendoring its code (MIT) instead of a submodule | The goal is to replace mini-sglang's Python frontend; vendoring lets the backend gain small shared fixes while staying one repo | — Pending |
| Keep the Python frontend frozen as the baseline; `--frontend python\|rust` on a shared backend | Isolates the frontend's effect in every benchmark | — Pending |
| Defer the Rust radix cache to v2; only record radix's share of scheduler time during baseline profiling | Radix lives in the scheduler (backend), not the frontend; it is not an immediate need and would not show up in frontend benchmarks | — Pending |
| FSM = request lifecycle FSM; constrained decoding deferred to v2 | Matches RFC Scenario 1 (dynamic requests/cancellations) | — Pending |
| Mock backend for Mac development, remote GPU for real runs | Mac has no CUDA | — Pending |
| Parity models: Qwen3-0.6B (hard gate) + one Llama-3.x | Llama exercises BOS and space-cleanup edge cases | — Pending |
| Baseline: report the Python frontend at default and at its best `--num-tokenizer` | Headline comparison against the best-tuned Python frontend | — Pending |
| Cold start and RAM measured for the frontend only; end-to-end reported separately | End-to-end start is dominated by weight loading, identical for both frontends | — Pending |
| Done = output parity + benchmark harness quantifying per-scenario improvement; ±2% only a reference | RFC numbers are estimates | — Pending |

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
*Last updated: 2026-10-02 after research and scoping decisions*
