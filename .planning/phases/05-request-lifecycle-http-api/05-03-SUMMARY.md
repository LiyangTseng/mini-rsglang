---
phase: 05-request-lifecycle-http-api
plan: 03
subsystem: api
tags: [axum, sse, openai-compat, json-escaping, rust-frontend]

requires:
  - phase: 05-01
    provides: "rsg_server::http::{AppState, router, serve, parse_json, ApiError, EVENT_STREAM_CONTENT_TYPE}, rsg_server::engine::{Engine, ActiveRequest, RequestEvent}, rsg_server::codec::{Prompt, ChatMessage, ChatRole}, tests/common's TestServer/http_client/ByteCodec harness"
provides:
  - "rsg_server::http::pyjson (push_ascii_json_str, chat_stream_chunk — Python json.dumps(ensure_ascii=True) parity)"
  - "rsg_server::http::chat (POST /v1/chat/completions, streaming and non-streaming)"
  - "rsg_server::http::models (GET /v1/models, multi-method /v1)"
affects: [05-04, 05-05, 05-06, 05-07, 05-08, 05-09]

actuals:
  tokens: 9041
  tasks: 3
  commits: 4
  plan_head_before: 61872820950beb5ab8b260a77452d328c47fe71a
  plan_head_after: 311cde22378630db3fd0d57e9123c130c406d7f8

tech-stack:
  added: []
  patterns:
    - "Python json.dumps(ensure_ascii=True) string escaping hand-rolled in pyjson.rs (never serde_json) for every streaming chat chunk; the non-streaming response goes through plain serde_json/axum::Json instead, matching Starlette's ensure_ascii=False JSONResponse"
    - "Every upstream-parity route calls state.engine()? first, before any other work, so not-ready (503) is structurally guaranteed to precede any uid allocation or body parsing past that point"

key-files:
  created:
    - crates/rsg-server/src/http/pyjson.rs
    - crates/rsg-server/src/http/chat.rs
    - crates/rsg-server/src/http/models.rs
    - crates/rsg-server/tests/http_chat.rs
    - crates/rsg-server/tests/http_models.rs
  modified:
    - crates/rsg-server/src/http/mod.rs

key-decisions:
  - "list_models is pub(crate), not pub like every other handler in this crate — its return type exposes the crate-private ModelList struct, and axum's router registration only needs same-crate visibility"
  - "The non-streaming chat_completions branch keeps the ActiveRequest (and its AbortGuard) alive in the handler's own async fn future — no into_parts()/background stream — so a dropped handler future (client disconnect before the response is ready) cancels the request the same way the streaming branch's body-stream guard does"
  - "chat.rs's #[cfg(test)] mod tests lives after chat_completions (not before, where it was first drafted), to satisfy clippy::items_after_test_module under -D warnings"

patterns-established:
  - "chat_stream_chunk(uid, role, content, finish_stop) is the single builder for every streaming chat chunk's JSON text; no call site hand-assembles that JSON directly"

requirements-completed: [API-01]

