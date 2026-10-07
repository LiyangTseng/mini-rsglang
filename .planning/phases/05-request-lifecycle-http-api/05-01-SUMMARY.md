---
phase: 05-request-lifecycle-http-api
plan: 01
subsystem: api
tags: [axum, tokio, http, sse, lifecycle-fsm, rust-frontend]

requires:
  - phase: 03-zmq-transport-mock-scheduler
    provides: "WriterHandle::submit/abort, DispatchHandle::register/deregister/stats, mock-scheduler subprocess harness"
provides:
  - "rsg_server::codec (TextCodec/IncrementalDecoder seam Phase 4 plugs into)"
  - "rsg_server::engine (Engine/ActiveRequest/RequestEvent, per-request driver task)"
  - "rsg_server::fsm (LifecycleState transition table + registry actor, LIFE-01 accounting)"
  - "rsg_server::http (AppState, router, POST /generate with upstream single-newline framing)"
  - "tests/common/{http_client,test_server}.rs shared HTTP test harness (reused by plans 05-03/04/06/07)"
affects: [05-02, 05-03, 05-04, 05-05, 05-06, 05-07, 05-08, 05-09]

actuals:
  tokens: 21463
  tasks: 2
  commits: 3
  plan_head_before: 80b7d204ae6fba531b0737d901d44f331d0e8f57
  plan_head_after: be3c768d6bb923e9fc65da39f6ada99a3176cd93

tech-stack:
  added: ["axum 0.8.9", "tokio-util 0.7.19", "futures 0.3 (workspace)"]
  patterns:
    - "Drop-based cancellation (AbortGuard wrapping tokio_util::sync::DropGuard), wired through but not yet acted on (plan 05-04)"
    - "One registry actor (tokio task owning FxHashMap<i64, ReqState>) is the single source of truth for LIFE-01 accounting; every lifecycle transition reports through one `finish` helper so each driver exit path reports exactly one terminal"
    - "Raw HTTP/1.1 test client with split TcpStream halves + tokio::join! for concurrent write/read (needed so a body large enough to trip DefaultBodyLimit doesn't deadlock on TCP backpressure)"

key-files:
  created:
    - crates/rsg-server/src/codec.rs
    - crates/rsg-server/src/engine.rs
    - crates/rsg-server/src/fsm/mod.rs
    - crates/rsg-server/src/fsm/state.rs
    - crates/rsg-server/src/http/mod.rs
    - crates/rsg-server/src/http/error.rs
    - crates/rsg-server/src/http/generate.rs
    - crates/rsg-server/tests/common/http_client.rs
    - crates/rsg-server/tests/common/test_server.rs
    - crates/rsg-server/tests/http_generate.rs
  modified:
    - Cargo.toml
    - Cargo.lock
    - crates/rsg-server/Cargo.toml
    - crates/rsg-server/src/lib.rs
    - crates/rsg-server/tests/common/mod.rs

key-decisions:
  - "Engine::new takes (writer, dispatch, codec, registry, config) in that order; the driver reports Received/Tokenizing/Submitted/Decoding/one-terminal through a single `finish` helper so LIFE-01's exactly-one-terminal invariant is structural, not a convention each call site has to remember"
  - "fsm::state::can_transition is an explicit match over all 12 allowed (from, to) pairs rather than a table/lookup structure, so the transition table reads as a spec in the source"
  - "The registry actor removes a uid's entry from its table the instant it reaches a terminal state; `active` is simply the map length at snapshot time, so a leaked or double-terminated request is directly visible as a non-zero active or invalid_transitions count"
  - "http_client::send() writes and reads concurrently via tokio::join! on split OwnedReadHalf/OwnedWriteHalf, keeping both halves alive in the same stack frame until the response is fully read — OwnedWriteHalf shuts down the connection's write direction on drop, and doing that before the server finishes its response caused the server to discard the in-flight response as an early client disconnect (found and fixed during this plan, see Deviations)"

patterns-established:
  - "AbortGuard lives in engine.rs (not http/), wrapping tokio_util::sync::DropGuard with no public methods beyond construction, matching the project's minimal-wrapper-type convention"
  - "ByteCodec/ByteDecoder test double lives only in tests/common/test_server.rs, never in the production crate; it is the Phase-4-shaped seam every later plan's tests reuse via TestServer::start"

requirements-completed: [LIFE-01, API-01]

