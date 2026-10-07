---
phase: 05-request-lifecycle-http-api
reviewed: 2026-10-06T00:00:00Z
depth: standard
files_reviewed: 59
files_reviewed_list:
  - Cargo.lock
  - Cargo.toml
  - crates/rsg-server/Cargo.toml
  - crates/rsg-server/src/codec.rs
  - crates/rsg-server/src/engine.rs
  - crates/rsg-server/src/fsm/mod.rs
  - crates/rsg-server/src/fsm/state.rs
  - crates/rsg-server/src/hf_codec.rs
  - crates/rsg-server/src/http/chat.rs
  - crates/rsg-server/src/http/error.rs
  - crates/rsg-server/src/http/generate.rs
  - crates/rsg-server/src/http/health.rs
  - crates/rsg-server/src/http/mod.rs
  - crates/rsg-server/src/http/models.rs
  - crates/rsg-server/src/http/pyjson.rs
  - crates/rsg-server/src/lib.rs
  - crates/rsg-server/src/main.rs
  - crates/rsg-server/src/metrics.rs
  - crates/rsg-server/tests/abort_timing.rs
  - crates/rsg-server/tests/api_parity.rs
  - crates/rsg-server/tests/cli.rs
  - crates/rsg-server/tests/common/http_client.rs
  - crates/rsg-server/tests/common/mod.rs
  - crates/rsg-server/tests/common/rsg_process.rs
  - crates/rsg-server/tests/common/test_server.rs
  - crates/rsg-server/tests/http_cancellation.rs
  - crates/rsg-server/tests/http_chat.rs
  - crates/rsg-server/tests/http_errors.rs
  - crates/rsg-server/tests/http_generate.rs
  - crates/rsg-server/tests/http_models.rs
  - crates/rsg-server/tests/http_nonstream.rs
  - crates/rsg-server/tests/observability.rs
  - crates/rsg-server/tests/server_binary.rs
  - crates/rsg-server/tests/stress_128.rs
  - crates/rsg-server/tests/transport_misbehavior_e2e.rs
  - fixtures/api/chat_nonstream_eos_final.body
  - fixtures/api/chat_nonstream_multibyte.body
  - fixtures/api/chat_nonstream_prompt_text.body
  - fixtures/api/chat_stream_after_errors.body
  - fixtures/api/chat_stream_default_max_tokens.body
  - fixtures/api/chat_stream_empty_messages_prompt.body
  - fixtures/api/chat_stream_system_user.body
  - fixtures/api/generate_ascii.body
  - fixtures/api/generate_multibyte.body
  - fixtures/api/generate_one_token.body
  - fixtures/api/manifest.json
  - fixtures/api/models_list.body
  - fixtures/api/v1_get.body
  - fixtures/api/v1_head.body
  - fixtures/api/v1_options.body
  - fixtures/api/v1_post.body
  - python/rsglang/sockets.py
  - python/rsglang/testing/python_frontend.py
  - python/tests/test_gen_api_fixtures.py
  - python/tests/test_launch_rust_e2e.py
  - python/tests/test_python_frontend.py
  - python/tests/test_topology.py
  - scripts/check_all.sh
  - scripts/gen_api_fixtures.py
findings:
  critical: 0
  warning: 3
  info: 2
  total: 5
status: issues_found
---

# Phase 05: Code Review Report

**Reviewed:** 2026-10-06T00:00:00Z
**Depth:** standard
**Files Reviewed:** 59
**Status:** issues_found

## Summary

This phase builds the request-lifecycle FSM, the `Engine`/driver task, the HTTP ingress layer
(`/generate`, `/v1/chat/completions`, `/v1/models`, `/health*`, `/metrics`), the Prometheus
metrics surface, and the API-01 byte-parity test harness against the frozen Python frontend.
The code is unusually well-documented and most edge cases I traced (lifecycle transition table
exhaustiveness, cancellation-during-prefill vs. after-first-token, backend-inactivity timeout,
overlong-prompt rejection, non-ASCII JSON escaping for SSE chat chunks, dispatcher route
lifecycle) are both correctly reasoned about in comments and covered by a matching test. I did
not find any BLOCKER-level defect (crash, security vulnerability, or data-loss bug) in the files
in scope.

I did find three WARNING-level issues, all in production code, none of which are exercised by
the existing test suite: a worker-thread-blocking call on the hot request path that can
starve *other* concurrent requests exactly the way the surrounding code was written to avoid;
a benign-looking but real Prometheus counter-monotonicity race under concurrent `/metrics`
scrapes; and an overly broad transient-error classifier in the tokenizer-asset retry loop.
I also note two INFO-level maintainability items.

