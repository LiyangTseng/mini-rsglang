# Project Research Summary

**Project:** mini-rsglang
**Domain:** Rust host-side "front half" of an LLM serving stack (HTTP/SSE ingress, request-lifecycle FSM, HF tokenization and detokenization, radix prefix trie) driving an unmodified Python/CUDA mini-sglang scheduler over ZMQ + MessagePack
**Researched:** 2026-10-02
**Confidence:** HIGH for the upstream boundary (all four researchers read mini-sglang source at `9a91cfa` directly). MEDIUM for tokenizer and template parity details, ZMQ crate choice, and whether benchmark wins will show up.

## Executive Summary

mini-rsglang replaces three upstream Python processes: the FastAPI `api_server`, the tokenizer worker, and the detokenizer worker. A single Rust process takes their place and talks directly to the unmodified scheduler (rank 0). All four researchers, working independently, found the same fixed boundary. It is **pyzmq PUSH/PULL over `ipc:///tmp/minisgl_{0,1}{suffix}`**, carrying **msgpack maps tagged with `"__type__"`** that the scheduler decodes with `cls(**kwargs)`. The frontend-to-scheduler message types are `UserMsg`, `AbortBackendMsg`, `ExitMsg` and `BatchBackendMsg`. The scheduler sends back `DetokenizeMsg` and `BatchTokenizerMsg`, which carry raw token ids rather than text. One extra or misnamed key kills the scheduler loop. This shape is close to how SGLang's Rust model gateway and vLLM V1 are built: tokenization and detokenization sit in the front, and the KV prefix cache stays beside the GPU allocator.