coverage:
  - id: D1
    description: "POST /v1/chat/completions with stream=true returns upstream's exact byte-for-byte `data: <json>\\n\\n` chunk framing: Python json.dumps(ensure_ascii=True) escaping, role only on the first delta, content omitted when empty, a closing finish_reason:\"stop\" chunk, then data: [DONE]\\n\\n"
    requirement: "API-01"
    verification:
      - kind: integration
        ref: "crates/rsg-server/tests/http_chat.rs#tracer_chat_stream_matches_upstream_framing"
        status: pass
      - kind: integration
        ref: "crates/rsg-server/tests/http_chat.rs#chat_stream_escapes_non_ascii_and_omits_empty_content"
        status: pass
      - kind: integration
        ref: "crates/rsg-server/tests/http_chat.rs#chat_default_max_tokens_is_16"
        status: pass
      - kind: unit
        ref: "crates/rsg-server/src/http/pyjson.rs#tests::escapes_like_python_json_dumps"
        status: pass
      - kind: unit
        ref: "crates/rsg-server/src/http/pyjson.rs#tests::chunk_shapes"
        status: pass
    human_judgment: false
  - id: D2
    description: "POST /v1/chat/completions with stream=false returns upstream's hardcoded non-streaming shape: chatcmpl- id, chat.completion object, live created, echoed model, one assistant-role choice with finish_reason always \"stop\", and usage always zero"
    requirement: "API-01"
    verification:
      - kind: integration
        ref: "crates/rsg-server/tests/http_chat.rs#chat_nonstream_response_shape"
        status: pass
    human_judgment: false
  - id: D3
    description: "Chat request defaults (max_tokens 16, temperature 1.0, top_k -1, top_p 1.0, stream false, ignore_eos false; n/stop/presence_penalty/frequency_penalty accepted and ignored) mirror OpenAICompletionRequest; a non-empty messages list wins over prompt, an empty one falls back to prompt, and neither gives 500 without consuming a uid"
    requirement: "API-01"
    verification:
      - kind: unit
        ref: "crates/rsg-server/src/http/chat.rs#tests::defaults_mirror_upstream"
        status: pass
      - kind: integration
        ref: "crates/rsg-server/tests/http_chat.rs#chat_prompt_text_used_when_no_messages"
        status: pass
      - kind: integration
        ref: "crates/rsg-server/tests/http_chat.rs#chat_empty_messages_falls_back_to_prompt"
        status: pass
      - kind: integration
        ref: "crates/rsg-server/tests/http_chat.rs#chat_missing_prompt_is_500_and_consumes_no_uid"
        status: pass
    human_judgment: false
  - id: D4
    description: "GET /v1/models and /v1 (GET/POST/HEAD/OPTIONS) return upstream's exact shapes; a role outside system/user/assistant, a missing model, or stop:null gets 422 and consumes no uid; every upstream-parity route returns 503 not_ready while the engine is unset"
    requirement: "API-01"
    verification:
      - kind: integration
        ref: "crates/rsg-server/tests/http_models.rs#models_list_shape"
        status: pass
      - kind: integration
        ref: "crates/rsg-server/tests/http_models.rs#v1_root_all_methods"
        status: pass
      - kind: integration
        ref: "crates/rsg-server/tests/http_models.rs#validation_422_cases_consume_no_uid"
        status: pass
      - kind: integration
        ref: "crates/rsg-server/tests/http_models.rs#not_ready_routes_return_503"
        status: pass
    human_judgment: false

duration: 25min
completed: 2026-10-06
status: complete
---

# Phase 5 Plan 3: Chat Completions, Models and the /v1 Route Summary

**`/v1/chat/completions` (streaming and non-streaming), `/v1/models` and `/v1` on the Rust frontend, with a hand-rolled Python `json.dumps(ensure_ascii=True)` encoder so every streaming chat chunk is byte-identical to upstream's own `json.dumps` output.**

## Performance

- **Duration:** ~25 min
- **Completed:** 2026-10-06
- **Tasks:** 3
- **Files modified:** 6 (5 created, 1 modified)

## Accomplishments
- A client can `POST /v1/chat/completions` with `stream=true` against the Rust frontend (backed by `mock-scheduler`) and get back upstream's exact chunk bytes: `cmpl-<uid>` ids, the `text_completion.chunk` object tag, role only on the first delta, content omitted when empty, a closing `finish_reason: "stop"` chunk, then `data: [DONE]\n\n` — proven byte-for-byte by the tracer test and further exercised by a non-ASCII/empty-content case and a default-`max_tokens`-of-16 case.
- `stream=false` returns upstream's hardcoded non-streaming shape (`chatcmpl-` id, `usage` always zero, `finish_reason` always `"stop"`) while keeping the `ActiveRequest`'s `AbortGuard` alive for the whole collection loop, so a client disconnect before the response is ready still cancels the backend request.
- `GET /v1/models` and the multi-method `/v1` route (GET/POST/HEAD/OPTIONS) are in place with upstream's exact field order and values, and every one of these four routes (plus `/generate` from plan 05-01) now provably returns `503 not_ready` before the engine is set and `422` without consuming a uid on request-shape violations (bad role, missing model, `stop: null`).
- `pyjson::push_ascii_json_str`/`chat_stream_chunk` is the single, unit-tested encoder every streaming chunk's JSON text goes through — never `serde_json`, which does not escape non-ASCII characters the way CPython's `json.dumps(ensure_ascii=True)` does.

