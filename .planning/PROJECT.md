# mini-rsglang

## What This Is

mini-sglang with its Python frontend replaced by an optimized Rust frontend. The repo is built on top of `sgl-project/mini-sglang` (MIT): its Python code is vendored in at a pinned commit, and the frontend processes — API server, tokenizer, detokenizer — are rewritten in Rust (concurrent ingress, request-lifecycle FSM, Hugging Face tokenization and detokenization). The Python/CUDA backend (scheduler, engine, KV cache, kernels) stays in Python and is shared by both frontends. The original Python frontend is kept frozen as the baseline, so the same repo can launch either `--frontend python` or `--frontend rust` and measure how much the Rust frontend improves each scenario. It is for the author as a learning-and-proof project.

## Core Value

Serving through the Rust frontend produces output identical to the Python frontend on the same backend, and a reproducible benchmark harness quantifies the Rust frontend's improvement in each of the three host-overhead-bound scenarios.

## Requirements

### Validated

- ✓ Rust frontend talks to the backend over the existing ZMQ + MessagePack boundary; lock-free channels are used inside the Rust process — Phase 3 (WIRE-03: single ordered `tx-zmq` writer + per-uid `rx-zmq` broadcast dispatcher, both built on `tokio::sync` channels, proven under 64-case concurrent-ordering property test against a real subprocess)
- ✓ One minimal mock backend so the Rust frontend is developed and tested on macOS without a GPU (no extra mocks beyond what tests need) — Phase 3 (MOCK-01: `mock-scheduler` binary with echo tokens, fixed delays, and the late-abort-token/drop-overlong/batched-reply misbehaviors the cancellation tests need)
- ✓ Rust Hugging Face tokenization, chat-template rendering and incremental detokenization matching the Python frontend exactly — Phase 4 (Qwen3-0.6B and Llama-3.2-1B-Instruct; TOK-01 through TOK-04)
- ✓ Rust concurrent ingress (HTTP/async) accepting requests, streaming responses, handling client disconnects as cancellations — Phase 5 (LIFE-02, API-01: axum HTTP layer, immediate/deferred abort timing, byte-exact `/v1/chat/completions`/`/generate`/`/v1/models`/`/v1` parity against the frozen Python frontend)
- ✓ Rust async request-lifecycle FSM (received, tokenizing, submitted, decoding, finished, cancelled, failed) supporting 128 concurrent agents with dynamic requests/cancellations — Phase 5 (LIFE-01, LIFE-03, LIFE-04, LIFE-05, API-02: transition-table + registry actor proving exactly-one-terminal-state; 128-agent/256-request stress test with zero leaks; `/health`, `/health/ready`, `/metrics`)

### Active