## Warnings

### WR-01: `decoder()` is built synchronously on the async driver task, blocking the tokio worker thread for the very reason the code says it must run early

**File:** `crates/rsg-server/src/engine.rs:412` (see also the comment at lines 401-411)

**Issue:** `drive_request` is a plain `async fn` running on a shared tokio multi-thread
worker. At line 412 it calls:

```rust
let mut decoder = engine.codec.decoder();
```

`HfCodec::decoder()` (`crates/rsg-server/src/hf_codec.rs:168-176`) constructs a fresh
`Detokenizer::new(self.assets.tokenizer.clone(), ...)`, and the comment directly above the
call in `engine.rs` states this clone is "tens of milliseconds, not free" for a real
~150k-entry vocab/merge table. That clone is a synchronous, CPU-bound `Clone` call with no
`.await` point — it runs to completion on whatever tokio worker thread happened to pick up
this task, blocking that worker thread for the whole duration.

This is exactly the antipattern the project's own stack guidance (`CLAUDE.md`, "What NOT to
Use") calls out for tokenizer work under concurrency ("the blocking pool is unbounded and
shared, which makes tail latency unpredictable with 128 concurrent agents" — recommending a
dedicated thread/pool instead), except here it is worse than `spawn_blocking` would be: the
clone runs directly on a shared async worker thread, not even on a separate blocking-pool
thread.

