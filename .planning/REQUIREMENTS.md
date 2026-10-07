# Requirements: mini-rsglang

**Defined:** 2026-10-02
**Core Value:** Serving through the Rust frontend produces output identical to the Python frontend on the same backend, and a reproducible benchmark harness quantifies the Rust frontend's improvement in each of the three host-overhead-bound scenarios.

## v1 Requirements

### Base

- [x] **BASE-01**: mini-sglang @ `9a91cfa` is vendored into the repo with its MIT LICENSE and copyright notice; `UPSTREAM.md` records the source commit and every modified file
- [x] **BASE-02**: One launch command starts the shared backend with either `--frontend python` (frozen original) or `--frontend rust`
- [x] **BASE-03**: The backend reports a readiness handshake to the frontend (max_seq_len, eos_token_id, page_size, max_running_req); both frontends use the same backend code

### Wire Protocol

- [x] **WIRE-01**: The Rust msgpack codec produces bytes identical to upstream's encoder for all 7 message types, verified by golden fixtures exported from Python
- [x] **WIRE-02**: Every message Rust sends decodes through the real Python decoder (`cls(**kwargs)`) without error
- [x] **WIRE-03**: Rust exchanges messages with the scheduler over ZMQ `ipc://` through a single ordered writer, so an abort can never overtake its own submit

### Tokenizer

- [x] **TOK-01**: Rust tokenization produces token ids identical to the Python frontend on a test corpus (Qwen3-0.6B)
- [x] **TOK-02**: Rust chat-template rendering produces a prompt string identical to the Python frontend
- [x] **TOK-03**: Rust incremental detokenization produces streamed text identical to the Python frontend, with no UTF-8 breakage on CJK or emoji
- [x] **TOK-04**: TOK-01 through TOK-03 also pass for one Llama-3.x model

### Request Lifecycle

- [ ] **LIFE-01**: Each request moves through received → tokenizing → submitted → decoding → finished / cancelled / failed, and each terminal state is reached exactly once
- [ ] **LIFE-02**: On client disconnect (streaming or non-streaming), Rust sends an abort to the backend immediately; tokens that arrive after the abort are dropped and counted
- [ ] **LIFE-03**: A stress test with 128 concurrent requests and random cancellations ends with no leaked requests and no stuck connections
- [ ] **LIFE-04**: Overlong prompts get an immediate 400 (upstream drops them silently and the request hangs), and a request whose backend stops responding times out with an error
- [ ] **LIFE-05**: Abort timing is configurable (immediate or deferred until first token) to work around the suspected upstream abort-during-prefill bug; default is immediate

### HTTP API

- [ ] **API-01**: `/v1/chat/completions` (streaming and non-streaming), `/generate`, `/v1/models` and `/v1` behave identically to the Python frontend, including response format and SSE framing
- [ ] **API-02**: `/health`, `/health/ready` and a minimal `/metrics` (request count, cancellation count, TTFT histogram)

### Mock Backend

- [x] **MOCK-01**: One minimal Rust mock scheduler speaks the same wire protocol, so the Rust frontend runs end to end on a Mac, and can reproduce the backend behaviors the cancellation tests need (late tokens after abort, silently dropped overlong prompts, batched replies). No other mocks; protocol fidelity is covered by WIRE-01/02 and PAR-01

### End-to-End Parity

- [ ] **PAR-01**: On the GPU machine, with greedy decoding (temperature 0) sent one request at a time, the Rust and Python frontends produce identical output on at least 100 prompts
- [ ] **PAR-02**: Under concurrent load, the output match rate is reported (not a hard gate, because GPU batch composition affects results)

### Benchmarks

