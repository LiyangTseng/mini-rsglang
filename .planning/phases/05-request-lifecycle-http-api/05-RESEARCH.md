# Phase 5: Request Lifecycle & HTTP API - Research

**Researched:** 2026-10-06
**Domain:** Rust async HTTP server (axum/hyper) fronting a request-lifecycle FSM, consuming an existing ZMQ transport and tokenizer, with OpenAI-compatible output parity against a frozen Python frontend
**Confidence:** MEDIUM — the HTTP/FSM architecture itself is HIGH confidence (CLAUDE.md pins are crates.io-verified and consistent with prior art); the two upstream biggest unknowns are genuinely open and tracked below, not assumed away: (1) exact hyper disconnect-detection latency while a request is queued, and (2) whether Phase 3's per-uid dispatcher and Phase 4's tokenizer crate will exist in code by the time this phase executes.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**Abort timing (LIFE-05)**
- **D-01:** Abort timing (immediate vs. deferred-until-first-token) is a **server-wide CLI flag** (`--abort-timing immediate|deferred`), not a per-request field. Same convention as `rsg-server`'s and `mock-scheduler`'s existing CLI-configured behavior. Default is `immediate`, per LIFE-05. Reversibility: costly — Phase 6's fairness comparison for the suspected abort-during-prefill bug depends on this being one global mode per benchmark run.

**API output-parity verification (API-01)**
- **D-02:** Parity for `/v1/chat/completions` (streaming + non-streaming), `/generate`, `/v1/models`, `/v1` is proven with **golden fixtures captured from a live Python-frontend run** (against the mock or a stub), diffed byte-for-byte against Rust's output — the same golden-fixture pattern Phase 1 used for the wire codec (`scripts/gen_wire_fixtures.py`) and Phase 4 is using for tokenizer/detokenizer output (`scripts/gen_tokenizer_fixtures.py`). Not a hand-port from reading `api_server.py` alone. Reversibility: reversible.

**Non-streaming disconnect handling (LIFE-02)**
- **D-03:** The gap between LIFE-02's "abort right away" and hyper's actual behavior (it only notices a dropped connection on its *next write*, so a non-streaming/buffered response has no write until fully done) is **accepted as a known limitation and documented**, not patched with extra liveness-probing plumbing. Matches CLAUDE.md's Pattern A (`AbortGuard` + `Drop`) as designed; CLAUDE.md already flags this as "verify with a disconnect test" (MEDIUM confidence) — that test is how Phase 5 proves the actual bound on this gap, it does not try to eliminate the gap.

**128-agent stress test scope (LIFE-03)**
- **D-04:** Phase 5 builds a **minimal, Phase-5-only stress test**, not a shared foundation for Phase 7's benchmark harness. It uses `mock-scheduler`'s existing fixed-delay flags (Phase 3 D-10) plus randomized abort-after-N-tokens logic living in the *test driver* itself (not added to `mock-scheduler`), just enough to prove LIFE-03's no-leaked-requests / no-stuck-connections / exactly-one-terminal-state criterion. Phase 7 builds its own instrumented load generator later, unconstrained by this throwaway tool. Reversibility: reversible.

