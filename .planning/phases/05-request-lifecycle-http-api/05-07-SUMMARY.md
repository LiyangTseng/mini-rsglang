---
phase: 05-request-lifecycle-http-api
plan: 07
subsystem: api
tags: [stress-test, cancellation, lifecycle-fsm, tokio, rust-frontend]

requires:
  - phase: 05-03
    provides: "rsg_server::http::{chat, generate, models}; the streaming/non-streaming chat route this plan's non-streaming disconnect and LIFE-04 tests exercise"
  - phase: 05-04
    provides: "rsg_server::engine cancellation, abort-timing, overlong-prompt rejection and backend-inactivity timeout; the driver this plan's stress test proves under 128-way concurrency"
provides:
  - "crates/rsg-server/tests/http_nonstream.rs — the non-streaming disconnect D-03 measurement, and LIFE-04 coverage on the chat route (504, 400 in both stream modes, truncated stream)"
  - "crates/rsg-server/tests/stress_128.rs — a minimal, Phase-5-only 128-agent concurrent-cancellation correctness stress test (D-04), proving LIFE-03 end to end against the mock"
  - "scripts/check_all.sh's open-file soft-limit raise, so cargo test --workspace can run the 128-agent test without macOS's default 256-fd ceiling"
affects: [06, 07]

actuals:
  tokens: 7233
  tasks: 3
  commits: 3
  plan_head_before: 297ba5b947a5685bd509426e99ad94cef5560c4f
  plan_head_after: 99703b96a0f7a0d3dca891555d4a5d3bb99deb02

tech-stack:
  added: []
  patterns:
    - "A test-local splitmix64 PRNG (no new crate) fully precomputes every request's plan (endpoint, max_tokens, think time, disconnect mode) from one seeded draw sequence before any agent task is spawned, so the random choices are reproducible regardless of how the 128 concurrent tasks actually interleave at runtime"
    - "A disconnect-mode request uses a raw tokio::net::TcpStream (not tests/common's OpenStream/http_client) whenever the test needs to abandon a connection before the server would ever send response headers — a non-streaming chat response has no headers until the whole generation is ready, so OpenStream::open (which blocks on reading the status line) cannot model an early disconnect for it"

key-files:
  created:
    - crates/rsg-server/tests/http_nonstream.rs
    - crates/rsg-server/tests/stress_128.rs
  modified:
    - scripts/check_all.sh

key-decisions:
  - "No driver fix was needed in engine.rs: the stress test's own assertions (no leaks, no invalid transitions, no duplicate submits, every abort preceded by its own submit, byte-exact echo under concurrency) passed on the first real run and stayed stable across repeated runs and multiple seeds (0x1, 0x2a, 0xf423f, 0x75bcd15 and more), so plan 05-04's existing cancellation/abort-timing/timeout logic is what's being proven here, not patched"
  - "Mode::DisconnectAfterK draws both a chunk count (k, for /generate and chat-stream) and a non-stream disconnect delay (non_stream_ms, for chat non-stream) on every draw, regardless of which the selected endpoint will actually use — keeps the PRNG's draw sequence fixed per request regardless of which endpoint was chosen earlier in that same draw, so the seed's requests are the same requests on every run"
  - "The open-file soft-limit raise in check_all.sh is guarded with `|| true`: a shell whose hard limit is itself below 4096 would otherwise fail the whole gate under `set -e` on an unrelated, earlier step than the one that actually needs the higher limit; the stress test's own guard (a clear panic naming the exact `ulimit -n` command to run) is the one that should report this, not a hard-to-diagnose script abort"

patterns-established:
  - "A disconnect-mode agent in a concurrency stress test must pick its I/O primitive (OpenStream vs. a raw TcpStream) based on when the server actually commits to writing response headers for that route, not just on 'streaming vs. non-streaming' as a label — a non-streaming JSON response in this codebase only gets headers once the full body is ready, which a naive OpenStream::open would block on"

requirements-completed: [LIFE-03, LIFE-01, LIFE-02, LIFE-04]