coverage:
  - id: D1
    description: "POST /generate streams upstream's exact single-newline data: framing end to end (HTTP -> engine driver -> writer -> mock-scheduler -> dispatcher -> decoder -> body stream)"
    requirement: "API-01"
    verification:
      - kind: integration
        ref: "crates/rsg-server/tests/http_generate.rs#tracer_generate_streams_echo_tokens_end_to_end"
        status: pass
      - kind: integration
        ref: "crates/rsg-server/tests/http_generate.rs#single_token_request_has_one_data_line"
        status: pass
      - kind: integration
        ref: "crates/rsg-server/tests/http_generate.rs#multibyte_char_split_across_tokens_streams_an_empty_chunk_then_the_char"
        status: pass
      - kind: integration
        ref: "crates/rsg-server/tests/http_generate.rs#empty_prompt_is_forwarded_and_finishes"
        status: pass
    human_judgment: false
  - id: D2
    description: "Every request moves through received -> tokenizing -> submitted -> decoding -> finished/cancelled/failed; the transition table allows exactly 12 of 49 pairs and rejects every exit from a terminal state"
    requirement: "LIFE-01"
    verification:
      - kind: unit
        ref: "crates/rsg-server/src/fsm/state.rs#tests::transition_table_is_exact"
        status: pass
      - kind: unit
        ref: "crates/rsg-server/src/fsm/state.rs#tests::advance_records_first_token_and_rejects_terminal_exit"
        status: pass
    human_judgment: false
  - id: D3
    description: "The registry actor counts a second terminal report, or any report for an unknown/already-terminal uid, as an invalid transition instead of a second terminal, and active reflects only truly live requests"
    requirement: "LIFE-01"
    verification:
      - kind: unit
        ref: "crates/rsg-server/src/fsm/mod.rs#tests::second_terminal_is_counted_invalid"
        status: pass
      - kind: unit
        ref: "crates/rsg-server/src/fsm/mod.rs#tests::unknown_uid_and_duplicate_received_are_invalid"
        status: pass
      - kind: unit
        ref: "crates/rsg-server/src/fsm/mod.rs#tests::active_counts_live_requests"
        status: pass
      - kind: integration
        ref: "crates/rsg-server/tests/http_generate.rs#lifecycle_counts_one_finished_request"
        status: pass
  - id: D4
    description: "uids come from one AtomicI64 counter shared by every generation endpoint; concurrent requests never leak another request's tokens into their own response"
    requirement: "LIFE-01"
    verification:
      - kind: integration
        ref: "crates/rsg-server/tests/http_generate.rs#concurrent_requests_get_only_their_own_tokens"
        status: pass
    human_judgment: false
  - id: D5
    description: "Oversized request bodies are refused with 413 before parsing; malformed JSON gets 422 without consuming a uid"
    requirement: "API-01"
    verification:
      - kind: integration
        ref: "crates/rsg-server/tests/http_generate.rs#oversized_body_gets_413"
        status: pass
      - kind: integration
        ref: "crates/rsg-server/tests/http_generate.rs#invalid_body_gets_422_and_consumes_no_uid"
        status: pass
    human_judgment: false

duration: 45min
completed: 2026-10-06
status: complete
---

# Phase 5 Plan 1: Request Lifecycle Engine & POST /generate Summary

**POST /generate streams upstream's exact single-newline SSE-like framing through a new per-request engine driver, backed by a tokio-task registry actor that proves LIFE-01's "exactly one terminal state" invariant structurally.**

## Performance

- **Duration:** ~45 min
- **Completed:** 2026-10-06
- **Tasks:** 2
- **Files modified:** 15 (10 created, 5 modified)

## Accomplishments
- A client on the Mac can `POST /generate` against the Rust HTTP layer backed by `mock-scheduler` and get back upstream's byte-exact `data: <text>\n` ... `data: [DONE]\n` framing, proven end to end by a tracer test against a real mock-scheduler subprocess.
- The `codec`/`engine`/`http` module contracts plans 05-03 through 05-09 and Phase 4's tokenizer adapter build on are now locked in (`TextCodec`, `IncrementalDecoder`, `Engine`, `ActiveRequest`, `RequestEvent`, `AbortGuard`, `ApiError`, `AppState`).
- The request lifecycle is now an explicit, unit-tested transition table (`fsm::state::can_transition`, 12 of 49 pairs) plus one registry actor that owns the system-wide table of live requests and proves exactly-one-terminal accounting — the foundation plan 05-07's 128-agent stress test (LIFE-03) relies on.
- The shared HTTP test harness (`tests/common/{http_client,test_server}.rs`) is in place for every remaining plan in this phase to extend.

## Task Commits

Each task was committed atomically (Task 2 followed the TDD RED/GREEN cycle per its `tdd="true"` attribute):

1. **Task 1: Tracer — POST /generate through HTTP, engine driver, writer, mock-scheduler, dispatcher, decoder** - `7343865` (feat)
2. **Task 2: Lifecycle transition table and registry actor (RED)** - `1050c56` (test)
3. **Task 2: Lifecycle transition table and registry actor (GREEN)** - `be3c768` (feat)

**Plan metadata:** (this commit)