## Task Commits

Each task was committed atomically (Task 2 followed the TDD RED/GREEN cycle per its `tdd="true"` attribute):

1. **Task 1: Tracer — streaming /v1/chat/completions with upstream's exact chunk bytes** - `db391ca` (feat)
2. **Task 2: Python json.dumps escaping, non-streaming chat, prompt selection and defaults (RED)** - `910425b` (test)
3. **Task 2: Python json.dumps escaping, non-streaming chat, prompt selection and defaults (GREEN)** - `a1bc3e0` (feat)
4. **Task 3: /v1/models, the multi-method /v1 route, 422 validation and not-ready responses** - `311cde2` (feat)

**Plan metadata:** (this commit)

## Files Created/Modified
- `crates/rsg-server/src/http/pyjson.rs` - `push_ascii_json_str` (Python `json.dumps(ensure_ascii=True)` string escaping) and `chat_stream_chunk` (the streaming chunk JSON builder)
- `crates/rsg-server/src/http/chat.rs` - `ChatCompletionRequest` (upstream's `OpenAICompletionRequest` defaults), `prompt()`/`sampling_params()`, the streaming and non-streaming `chat_completions` handler
- `crates/rsg-server/src/http/models.rs` - `list_models` (`GET /v1/models`) and `v1_root` (`/v1`, `{"status":"ok"}`), both gated on `state.engine()?` first
- `crates/rsg-server/src/http/mod.rs` - register `chat`/`models`/`pyjson` modules; route `POST /v1/chat/completions`, `GET /v1/models`, and `/v1` with get/post/head/options
- `crates/rsg-server/tests/http_chat.rs` - tracer plus 6 integration tests (non-streaming shape, prompt/messages fallback, missing-prompt 500, default max_tokens, non-ASCII escaping)
- `crates/rsg-server/tests/http_models.rs` - `models_list_shape`, `v1_root_all_methods`, `validation_422_cases_consume_no_uid`, `not_ready_routes_return_503`

## Decisions Made
- `list_models` is `pub(crate)`, not `pub` like every other handler in this crate, because its return type (`Json<ModelList>`) exposes the crate-private `ModelList` struct — Rust's privacy rules reject a `pub` function with a non-`pub` type in its signature, and axum's router registration inside `http/mod.rs` only needs same-crate visibility.
- The non-streaming branch of `chat_completions` calls `active.next_event()` directly on the still-owned `ActiveRequest` inside the handler's own `async fn` body, rather than destructuring into parts and spawning a background stream (the streaming branch's approach) — this keeps the `AbortGuard` alive for exactly as long as the handler future runs, so a dropped handler future (client disconnect before the response is ready) cancels the backend request the same way the streaming branch's body-stream guard does.
- `chat.rs`'s `#[cfg(test)] mod tests` block lives after `chat_completions` (moved there during Task 2's GREEN phase), not before it as first drafted — `clippy::items_after_test_module` (enabled under `-D warnings`) rejects items declared after a test module.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Targeted `#[allow(dead_code)]` on fields read only by this module's own unit test**
- **Found during:** Task 1 (placeholder non-streaming stub) and Task 2 (GREEN implementation)
- **Issue:** `cargo clippy -p rsg-server --all-targets -- -D warnings` compiles the plain `lib` target without `#[cfg(test)]`; `ChatCompletionRequest`'s `n`, `stop`, `presence_penalty` and `frequency_penalty` fields are (by design, matching upstream's own TODO) never read by `chat_completions` itself, only by `chat::tests::defaults_mirror_upstream` — so the plain-lib compile reported them as dead code and failed the plan's own clippy gate.
- **Fix:** Added `#[allow(dead_code)]` to each of those four fields (not the whole struct) with a doc comment explaining why.
- **Files modified:** `crates/rsg-server/src/http/chat.rs`
- **Verification:** `cargo clippy -p rsg-server --all-targets -- -D warnings` exits 0.
- **Committed in:** `a1bc3e0` (Task 2 GREEN commit)