### Claude's Discretion
- Exact FSM implementation shape (actor task owning `FxHashMap<u64, ReqState>` per CLAUDE.md's stack pattern, vs. an alternative) — CLAUDE.md already names the pattern; internal structuring is Claude's call.
- `/metrics` label cardinality and TTFT histogram bucket boundaries, beyond API-02's required counters (request count, cancellation count, TTFT histogram).
- The exact backend-unresponsive timeout duration/config surface for LIFE-04's "times out with an error instead of hanging" criterion.
- Whether the overlong-prompt 400 rejection (LIFE-04) happens before or after invoking Phase 4's tokenizer, as long as it's immediate and uses the readiness-handshake `max_seq_len` (already available per BASE-03).
- Exact SSE keep-alive interval, if CLAUDE.md's "fast abort of queued requests" keep-alive note is implemented in this phase at all — not raised as a separate gray area because no scenario in this phase's success criteria explicitly requires fast abort of a *queued* (not yet dispatched) request; revisit only if testing shows queued requests aren't aborted promptly enough under D-03's accepted-limitation stance.

### Deferred Ideas (OUT OF SCOPE)
None — discussion stayed within phase scope.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| LIFE-01 | Each request moves through received → tokenizing → submitted → decoding → finished / cancelled / failed, exactly one terminal state | See Architecture Patterns §FSM Actor Pattern; `ReqState`/state enum sketch in Code Examples |
| LIFE-02 | On client disconnect (streaming or non-streaming), Rust sends an abort to the backend immediately; tokens arriving after abort are dropped and counted | See Pattern A (`AbortGuard`+`Drop`) in Code Examples; D-03's accepted non-streaming gap; Phase 3's per-uid dropped-token counter (D-06) is what "counted" wires into |
| LIFE-03 | 128 concurrent requests + random cancellations end with no leaked requests, no stuck connections | See Validation Architecture §Stress Test; `mock-scheduler`'s current fixed-delay flags (verified in code) |
| LIFE-04 | Overlong prompt → immediate 400; backend-unresponsive → timeout with error, not a hang | See Common Pitfalls §Overlong Prompt / §Backend Timeout; `Handshake.max_seq_len` field (verified) |
| LIFE-05 | Abort timing configurable: immediate (default) or deferred until first token | See D-01 above; CLI flag convention in Code Examples |
| API-01 | `/v1/chat/completions` (stream+non-stream), `/generate`, `/v1/models`, `/v1` byte-parity with Python frontend incl. SSE framing | See §Upstream API Contract (verbatim-quoted from `api_server.py`) — this is the hard parity oracle |
| API-02 | `/health`, `/health/ready`, minimal `/metrics` (request count, cancellation count, TTFT histogram) | See Standard Stack (`metrics` + `metrics-exporter-prometheus`); these routes have **no upstream equivalent** — free design space |
</phase_requirements>

## Project Constraints (from CLAUDE.md)

- **Architecture**: Rust owns ingress, lifecycle FSM, tokenization, detokenization; Python/CUDA owns scheduler, weights, batching, kernels, KV cache. Phase 5 is squarely in the Rust-owned slice.
- **Fair comparison**: backend changes apply to both frontends; the Python frontend stays frozen — Phase 5 must never modify `vendor/mini-sglang`.
- **IPC**: ZMQ + MessagePack wire format must match byte-for-byte; an extra key crashes the scheduler. Phase 5 *consumes* `rsg-wire`'s existing `BackendMsg`/`TokenizerMsg` types unmodified — it does not add fields.
- **Environment**: GPU-free dev on a Mac — Phase 5 runs entirely against `mock-scheduler`, never the real backend.
- **License**: mini-sglang is MIT; no vendored-file edits in this phase.
- **Required crate pins** (§Recommended Stack, already crates.io-verified in CLAUDE.md and re-verified this session): `axum` 0.8.9, `hyper` 1.11.1 (transitive), `tower-http` 0.7.1, `tokio-util` 0.7.19, `metrics` 0.24.6, `metrics-exporter-prometheus` 0.18.3.
- **Forbidden patterns** (§What NOT to Use): `axum::response::Sse` for `/generate` (must use raw `Body::from_stream`, single-`\n` framing); `tower_http::timeout` on streaming routes; `tokenizers::DecodeStream` (Phase 4 concern, not this phase, but the FSM must not introduce it either); extra/renamed wire-message fields.
- **GSD Workflow Enforcement**: file-changing work in this repo must go through a GSD command (`/gsd-execute-phase` etc.) — this research output feeds the planner, which the executor will follow.

## Summary

Phase 5 builds the first piece of the Rust frontend that a human can actually curl: an axum HTTP server that accepts `/generate`, `/v1/chat/completions`, `/v1/models`, `/v1`, plus new `/health`, `/health/ready`, `/metrics` routes, drives a per-request lifecycle FSM, and talks to `mock-scheduler` over the existing ZMQ transport. The hard parity contract is `vendor/mini-sglang/python/minisgl/server/api_server.py`, read directly this session (verbatim quotes below) — it reveals several byte-level details that are easy to get wrong from memory: `/generate`'s SSE-like stream uses a **single** `\n` per chunk (`f"data: {ack.incremental_output}\n"`), while `/v1/chat/completions`'s streaming uses **proper** double-`\n\n` SSE framing with an explicit final `finish_reason: "stop"` chunk before `data: [DONE]\n\n`. The non-streaming chat-completions response hardcodes `usage` to all-zero and `finish_reason` to the literal string `"stop"` always — there is no token counting or other finish reason upstream, so Rust's golden-fixture diff must byte-match these fixed values, not compute them. `/v1/models`'s `created` field is `int(time.time())`, a wall-clock value that cannot ever byte-match between two separate runs — the fixture-diff tooling (D-02) must treat it as a normalized/excluded field, the same way Phase 1's fixture-check excludes the generator-version block.

Two upstream code facts materially change the design posture versus a naive port: upstream's own cancellation detection is **polling-based** (`await request.is_disconnected()` checked once per yielded chunk) with a hardcoded **100 ms sleep before sending the abort** (`await asyncio.sleep(0.1)`) — this is *not* "immediate" by any literal reading, and Rust's drop-based `AbortGuard` (CLAUDE.md Pattern A) is a deliberate architectural improvement, not a parity requirement; parity is scoped to response bytes, not abort latency. Second, `/health`, `/health/ready`, `/metrics` have **zero upstream equivalent** — these are new Rust-only routes with no parity oracle, so their response shape is this phase's own design decision (API-02 only fixes the three required Prometheus series).

The codebase state as of this research session is less mature than 05-CONTEXT.md's dependency note implies: `mock-scheduler` is already a complete, working subprocess (handshake, echo-token engine, prefill/decode delays) — not just a "Wave-1 tracer" — but it does **not yet** implement Phase 3's D-09 misbehavior flags (`--misbehave-uids`, `--behavior`, `--batch-size`) that LIFE-02/LIFE-04 tests and the LIFE-03 stress test need to reproduce late-tokens-after-abort and dropped-overlong-prompt scenarios. Similarly, `crates/rsg-server/src/transport.rs` has only the raw split-socket halves (`ZmqBackendTx`/`ZmqDetokRx`); the per-uid bounded-channel dispatcher (Phase 3 D-04/D-05/D-06) that the FSM is specified to register against does not exist in code yet. Per 05-CONTEXT.md's explicit instruction, this research treats the Phase 3/4 CONTEXT.md decisions as the authoritative contract to plan against — but the planner should sequence Phase 5's waves so that FSM/dispatcher-integration tasks are not scheduled before Phase 3's dispatcher code actually lands, or should treat this as a cross-phase blocking dependency to flag explicitly.

**Primary recommendation:** Build the HTTP layer as a thin axum router over a single FSM actor task (`tokio::sync::mpsc` inbox, `FxHashMap<u64, ReqState>`), with cancellation wired through `tokio_util::sync::CancellationToken` + a `Drop`-based `AbortGuard`; prove API-01 parity with a Python-frontend fixture-capture script (new, modeled on `scripts/gen_wire_fixtures.py`) with explicit field-normalization for non-deterministic fields (`created`, timing); build `/health`/`/health/ready`/`/metrics` as free-standing new routes with no parity constraint.

## Architectural Responsibility Map

This project has no browser/CDN/DB tiers in the conventional sense — map capabilities onto the project's own two-tier split (Rust frontend process vs. Python/CUDA backend process) plus the HTTP client as the external caller.

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| HTTP routing (`/generate`, `/v1/*`, `/health*`, `/metrics`) | Rust Frontend (ingress) | — | CLAUDE.md: "Rust owns ingress" |
| Request-lifecycle FSM (received→...→terminal) | Rust Frontend (lifecycle) | — | CLAUDE.md: "Rust owns... lifecycle FSM"; owns the actor task and `ReqState` table |
| SSE/chunked response framing | Rust Frontend (ingress) | — | Byte-identical to upstream's own FastAPI/Starlette layer; no backend involvement |
| Disconnect detection / cancellation propagation | Rust Frontend (ingress → lifecycle) | — | hyper notices the drop; FSM turns it into `AbortBackendMsg` |
| Token generation (the actual decode step) | Backend Process (scheduler/mock) | — | Out of scope; Phase 5 only talks to `mock-scheduler`'s echo engine over ZMQ |
| Tokenization / chat-template rendering | Rust Frontend (consumes Phase 4 crate) | — | Phase 5 calls into it, does not reimplement it |
| Detokenization (incremental decode) | Rust Frontend (consumes Phase 4 crate) | — | Same |
| ZMQ framing / per-uid routing | Rust Frontend (consumes Phase 3 dispatcher) | Process boundary | Phase 5 registers/deregisters against Phase 3's per-uid channel API; does not rebuild uid routing |
| Metrics aggregation & exposition | Rust Frontend (ingress) | — | New `/metrics` route; no backend involvement, `metrics` crate's global recorder |
| Health/readiness signaling | Rust Frontend (ingress) | Backend Process (indirectly, via handshake state) | `/health` = process alive; `/health/ready` plausibly reflects whether the backend handshake has completed |

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard | Conf. |
|---------|---------|---------|---------------|-------|
| `axum` | 0.8.9 | HTTP server/routing | Standard Rust web framework on hyper 1.x; built-in `axum::response::sse::{Sse, Event, KeepAlive}` for the one route that needs real SSE. Prior art: sglang's own Rust gateway (SMG) uses axum 0.8. [VERIFIED: crates.io registry — `max_stable_version 0.8.9`, checked 2026-10-06] | HIGH |
| `hyper` | 1.11.1 (transitive via axum) | HTTP/1.1 engine | Drops the response body when a client disconnects — that drop is the cancellation signal for Pattern A | HIGH (version is transitive, not independently pinned) |
| `tower-http` | 0.7.1 | Middleware (`trace`, request-id) | Do **not** add its `timeout` layer to streaming routes (CLAUDE.md §What NOT to Use). [VERIFIED: crates.io registry — `max_stable_version 0.7.1`, checked 2026-10-06] | HIGH |
| `tokio-util` | 0.7.19 | `CancellationToken`, `DropGuard` | Per-request cancellation propagation from the HTTP stream to the FSM. [VERIFIED: crates.io registry — `max_stable_version 0.7.19`, checked 2026-10-06]. `DropGuard` "cancels the wrapped token" on drop [CITED: docs.rs tokio_util::sync::DropGuard] | HIGH |
| `metrics` + `metrics-exporter-prometheus` | 0.24.6 / 0.18.3 | `/metrics` counters/histograms | Lightweight facade + Prometheus exporter — the pair SMG uses. [VERIFIED: crates.io registry, both checked 2026-10-06] | HIGH |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `tokio-stream` | 0.1.19 [VERIFIED: crates.io registry, checked 2026-10-06] | `ReceiverStream` | Turn a per-request `mpsc::Receiver<Chunk>` into the SSE/chunked body stream |
| `futures` | (workspace not yet pinned; `0.3.x` current) [VERIFIED: crates.io registry — `max_stable_version` resolves current, checked 2026-10-06] | `Stream` combinators | Needed by `tokio-stream`/`async-stream` interop; add to workspace if not already a transitive given |
| `async-stream` | 0.3.6 [VERIFIED: crates.io registry, checked 2026-10-06] | `stream!{}` macro | Optional — readable generator-style streams for `/generate`'s manual-framing body; hand-written `Stream` impls are also fine |
| `hdrhistogram` | 7.6.0 [VERIFIED: crates.io registry, checked 2026-10-06] | TTFT histogram if not using `metrics`'s own histogram type | Only if `metrics`'s built-in histogram granularity is insufficient for API-02's TTFT requirement; otherwise `metrics::histogram!` alone suffices |
| `serde_json` | (already workspace-pinned 1.0.151) | Request/response JSON | `OpenAICompletionRequest`/`GenerateRequest`/`ModelList` equivalents |
| `rustc-hash` (already workspace-pinned 2.1.3) | — | `FxHashMap<u64, ReqState>` | The FSM's request table, per CLAUDE.md's stack pattern |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `axum::response::Sse` for `/v1/chat/completions` | Hand-rolled `Body::from_stream` with manual `\n\n` framing | `Sse` already emits correct SSE (`\n\n`) and handles `KeepAlive`; only `/generate` needs the manual, non-spec single-`\n` path |
| Drop-based `AbortGuard` | Polling `request.is_disconnected()` per chunk like upstream does | Upstream's own approach (verified in `api_server.py`); Rust's Drop-based approach is strictly better-latency and is CLAUDE.md's chosen pattern — polling is not needed for parity (parity is response-byte-scoped) |
| `metrics-exporter-prometheus`'s built-in HTTP listener | A dedicated axum `/metrics` route that calls `render()` on the exporter's handle | Keeping `/metrics` inside the same axum router (not a second listening port) is simpler for a single-binary server and matches API-02's phrasing ("`/metrics` exposes...") as one of this server's own routes |

**Installation (crates to add to `crates/rsg-server/Cargo.toml` and/or workspace `Cargo.toml`):**
```toml
# workspace Cargo.toml — add to [workspace.dependencies]
axum = "0.8.9"
tower-http = { version = "0.7.1", features = ["trace"] }
tokio-util = "0.7.19"
metrics = "0.24.6"
metrics-exporter-prometheus = "0.18.3"
tokio-stream = "0.1.19"

# tokio's existing workspace feature list is missing "net", which axum's
# TcpListener/Server needs. Current pin (verified, crates/../Cargo.toml and
# workspace Cargo.toml this session):
#   tokio = { version = "1.53.1", features = ["rt-multi-thread", "macros", "signal", "sync", "time"] }
# Phase 5 must add "net" (and keep the rest) — this is a required Cargo.toml edit, not optional.
```

**Version verification:** All six new crates were checked against the crates.io registry API this session (`curl https://crates.io/api/v1/crates/<name>`) and match CLAUDE.md's pinned versions exactly — no drift since CLAUDE.md's 2026-10-02 crates.io query.

## Package Legitimacy Audit

| Package | Registry | Age | Downloads | Source Repo | Verdict | Disposition |
|---------|----------|-----|-----------|-------------|---------|-------------|
| axum | crates | pub. 2021-07-22 | ~9.9M/wk | github.com/tokio-rs/axum | OK | Approved |
| hyper | crates | pub. 2014-11-22 | ~17.3M/wk | github.com/hyperium/hyper | OK | Approved |
| tower-http | crates | pub. 2017-03-10 | ~12.9M/wk | github.com/tower-rs/tower-http | OK | Approved |
| tokio-util | crates | pub. 2018-02-01 | ~14.3M/wk | github.com/tokio-rs/tokio | OK | Approved |
| metrics | crates | pub. 2015-09-03 | ~1.7M/wk | github.com/metrics-rs/metrics | OK | Approved |
| metrics-exporter-prometheus | crates | pub. 2020-06-17 | ~1.1M/wk | github.com/metrics-rs/metrics | OK | Approved |
| tokio-stream | crates | pub. 2020-12-03 | ~9.2M/wk | github.com/tokio-rs/tokio | OK | Approved |
| futures | crates | pub. 2016-07-31 | ~14.4M/wk | github.com/rust-lang/futures-rs | OK | Approved |
| async-stream | crates | pub. 2019-06-07 | ~5.4M/wk | github.com/tokio-rs/async-stream | OK | Approved |
| hdrhistogram | crates | pub. 2015-07-07 | ~1.7M/wk | github.com/HdrHistogram/HdrHistogram_rust | OK | Approved |

Checked via `gsd-tools query package-legitimacy check --ecosystem crates`, 2026-10-06. **Packages removed due to [SLOP] verdict:** none. **Packages flagged as suspicious [SUS]:** none. No `checkpoint:human-verify` tasks required for any of these — all ten returned `OK` with long-lived, well-downloaded, source-repo-backed signals.

## Architecture Patterns

### System Architecture Diagram

```
   HTTP client (curl / load test driver)
        │  POST /generate, /v1/chat/completions, /v1/models, /v1
        │  GET  /health, /health/ready, /metrics
        ▼
 ┌─────────────────────────── rsg-server (this phase) ───────────────────────────┐
 │                                                                                │
 │  axum Router ──▶ handler: allocate uid, register (uid, mpsc::Sender<Chunk>)   │
 │        │              with FSM, return body stream from the Receiver         │
 │        │                                                                      │
 │        ▼ AbortGuard (Drop) ───────────────┐                                  │
 │  Response body stream                     │ on stream drop (disconnect)      │
 │        │ (SSE / raw chunked, framed per   │ or on finish                     │
 │        │  endpoint — see SSE Framing)     ▼                                  │
 │        │                         FSM actor task                              │
 │        │                         FxHashMap<uid, ReqState>                    │
 │        │                         states: Queued→Prefill→Decode→              │
 │        │                                  {Finished|Cancelled|Failed}        │
 │        │                                   │           │                     │
 │        │                     tokenize (Phase 4 crate)  │ detokenize (Phase 4) │
 │        │                                   │           │                     │
 │        │                                   ▼           ▲                     │
 │        │                     tx-zmq thread        rx-zmq thread              │
 │        │                     (PUSH, coalesce          (PULL,                 │
 │        │                      into BatchBackendMsg)    per-uid dispatch,     │
 │        │                                   │            drop-oldest on       │
 │        │                                   │            backpressure)        │
 │        ◀───────────────────────────────────┘           │                     │
 │                                                          │                     │
 └──────────────────────────────────────────────────────────┼─────────────────────┘
                                   ipc:// ZMQ (msgpack)      │
                                   UserMsg / AbortBackendMsg  │ DetokenizeMsg /
                                   BatchBackendMsg            │ BatchTokenizerMsg
                                                               ▼
                                              mock-scheduler (Phase 3, this phase's
                                              only backend — echo-token engine with
                                              configurable prefill/decode delay)
```

### Recommended Project Structure

```
crates/rsg-server/src/
├── main.rs              # extend: new CLI subcommand/flags for HTTP mode, --abort-timing
├── handshake.rs          # existing — max_seq_len read here for LIFE-04's overlong-prompt check
├── transport.rs          # existing split-socket halves; Phase 3 adds the per-uid dispatcher here or adjacent
├── http/
│   ├── mod.rs            # axum::Router assembly, shared AppState
│   ├── routes/
│   │   ├── generate.rs   # /generate — raw Body::from_stream, single-\n framing
│   │   ├── chat.rs       # /v1/chat/completions — stream (axum::Sse) + non-stream (JSON)
│   │   ├── models.rs     # /v1/models, /v1
│   │   └── health.rs     # /health, /health/ready, /metrics
│   └── abort_guard.rs    # Drop-based cancellation hook (Pattern A)
├── fsm/
│   ├── mod.rs             # FSM actor: inbox, FxHashMap<u64, ReqState>, state transitions
│   └── state.rs           # ReqState, lifecycle enum (LIFE-01's 6 states)
└── bin/
    └── mock-scheduler.rs  # existing, Phase 3-owned
```

### Pattern A: Drop-based cancellation (`AbortGuard`)

**What:** Wrap the per-request response body stream in a struct holding a `tokio_util::sync::CancellationToken`'s `DropGuard`. When hyper drops the body (client disconnected) or the stream finishes normally and the guard is `disarm()`-ed first, the `Drop` impl either cancels (triggering an abort) or does nothing.
**When to use:** Every streaming and non-streaming request registered with the FSM.
**Caveat (D-03, accepted limitation):** hyper only notices a dead connection on its *next write attempt*. A request still queued (no token emitted yet) may not be noticed until its first token tries to write. This phase's disconnect test is what measures this bound, not what eliminates it.

```rust
// Illustrative sketch — exact axum/tokio-util API surface not independently
// verified against docs.rs this session; verify signatures during implementation.
// [CITED: docs.rs tokio_util::sync::DropGuard — "automatically cancels [the
// token] on drop"] [ASSUMED: exact axum Body::from_stream wiring]
struct AbortGuard {
    uid: u64,
    token: tokio_util::sync::CancellationToken,
    fsm_tx: tokio::sync::mpsc::Sender<FsmEvent>,
    finished: bool,
}

impl Drop for AbortGuard {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.fsm_tx.try_send(FsmEvent::Cancel(self.uid));
        }
    }
}
```

### Pattern B: `/generate`'s non-spec SSE framing

**What:** Upstream's `/generate` route yields `f"data: {ack.incremental_output}\n".encode()` per chunk and `b"data: [DONE]\n"` at the end — a **single** trailing `\n`, not the two-`\n\n` SSE spec requires.
**Source (verbatim, read this session):**
```
# vendor/mini-sglang/python/minisgl/server/api_server.py:152-158
async def stream_generate(self, uid: int):
    async for ack in self.wait_for_ack(uid):
        yield f"data: {ack.incremental_output}\n".encode()
        if ack.finished:
            break
    yield "data: [DONE]\n".encode()
    logger.debug("Finished streaming response for user %s", uid)
```
[VERIFIED: vendor/mini-sglang/python/minisgl/server/api_server.py:152-158]
**When to use:** `/generate` only. Use `axum::body::Body::from_stream` with manual byte framing — **not** `axum::response::Sse`, which always emits `\n\n` and would break byte-parity (CLAUDE.md §What NOT to Use already states this; this session's source read confirms the exact bytes to match).

### Pattern C: `/v1/chat/completions` streaming — real SSE with a closing finish_reason chunk

**Source (verbatim, read this session):**
```
# vendor/mini-sglang/python/minisgl/server/api_server.py:160-188
async def stream_chat_completions(self, uid: int):
    first_chunk = True
    async for ack in self.wait_for_ack(uid):
        delta = {}
        if first_chunk:
            delta["role"] = "assistant"
            first_chunk = False
        if ack.incremental_output:
            delta["content"] = ack.incremental_output

        chunk = {
            "id": f"cmpl-{uid}",
            "object": "text_completion.chunk",
            "choices": [{"delta": delta, "index": 0, "finish_reason": None}],
        }
        yield f"data: {json.dumps(chunk)}\n\n".encode()

        if ack.finished:
            break

    # send final finish_reason
    end_chunk = {
        "id": f"cmpl-{uid}",
        "object": "text_completion.chunk",
        "choices": [{"delta": {}, "index": 0, "finish_reason": "stop"}],
    }
    yield f"data: {json.dumps(end_chunk)}\n\n".encode()
    yield b"data: [DONE]\n\n"
```
[VERIFIED: vendor/mini-sglang/python/minisgl/server/api_server.py:160-188]
**Note the literal object tag:** `"object": "text_completion.chunk"` — **not** `"chat.completion.chunk"**, which is what real OpenAI-compatible servers emit. This is an upstream quirk the byte-parity contract requires Rust to replicate exactly (API-01 is "identical to the Python frontend", not "identical to the OpenAI spec").
**When to use:** `axum::response::sse::{Sse, Event}` is appropriate here since the framing already matches spec SSE — but the exact JSON field order/content above must still be replicated field-for-field, including the `id: f"cmpl-{uid}"` format (not `"chatcmpl-{uid}"`, which the *non-streaming* path uses instead — these two ID prefixes differ between streaming and non-streaming chat-completions, verified below).

### Pattern D: Non-streaming responses — hardcoded zero usage, hardcoded "stop"

**Source (verbatim, read this session):**
```
# vendor/mini-sglang/python/minisgl/server/api_server.py:286-310
    # Non-streaming: collect all chunks and return a single JSON response
    full_content = ""
    async for ack in state.wait_for_ack(uid):
        full_content += ack.incremental_output
        if ack.finished:
            break

    return {
        "id": f"chatcmpl-{uid}",
        "object": "chat.completion",
        "created": int(time.time()),
        "model": req.model,
        "choices": [
            {
                "index": 0,
                "message": {"role": "assistant", "content": full_content},
                "finish_reason": "stop",
            }
        ],
        "usage": {
            "prompt_tokens": 0,
            "completion_tokens": 0,
            "total_tokens": 0,
        },
    }
```
[VERIFIED: vendor/mini-sglang/python/minisgl/server/api_server.py:286-310]
**Critical for API-01 byte parity:** `usage` is **always** `{0,0,0}` — never real token counts (EXT-01, "Stop strings, correct finish_reason and usage", is explicitly v2/out-of-scope). `finish_reason` is **always** the literal string `"stop"` — there is no length-based or EOS-based branching upstream. `id` here is `f"chatcmpl-{uid}"` (streaming's chunks use `f"cmpl-{uid}"` instead — verify this discrepancy is intentional upstream behavior, not a research transcription error, before building fixture diffs around it). `created` is a live Unix timestamp — **not byte-reproducible across two separate runs**; the fixture-diff script must normalize/exclude this field.

### Pattern E: `/v1/models`, `/v1` — simple, fully deterministic given a fixed `--model` value

**Source (verbatim, read this session):**
```
# vendor/mini-sglang/python/minisgl/server/api_server.py:86-97, 250-252, 313-316
class ModelCard(BaseModel):
    id: str
    object: str = "model"
    created: int = Field(default_factory=lambda: int(time.time()))
    owned_by: str = "mini-sglang"
    root: str

class ModelList(BaseModel):
    object: str = "list"
    data: List[ModelCard] = Field(default_factory=list)
...
@app.api_route("/v1", methods=["GET", "POST", "HEAD", "OPTIONS"])
async def v1_root():
    return {"status": "ok"}
...
@app.get("/v1/models")
async def available_models():
    state = get_global_state()
    return ModelList(data=[ModelCard(id=state.config.model_path, root=state.config.model_path)])
```
[VERIFIED: vendor/mini-sglang/python/minisgl/server/api_server.py:86-97,250-252,313-316]
**Both `id` and `root` equal the `--model` CLI value verbatim** — the fixture generator must invoke both the Python and Rust frontends with the *identical* `--model`/`model-path` string for this to byte-match. `/v1` responds `{"status": "ok"}` to `GET`, `POST`, `HEAD`, and `OPTIONS` alike — Rust's router must register all four methods on the same handler, not just `GET`.

### Pattern F: Request body defaults (for byte-identical behavior on partial JSON bodies)

**Source (verbatim, read this session):**
```
# vendor/mini-sglang/python/minisgl/server/api_server.py:53-83
class GenerateRequest(BaseModel):
    prompt: str
    max_tokens: int
    ignore_eos: bool = False

class Message(BaseModel):
    role: Literal["system", "user", "assistant"]
    content: str

class OpenAICompletionRequest(BaseModel):
    model: str
    prompt: str | None = None
    messages: List[Message] | None = None
    max_tokens: int = 16
    temperature: float = 1.0
    top_k: int = -1
    top_p: float = 1.0
    n: int = 1
    stream: bool = False
    stop: List[str] = []
    presence_penalty: float = 0.0
    frequency_penalty: float = 0.0
    ignore_eos: bool = False
```
[VERIFIED: vendor/mini-sglang/python/minisgl/server/api_server.py:53-83]
**Rust's serde request structs must mirror every default exactly** — e.g. a chat-completions request with no `max_tokens` key must behave as `max_tokens=16`, not `0` or a required-field error. `Message.role` is restricted to exactly `"system" | "user" | "assistant"` (Pydantic `Literal`) — no `"tool"` role is accepted upstream; a request with any other role value gets FastAPI's 422, which Rust's serde-based extraction should also reject (as a 422, matching HTTP-level behavior, even though the literal body differs — FastAPI's validation-error body shape is not itself claimed as part of API-01's parity scope since it's not one of the four named success-criteria endpoints' *successful* response shapes).

### Pattern G: FSM actor pattern (CLAUDE.md-specified, Claude's discretion on internals)

**What:** One tokio task owns `FxHashMap<u64, ReqState>` and handles `New`, `Tokens`, `Cancel`, `Finished` events from an `mpsc` inbox. No `DashMap` or locks on the hot path (CLAUDE.md §Stack Patterns).
**States:** `Queued → Prefill → Decode → {Finished | Cancelled | Failed}`. `Prefill→Decode` is inferred from the first `DetokenizeMsg`; TTFT is recorded there.
**LIFE-01's "exactly one terminal state" guarantee:** structurally enforced by the actor pattern — a single task owning the map means a `Cancel` racing a `Finished` for the same uid is resolved by whichever event the actor processes first from its single inbox; the loser is a no-op against an already-removed/already-terminal entry. This requires the actor to check "is this uid still active" before acting on any event, not assume it always is.

### Anti-Patterns to Avoid

- **Polling `is_disconnected()` per chunk (upstream's own pattern):** Works, but adds a syscall-ish check on every token and still has the 100ms-sleep-before-abort upstream quirk. CLAUDE.md's Drop-based `AbortGuard` is strictly better and is the locked architecture — do not port upstream's polling loop.
- **Using `axum::response::Sse` for `/generate`:** Always emits `\n\n`; upstream's `/generate` emits a single `\n`. This silently breaks API-01 byte parity for that one route only (the chat-completions streaming route *does* want real SSE).
- **Computing real `usage`/`finish_reason` values:** Byte-parity requires the *wrong*, hardcoded upstream values (`{0,0,0}` usage, always `"stop"`), not a more-correct Rust implementation. EXT-01 (correct values) is explicitly deferred to v2 — implementing it early would make Rust's non-streaming response diverge from the Python frontend it must match.
- **Diffing `created`/timestamp fields byte-for-byte:** They are inherently non-reproducible; normalize or exclude them in the fixture-diff tool, the same way Phase 1 excludes the wire-fixture generator-version block.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|--------------|-----|
| Per-request cancellation signal from a dropped HTTP connection | A custom polling loop calling `is_disconnected()` | `tokio_util::sync::CancellationToken` + `DropGuard`, triggered by hyper's body drop | hyper already surfaces the drop; re-implementing polling just reproduces upstream's own latency bug |
| Prometheus text exposition format | Hand-written `/metrics` text formatter | `metrics-exporter-prometheus`'s `render()` | Exact Prometheus exposition-format edge cases (label escaping, histogram `_bucket`/`_sum`/`_count` suffixes) are easy to get subtly wrong |
| SSE/event-stream framing in general | A hand-rolled byte-pusher for *every* route | `axum::response::sse::{Sse, Event, KeepAlive}` for the one route (`/v1/chat/completions`) that wants real SSE; raw `Body::from_stream` only for `/generate`'s non-spec framing | Only one route actually needs non-spec framing; using `Sse` elsewhere avoids re-deriving spec-correct chunking by hand |
| Golden-fixture byte-diffing with non-deterministic fields | A one-off ad-hoc diff script per field discovered | A single fixture-diff tool with an explicit "normalized fields" list (mirrors Phase 1's wire-fixture `--check` pattern) | Prevents each new non-deterministic field (there are at least two already: `created` in two places) from needing its own bespoke exclusion hack |

**Key insight:** Nearly everything novel in this phase (SSE framing per-route, hardcoded-wrong response fields, exact request-model defaults) is dictated by upstream's *existing, frozen* Python source — the risk is not "what's the right design" but "did you read the actual upstream file instead of pattern-matching to what a normal OpenAI-compatible server would do." This research read `api_server.py` directly this session specifically to surface the several places where upstream's behavior is *not* what a careful engineer would build from scratch (fixed-zero usage, mismatched streaming/non-streaming id prefixes, single-vs-double newline framing).

## Common Pitfalls

### Pitfall 1: Treating `/v1/chat/completions`'s `object`/`id` fields as "should match OpenAI's spec"
**What goes wrong:** A plausible-looking Rust implementation emits `"object": "chat.completion.chunk"` (the real OpenAI value) instead of upstream's actual `"object": "text_completion.chunk"` literal.
**Why it happens:** OpenAI-compatible server conventions are well-known from training data; upstream's own deviation from them is not.
**How to avoid:** Byte-diff against captured Python-frontend fixtures (D-02), not against external OpenAI API documentation.
**Warning signs:** A fixture diff failing only on the `object` or `id` field while `content`/`delta` match.

### Pitfall 2: Non-deterministic fields breaking fixture-diff CI
**What goes wrong:** `created` (both in `/v1/models`' `ModelCard` and in the non-streaming chat-completions response) is a live Unix timestamp. A byte-diff test will flake/fail every run unless this field is normalized.
**Why it happens:** Easy to miss until the fixture tooling is actually run twice.
**How to avoid:** The fixture-diff tool needs an explicit per-endpoint "ignore/normalize these JSON paths" list, mirroring Phase 1's `gen_wire_fixtures.py --check`'s exclusion of the generator-version block.
**Warning signs:** Fixture tests pass locally once, then fail on re-run or in CI.

### Pitfall 3: Scheduling FSM work ahead of Phase 3's per-uid dispatcher
**What goes wrong:** `crates/rsg-server/src/transport.rs` (read this session) only exposes `ZmqBackendTx`/`ZmqDetokRx` raw split-socket halves — there is no per-uid bounded-channel dispatch table (Phase 3 D-04/D-05/D-06) in code yet. The FSM is specified to register/deregister against that dispatcher's API.
**Why it happens:** 05-CONTEXT.md explicitly instructs planning against Phase 3's *decisions*, not its current code — but if Phase 5 executes before Phase 3's dispatcher waves land, there is nothing to register against.
**How to avoid:** Sequence Phase 5's plan waves so the FSM-to-dispatcher integration task has an explicit dependency check (or build a minimal interim dispatcher inline if Phase 3 hasn't delivered one, clearly marked as throwaway/to-be-replaced).
**Warning signs:** Attempting `cargo build` against an API (`register_uid`, a per-uid `Receiver<TokenizerMsg>`) that doesn't exist in `transport.rs`.

### Pitfall 4: `mock-scheduler`'s current CLI surface doesn't yet support the misbehaviors this phase's tests need
**What goes wrong:** `crates/rsg-server/src/bin/mock-scheduler.rs` (read this session, full file) currently only accepts `--backend-addr`, `--backend-role`, `--detok-addr`, `--detok-role`, `--prefill-delay-ms`, `--decode-delay-ms`, `--max-seq-len`. There is no `--misbehave-uids`/`--behavior late-abort-token`/`--behavior drop-overlong`/`--batch-size` flag (Phase 3 D-09) yet.
**Why it happens:** Same cross-phase sequencing issue as Pitfall 3 — the mock's misbehavior flags are Phase 3's deliverable, consumed by Phase 5's tests.
**How to avoid:** LIFE-02's "tokens that arrive after the abort are dropped and counted" test needs a late-token scenario; LIFE-04's overlong-prompt test needs either a real `max_seq_len` check in Rust itself (recommended — see Pitfall 5) or a mock misbehavior. Flag this dependency explicitly in the plan rather than assuming the flags exist.
**Warning signs:** A planned test references a `mock-scheduler` flag that `clap`'s `Cli::parse()` rejects as unknown.

### Pitfall 5: Overlong-prompt 400 — doing the check in the wrong place or using the wrong value
**What goes wrong:** Implementing the overlong-prompt check against a hardcoded constant, or against a value read from somewhere other than the live handshake.
**Why it happens:** The ergonomic spot to check prompt length is before invoking Phase 4's tokenizer (character count isn't the same as token count, so the check is necessarily approximate pre-tokenize) — CONTEXT.md leaves pre- vs. post-tokenize timing to Claude's discretion, as long as "immediate" and tied to the readiness handshake's `max_seq_len`.
**Source for the exact field (verbatim, read this session):**
```
// crates/rsg-server/src/handshake.rs:20-33
pub struct Handshake {
    pub handshake_version: u32,
    pub upstream_sha: String,
    pub max_seq_len: u64,
    #[serde(deserialize_with = "Option::deserialize")]
    pub eos_token_id: Option<u64>,
    pub page_size: u64,
    pub max_running_req: u64,
    pub num_pages: u64,
}
```
[VERIFIED: crates/rsg-server/src/handshake.rs:20-33]
**How to avoid:** Store the parsed `Handshake.max_seq_len: u64` from the readiness handshake in shared server state at startup; compare the tokenized prompt's `input_ids.len()` (post-tokenize, most accurate) or prompt length as a cheap pre-tokenize guard (less accurate but catches pathological cases before spending tokenizer CPU) against it. Either timing satisfies CONTEXT.md's discretion clause.
**Warning signs:** A magic-number `4096` or similar hardcoded length limit appearing anywhere in the Rust handler code instead of a reference to the stored handshake value.

### Pitfall 6: Missing `tokio` "net" feature
**What goes wrong:** `axum::serve` (or any `tokio::net::TcpListener` construction) fails to compile.
**Source (verbatim, read this session):**
```
# Cargo.toml (workspace), [workspace.dependencies]
tokio = { version = "1.53.1", features = ["rt-multi-thread", "macros", "signal", "sync", "time"] }
```
[VERIFIED: /Users/li-yangtseng/Codes/mini-rsglang/.claude/worktrees/twinkly-brewing-cosmos/Cargo.toml]
**Why it happens:** Phase 1/3 never needed a TCP listener (ZMQ uses `ipc://`), so `"net"` was never added.
**How to avoid:** Add `"net"` (and optionally `"io-util"` if needed by a chosen streaming helper) to the workspace `tokio` feature list as part of this phase's first Cargo.toml edit.
**Warning signs:** A compile error naming `TcpListener` or `axum::serve` as unresolved/feature-gated.

### Pitfall 7: `/v1`'s multi-method route
**What goes wrong:** Registering `/v1` only as `GET`, missing upstream's `POST`/`HEAD`/`OPTIONS` acceptance.
**How to avoid:** Use axum's `MethodRouter`/`any()` or chain `.get().post()...` on the same handler, matching `@app.api_route("/v1", methods=["GET", "POST", "HEAD", "OPTIONS"])` [VERIFIED: vendor/mini-sglang/python/minisgl/server/api_server.py:250-252].

## Code Examples

### SSE framing for `/generate` (raw, single-`\n`)

```rust
// [ASSUMED: exact axum 0.8 Body::from_stream call signature not verified
// against docs.rs this session — verify during implementation] Illustrative only.
use axum::body::Body;
use axum::response::Response;
use tokio_stream::wrappers::ReceiverStream;

async fn generate_handler(/* ... */) -> Response {
    let (tx, rx) = tokio::sync::mpsc::channel::<String>(16);
    // ... register uid with FSM, spawn forwarding of DetokenizeMsg -> tx ...
    let stream = ReceiverStream::new(rx).map(|incremental_output: String| {
        Ok::<_, std::io::Error>(bytes::Bytes::from(format!("data: {incremental_output}\n")))
    });
    Response::builder()
        .header("content-type", "text/event-stream")
        .body(Body::from_stream(stream))
        .unwrap()
    // Final "data: [DONE]\n" chunk appended by the stream's terminal item,
    // per api_server.py:157 (verified above).
}
```

### CLI flag convention for `--abort-timing` (D-01, D-05 in LIFE-05)

```rust
// Follows the existing Cli struct pattern in crates/rsg-server/src/main.rs
// (clap::Parser, value_name/value_enum conventions verified this session).
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
enum AbortTiming {
    Immediate,
    Deferred,
}

// in Cli:
/// Immediate: abort as soon as disconnect is noticed. Deferred: wait for the
/// first token before sending the abort (works around a suspected upstream
/// abort-during-prefill bug — see STATE.md's Phase 6 blocker note).
#[arg(long, value_enum, default_value_t = AbortTiming::Immediate)]
abort_timing: AbortTiming,
```

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | Rust built-in `cargo test` (no `cargo-nextest` config present yet — `.config/nextest.toml` does not exist; `cargo-nextest` is listed in CLAUDE.md's dev tools but not yet adopted in `scripts/check_all.sh`) [VERIFIED: scripts/check_all.sh, read this session] |
| Config file | none — `scripts/check_all.sh` is the Mac gate script |
| Quick run command | `cargo test -p rsg-server <test_name_substring>` |
| Full suite command | `cargo test --workspace` (step 1 of `scripts/check_all.sh`, verified) |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|---------------------|-------------|
| LIFE-01 | Every uid reaches exactly one terminal state | unit | `cargo test -p rsg-server fsm::` | ❌ Wave 0 — `fsm` module doesn't exist |
| LIFE-02 | Disconnect → immediate abort; late tokens dropped+counted | integration | `cargo test -p rsg-server --test http_cancellation` | ❌ Wave 0 |
| LIFE-03 | 128-agent stress, no leaks/stuck connections | integration (throwaway stress test, D-04) | `cargo test -p rsg-server --test stress_128 -- --ignored` (long-running, gate behind `--ignored` like a manual/slow test) | ❌ Wave 0 |
| LIFE-04 | Overlong prompt 400; backend-unresponsive timeout | integration | `cargo test -p rsg-server --test http_errors` | ❌ Wave 0 |
| LIFE-05 | `--abort-timing` flag changes behavior | integration (parametrized, both flag values) | `cargo test -p rsg-server --test abort_timing` | ❌ Wave 0 |
| API-01 | Byte-parity on 4 endpoints incl. SSE framing | fixture-diff (new Python capture script + Rust-side comparison test) | `python scripts/gen_api_fixtures.py --check` (new, modeled on `gen_wire_fixtures.py`) | ❌ Wave 0 — script doesn't exist |
| API-02 | `/health`, `/health/ready`, `/metrics` respond with required series | integration | `cargo test -p rsg-server --test observability` | ❌ Wave 0 |

### Sampling Rate
- **Per task commit:** targeted `cargo test -p rsg-server <module>`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** `scripts/check_all.sh` extended with a new fixture-freshness step for API fixtures (mirroring the existing wire-fixture step), full suite green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `crates/rsg-server/src/fsm/` module + unit tests — the lifecycle state machine doesn't exist
- [ ] `crates/rsg-server/src/http/` module + axum `Router` — no HTTP code exists anywhere in the workspace yet
- [ ] `crates/rsg-server/tests/http_cancellation.rs`, `http_errors.rs`, `abort_timing.rs`, `observability.rs`, `stress_128.rs` — new integration test files, likely reusing/extending `tests/common/mod.rs`'s `MockScheduler` harness (verified reusable this session) plus a new HTTP-client test helper (`reqwest` or axum's own `TestServer`-style in-process call)
- [ ] `scripts/gen_api_fixtures.py` — new Python fixture-capture script against a live Python-frontend process, modeled on `scripts/gen_wire_fixtures.py`'s `--check` convention; must include a normalized/excluded-fields list for `created` timestamps
- [ ] Workspace `Cargo.toml` edits: add `axum`, `tower-http`, `tokio-util`, `metrics`, `metrics-exporter-prometheus`, `tokio-stream`; add `"net"` feature to the existing `tokio` dependency
- [ ] Cross-phase dependency: Phase 3's per-uid dispatcher (D-04/D-05/D-06) and misbehavior flags (D-09) on `mock-scheduler`, and Phase 4's tokenizer/detokenizer crate — none exist in code as of this research session (see Pitfalls 3-4)

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-------------------|
| V2 Authentication | No | No auth system in scope — this is a local, single-tenant dev/benchmark server (matches upstream, which has none either) |
| V3 Session Management | No | No sessions/cookies involved |
| V4 Access Control | No | Single-tenant, no multi-user resource isolation needed |
| V5 Input Validation | Yes | serde-typed request structs with explicit defaults (Pattern F); explicit `max_seq_len`-bounded prompt-length check (LIFE-04, Pitfall 5); `Message.role` restricted to the three literal values upstream accepts |
| V6 Cryptography | No | No secrets/credentials handled by this phase's routes |

### Known Threat Patterns for this stack

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Unbounded/oversized request bodies (JSON bomb) | Denial of Service | axum's extractor-level default body-size limit, or explicit `axum::extract::DefaultBodyLimit` layer [ASSUMED: exact axum 0.8 API name not independently verified this session] |
| Many concurrent slow/stalled streaming connections (slowloris-like) | Denial of Service | Phase 3's bounded per-uid channel + drop-oldest backpressure policy (D-05/D-06) already bounds per-request memory; LIFE-03's 128-concurrent stress test is this phase's own proof that no connection gets structurally stuck |
| Backend process hangs, HTTP request never resolves | Denial of Service | LIFE-04's required timeout-with-error behavior — do **not** rely on `tower_http::timeout` (forbidden on streaming routes per CLAUDE.md); implement the timeout inside the FSM instead |
| Overlong prompt causing excessive tokenizer/backend work | Denial of Service / Resource Exhaustion | LIFE-04's immediate 400 against the handshake's `max_seq_len`, before (or just after) tokenization |

## Sources

### Primary (HIGH confidence)
- `crates/rsg-wire/src/lib.rs` (read this session, full file) — `BackendMsg`, `TokenizerMsg`, `SamplingParams`, `Tensor` wire types this phase's FSM sends/receives unmodified
- `crates/rsg-server/src/transport.rs` (read this session, full file) — current split-socket API state; confirms no per-uid dispatcher exists yet
- `crates/rsg-server/src/main.rs`, `crates/rsg-server/src/handshake.rs` (read this session, full files) — CLI/exit-code conventions, `Handshake.max_seq_len` field
- `crates/rsg-server/src/bin/mock-scheduler.rs`, `crates/rsg-server/tests/common/mod.rs`, `crates/rsg-server/tests/mock_scheduler_process.rs` (read this session, full files) — current mock-scheduler CLI surface and reusable test harness
- `vendor/mini-sglang/python/minisgl/server/api_server.py` (read this session, full file) — the parity oracle for API-01; all verbatim quotes above cite specific line ranges
- `vendor/mini-sglang/python/minisgl/server/args.py`, `vendor/mini-sglang/python/minisgl/tokenizer/detokenize.py` (read this session, full files) — CLI-arg and incremental-decode context
- `Cargo.toml` (workspace), `crates/rsg-server/Cargo.toml`, `crates/rsg-wire/Cargo.toml`, `rust-toolchain.toml` (read this session) — current dependency/feature state
- `scripts/check_all.sh` (read this session) — current Mac gate script and test-run convention
- crates.io registry API, queried 2026-10-06, for `axum`, `hyper`(not independently queried — transitive), `tower-http`, `tokio-util`, `metrics`, `metrics-exporter-prometheus`, `tokio-stream`, `futures`, `async-stream`, `hdrhistogram` — all match CLAUDE.md's pinned versions
- `gsd-tools query package-legitimacy check --ecosystem crates`, run 2026-10-06 — all 10 new crates verdict `OK`

### Secondary (MEDIUM confidence)
- `.claude/CLAUDE.md` §Recommended Stack / §Stack Patterns by Variant / §What NOT to Use — the locked architectural decisions this research builds on (project's own authoritative, already crates.io-verified per its own Sources section)
- `docs.rs` (tokio_util::sync::DropGuard, "automatically cancels [the token] on drop") — via WebSearch, not independently navigated to the live page this session

### Tertiary (LOW confidence)
- WebSearch results on hyper's body-drop disconnect-detection semantics and `axum::extract::DefaultBodyLimit`'s exact name — general knowledge, not confirmed against a live docs.rs page this session; CLAUDE.md itself already flags the disconnect-latency claim as "MEDIUM: verify with a disconnect test," which this phase's own test is designed to do

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|-----------------|
| A1 | Exact `axum::body::Body::from_stream` and `axum::extract::DefaultBodyLimit` API names/signatures (0.8.9) | Code Examples; Security Domain | Low — these are well-known, stable axum APIs; worst case is a compile-time signature mismatch caught immediately, not a silent behavioral bug |
| A2 | hyper 1.11.1's exact disconnect-detection latency (how soon after a client closes the socket does the next write fail) | Common Pitfalls / Pattern A | Already tracked as a known open question in STATE.md and CLAUDE.md; D-03 explicitly scopes this phase's own disconnect test as the way to measure it, not to assume a number |
| A3 | `futures` crate's "current" version string wasn't independently re-queried with a fixed version number this session (registry returned a resolving value, not pinned to a literal digit here) | Standard Stack §Supporting | Low — `futures` is near-universally used transitively already (via `tokio-stream`); pin whatever `cargo add` resolves at implementation time |
| A4 | `/v1`'s streaming chat-completions `id` prefix (`cmpl-{uid}`) vs. non-streaming's (`chatcmpl-{uid}`) is an intentional upstream inconsistency, not a research transcription slip | Pattern D | If actually a slip, a fixture-diff test would catch it immediately on first run (both are verbatim-quoted from the same read file in this document, so transcription risk is low, but re-verify against the live file before building the diff tool) |

**All other claims in this research were verified by direct file reads (this session) or the crates.io registry/package-legitimacy seam (this session).**

## Open Questions

1. **Will Phase 3's per-uid dispatcher (D-04/D-05/D-06) and misbehavior flags (D-09) exist in code by the time Phase 5 executes?**
   - What we know: `transport.rs` and `mock-scheduler.rs` as read this session do not yet have them; 05-CONTEXT.md instructs planning against the *decisions*, not current code.
   - What's unclear: execution ordering across the three worktrees (`twinkly-brewing-cosmos` for Phase 5, presumably another for Phase 3's remaining waves).
   - Recommendation: the planner should make the FSM's dependency on the per-uid dispatcher an explicit, checked precondition (e.g., a Wave-0 task that verifies the dispatcher API exists before FSM-integration tasks proceed), not an assumption.

2. **Exact backend-unresponsive timeout duration (LIFE-04)**
   - What we know: CONTEXT.md leaves this entirely to Claude's discretion; no upstream precedent exists (upstream hangs forever on this scenario, which is the bug LIFE-04 fixes).
   - What's unclear: no value researched here because none exists to discover — it's a pure design choice.
   - Recommendation: pick a value proportional to this phase's own test timeouts (e.g., a few seconds) and make it CLI-configurable alongside `--abort-timing`, consistent with the project's CLI-configuration convention.

3. **Exact format of `/health`, `/health/ready` responses**
   - What we know: zero upstream precedent; API-02 only fixes the three `/metrics` series.
   - What's unclear: whether `/health/ready` should reflect "backend handshake received" state specifically, or something broader.
   - Recommendation: `/health` = process liveness only (always 200 once the HTTP listener is up); `/health/ready` = 200 only after the readiness handshake has been parsed successfully, consistent with the project's existing handshake-gated startup sequencing in `main.rs`.

## RESEARCH COMPLETE

**Phase:** 5 - Request Lifecycle & HTTP API
**Confidence:** MEDIUM

### Key Findings
- `vendor/mini-sglang/python/minisgl/server/api_server.py` (read in full this session) is the complete parity oracle for API-01: `/generate` uses single-`\n` framing (not spec SSE), `/v1/chat/completions` streaming uses real double-`\n\n` SSE with a non-standard `"object": "text_completion.chunk"` tag, non-streaming responses hardcode `usage` to all-zero and `finish_reason` to `"stop"` always, and `/v1/models`'/non-streaming's `created` fields are live timestamps that must be excluded from byte-diffs.
- Upstream's own cancellation is polling-based with a 100ms pre-abort sleep — Rust's Drop-based `AbortGuard` (CLAUDE.md Pattern A) is an intentional improvement, not a parity requirement; parity is scoped to response bytes only.
- The codebase is further along than 05-CONTEXT.md's dependency note suggests for `mock-scheduler` (a complete echo-token engine already exists) but behind for the per-uid dispatcher Phase 3 owes the FSM (D-04/D-05/D-06) and the misbehavior flags Phase 3 owes the stress test/cancellation tests (D-09) — neither exists in code yet.
- All 10 new crate dependencies (axum, hyper-transitive, tower-http, tokio-util, metrics, metrics-exporter-prometheus, tokio-stream, futures, async-stream, hdrhistogram) are crates.io-verified at CLAUDE.md's pinned versions and pass the package-legitimacy gate with verdict `OK`.
- The workspace's current `tokio` feature list (`rt-multi-thread, macros, signal, sync, time`) is missing `"net"`, required for axum's TCP listener — a mandatory first Cargo.toml edit.

### File Created
`.planning/phases/05-request-lifecycle-http-api/05-RESEARCH.md`

### Confidence Assessment
| Area | Level | Reason |
|------|-------|--------|
| Standard Stack | HIGH | All versions crates.io-verified this session, consistent with CLAUDE.md's own prior verification |
| Architecture | HIGH | FSM/axum/cancellation patterns are CLAUDE.md-locked and consistent with read source code |
| API byte-parity details (Patterns B-F) | HIGH | Directly quoted from `api_server.py`, read in full this session |
| Pitfalls 3-4 (cross-phase dependency gaps) | HIGH | Directly observed via reading current `transport.rs`/`mock-scheduler.rs` state this session |
| Exact axum/hyper API signatures in Code Examples | LOW-MEDIUM | Not independently verified against live docs.rs pages this session — flagged `[ASSUMED]`, low risk (compile-time catchable) |
| hyper disconnect-latency bound | LOW (tracked, not resolved) | Open question by design — this phase's own disconnect test is what answers it |

### Open Questions
See `## Open Questions` above: Phase 3 dependency-readiness timing, exact backend-timeout duration, exact `/health`/`/health/ready` response shape.

### Ready for Planning
Research complete. Planner can now create PLAN.md files — recommend the plan explicitly sequence the FSM-to-dispatcher integration task behind a check for Phase 3's dispatcher API, per Pitfall 3.
