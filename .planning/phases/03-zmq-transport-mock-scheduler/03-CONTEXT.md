# Phase 3: ZMQ Transport & Mock Scheduler - Context

**Gathered:** 2026-10-05
**Status:** Ready for planning

<domain>
## Phase Boundary

This phase delivers two things:
- **WIRE-03**: the Rust frontend's ZMQ transport sends every outgoing message (`UserMsg`, `AbortBackendMsg`, `BatchBackendMsg`) through a single ordered writer, so under concurrent load the scheduler can never observe an abort before the submit it cancels. Replies are routed back to the right in-flight request by `uid`, a slow consumer on one request never stalls replies for others, and replies for unknown uids (e.g. late tokens after an abort) are dropped without crashing.
- **MOCK-01**: one minimal Rust `mock-scheduler` binary that speaks the exact same wire protocol as the real backend (reusing `rsg-wire`'s `BackendMsg`/`TokenizerMsg`), runs as a real subprocess over `ipc://` sockets, and can be configured to reproduce the specific backend misbehaviors later cancellation tests need: late tokens after an abort, silently dropped overlong prompts, and batched replies across several requests in one `BatchTokenizerMsg`.

Requirements: WIRE-03, MOCK-01.

Not in this phase:
- The request-lifecycle FSM, HTTP API, and cancellation-on-disconnect wiring (LIFE-*, API-*, Phase 5). Phase 3 only builds the transport's uid-routing/dispatch layer that Phase 5's FSM will sit on top of — it does not build the FSM itself.
- Tokenization/detokenization (TOK-*, Phase 4) — independent of this phase, can run in parallel.
- Any Rust radix cache or prefix-hit modeling in the mock. `PROJECT.md` and `REQUIREMENTS.md` both defer the Rust radix cache to v2, and MOCK-01's text ("No other mocks; protocol fidelity is covered by WIRE-01/02 and PAR-01") does not call for prefix-hit simulation. `.claude/CLAUDE.md`'s stack-pattern note that the mock "uses the Rust radix trie to model prefix hits" is stale draft text from before that v2 deferral was locked — ignore it. The mock emits deterministic tokens with configurable delays; it does not model prefix caching.
- Benchmark-grade timing realism (seeded random delay distributions, scenario replay files) — deferred until Phase 5/7 actually need it (see D-10/D-11 below).

</domain>

<decisions>
## Implementation Decisions

### Single ordered writer (WIRE-03)
- **D-01:** All outgoing messages to the scheduler (`UserMsg`, `AbortBackendMsg`, and any coalesced `BatchBackendMsg`) go through one single-writer component, per the project's existing `tx-zmq` thread pattern (`.claude/CLAUDE.md` §Stack Patterns). The exact internal mechanism (dedicated OS thread fed by a channel, vs. an async task owning the socket) is Claude's discretion — the user-facing guarantee is "one path, FIFO, no second writer can interleave," not the specific thread model.
- **D-02:** The ordering guarantee is proven by a **property-based test (proptest)**, generating random interleavings of concurrent submit/abort calls across many uids, asserting the scheduler never observes an abort before its own submit for any uid. — **Reversibility:** reversible — the test strategy can be swapped later without touching the writer's production code.
- **D-03:** The ordering property test runs **end-to-end through the real transport and a real `mock-scheduler` subprocess** (not an isolated in-memory stand-in for the writer). This proves the guarantee survives real socket I/O and the real process boundary, not just the in-process logic. — **Reversibility:** reversible — a faster isolated unit test can be added later as a supplement without removing this one.

### Reply routing & slow-consumer backpressure
- **D-04:** Replies are dispatched to in-flight requests via a **per-uid bounded channel**: a dispatcher reads frames off the detokenizer socket and routes each `DetokenizeMsg`/`BatchTokenizerMsg` entry to the channel registered for its `uid`. Replies for unknown/deregistered uids (late tokens after an abort) are dropped without error.
- **D-05:** Each per-uid channel is **bounded with a small fixed capacity (around 16)**. When a slow consumer leaves its channel full, the **oldest buffered token for that uid is dropped** rather than blocking the dispatcher — this is what structurally guarantees a slow consumer on one request never stalls replies for others (criterion 4). — **Reversibility:** costly — Phase 5's FSM will build directly on this per-uid channel API; changing the drop policy later means revisiting whatever Phase 5 code depends on "no token is ever silently gapped without a signal."
- **D-06:** Every dropped token is counted and surfaced — a per-uid dropped-token counter (and a `tracing` warning) — so tests (and later, Phase 6 parity debugging) can distinguish "we intentionally dropped a backed-up token" from "the backend sent something wrong." A silent, unsignaled gap in a uid's token stream is never acceptable.
- **D-07:** The channel bound (around 16) is a **fixed constant for Phase 3**, not an exposed CLI/config knob. If Phase 5 or Phase 7 need to tune it for their own stress/benchmark scenarios, they change the constant or add a knob then — not speculatively now.

### Mock scheduler (MOCK-01)
- **D-08:** `mock-scheduler` is a **standalone subprocess binary**, spawned like the real backend over real `ipc://` sockets (not an in-process library/fake). It reuses `rsg-wire`'s existing `BackendMsg`/`TokenizerMsg` types and the bind/connect role convention from Phase 1's `transport.rs`. — **Reversibility:** costly — Phase 5/6/7 all spawn this same binary in place of the real backend; changing it to an in-process model later means reworking every one of those test harnesses.
- **D-09:** Misbehavior scenarios are configured via **CLI flags at spawn**, matching the existing convention that `rsg-server` itself is CLI-configured by the launcher. No new scenario-file format is introduced in this phase.
  - Behaviors apply to **specific uids via uid-range flags** (e.g. `--misbehave-uids 3,7 --behavior late-abort-token`), so a single mock process/test run can exercise multiple simultaneous behaviors (one uid gets late-tokens-after-abort, another behaves normally, a third triggers the overlong-prompt drop) rather than needing one process per behavior.
  - Batched replies use a **fixed `--batch-size N` flag**: the mock accumulates up to N pending replies (flushing on a short timer if fewer are ready) before sending one `BatchTokenizerMsg`. No randomized/jittered batch timing in this phase.
- **D-10:** Prefill/decode timing is **minimal for Phase 3's own criteria**: fixed, uniform `--prefill-delay-ms` and `--decode-delay-ms` flags applied to every request the mock serves. This is enough to produce a realistic-over-time token stream for routing/dispatch and cancellation-window tests.
- **D-11:** Richer timing configurability — seeded-random delay distributions, per-uid delay overrides, replayable scenario profiles — is **explicitly deferred**. Phase 5 (128-agent cancellation stress test) and Phase 7 (benchmark harness) both reuse `mock-scheduler`, but their exact needs aren't locked yet (no CONTEXT.md/PLAN.md for those phases exists). Building that configurability now would be guessing; extend the fixed-delay flags from D-10 when those phases' own planning determines what they actually need.

### Claude's Discretion
- The exact single-writer mechanism (dedicated thread + channel vs. async task owning the socket) — D-01 fixes the guarantee, not the implementation.
- Internal module/crate layout for `mock-scheduler` (e.g. a new `crates/mock-scheduler` binary crate vs. a module inside an existing crate) and for the per-uid dispatch table.
- Exact proptest case count, shrinking configuration, and how many concurrent simulated callers the ordering test uses.
- The precise `--behavior` flag vocabulary and value syntax for `mock-scheduler` (e.g. `late-abort-token`, `drop-overlong`), as long as it covers the three behaviors named in MOCK-01.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Project scope & requirements
- `.planning/ROADMAP.md` §Phase 3: goal and the 4 success criteria this phase must satisfy.
- `.planning/REQUIREMENTS.md`: WIRE-03 and MOCK-01 full text; v1 Out of Scope table (no backend/frontend behavior changes beyond what's already shared).
- `.planning/PROJECT.md`: "Defer the Rust radix cache to v2" key decision (resolves the mock-scheduler radix-trie question — see `<domain>` above); Constraints (fair comparison, frozen frontend).
- `.claude/CLAUDE.md` §Stack Patterns: the `tx-zmq`/`rx-zmq` thread pattern ("coalesce any already-queued msgs into BatchBackendMsg"), and the `mock-scheduler` description (minus the stale radix-trie line, per `<domain>`).

### Prior phase context (carried forward)
- `.planning/phases/01-vendored-base-wire-codec/01-CONTEXT.md`: D-07 (bind/connect roles mirror upstream per endpoint) and the Claude's-Discretion note that "the full transport decision belongs to Phase 3 (WIRE-03)" — this phase is where that decision is made.

### Existing code this phase builds on
- `crates/rsg-server/src/transport.rs`: the `Transport` trait, `ZmqTransport`, and the `Endpoint`/`Role` bind-vs-connect convention from Phase 1. The single-writer and per-uid dispatch layer extend this, they don't replace it.
- `crates/rsg-wire/src/lib.rs`: `BackendMsg`, `TokenizerMsg`, `Tensor`, `SamplingParams`, and `encode_backend`/`decode_tokenizer` — the message types `mock-scheduler` and the writer/dispatcher both use unmodified.
- `crates/rsg-server/src/main.rs`: the existing CLI (`clap`) and exit-code convention (`EXIT_STARTUP`/`EXIT_BAD_HANDSHAKE`/`EXIT_STDIN_EOF`) that `mock-scheduler`'s own CLI should follow for consistency.

### Upstream source (already vendored, read-only reference)
- `vendor/mini-sglang/python/minisgl/utils/mp.py`: the real scheduler's own batching/coalescing behavior, for the writer's coalescing logic to mirror.
- `vendor/mini-sglang/python/minisgl/message/{backend,tokenizer}.py`: confirms `BatchBackendMsg`/`BatchTokenizerMsg` shapes already implemented in `rsg-wire`.

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `crates/rsg-server/src/transport.rs` — `Transport` trait already abstracts over the `zmq` crate; the single-writer and dispatcher sit behind/around this, not as a replacement.
- `crates/rsg-wire/src/lib.rs` — all wire types needed by both the real writer and `mock-scheduler` already exist and are byte-verified (Phase 1 golden fixtures); this phase does no wire-format work.
- `crates/rsg-server/src/main.rs` — CLI parsing (`clap`), exit-code convention, and the stdin-handshake/signal-handling pattern are a template for `mock-scheduler`'s own process shape.

### Established Patterns
- Workspace already pins `zmq = "0.10.0"` (not `zeromq`) — Phase 1 settled this; Phase 3 doesn't revisit the transport crate choice, only the ordering/dispatch logic built on top of it.
- `ZmqTransport::open` already encodes the bind/connect-per-endpoint convention; `mock-scheduler` reuses the same `Endpoint`/`Role` types to open its matching sockets (whichever role the real scheduler doesn't take, per Phase 1 D-07).

### Integration Points
- `mock-scheduler` is a new binary that will be spawned by Phase 5/6/7 test harnesses in place of the real backend — its CLI surface (flags from D-09/D-10) is effectively a contract those later phases depend on.
- The per-uid dispatch table built in this phase (D-04/D-05) is the layer Phase 5's request-lifecycle FSM registers/deregisters against; Phase 5 does not rebuild uid routing, it consumes what Phase 3 exposes.

</code_context>

<specifics>
## Specific Ideas

- The slow-consumer drop policy (D-05/D-06) was chosen specifically so a later parity-debugging session (Phase 6) never confuses "we dropped an old token on purpose" with "the backend did something wrong" — the dropped-count signal exists for that distinguishing purpose, not just as a nice-to-have metric.
- The "minimal now, extend later" stance on mock timing (D-10/D-11) is a deliberate anti-speculation choice: Phase 5 and Phase 7 don't have locked plans yet, so building their timing needs now would be guessing.

</specifics>

<deferred>
## Deferred Ideas

- **Richer mock-scheduler timing** (seeded random delay distributions, per-uid delay overrides, replayable scenario profiles) — revisit when Phase 5 (cancellation stress test) or Phase 7 (benchmark harness) plans actually specify what they need (D-11).
- **Configurable per-uid channel bound** — if a later phase's stress/benchmark scenario needs to tune the backpressure channel size, add the knob then (D-07).

### Reviewed Todos (not folded)
None — no pending todos matched this phase.

</deferred>

---

*Phase: 03-zmq-transport-mock-scheduler*
*Context gathered: 2026-10-05*