The recommended approach is a tokio/axum Rust workspace with a few fixed design choices. A hand-pinned msgpack codec is locked to golden fixtures produced by upstream's own encoder. One actor owns the ZMQ sockets and routes replies by uid. Each request gets its own task, which owns the request's FSM state and its incremental decoder. The HF `tokenizers` crate is pinned to `=0.22.2` (the version the baseline's Python environment resolves), and chat templates are rendered with `minijinja` plus the `pycompat` extensions. Upstream's `DetokenizeManager` is ported verbatim. A **small Python launcher shim in this repo** (not an upstream patch) starts the scheduler with a fixed socket suffix and prints a readiness handshake: `max_seq_len`, `eos_token_id`, `page_size`, `num_pages` and `max_running_req`. A same-wire mock scheduler means everything except GPU parity and the benchmarks can be built on the Mac.

The main risks fall into three groups:

1. **Scope honesty.** The Rust radix trie cannot replace the scheduler's KV radix cache over the wire. Benchmark wins must not be credited to it. The "lock-free channel" in PROJECT.md can only describe queues inside the Rust process.
2. **Parity.** The risky parts are chat templates, double-BOS behaviour, `clean_up_tokenization_spaces`, UTF-8 slicing between Python code points and Rust bytes, and greedy outputs drifting when batches differ in composition.
3. **Benchmark validity.** mini-sglang already uses separate processes and overlap scheduling, so host overhead may not be the bottleneck. A Python benchmark client also skews results.

Mitigations: a parity ladder with hard gates; an early profiling spike of the baseline on the GPU box before investing in the lifecycle and radix work; a custom open-loop Rust load generator; a baseline tuned with `--num-tokenizer`; and cold-start and RAM claims stated as front-half-only.

## Key Findings

### Recommended Stack

Rust 1.99 (edition 2024), pinned with `rust-toolchain.toml`. The runtime is tokio 1.53 and the HTTP layer is axum 0.8.9. Use axum's `Sse` for the chat endpoint. `/generate` needs raw `Body::from_stream`, because upstream frames it with a single `\n`. At the boundary, the primary choice is `zmq` 0.10 (libzmq bindings) running on two dedicated OS threads and bridged with `tokio::sync::mpsc`. The pure-Rust `zeromq` 0.6 crate is the alternative, kept behind a `Transport` trait and spiked in Phase 1. Python-side pins: Python 3.12, upstream pinned by SHA as a submodule, plus a lockfile. Upstream supports Linux only.

**Core technologies:**
- **`tokenizers =0.22.2`** (`onig`, `esaxx_fast`, no `http`): the same core version as the baseline's Python wheel, which removes a class of id-drift bugs.
- **`minijinja` + `minijinja-contrib` 2.24 (`pycompat`)**: renders HF chat templates with `trim_blocks`/`lstrip_blocks` plus `raise_exception`, `strftime_now` and `tojson` shims. Prior art is SMG's `llm-tokenizer`.
- **msgpack codec**: `rmp-serde` `to_vec_named` + `serde_bytes`, or a hand-rolled `rmp`/`rmpv` codec as ARCHITECTURE prefers, since there are only 7 types. Never use `to_vec`, which writes arrays. Tensor buffers must be msgpack `bin` int32 LE with `dtype:"torch.int32"`.
- **`tokio-util` `CancellationToken`, `slab`, `rustc-hash`, `hdrhistogram`, `reqwest`, `sysinfo`, `nix`**: cancellation, the radix arena, the FSM maps, the load generator, PSS sampling, and process-group cleanup of the shim.
- **`metrics` + Prometheus exporter, `tracing`; `criterion`, `proptest`, `insta`, `cargo-nextest`**: observability and testing.
- **`mimalloc`**: use the same allocator on Mac and Linux so the allocator is not a hidden variable.

### Expected Features

"Parity" means matching what mini-sglang actually does, which is much less than the OpenAI spec. Upstream behaviour:
- Endpoints are `/v1/chat/completions` (accepts `messages` or a raw `prompt`), `/generate`, `/v1/models` and `/v1`.
- Defaults are `max_tokens=16` and `temperature=1.0`, so sampling is stochastic unless the request says otherwise.
- `stop`, `n` and penalties are ignored. `finish_reason` is always `"stop"`, and `usage` is zeroed.
- Streaming sends one SSE event per token, even when the token adds no text.

**Must have (table stakes):**
- A wire-exact codec and the launcher shim with its readiness handshake.
- A Rust mock scheduler plus a Python contract oracle that uses upstream's `minisgl.message`.
- Parity for tokenization, chat-template rendering and detokenization, with Qwen3-0.6B as the canonical model.
- A lifecycle FSM with disconnect cancellation (streaming and non-streaming) that sends the abort immediately in one hop.
- `/v1/chat/completions` (streaming and non-streaming), `/v1/models`, `/v1`, `/health` and `/health/ready`.
- Prompt-length validation against `max_seq_len` and a backend-silence watchdog. Upstream drops overlong prompts silently and the request hangs forever.
- A radix trie library proven equivalent to upstream by differential testing.
- A GPU parity harness, a three-scenario benchmark harness, and a minimal `/metrics`.

**Should have (differentiators, v1.x):**
- Coalesced `BatchBackendMsg` sends.
- Stop strings, implemented by reusing the abort path.
- Correct `finish_reason` and `usage`.
- `/v1/completions`, `/v1/tokenize` and `/v1/detokenize`.
- A shadow radix index for prefix-hit metrics and opt-in prefix-aware admission.
- Front-side admission control, off by default and labelled when used.
- Shim probes for backend-side abort latency.

**Defer (v2+):**
- Constrained decoding.
- Swapping the backend radix cache.
- Tool calling and reasoning parsers.
- Multi-backend routing.
- `DecodeStream` or SSE-coalescing fast paths.
- HTTP/2, gRPC or WebSocket ingress.
- `n>1` and logprobs. The backend cannot do these, so reject or ignore them and never fake them.

### Architecture Approach

A Cargo workspace of small crates. Each request makes 2 IPC hops instead of upstream's 4, and there are no Python front processes importing torch. That structural change is the honest source of the expected host-overhead wins. The scheduler loop is unchanged and remains the throughput ceiling.

**Major components:**
1. **rsg-wire**: the exact encode/decode for the 7 upstream message types, with no I/O. Golden fixtures come from upstream's own `serialize_type`.
2. **rsg-transport (BackendClient actor)**: a single owner of PUSH, PULL and the uid-to-sender routing map. It gives a single ordered writer, so an abort can never overtake its submit. It inserts each route before sending, drops and counts unknown or late uids, and never blocks on a slow client: it uses `try_send` and falls back to a local buffer.
3. **rsg-tokenizer**: encodes by rendering the template and then calling `encode(add_special_tokens=true)` on a bounded blocking pool. Its `IncrementalDecoder` is a port of the surr/read-offset scheme with the U+FFFD hold-back, `find_printable_text`/CJK handling, and the EOS drop, and it runs inline in each request's task.
4. **rsg-lifecycle**: the FSM `Received → Tokenizing → [Queued] → Submitted(opaque queued/prefill) → Decoding → Finished | Cancelled | Failed`. It owns uid allocation, validation, and an RAII `CancelGuard` triggered when the SSE body is dropped.
5. **rsg-radix**: a slab-arena trie with no async or I/O dependencies and an injectable logical clock. It must be equivalent to `RadixPrefixCache` for `page_size` 1 and greater than 1.
6. **rsg-server, rsg-mock-scheduler, rsg-bench, python/rsglang_shim, python/conformance**: the HTTP server, the mock scheduler, the load generator, the launcher shim, and the conformance scripts.

**Radix cache options (decision needed):**
- **A. Equivalence-tested library.** Required, and it satisfies the requirement as written.
- **B. Advisory shadow index.** Recommended as a v1.x addition, with prefix-aware ordering off by default.
- **C. PyO3 plug-in registered through upstream's `SUPPORTED_CACHE_MANAGER`.** This is the only way the Rust radix serves real traffic without patching upstream. It is a stretch goal and changes the project boundary.
- **D. Rust owns slots across the wire.** Rejected: the wire has no slot fields.

### Critical Pitfalls

1. **Treating upstream as a stable API, and msgpack that only looks right.** Pin upstream by SHA. Regenerate the golden fixtures in CI. Prove every Rust message decodes through upstream's real decoder. Watch scheduler liveness so a dead scheduler shows up as a 503 rather than a hang.
2. **Tokenizer and template parity drift.** transformers adds load-time overrides on top of `tokenizer.json`, and Llama-style templates produce a double BOS that must be replicated, not fixed. Build an exact-id differential corpus on the Mac. Consider exporting the *effective* tokenizer from Python (PITFALLS) instead of loading the raw `tokenizer.json`. Start with Qwen3-0.6B.
3. **Detokenizer UTF-8 bugs.** Python slices by code point and Rust by byte, which panics on CJK and emoji. Track offsets in token space. The gate is concatenated final text plus token ids, not chunk boundaries. Fuzz against upstream's `DetokenizeManager`.
4. **Cancellation races.** There is no abort ack, and late tokens arrive after an abort. Use monotonic u64 uids that are never reused, starting from a random or time-based base so a restart cannot collide. Keep a single ordered writer, never send a `UserMsg` after a cancel, and make terminal transitions idempotent. There is also a possible upstream double free when an abort lands during an in-flight prefill under overlap scheduling. This came from code reading and has not been reproduced. Provide an abort policy switch.
5. **Wins that don't materialize, or invalid benchmarks.** Profile the baseline before the lifecycle and radix work. Use an open-loop Rust load generator with validated headroom and the client on pinned cores. Use one TTFT definition for both stacks. Run the baseline at its best `--num-tokenizer` setting. Restart the backend between runs, interleave A/B runs, and report confidence intervals. Never credit radix, and keep cold-start and RAM claims to the front half only.

Also important: the mock must be adversarial (late tokens after abort, silent drops, batched replies). Greedy output is only deterministic when batch composition is fixed. The ipc socket path limit is 104 bytes on macOS. All performance claims must be measured on Linux.

## Implications for Roadmap

### Phase 1: Boundary and Baseline Spike
**Rationale:** Every other component sits behind the wire contract, and one schema error crashes the scheduler. PROJECT.md already asks for the IPC boundary to be de-risked first.
**Delivers:**
- Upstream submodule pinned by SHA, plus the lockfile.
- Rust workspace skeleton.
- `rsg-wire` codec checked against golden fixtures.
- The Python launcher shim and its handshake.
- A `Transport` trait, with `zmq` vs `zeromq` interop spiked against pyzmq.
- A Python contract oracle running on macOS.
- Fixed definitions for TTFT, the load generator, and the baseline config matrix.
- A **GPU profiling spike of the baseline** (`py-spy --subprocesses`) that confirms front-half saturation.
**Addresses:** the codec, the shim, and the readiness signal.
**Avoids:** Pitfalls 1, 2, 9, 10, 11 (definitions), 12.

### Phase 2: Transport Actor and Mock Scheduler
**Rationale:** Needs the codec. It gives a GPU-free backend for everything that follows.
**Delivers:**
- The BackendClient actor: single writer, routing table, unknown-uid drop, non-blocking fan-out.
- `rsg-mock-scheduler` with these behaviours: FIFO with head-of-line blocking, `max_running_req`, batched replies, late tokens after abort, silent overlong drops, `max_tokens` clamping, EOS with `finished`, configurable timing, and fault injection.
**Uses:** `zmq`/`zeromq` behind `Transport`, `tokio::sync::mpsc`.
**Implements:** rsg-transport, rsg-mock-scheduler.
**Avoids:** Pitfalls 6 and 9.

### Phase 3: Tokenizer, Chat Template and Detokenizer Parity (can run in parallel with Phases 1-2)
**Rationale:** Independent of the request path, and it carries the largest parity risk. It can be proven entirely on the Mac.
**Delivers:**
- Encode via template rendering then `encode(true)`.
- `IncrementalDecoder` (verbatim port).
- Differential corpora against transformers 4.57.3 and upstream `DetokenizeManager`: Qwen3-0.6B first, then one Llama-3.x model.
- Criterion benchmarks.
**Uses:** `tokenizers =0.22.2`, `minijinja` with `pycompat`, `hf-hub`.
**Implements:** rsg-tokenizer.
**Avoids:** Pitfalls 3, 4, 16 (parity ladder levels 1-2).

### Phase 4: Lifecycle FSM, Cancellation and HTTP Ingress
**Rationale:** Needs Phases 2 and 3. This is the Scenario 1 machinery.
**Delivers:**
- The FSM, uid allocation, validation against the handshake's `max_seq_len`, and a watchdog.
- The RAII `CancelGuard` and SSE keep-alive.
- Routes with byte-identical framing (`/v1/chat/completions`, `/generate`, `/v1/models`, `/v1`, `/health*`) and a minimal `/metrics`.
- A cancellation stress test on the mock, ending with zero leaked state.
- HTTP snapshot tests against recorded baseline responses.
**Uses:** axum, tokio-util, metrics.
**Implements:** rsg-lifecycle, rsg-server.
**Avoids:** Pitfalls 5, 6, 13, 14, 15.

### Phase 5: Radix Trie Library and Differential Harness (can run in parallel from Phase 1)
**Rationale:** A pure data structure with no request-path dependency. The scope statement must go into REQUIREMENTS before work starts.
**Delivers:**
- `rsg-radix`.
- A Python replay harness (stubbed `minisgl.kernel`, logical clock) covering `page_size` 1 and greater than 1, the split/timestamp semantics from #124, ties, and over-eviction.
- Optionally, the shadow index (option B) behind a flag.
**Uses:** `slab`, `proptest`.
**Implements:** rsg-radix.
**Avoids:** Pitfall 7.

### Phase 6: Mac A/B Rehearsal
**Rationale:** Gives cheap, early host-overhead numbers and validates the harness before any GPU time is spent.
**Delivers:**
- Upstream's API server and tokenizer worker running against the same mock, compared with the Rust front.
- A first version of `rsg-bench`: open-loop and closed-loop modes, an SSE parser, hdrhistogram, and a headroom check of the client itself.
**Note:** this is a rehearsal only. Do not publish numbers from it.
**Avoids:** Pitfalls 11 and 12.

### Phase 7: GPU End-to-End Parity
**Rationale:** First contact with the real scheduler. It gates the benchmarks.
**Delivers:**
- Parity ladder levels 3-5: protocol-exact; sequential greedy exact on at least 100 prompts with a fresh backend for each run; and the reported concurrent match rate.
- A reproduction attempt for the abort race.
- Checks of `check_integrity` after the stress run.
**Avoids:** Pitfalls 5 and 8.

### Phase 8: Benchmarks (three scenarios plus throughput parity)
**Rationale:** Needs Phase 7. The methodology was fixed back in Phase 1.
**Delivers:**
- Scenario 1: 128 closed-loop agents with seeded cancellations, P99 TTFT for non-cancelled requests, and abort-to-quiescence time.
- Scenario 2: open-loop Poisson load with 32-token prompts, reporting the RPS-vs-latency curve, cross-checked with `vllm bench serve` and `sglang.benchmark.serving`.
- Scenario 3: cold start broken into phases, plus PSS for the front-half process group.
- Standard throughput using `bench_simple.py`.
- ABAB runs, confidence intervals, and run manifests.
**Avoids:** Pitfalls 10 and 11.

### Phase 9 (stretch): Radix plug-in via `SUPPORTED_CACHE_MANAGER` (option C)
**Rationale:** Only if the user explicitly widens the boundary to "msgpack plus an in-process PyO3 plug-in".
**Delivers:** A PyO3 build of `rsg-radix` wrapped in a Python `BasePrefixCache` adapter, registered from the shim and started with `cache_type="rs-radix"`.

### Phase Ordering Rationale

- The codec comes first because every component and both backends depend on it.
- The tokenizer and radix tracks are pure and Mac-provable, so they run in parallel with the boundary work.
- The baseline profiling spike sits in Phase 1 so claims can be rescoped *before* heavy investment (Pitfall 10).
- The parity gate comes before the benchmarks: a faster front half with different output proves nothing.
- The research files disagree slightly on the mock. PITFALLS favours a Python mock as the contract mock; ARCHITECTURE and FEATURES favour a Rust mock plus a Python oracle. Recommendation: build both. The Rust mock carries tests and load, and the Python oracle is a CI gate for byte compatibility.

### Research Flags

Phases likely needing deeper research during planning:
- **Phase 1:** `zmq` vs `zeromq` ipc interop with pyzmq. Bind/connect roles under the shim: with a standalone `SchedulerConfig` the scheduler binds both sockets, but with `ServerArgs` and `num_tokenizer=0` Rust must bind `_1`. Whether the shim can be imported on macOS. Setup of the GPU profiling spike.
- **Phase 3:** minijinja coverage of the Qwen3 and Llama-3 templates; the effective-tokenizer export; whether `clean_up_tokenization_spaces` is set for Llama-3.x.
- **Phase 4:** how quickly hyper/axum notices a client disconnect while a body is pending (hyper #2787).
- **Phase 7:** the upstream abort-during-prefill double free.
- **Phase 9:** the PyO3 adapter for `BasePrefixCache`.

Phases with standard patterns (skip research-phase):
- **Phase 2:** the single-owner actor pattern is well established.
- **Phase 5:** the algorithm is fully specified from upstream source.
- **Phase 8:** the methodology is already specified in PITFALLS.

### Open Decisions for the User (surface before requirements)

1. **Radix scope:** A only, A plus B, or A plus B plus stretch C. Recommendation: A in v1, B in v1.x, C only by explicit opt-in. Whatever the choice, REQUIREMENTS must state that radix is not credited with any benchmark win.
2. **Baseline fairness (`--num-tokenizer`):** report against both the default (0) and the best swept value (0, 1, 2 or 4)? Recommendation: report both, with the best value as the headline comparison.
3. **Parity model set:** Qwen3-0.6B only, or Qwen3-0.6B plus one Llama-3.x model (gated repo, exercises BOS and cleanup)? Recommendation: both, with Qwen3 as the hard gate.
4. **Cold-start definition:** front-half-only readiness (backend warm or mocked) vs end-to-end. Recommendation: the claim covers front-half readiness only. Report end-to-end separately, and expect it to be dominated by weight loading and CUDA graph capture.
5. **Abort policy:** `immediate` (default) vs `defer-until-first-token`. This depends on whether the upstream double free reproduces. If a delay is needed, apply the same policy to the baseline comparison.
6. Also to decide:
   - Hang behaviour: return a 400 on overlong prompts instead of hanging (a deliberate, documented divergence).
   - Whether front-side admission control is ever used in headline runs. Recommendation: no.
   - Whether to stay at TP=1. Recommendation: yes.

### Corrections Recommended for PROJECT.md

- **"MessagePack byte-stream boundary over a lock-free channel"** should become: "msgpack (`__type__`-tagged maps) over pyzmq-compatible ZMQ PUSH/PULL `ipc://` sockets, as fixed by upstream. Lock-free channels are used *inside* the Rust process." Shared memory or any other transport would require patching upstream.
- **"Upstream used unmodified"** should be clarified as "no patches to upstream files". A launcher shim in this repo that imports upstream as a library is allowed. Pin upstream by **git SHA**, not by pip version (the package version is fixed at `0.1.0`).
- **"Rust radix cache trie … behaviorally equivalent"** should be reworded as a standalone, differentially proven library. It does not serve KV traffic in v1, and the scheduler's Python radix cache remains authoritative.
- **Request-lifecycle FSM states:** "prefill" cannot be observed from the front. Map it to `Submitted` (queued or prefilling in the backend). Add `Tokenizing` and `Failed`.
- **Tokenization requirement:** add chat-template rendering and incremental **detokenization** parity, which the current wording omits.
- **Benchmark claims:** cold-start and under-500 MB RAM apply to the front-half process group only. Wins are host-overhead effects, so state the regime they were measured in: a small model, short prompts, high concurrency.
- **Mac CUDA note:** upstream supports Linux only, so even a CPU-only scheduler cannot run on macOS.

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH / MEDIUM | Crate versions come from the registries and the upstream deps from source. The ZMQ crate choice and template parity still need the Phase 1 and Phase 3 spikes. |
| Features | HIGH | The upstream API and scheduler behaviours were read from source. Competitor comparisons are MEDIUM. |
| Architecture | HIGH | The boundary and radix ownership were verified in source. The macOS import chain and the abort race are unverified. |
| Pitfalls | HIGH / MEDIUM | Upstream-derived pitfalls are HIGH. Benchmark methodology is established practice (MEDIUM). The double free and the Llama cleanup flag are LOW. |

**Overall confidence:** HIGH on *what* to build. MEDIUM on whether the projected wins will materialize, which is settled by the Phase 1 spike.

### Gaps to Address

- **Whether host overhead is actually the bottleneck:** settle with the Phase 1 GPU profiling spike before the Phase 4 and 5 investment.
- **Bind/connect topology:** the researchers differ on whether Rust binds `_1`, because it depends on how the shim builds its config. Pin it in Phase 1, and make bind vs connect configurable per endpoint.
- **Raw `tokenizer.json` vs a Python-exported effective tokenizer:** decide in Phase 3 based on the corpus results.
- **Whether `minisgl.message`, `minisgl.utils` and `RadixPrefixCache` import on macOS** (with a stubbed `minisgl.kernel`): verify in Phase 1.
- **Disconnect-detection latency while a request is queued:** verify with a test in Phase 4.
- **Upstream abort double free:** try to reproduce in Phase 7, document it, and do not patch upstream.
- **Radix eviction ties:** either replicate the heapq and DFS order exactly or define equivalence modulo ties. Decide before coding Phase 5.
- **Mock language:** PITFALLS favours a Python mock as the contract mock; ARCHITECTURE and FEATURES favour a Rust mock plus a Python oracle. Recommendation: build both.

## Sources

### Primary (HIGH confidence)
- `sgl-project/mini-sglang` @ `9a91cfa` (2026-05-17), read directly:
  - `message/*`, `utils/mp.py`
  - `server/{launch,args,api_server}.py`
  - `tokenizer/*`, `scheduler/*`, `kvcache/radix_cache.py`
  - `tests/core/test_scheduler.py`, `benchmark/*`
  - `pyproject.toml`, and the git history (#16 cancellation, #124 radix timestamps)
- crates.io, PyPI and the Rust stable manifest for all versions; transformers 4.57.3 `setup.py`.
- Qwen3-0.6B `tokenizer_config.json`.
- SGLang `sgl-model-gateway/Cargo.toml` and `benchmark/serving.py`.

### Secondary (MEDIUM confidence)
- TGI `radix.rs`, the vLLM architecture docs, and the zmq.rs README.
- Thinking Machines on batch non-invariance; coordinated-omission literature.
- rmp-serde docs and issues; HF `DecodeStream` docs; minijinja docs.

### Tertiary (LOW confidence)
- SGLang Model Gateway docs (summarized).
- hyper #2787 on disconnect detection.
- Llama-3.x `clean_up_tokenization_spaces`.
- The upstream abort double free (from code reading only).

---
*Research completed: 2026-10-02*
*Ready for roadmap: yes*