- [x] **BENCH-01**: Profile the Python frontend on the GPU machine and quantify its host-side overhead in the three scenarios — GC pauses (count, duration, correlation with P99 spikes), memory allocation and resident growth, GIL contention between tokenize/detokenize/HTTP handling, serialization and IPC cost — as input for benchmark design and attribution; also record the scheduler's time share spent in the radix cache for v2 evaluation
- [ ] **BENCH-02**: A Rust load generator: open-loop, supports mid-stream cancellation, records TTFT, P99 and RPS
- [ ] **BENCH-03**: Scenario 1: 128 concurrent agents with random cancellations, P99 TTFT, Python vs Rust frontend
- [ ] **BENCH-04**: Scenario 2: 32-token short-prompt saturation, RPS-vs-latency curve, Python vs Rust frontend
- [ ] **BENCH-05**: Scenario 3: frontend cold-start time and frontend memory; end-to-end startup reported separately
- [ ] **BENCH-06**: Standard inference throughput shows no regression versus the Python frontend
- [ ] **BENCH-07**: The Python frontend is reported at both its default and its best `--num-tokenizer` setting; runs alternate A/B, results carry confidence intervals and a reproducible run manifest
- [ ] **BENCH-08**: Every scenario report shows frontend memory usage and Python GC pause counts alongside TTFT/P99/RPS, so P99 spikes can be compared against GC pauses

## v2 Requirements

Deferred to a future release. Tracked but not in the current roadmap.

### Radix Cache

- **RADIX-01**: Rust radix cache proven behaviorally equivalent to the Python `RadixPrefixCache` by differential tests — only if BENCH-01 shows radix time is a meaningful share of scheduler time
- **RADIX-02**: Replace the scheduler's radix cache with the Rust version via PyO3, measured as a separate experiment

### Extensions

- **EXT-01**: Stop strings, correct `finish_reason` and `usage`
- **EXT-02**: `/v1/completions`, `/v1/tokenize`, `/v1/detokenize`
- **EXT-03**: Frontend shadow prefix index (prefix hit-rate metrics)
- **EXT-04**: Constrained decoding (regex / JSON schema)

## Out of Scope

Explicitly excluded. Documented to prevent scope creep.

| Feature | Reason |
|---------|--------|
| Changes to GPU kernels, weight loading, batching loop | This project only replaces the frontend |
| Changes to the vendored Python frontend | It is the frozen baseline; unavoidable changes go in `UPSTREAM.md` |
| Multi-GPU (TP>1) | Rust only talks to rank 0; one GPU is enough to prove the frontend's effect |
| n>1, logprobs | The backend cannot do them; never fake them |
| Running the backend on a Mac | Upstream backend supports Linux/CUDA only |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| BASE-01 | Phase 1 | Complete |
| BASE-02 | Phase 1 | Complete |
| BASE-03 | Phase 1 | Complete |
| WIRE-01 | Phase 1 | Complete |
| WIRE-02 | Phase 1 | Complete |
| BENCH-01 | Phase 2 | Complete |
| WIRE-03 | Phase 3 | Complete |
| MOCK-01 | Phase 3 | Complete |
| TOK-01 | Phase 4 | Complete |
| TOK-02 | Phase 4 | Complete |
| TOK-03 | Phase 4 | Complete |
| TOK-04 | Phase 4 | Complete |
| LIFE-01 | Phase 5 | Pending |
| LIFE-02 | Phase 5 | Pending |
| LIFE-03 | Phase 5 | Pending |
| LIFE-04 | Phase 5 | Pending |
| LIFE-05 | Phase 5 | Pending |
| API-01 | Phase 5 | Pending |
| API-02 | Phase 5 | Pending |
| PAR-01 | Phase 6 | Pending |
| PAR-02 | Phase 6 | Pending |
| BENCH-02 | Phase 7 | Pending |
| BENCH-03 | Phase 7 | Pending |
| BENCH-04 | Phase 7 | Pending |
| BENCH-05 | Phase 7 | Pending |
| BENCH-06 | Phase 7 | Pending |
| BENCH-07 | Phase 7 | Pending |
| BENCH-08 | Phase 7 | Pending |

**Coverage:**
- v1 requirements: 28 total
- Mapped to phases: 28
- Unmapped: 0

---
*Requirements defined: 2026-10-02*
*Last updated: 2026-10-02 after roadmap creation (traceability mapped)*