coverage:
  - id: D1
    description: "A non-streaming client disconnect mid-decode ends in exactly one terminal state (cancelled or finished, never both), bounded by the D-03 accepted limitation (hyper only notices a dead connection on its next write) rather than eliminated by it"
    requirement: "LIFE-02"
    verification:
      - kind: integration
        ref: "crates/rsg-server/tests/http_nonstream.rs#tracer_nonstream_disconnect_reaches_one_terminal_state"
        status: pass
    human_judgment: false
  - id: D2
    description: "The chat route fails loudly and correctly: a silent backend gets 504 backend_timeout, an overlong prompt gets 400 invalid_request_error before any data line in both stream modes (and never reaches the backend), and a streaming request whose backend goes silent ends without the finish_reason: \"stop\" chunk or data: [DONE] — a truncated stream can never be mistaken for a complete one"
    requirement: "LIFE-04"
    verification:
      - kind: integration
        ref: "crates/rsg-server/tests/http_nonstream.rs#nonstream_backend_timeout_gets_504"
        status: pass
      - kind: integration
        ref: "crates/rsg-server/tests/http_nonstream.rs#chat_overlong_gets_400_in_both_modes"
        status: pass
      - kind: integration
        ref: "crates/rsg-server/tests/http_nonstream.rs#chat_stream_timeout_ends_without_stop_chunk_or_done"
        status: pass
    human_judgment: false
  - id: D3
    description: "128 concurrent agents, each making 2 requests with a seeded-random endpoint, max_tokens, think time and abort-after-N-chunks/immediate-disconnect behavior, all finish within 60s against the mock with no leaked requests (active 0), no invalid transitions, every received request reaching exactly one terminal state, no uid ever submitted twice, every observed backend abort preceded by its own submit, and the server still serving one more request afterward"
    requirement: "LIFE-03"
    verification:
      - kind: integration
        ref: "crates/rsg-server/tests/stress_128.rs#stress_128_concurrent_requests_with_random_cancellations"
        status: pass
    human_judgment: false
  - id: D4
    description: "Every completed-mode stress response (across /generate, chat streaming and chat non-streaming) is checked byte-for-byte against its own request's echo, with late-abort-token enabled on every uid so late tokens from other aborted requests are always in flight while other uids stream — proves no cross-request token leakage under 128-way concurrency and cancellation races"
    requirement: "LIFE-01"
    verification:
      - kind: integration
        ref: "crates/rsg-server/tests/stress_128.rs#stress_128_concurrent_requests_with_random_cancellations"
        status: pass
    human_judgment: false

duration: 50min
completed: 2026-10-06
status: complete
---

# Phase 5 Plan 7: Non-Streaming Disconnect Bound, Chat-Route LIFE-04, and the 128-Agent Stress Test Summary

**A 128-agent, 256-request concurrent-cancellation stress test proves LIFE-03 end to end against the mock (no leaks, no stuck connections, exactly one terminal state each, no cross-request token leakage), alongside the measured non-streaming disconnect bound (D-03) and the chat route's 504/400/truncated-stream LIFE-04 coverage — all passing against the existing engine.rs/chat.rs implementation with no production-code changes required.**

## Performance

- **Duration:** ~50 min
- **Completed:** 2026-10-06
- **Tasks:** 3
- **Files modified:** 3 (2 created, 1 modified)