Under concurrent load (the exact scenario `tests/stress_128.rs` exercises, though with the
`ByteCodec` test double which has no expensive `decoder()`, so the real `HfCodec` path is
never stressed this way), every other task scheduled on that same worker thread — including
other requests' `stream.recv()` polls — is delayed for the duration of the clone. Those other
requests' per-uid broadcast channel has a fixed capacity of 16
(`crates/rsg-server/src/dispatch.rs:59`, `UID_CHANNEL_CAPACITY`); a delay of tens of
milliseconds while tokens are arriving can overflow it, producing spurious
`RequestError::SlowConsumer` failures for *unrelated* requests, or can push them past
`backend_timeout` and fail them with `RequestError::BackendTimeout`. This is precisely the
failure mode the comment at lines 401-411 says it is trying to avoid (buffer overflow before
the decode loop's first `recv()`) — just relocated from "this request, if built late" onto
"other concurrent requests, while this request's decoder is built".

**Fix:** Move the clone off the async worker thread, e.g.:

```rust
let decoder = {
    let codec = Arc::clone(&engine.codec);
    tokio::task::spawn_blocking(move || codec.decoder())
        .await
        .expect("decoder() panicked")
};
```

or (preferred, matching the project's own stated guidance over `spawn_blocking`'s unbounded
shared pool) build decoders on a small dedicated thread pool and hand them over via a channel,
the same shape already used for the tokenizer encode path.

### WR-02: `/metrics`'s `late_tokens_dropped_total` can transiently violate Prometheus counter monotonicity under concurrent scrapes

**File:** `crates/rsg-server/src/metrics.rs:171-181`, called from `crates/rsg-server/src/http/health.rs:45-49`

**Issue:** `ServerMetrics::render` does:

```rust
pub fn render(&self, dispatch: Option<DispatchStatsSnapshot>) -> String {
    if let Some(d) = dispatch {
        self.late_tokens_dropped_total
            .absolute(d.unknown_uid + d.closed_route);
    }
    self.handle.render()
}
```

and `health::metrics` (`crates/rsg-server/src/http/health.rs:45-49`) calls `e.dispatch_stats()`
then `state.metrics().render(dispatch)` with no synchronization between the two steps and no
lock around them. `/metrics` is an ordinary axum handler, so two concurrent `GET /metrics`
requests run this read-then-`absolute()` sequence concurrently on different tokio worker
threads (there is no `.await` between the snapshot read and the `absolute()` call, but that
only protects against *interleaving within one task* — it does nothing to order two different
OS threads against each other).

Because `dispatch_stats()`'s two atomics (`unknown_uid`, `closed_route`) only grow, a slower
scrape's *older* snapshot can be applied via `absolute()` *after* a faster, concurrent scrape's
*newer* snapshot, making the series briefly go backwards. `rsg_late_tokens_dropped_total` is
documented as a `Counter` (monotonic, `_total` suffix); Prometheus `rate()`/`increase()`
consumers assume monotonicity and will report a (brief, self-correcting) negative rate spike
when this happens. `tests/observability.rs::concurrent_scrapes_are_monotonic` checks
monotonicity only for `rsg_requests_total`/`rsg_requests_finished_total` (which are only ever
`increment()`-ed from the single-threaded registry actor, so they are not subject to this
race) — it does not cover `rsg_late_tokens_dropped_total`, so this race is untested.

**Fix:** Either make the dispatcher counters themselves the single source of truth exposed
directly as a `metrics::Counter` driven by `fetch_add` on the `rx-zmq` thread (no snapshot/set
step at scrape time at all), or guard the read-and-`absolute()` sequence with a mutex /
`compare_exchange`-style "only advance forward" check, e.g. track the last value written and
skip the `absolute()` call if the new snapshot is not greater.

### WR-03: Transient-cache-race retry classifier is too broad, risking a masked persistent failure

**File:** `crates/rsg-server/src/hf_codec.rs:45-47`, `56-75`

**Issue:** `is_transient_cache_race` is:

```rust
fn is_transient_cache_race(err: &TokenizerError) -> bool {
    matches!(err, TokenizerError::Io(_) | TokenizerError::HfHub(_))
}
```

`load_model_assets_with_retry` retries *any* `TokenizerError::Io` or `TokenizerError::HfHub`
up to `LOAD_RETRY_ATTEMPTS` (5) times with a short linear backoff, on the stated rationale that
hf-hub's `create_pointer_symlink` has a non-atomic `remove_file`-then-`symlink` race between
concurrent processes. That rationale is real, but the two matched variants are also exactly
what a *permanent* failure looks like: permission denied, disk full, a corrupted/incomplete
cache directory, a DNS failure during a cold download, or a genuinely gated/unauthorized repo
surfacing through the `HfHub` variant rather than `GatedAccessUnavailable` (the comment at
`hf_codec.rs:105-110` already acknowledges "a genuinely gated model would surface as an
ordinary `TokenizerError` auth error here"). Any of these now takes up to
`20+40+60+80 = 200ms` longer to report, and a caller watching for the *specific* transient
race (e.g., an operator diagnosing a flaky multi-process cold start) cannot distinguish "it
retried because of the documented race" from "it retried and then gave up on something else"
from the resulting log line alone, since `tracing::warn!` fires identically for both.

**Fix:** Narrow the check to the actual failure shape the race produces (e.g., match on the
underlying `io::ErrorKind::NotFound` specifically, if `TokenizerError::Io` wraps one, rather
than every `Io`/`HfHub` variant), or at minimum include the underlying error's kind/message in
the retry log line so a genuinely permanent failure is distinguishable from the documented
race while it's still retrying.

## Info

### IN-01: `now_unix()` is duplicated verbatim in two handler modules

**File:** `crates/rsg-server/src/http/chat.rs:167-172`, `crates/rsg-server/src/http/models.rs:33-38`

**Issue:** Both files define an identical private helper:

```rust
fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
```

Harmless today, but a future change to the "created" semantics (e.g., switching to
milliseconds, or adding a clock-skew guard) has two call sites to find and keep in sync instead
of one.

**Fix:** Move `now_unix()` into `crates/rsg-server/src/http/mod.rs` (or a small `time` helper
module) and have both `chat.rs` and `models.rs` import it.

### IN-02: `ApiError::MissingPrompt` surfaces as an opaque 500 with no caller-actionable detail

**File:** `crates/rsg-server/src/http/error.rs:32-33, 47, 61`

**Issue:** This is a deliberate, documented, and tested parity choice (upstream's own
`assert req.prompt is not None` leaks as an unhandled-exception 500 through Starlette), and I
am not asking for it to change for parity reasons. Flagging only as a note for anyone extending
this error surface later: every *other* client-input problem in this API (missing `max_tokens`,
bad `role`, oversized body, overlong prompt) maps to a 4xx, while this one particular missing-
field case maps to 500 with `"type":"internal_error"`. A future contributor adding a new
non-parity-constrained endpoint could easily copy `MissingPrompt`'s 500 mapping as a template
for "missing required field," which would be the wrong default outside of this specific
upstream-compatibility constraint.

**Fix:** None needed for this phase. Consider a one-line comment at the `MissingPrompt` arm in
`ApiError::status`/`kind` making explicit that 500 here is a parity requirement, not a general
pattern to copy.

---

_Reviewed: 2026-10-06T00:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
