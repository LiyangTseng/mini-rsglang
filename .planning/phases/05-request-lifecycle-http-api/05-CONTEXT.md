# Phase 5: Request Lifecycle & HTTP API - Context

**Gathered:** 2026-10-06
**Status:** Ready for planning

<domain>
## Phase Boundary

The first full request runs on the Mac: a client calls the Rust frontend's HTTP API, backed by Phase 3's `mock-scheduler` (not the real backend — that's Phase 6). This phase builds the request-lifecycle FSM (received → tokenizing → submitted → decoding → finished/cancelled/failed) and wires it to concurrent HTTP ingress, so every request reaches exactly one terminal state and none are leaked.

Requirements: LIFE-01, LIFE-02, LIFE-03, LIFE-04, LIFE-05, API-01, API-02.

Not in this phase:
- The ZMQ transport and per-uid dispatch mechanics themselves (WIRE-03, MOCK-01, Phase 3) — this phase *consumes* Phase 3's per-uid channel API, it does not rebuild uid routing.
- Tokenization/chat-template/detokenization logic (TOK-01..04, Phase 4) — this phase *consumes* Phase 4's tokenizer/detokenizer, it does not reimplement encode/decode.
- Real GPU backend, real-model output parity (PAR-01/02, Phase 6).
- The benchmark harness proper (BENCH-02..08, Phase 7) — Phase 5's own 128-agent stress test is a minimal, throwaway correctness check for LIFE-03, not Phase 7's instrumented load generator.

**Dependency note (as of this discussion):** Phase 3 has only completed its Wave-1 tracer (`transport.rs` has basic send/recv, no per-uid dispatcher yet); Phase 4 discussion is in progress but unplanned. Phase 5 planning proceeds against the *decisions* in 03-CONTEXT.md and 04-CONTEXT.md as the contract, not against code that exists yet — the planner/executor should treat those two files as authoritative interface specs until Phase 3/4 code lands.

</domain>

<decisions>
## Implementation Decisions

### Abort timing (LIFE-05)
- **D-01:** Abort timing (immediate vs. deferred-until-first-token) is a **server-wide CLI flag** (`--abort-timing immediate|deferred`), not a per-request field. Same convention as `rsg-server`'s and `mock-scheduler`'s existing CLI-configured behavior. Default is `immediate`, per LIFE-05. — **Reversibility:** costly — Phase 6's fairness comparison for the suspected abort-during-prefill bug depends on this being one global mode per benchmark run (STATE.md's Phase 6 blocker note: "the abort-timing setting must apply equally to the baseline"); switching to per-request later would require redesigning how Phase 6/7 control it for both frontends symmetrically.

### API output-parity verification (API-01)
- **D-02:** Parity for `/v1/chat/completions` (streaming + non-streaming), `/generate`, `/v1/models`, `/v1` is proven with **golden fixtures captured from a live Python-frontend run** (against the mock or a stub), diffed byte-for-byte against Rust's output — the same golden-fixture pattern Phase 1 used for the wire codec (`scripts/gen_wire_fixtures.py`) and Phase 4 is using for tokenizer/detokenizer output (`scripts/gen_tokenizer_fixtures.py`). Not a hand-port from reading `api_server.py` alone. — **Reversibility:** reversible — the fixture generator can be regenerated/extended without touching Rust's response-building code.

### Non-streaming disconnect handling (LIFE-02)
- **D-03:** The gap between LIFE-02's "abort right away" and hyper's actual behavior (it only notices a dropped connection on its *next write*, so a non-streaming/buffered response has no write until fully done) is **accepted as a known limitation and documented**, not patched with extra liveness-probing plumbing. Matches CLAUDE.md's Pattern A (`AbortGuard` + `Drop`) as designed; CLAUDE.md already flags this as "verify with a disconnect test" (MEDIUM confidence) — that test is how Phase 5 proves the actual bound on this gap, it does not try to eliminate the gap.

### 128-agent stress test scope (LIFE-03)
- **D-04:** Phase 5 builds a **minimal, Phase-5-only stress test**, not a shared foundation for Phase 7's benchmark harness. It uses `mock-scheduler`'s existing fixed-delay flags (Phase 3 D-10) plus randomized abort-after-N-tokens logic living in the *test driver* itself (not added to `mock-scheduler`), just enough to prove LIFE-03's no-leaked-requests / no-stuck-connections / exactly-one-terminal-state criterion. Phase 7 builds its own instrumented load generator (hdrhistogram, seeded RNG, TTFT/P99 recording per the benchmark stack) later, unconstrained by this throwaway tool. — **Reversibility:** reversible — this test can be discarded once Phase 7's harness exists; it is not an API Phase 7 depends on.