## Files Created/Modified
- `crates/rsg-server/src/codec.rs` - `TextCodec`/`IncrementalDecoder` seam; `Prompt`/`ChatMessage`/`ChatRole`/`CodecError`
- `crates/rsg-server/src/engine.rs` - `Engine`, `ActiveRequest`, `RequestEvent`, `AbortGuard`, the per-request driver task; reports every lifecycle transition to the registry
- `crates/rsg-server/src/fsm/state.rs` - `LifecycleState`, `can_transition` (the 12-of-49 table), `ReqState`, `TransitionError`
- `crates/rsg-server/src/fsm/mod.rs` - `spawn_registry`, `RegistryHandle`, `RegistrySnapshot` — the registry actor
- `crates/rsg-server/src/http/mod.rs` - `AppState`, `router`, `serve`, `parse_json`, body-size/content-type constants
- `crates/rsg-server/src/http/error.rs` - `ApiError` vocabulary, status/body mapping, `From<SubmitError>`/`From<RequestError>`
- `crates/rsg-server/src/http/generate.rs` - `POST /generate` handler, upstream single-newline framing via `futures::stream::unfold` + `Body::from_stream`
- `crates/rsg-server/tests/common/http_client.rs` - raw HTTP/1.1 test client (`send`, `OpenStream`), Content-Length/chunked decoding
- `crates/rsg-server/tests/common/test_server.rs` - `TestServer`, `TestConfig`, `ByteCodec` test double, `snapshot_when_idle`
- `crates/rsg-server/tests/http_generate.rs` - tracer plus 7 lifecycle/edge-case integration tests
- `Cargo.toml` / `crates/rsg-server/Cargo.toml` / `Cargo.lock` - add axum, tokio-util, futures; add `net`/`io-util` tokio features
- `crates/rsg-server/src/lib.rs` - `pub mod codec; pub mod engine; pub mod fsm; pub mod http;`
- `crates/rsg-server/tests/common/mod.rs` - `pub mod http_client; pub mod test_server;`

## Decisions Made
- `Engine::new`'s parameter order is `(writer, dispatch, codec, registry, config)`, matching the plan's interfaces-block contract exactly, so later plans (05-02 onward) can rely on the signature staying fixed.
- The driver reports `Decoding` only once, on the first `UidEvent::Token`, guarded by a local `reported_decoding` flag — reporting it on every token would make every token after the first an invalid `Decoding -> Decoding` self-transition, since the transition table correctly rejects self-transitions.
- `http_client::send()` was redesigned mid-task (see Deviations) to write and read concurrently on split stream halves rather than sequentially, which both fixes a disconnect bug and is what makes `oversized_body_gets_413` safe to run without a hang.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] `http_client::send()`'s sequential write-then-read discarded the server's response when writing the request body alone (via a split `OwnedWriteHalf`) shut down the connection's write direction before the response was read**
- **Found during:** Task 2, while adding `oversized_body_gets_413` and friends (the Task 1 tracer test happened to pass under the original sequential, unsplit design; the bug only surfaced once a second iteration toward a split-stream, deadlock-safe design was needed for the oversized-body case)
- **Issue:** An initial `tokio::spawn`-based concurrent write/read design dropped the request's `OwnedWriteHalf` as soon as the (small, fast) write finished — `OwnedWriteHalf`'s `Drop` impl calls `shutdown(Write)` on the socket. That half-close, arriving before axum/hyper had finished producing the chunked `/generate` response, was read by the server as an early client disconnect, so hyper discarded the in-flight response and closed the connection without ever writing any bytes back (confirmed via a `curl` A/B check against the same server, which worked, and a temporary debug print showing a clean EOF with zero bytes read).
- **Fix:** Rewrote `send()` to run the write and read futures concurrently via `tokio::join!` in the same async function (no `tokio::spawn`), so both split halves stay alive in the same stack frame until *after* the response has been fully read — the eventual write-direction shutdown now happens only once the caller already has its answer.
- **Files modified:** `crates/rsg-server/tests/common/http_client.rs`
- **Verification:** All 8 `tests/http_generate.rs` tests pass, including `oversized_body_gets_413` (which needs the concurrent write to avoid deadlocking on TCP backpressure the server stops draining) and the Task 1 tracer (confirming the fix didn't regress the simple case).
- **Committed in:** `1050c56` (test commit — the fix was needed to get the RED tests running at all, before the fsm logic itself was exercised)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** The fix is confined to the test-only HTTP client; no production code changed. No scope creep — this is exactly the kind of test-harness subtlety the plan's own research flagged as needing verification ("exact axum/hyper disconnect-detection latency... not independently verified this session").

## Issues Encountered
None beyond the deviation above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- The `codec`/`engine`/`http`/`fsm` module contracts are locked in exactly as the interfaces block specifies; plans 05-02 through 05-09 (chat completions, cancellation, errors, observability, the 128-agent stress test, and Phase 4's tokenizer wiring) can build directly on `Engine::start`/`ActiveRequest`/`RegistryHandle` without re-opening these signatures.
- `AbortGuard` is wired through end-to-end but inert (the `CancellationToken` it owns is never read): plan 05-04 is expected to make the driver observe cancellation, add the backend timeout, and implement abort timing — all three already have their config fields and constants (`EngineConfig::abort_timing`, `EngineConfig::backend_timeout`, `DEFAULT_BACKEND_TIMEOUT_MS`) in place from this plan.
- No blockers. The dispatcher/writer precondition this plan's Task 1 required (Phase 3's `writer.rs`/`dispatch.rs` merged with `abort`/`deregister`/`stats`) was already satisfied at plan start.

## Self-Check: PASSED

---
*Phase: 05-request-lifecycle-http-api*
*Completed: 2026-10-06*