- [ ] Vendor mini-sglang @ `9a91cfa` into the repo with its MIT license and attribution; record the source commit and every modified file in `UPSTREAM.md`
- [ ] One launcher that starts the shared backend with either the frozen Python frontend or the Rust frontend
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
| Defer the Rust radix cache to v2; only record radix's share of scheduler time during baseline profiling | Radix lives in the scheduler (backend), not the frontend; it is not an immediate need and would not show up in frontend benchmarks | ✓ Measured — Phase 2: radix share was 1.58%/0.76%/0.98% of scheduler time across the 3 scenarios, on real GPU hardware. Report recommends this does not clear RADIX-01's "meaningful share" bar; the project author's decision, not an automatic verdict |
| FSM = request lifecycle FSM; constrained decoding deferred to v2 | Matches RFC Scenario 1 (dynamic requests/cancellations) | — Pending |
| Mock backend for Mac development, remote GPU for real runs | Mac has no CUDA | — Pending |
| Parity models: Qwen3-0.6B (hard gate) + one Llama-3.x | Llama exercises BOS and space-cleanup edge cases | — Pending |
| Baseline: report the Python frontend at default and at its best `--num-tokenizer` | Headline comparison against the best-tuned Python frontend | — Pending |
| Cold start and RAM measured for the frontend only; end-to-end reported separately | End-to-end start is dominated by weight loading, identical for both frontends | — Pending |
| Done = output parity + benchmark harness quantifying per-scenario improvement; ±2% only a reference | RFC numbers are estimates | — Pending |
| Phase 7 benchmark design must attribute cost to frontend (api_server + tokenizer: IPC/serde, tokenize/detokenize, HTTP, frontend GC) vs. shared backend (scheduler CPU, radix, scheduler GC) separately | Phase 2's baseline profile found the scheduler process already at 93% CPU-active in the heaviest scenario (128-agent load) — a faster Rust frontend cannot exceed the throughput ceiling the backend itself sets there. Frontend-attributable cost (ipc_zmq+serde) was measured at 6.77-11.01 ms/request, ~7% of p50 TTFT in that same scenario | ✓ Confirmed — Phase 2 `docs/benchmarks/baseline-profile.md`. Rust-frontend gains are more likely to be visible in lower-backend-load scenarios (e.g. short-prompt saturation) than the heaviest-load one |
| Writer's ordering point is the submit's mpsc enqueue, not the later wire send; abort requires a `Submitted` ticket obtainable only after that enqueue completes | Makes "abort can never precede its own submit" structural rather than timing-dependent; a raw-uid abort would need the writer to buffer/guess about submits that might never come | ✓ Shipped — Phase 3, proven by a 64-case property test against a real mock subprocess |
| Per-uid reply channel is `tokio::sync::broadcast::channel(16)`, fixed capacity, drop-oldest | A slow consumer on one request must never stall replies for others (WIRE-03 criterion 4); fixed capacity avoids a CLI/config knob surface | ✓ Shipped — Phase 3 |
| mock-scheduler's readiness travels out-of-band on stdout (the Phase 1 handshake JSON line); misbehaviors and observation never become a 9th wire tag | Any new wire field crashes the real scheduler; the mock must prove it speaks only rsg-wire's existing types | ✓ Shipped — Phase 3 |
| Promote model identity to a first-class `ModelSpec` parameter (slug, repo_id, gated) from the first tokenizer plan, rather than writing Qwen3-only code and bolting Llama on after | ROADMAP's pluralized phrasing ("Qwen3-0.6B and one Llama-3.x model") was ambiguous; CONTEXT.md D-10 already specified a parametrized shape for the Llama BOS/clean_up assertions, so building it in from the start avoided re-deriving the same shape later | ✓ Done — Phase 4 Plan 04-01. Every loader/encode/template/detokenize function takes a `ModelSpec`; both Rust and Python test/fixture code iterate one shared `MODELS` list |
| `chrono` added as a new workspace dependency, approved via a blocking-human package-legitimacy checkpoint (not auto-approved) | Needed for `strftime_now` in Llama's chat template; not covered by RESEARCH.md's original 6-crate Package Legitimacy Audit, so treated as `[ASSUMED]` and gated on explicit human sign-off rather than silently added alongside the audited set | ✓ Approved — Phase 4 Plan 04-04 (crates.io verdict `OK`, v0.4.45, human confirmed) |
| Canonical `meta-llama/Llama-3.2-1B-Instruct` tokenizer facts are authoritative over RESEARCH.md's third-party-mirror-sourced assumptions when they diverge | RESEARCH.md's Pitfalls 2/3/5 were sourced from `unsloth/Llama-3.2-1B-Instruct` (a mirror), not the canonical gated repo. Once gated access was obtained, the real repo was fetched and spot-checked directly | ✓ Measured — Phase 4 Plan 04-04/04-06: the canonical config has no `add_bos_token` key at all (mirror had set it `true`); the real BOS occurrence count in a rendered chat prompt is **2**, not the 1 the project had assumed. Both are now asserted directly against the live oracle, not papered over |
| Lifecycle driver is a per-request engine driver plus one single-owner registry actor, not one monolithic actor | Fits Phase 3's per-request reply-stream shape; CONTEXT.md left the exact architecture to the planner | ✓ Shipped — Phase 5 Plan 05-01. `Engine::new(writer, dispatch, codec, registry, config)`; the registry removes a uid's entry the instant it reaches a terminal state, so LIFE-01's exactly-one-terminal invariant is structural, not asserted after the fact |
| Streaming `/v1/chat/completions` uses manual SSE framing, not `axum::response::Sse` | Upstream sends `text/event-stream; charset=utf-8` with no keep-alive comments; axum's built-in SSE type would diverge from byte-exact parity | ✓ Shipped — Phase 5 Plan 05-03 |
| API-01's golden-fixture oracle is captured live from upstream's own frozen Python frontend (D-02), not hand-ported | A hand-port risks encoding the project's own assumptions about upstream's behavior rather than upstream's actual behavior | ✓ Shipped — Phase 5 Plan 05-05/05-09: 18 cases captured via `gen_api_fixtures.py`, replayed byte-for-byte against the real `rsg-server` binary in Plan 05-09 |
| Abort-timing (`--abort-timing immediate\|deferred`, LIFE-05) is a server-wide CLI flag added alongside the default immediate path, not a replacement for it | The assumption-delta check flagged a singular→plural transition (one abort path becoming two); CONTEXT.md's own D-01 already called for both to coexist | ✓ Shipped — Phase 5 Plan 05-04/05-08 |
| `ServerMetrics` installs a per-server Prometheus recorder, never a process-global one | Keeps multiple `rsg-server` instances (e.g. in-process tests) from fighting over one global registry | ✓ Shipped — Phase 5 Plan 05-06 |
| The 128-agent cancellation stress test (LIFE-03) runs in the default `cargo test` suite, not gated behind `--ignored` | It completes in well under a second against the mock, so there is no latency reason to hide it from the normal gate | ✓ Shipped — Phase 5 Plan 05-07; `scripts/check_all.sh` raises its own open-file soft limit (`ulimit -n`) to cover the ~256 sockets the test opens, rather than pulling in a crate for it |
| The per-request `IncrementalDecoder` (which clones the full tokenizer vocab/merge table) must be constructed *before* `register`/`submit`, never after | Building it after submit left a window where the clone's real cost (tens of ms for the full HF tokenizer) could let a fast backend overflow the per-uid broadcast buffer before the decode loop's first `recv()` — a real, reproduced token-drop bug, not a hypothetical | ✓ Fixed — Phase 5 Plan 05-09, found while making the API-01 byte-parity test pass against the real tokenizer; confirmed stable across 7+ repeated runs. Code review (05-REVIEW.md, WR-01) flagged that the same decoder-construction call is still synchronous on the async driver task with no `.await`, which can still starve *other* concurrent requests' polls even though it no longer starves its own — open, non-blocking, tracked in `05-REVIEW-DISPOSITION.md` |

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
*Last updated: 2026-10-07 after Phase 5*