**2. [Rule 3 - Blocking] Moved the `chat::tests` module to the end of the file**
- **Found during:** Task 2 (GREEN implementation)
- **Issue:** The test module was first drafted directly after `ChatCompletionRequest`'s `impl` block, ahead of `chat_completions` and its supporting types. `clippy::items_after_test_module` (enabled under `-D warnings`) rejects any item declared after a `#[cfg(test)] mod tests` block.
- **Fix:** Moved the test module to the end of the file, after `chat_completions`.
- **Files modified:** `crates/rsg-server/src/http/chat.rs`
- **Verification:** `cargo clippy -p rsg-server --all-targets -- -D warnings` exits 0; all unit and integration tests still pass.
- **Committed in:** `a1bc3e0` (Task 2 GREEN commit)

**3. [Rule 1 - Bug] Narrowed `list_models`'s visibility from `pub` to `pub(crate)`**
- **Found during:** Task 3
- **Issue:** `pub async fn list_models(...) -> Result<Json<ModelList>, ApiError>` with a private (default-visibility) `ModelList` struct is a compile error (`private_interfaces`/"type is private") the moment another module (`http/mod.rs`'s router) references the function at `pub` visibility.
- **Fix:** Changed `list_models` to `pub(crate)`, matching `ModelList`'s own `pub(crate)` visibility; router registration only needs same-crate access.
- **Files modified:** `crates/rsg-server/src/http/models.rs`
- **Verification:** `cargo build --workspace` and `cargo clippy -p rsg-server --all-targets -- -D warnings` both exit 0.
- **Committed in:** `311cde2` (Task 3 commit)

---

**Total deviations:** 3 auto-fixed (2 blocking clippy-gate fixes, 1 compile-error bug fix)
**Impact on plan:** All three are narrow compliance/compile fixes required to satisfy the plan's own stated verification commands (`cargo clippy -p rsg-server --all-targets -- -D warnings`, `cargo build --workspace`). No behavior changes, no scope creep.

## Issues Encountered
- **Task 1's tracer requirement overlapped with Task 2's TDD behavior tests.** Task 1 (type="tracer") needed the full `pyjson` escaping/chunk-builder implementation and the complete streaming branch (including `ChatCompletionRequest::prompt()`/`sampling_params()`) to pass its own byte-exact test — which meant several of Task 2's listed behavior tests (`pyjson::tests::escapes_like_python_json_dumps`, `pyjson::tests::chunk_shapes`, `chat::tests::defaults_mirror_upstream`, and 3 of the 6 new integration tests) were already green the moment they were written, before any Task-2-specific implementation code existed. This was investigated per the TDD "unexpected GREEN" rule and found to be the expected shape of this plan (Task 1 and Task 2 share the same `ChatCompletionRequest`/streaming code), not a sign of a wrong or pre-existing test — the RED-evidence commit documents which 3 of the 7 `http_chat.rs` tests genuinely failed against the non-streaming stub (the right assertion: `500` vs expected `200`, never a compile error or unrelated panic) before Task 2's GREEN implementation.
- A test-authoring mistake was caught and fixed before any RED-evidence was recorded: the first draft of `chat_stream_escapes_non_ascii_and_omits_empty_content` pasted a literal `é` character into a Rust raw string instead of the escaped `é` text it was meant to assert against the server's JSON output, which made the assertion fail for the wrong reason. Fixed immediately (before the RED commit) by rewriting the assertion with an explicit escaped string literal.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- `rsg_server::http::pyjson::{push_ascii_json_str, chat_stream_chunk}` and `rsg_server::http::chat`/`models` are locked in exactly as this plan's interfaces-block contract specifies; the only remaining upstream-parity routes from the phase's success criteria (`/generate`, `/v1/chat/completions`, `/v1/models`, `/v1`) are now all in place.
- The full byte-level diff against live Python-frontend fixtures (D-02, plan 05-09) is still outstanding — this plan's tests prove self-consistency against the hand-read `api_server.py` source and the mock, not an actual fixture-diff run.
- No blockers. Plans 05-04 through 05-09 (cancellation/abort-timing, errors/timeouts, observability, the 128-agent stress test, and Phase 4's tokenizer wiring) can build directly on this plan's routes without re-opening their signatures.

## Self-Check: PASSED

---
*Phase: 05-request-lifecycle-http-api*
*Completed: 2026-10-06*