### Claude's Discretion
- Exact FSM implementation shape (actor task owning `FxHashMap<u64, ReqState>` per CLAUDE.md's stack pattern, vs. an alternative) — CLAUDE.md already names the pattern; internal structuring is Claude's call.
- `/metrics` label cardinality and TTFT histogram bucket boundaries, beyond API-02's required counters (request count, cancellation count, TTFT histogram).
- The exact backend-unresponsive timeout duration/config surface for LIFE-04's "times out with an error instead of hanging" criterion.
- Whether the overlong-prompt 400 rejection (LIFE-04) happens before or after invoking Phase 4's tokenizer, as long as it's immediate and uses the readiness-handshake `max_seq_len` (already available per BASE-03).
- Exact SSE keep-alive interval, if CLAUDE.md's "fast abort of queued requests" keep-alive note is implemented in this phase at all — not raised as a separate gray area because no scenario in this phase's success criteria explicitly requires fast abort of a *queued* (not yet dispatched) request; revisit only if testing shows queued requests aren't aborted promptly enough under D-03's accepted-limitation stance.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Project scope and requirements
- `.planning/ROADMAP.md` §Phase 5 — goal, dependencies (Phase 3 + Phase 4), 5 success criteria
- `.planning/REQUIREMENTS.md` — LIFE-01..05, API-01, API-02 full text; traceability row confirming Phase 5 ownership
- `.planning/PROJECT.md` — Constraints (Rust owns ingress/lifecycle FSM; fair comparison; GPU-free dev environment)
- `.planning/STATE.md` — Phase 5 blocker ("not yet known how quickly hyper/axum detects a client disconnect while a request is queued") and Phase 6 blocker (abort-during-prefill bug, ties directly to D-01's abort-timing flag)

### Stack and pattern guidance
- `.claude/CLAUDE.md` §Stack Patterns by Variant — the `AbortGuard`/`Drop` cancellation hook (Pattern A), the FSM states and actor-task pattern, the `tx-zmq`/`rx-zmq` coalescing convention, the SSE-framing rule (`axum::Sse` for `/v1/chat/completions`, raw `Body::from_stream` with single-newline framing for `/generate` — **not** spec SSE), the keep-alive caveat for queued-request abort
- `.claude/CLAUDE.md` §Recommended Stack — `axum` 0.8.9, `hyper` 1.11.1, `tower-http` 0.7.1 (no `timeout` layer on streaming routes), `tokio-util` (`CancellationToken`/`DropGuard`), `metrics` + `metrics-exporter-prometheus` for `/metrics`
- `.claude/CLAUDE.md` §What NOT to Use — `axum::response::Sse` for `/generate`; `tower_http::timeout` on streaming routes

### Prior-phase context (carried forward — contract to build against)
- `.planning/phases/03-zmq-transport-mock-scheduler/03-CONTEXT.md` — D-04/D-05/D-06 (per-uid bounded channel API, drop-oldest-on-backpressure policy, dropped-token counter) is the layer this phase's FSM registers/deregisters against; D-08/D-09/D-10 (`mock-scheduler`'s CLI-configured misbehaviors and fixed-delay flags) is what Phase 5's stress test (D-04) reuses
- `.planning/phases/04-tokenizer-detokenizer-parity/04-CONTEXT.md` (in the `ethereal-brewing-milner` worktree at the time of this discussion) — tokenizer/chat-template/detokenizer output shape this phase's FSM consumes for the tokenizing→submitted and decoding→finished transitions

### Upstream code this phase mirrors (read, do not modify)
- `vendor/mini-sglang/python/minisgl/server/api_server.py` — FastAPI route handlers, response shapes, and SSE framing for `/v1/chat/completions`, `/generate`, `/v1/models`, `/v1` — the parity oracle for D-02's golden fixtures
- `vendor/mini-sglang/python/minisgl/server/args.py`, `server/launch.py` — existing CLI-flag conventions D-01's `--abort-timing` flag should follow

### Existing code this phase builds on
- `crates/rsg-server/src/transport.rs` — `Transport`/`BackendSink`/`DetokSource` traits and `ZmqTransport`/`ZmqBackendTx`/`ZmqDetokRx` (Phase 1/3 Wave-1 state); the per-uid dispatcher this phase depends on (03-CONTEXT.md D-04) is not yet built in code as of this discussion
- `crates/rsg-server/src/main.rs`, `handshake.rs` — existing CLI (`clap`) parsing and exit-code convention D-01's new flag and this phase's binary should follow
- `crates/rsg-wire/src/lib.rs` — `BackendMsg`, `TokenizerMsg`, `SamplingParams` types the FSM sends/receives unmodified

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `crates/rsg-server/src/transport.rs` — `ZmqBackendTx`/`ZmqDetokRx` split-socket halves already exist for the tx-zmq/rx-zmq thread pattern; the FSM's registration table sits on top of these, not a replacement
- `crates/rsg-server/src/main.rs` — CLI parsing and exit-code convention (`EXIT_STARTUP`/`EXIT_BAD_HANDSHAKE`/`EXIT_STDIN_EOF`) to extend for the new HTTP-server binary/mode and the `--abort-timing` flag

### Established Patterns
- No HTTP/axum code exists yet anywhere in the workspace — this phase introduces the HTTP layer from nothing
- No FSM/request-registry code exists yet — Phase 3 only built the transport primitives (Wave 1 of 4), not the per-uid dispatch table the FSM needs (03-CONTEXT.md D-04/D-05 describe the target API, not yet implemented)
- Workspace already pins `zmq = "0.10.0"`, `clap` with the launcher's CLI conventions — this phase doesn't revisit either

### Integration Points
- The per-uid bounded channel from Phase 3 (03-CONTEXT.md D-04/D-05) is what the FSM registers/deregisters against for routing tokens to the right in-flight request
- Phase 4's tokenizer/detokenizer crate (not yet named/located as of this discussion — see 04-CONTEXT.md's "Claude's Discretion" on exact crate layout) is what the FSM calls into for the tokenizing and decoding transitions
- `mock-scheduler` (Phase 3, D-08/D-09/D-10) is what this phase's HTTP server and stress test both spawn in place of the real backend

</code_context>

<specifics>
## Specific Ideas

- The abort-timing flag (D-01) exists specifically because of a suspected bug named in STATE.md's Phase 6 blocker, not as a general-purpose tuning knob — its design priority is "make the Phase 6 A/B comparison fair," not flexibility.
- The non-streaming disconnect gap (D-03) is treated the same way Phase 3 treated its slow-consumer drop policy: name the limitation precisely, prove its bound with a test, and don't over-engineer around it.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

### Reviewed Todos (not folded)
None — no pending todos matched this phase.

</deferred>

---

*Phase: 05-request-lifecycle-http-api*
*Context gathered: 2026-10-06*