## Accomplishments
- `tracer_nonstream_disconnect_reaches_one_terminal_state` measures the D-03 gap for a non-streaming chat request: a raw TCP disconnect 200ms into a 2s decode reaches exactly one terminal state (consistently `cancelled` on this Mac, across repeated runs), with `invalid_transitions` 0 and `failed` 0, and prints the actual outcome and elapsed time rather than asserting a single hardcoded path.
- Three new tests close LIFE-04 on the chat route — a silent backend gets 504 `backend_timeout` with `Submit` then `Abort` observed on the backend; an overlong chat prompt gets 400 `invalid_request_error` before any data line in both stream modes and never reaches the backend; a streaming chat request whose backend goes silent ends without the stop chunk or `data: [DONE]`, so a truncated stream can never look complete. All three passed against the existing `engine.rs`/`chat.rs` implementation from plans 05-03/05-04 — no production code changes were needed.
- `stress_128_concurrent_requests_with_random_cancellations` spawns 128 concurrent agents (256 total requests) across `/generate`, chat streaming and chat non-streaming, each request randomly complete, disconnected mid-stream after a random chunk count (or random delay for non-streaming), or disconnected immediately after the request bytes are written. Every completed-mode response is checked byte-for-byte against its own request's expected echo, with every uid flagged `late-abort-token` so late tokens are always racing live uids' own streams. The run consistently ends with zero leaked requests, zero invalid transitions, no uid submitted twice, every observed abort preceded by its own submit, and the server still serving a final sanity request afterward — stable across repeated runs and multiple PRNG seeds.
- `scripts/check_all.sh` now raises the open-file soft limit to 4096 before `cargo test --workspace` when it's lower, so the 128-client/128-server/mock-fd load doesn't hit macOS's default 256-fd ceiling; the raise is guarded so a shell with a lower hard limit doesn't fail the whole gate (the stress test's own guard reports that case with a precise remediation command).

## Task Commits

Each task was committed atomically:

1. **Task 1: Tracer — non-streaming disconnect reaches one terminal state within the D-03 bound** - `3e32a7c` (feat)
2. **Task 2: LIFE-04 on the chat route (504, 400 in both modes, truncated stream)** - `1f20eee` (feat)
3. **Task 3: 128-agent stress test with random cancellations, plus the check_all.sh open-file guard** - `99703b9` (test)

**Plan metadata:** (this commit)

## Files Created/Modified
- `crates/rsg-server/tests/http_nonstream.rs` — `tracer_nonstream_disconnect_reaches_one_terminal_state`, `nonstream_backend_timeout_gets_504`, `chat_overlong_gets_400_in_both_modes`, `chat_stream_timeout_ends_without_stop_chunk_or_done`
- `crates/rsg-server/tests/stress_128.rs` — `stress_128_concurrent_requests_with_random_cancellations`, `STRESS_SEED`, the test-local `SplitMix64` PRNG, `RSG_STRESS_SEED` env override, the open-file soft-limit guard
- `scripts/check_all.sh` — raises the open-file soft limit to 4096 before `cargo test --workspace` when it's below that, guarded with `|| true`

## Decisions Made
- No driver fix was needed in `engine.rs`: every stress-test assertion (no leaks, no invalid transitions, no duplicate submits, abort-after-submit ordering, byte-exact echo under concurrency) passed on the first real run and stayed stable across repeated runs and four different seeds — plan 05-04's existing cancellation/abort-timing/timeout logic is what this test proves, not what it had to fix.
- `Mode::DisconnectAfterK` always draws both its chunk count (`k`) and its non-stream delay (`non_stream_ms`) from the PRNG on every request, regardless of which the chosen endpoint actually uses, so the draw sequence per request is fixed and the same seed always produces the same 256 requests.
- A disconnect-mode agent picks its I/O primitive based on when the server actually commits to writing response headers, not just "streaming vs. non-streaming" as a label: `/generate` and chat-stream use `OpenStream` (headers arrive right after the `Accepted` event, well before any token), while chat non-stream uses a raw `TcpStream` (its JSON response has no headers until the whole generation is ready, so `OpenStream::open` would block past the intended disconnect point).
- The `check_all.sh` open-file raise is guarded with `|| true`: a shell whose hard limit is itself below 4096 would otherwise abort the whole gate under `set -e`, with a message that doesn't point at the actual cause; the stress test's own guard (a `panic!` naming the exact `ulimit -n 4096` remediation) is the one that should report this case.

## Deviations from Plan

None — plan executed exactly as written. All four tasks' tests passed against the existing implementation from plans 05-03/05-04 on the first real run; no production code in `engine.rs` or `chat.rs` needed any change.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- LIFE-01 through LIFE-04 and criterion 3 (128 concurrent agents, dynamic requests/cancellations) are now proven end to end against the mock; Phase 5's success criteria for the request-lifecycle FSM and HTTP API are fully covered.
- The stress test is explicitly a throwaway, Phase-5-only correctness check (D-04): Phase 7's benchmark harness should not extend or reuse `stress_128.rs` as its load generator, nor cite any of its timings as performance evidence — it builds its own instrumented harness per CONTEXT D-04.
- `scripts/check_all.sh`'s open-file raise is now in place for any future test that needs more than macOS's default 256-fd ceiling; no further action needed for this plan's own test.
- No blockers.

## Self-Check: PASSED

All created/modified files verified present on disk; all 3 task commits
(`3e32a7c`, `1f20eee`, `99703b9`) verified present in `git log`.

---
*Phase: 05-request-lifecycle-http-api*
*Completed: 2026-10-06*
