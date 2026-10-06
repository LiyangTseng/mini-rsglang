---
phase: 03-zmq-transport-mock-scheduler
reviewed: 2026-10-06T00:00:00Z
depth: standard
files_reviewed: 17
files_reviewed_list:
  - Cargo.lock
  - Cargo.toml
  - crates/rsg-server/Cargo.toml
  - crates/rsg-server/src/bin/mock-scheduler.rs
  - crates/rsg-server/src/dispatch.rs
  - crates/rsg-server/src/handshake.rs
  - crates/rsg-server/src/lib.rs
  - crates/rsg-server/src/main.rs
  - crates/rsg-server/src/transport.rs
  - crates/rsg-server/src/writer.rs
  - crates/rsg-server/tests/common/mod.rs
  - crates/rsg-server/tests/dispatch_backpressure.rs
  - crates/rsg-server/tests/mock_scheduler_behaviors.rs
  - crates/rsg-server/tests/mock_scheduler_process.rs
  - crates/rsg-server/tests/ordering_proptest.rs
  - crates/rsg-server/tests/transport_e2e.rs
  - crates/rsg-server/tests/transport_misbehavior_e2e.rs
findings:
  critical: 0
  warning: 3
  info: 2
  total: 5
status: issues_found
---

# Phase 03: Code Review Report

**Reviewed:** 2026-10-06T00:00:00Z
**Depth:** standard
**Files Reviewed:** 17
**Status:** issues_found

## Summary

I read every production module (`transport.rs`, `handshake.rs`, `dispatch.rs`, `writer.rs`, `mock-scheduler.rs`, `main.rs`, `lib.rs`) and every test file end to end, then traced the control flow of the single-writer / per-uid-dispatcher / mock-engine triangle for ordering, backpressure, and shutdown correctness (D-01 through D-10, MOCK-01). I did not find a reproducible logic bug: the FIFO-ordering guarantee for abort-after-submit (WIRE-03), the per-uid lag/drop accounting in the dispatcher, the mock scheduler's overlong-drop/clamp rule, its batching/flush-timer, and its `late-abort-token` draining state machine all check out against their own tests and against the documented upstream semantics they claim to mirror. `rsg-wire` itself (`Tensor`/`SamplingParams`/`BackendMsg`/`TokenizerMsg` encode/decode) was out of scope (not in the file list) and was treated as a trusted dependency.

What I did find is a real, provable gap in failure-path robustness: every ZMQ `send()` call across this phase's transport and mock-scheduler code uses blocking send with no timeout, so a stalled peer on either side of the wire can hang the single writer thread (or the mock engine thread) indefinitely, with no circuit breaker — a worse-than-it-looks finding because this project's whole design puts every outbound message through that one thread. I also found two smaller silent-failure/consistency gaps (an unlogged encode error in the writer thread, and a thread-spawn failure in `mock-scheduler` that panics instead of using the file's own `EXIT_STARTUP` convention), plus two minor polish items.

## Warnings

### WR-01: No send timeout on any ZMQ PUSH socket — a stalled peer can hang the single writer/engine thread forever, with a theoretical two-way deadlock

**File:** `crates/rsg-server/src/transport.rs:91`, `:122`, `:195`; also exercised by `crates/rsg-server/src/writer.rs:149`

**Issue:** `BackendSink::send_backend` (both `ZmqTransport` and `ZmqBackendTx`) and `ZmqSchedulerTransport::send_detok` all call `.send(frame, 0)` — flag `0` is a **blocking** send in the `zmq` crate (no `DONTWAIT`, no `SNDTIMEO` set anywhere in `open_socket`). `recv_backend`/`recv_detok` are bounded by an explicit `poll(timeout_ms)` before reading, but nothing bounds how long a send can block if the peer's receive queue is at its high-water mark (default 1000 messages) or the peer process has died without the socket noticing yet.

Because this phase's architecture is explicitly "one dedicated OS thread owns the only path to the scheduler" (`writer.rs`'s own module doc, D-01) and "one dedicated OS thread owns the only path from the scheduler" (`dispatch.rs`'s `rx-zmq`), a single stuck `send()` on either side stalls *every* uid, not just the slow one:
- If the real scheduler stops draining its backend PULL socket (e.g. GPU overload), `tx-zmq`'s `send_backend` blocks forever; the bounded `mpsc` queue (capacity 1024, `writer.rs:23`) then backs up, and every subsequent `WriterHandle::submit`/`abort`/`exit` call across the whole process blocks on its own `.send().await` — the frontend silently stops accepting new work with no error surfaced to callers.
- Symmetrically, `mock-scheduler.rs`'s `flush_pending` → `transport.send_detok` (and the real scheduler's own detokenizer PUSH) has the same blocking-send exposure; if the frontend's dispatcher ever stops draining (e.g. a bug, or simply 1000+ undelivered replies while every uid's broadcast channel is already full — broadcast channels don't apply backpressure to the *sender*, but the OS-level zmq HWM still will), the engine thread hangs too.
- In the worst case this is a genuine **two-way deadlock**: frontend blocked sending to backend while backend is simultaneously blocked sending to frontend. `ordering_proptest.rs`'s own property test is implicitly aware of half of this risk — it keeps a no-op dispatcher alive specifically "so the mock's PUSH never blocks" (comment at `tests/ordering_proptest.rs:282-285`) — but that mitigation only covers the test harness, not the production path.

**Fix:** Set `ZMQ_SNDTIMEO` (via `sock.set_sndtimeo(ms)`) on both PUSH sockets in `open_socket`, and treat a timeout as a recoverable error (log + retry with backoff, or surface a `WriterClosed`-style error) rather than hanging the thread indefinitely. At minimum, document this as a known, deliberately deferred limitation if it is intentionally out of scope for this phase, so it isn't mistaken for "transport is fully production-hardened."

### WR-02: `tx-zmq` logs on a failed socket send but not on a failed encode, unlike every other error path in this codebase

**File:** `crates/rsg-server/src/writer.rs:148-152`

**Issue:**
```rust
let bytes = rsg_wire::encode_backend(&msg).context("encode BackendMsg")?;
if let Err(e) = sink.send_backend(&bytes).context("send on backend socket") {
    tracing::error!("tx-zmq: {e:#}");
    return Err(e);
}
```
If `encode_backend` fails, the `?` propagates immediately with **no `tracing::error!`** before the thread exits — the only artifact is the returned `anyhow::Result<()>` from the `JoinHandle`, which nothing outside the test suite currently joins. Every analogous failure path elsewhere in this phase logs before propagating: `dispatch.rs`'s undecodable-frame branch (`tracing::warn!` at line 239), and `mock-scheduler.rs`'s own `flush_pending` encode-failure branch (`tracing::error!` at line 350) both log first. This one path is the odd one out, and it is exactly the path that silently kills the single writer thread — the one failure mode an operator would most want logged, since every subsequent `submit`/`abort`/`exit` call will mysteriously start returning `WriterClosed` with no prior log line explaining why.

**Fix:**
```rust
let bytes = match rsg_wire::encode_backend(&msg) {
    Ok(b) => b,
    Err(e) => {
        tracing::error!("tx-zmq: encode BackendMsg: {e:#}");
        return Err(e).context("encode BackendMsg");
    }
};
```

### WR-03: `mock-scheduler`'s own thread spawn panics instead of using the file's own `EXIT_STARTUP` convention

**File:** `crates/rsg-server/src/bin/mock-scheduler.rs:456-507`

**Issue:** Every other setup failure in this binary — socket open (`line 464`), observe-file create (`line 474`), signal install (`install_signal`, `line 405`) — logs via `tracing::error!` and exits with the documented `EXIT_STARTUP` (1), per the file's own exit-code table in its module doc. The `mock-engine` thread's own spawn call breaks that pattern:
```rust
std::thread::Builder::new()
    .name("mock-engine".to_string())
    .spawn(move || { ... })
    .expect("spawn mock-engine thread");
```
If OS thread creation fails (resource exhaustion, `ulimit -u`, container cgroup limits — all realistic in CI or a loaded dev box), this panics with Rust's default unwind, producing exit code 101 — not any of the four codes (`0`, `1`, `3`, `4`) this binary's own docstring says it can exit with. A test harness or operator script that greps for `EXIT_STARTUP` (1) on setup failure will misinterpret or mishandle this case.

**Fix:**
```rust
let spawn_result = std::thread::Builder::new()
    .name("mock-engine".to_string())
    .spawn(move || { ... });
let _join = match spawn_result {
    Ok(j) => j,
    Err(e) => {
        tracing::error!("failed to spawn mock-engine thread: {e}");
        std::process::exit(EXIT_STARTUP);
    }
};
```

## Info

### IN-01: `parse_uid_list` rejects a leading-`-` (negative) uid for the wrong stated reason, producing a misleading error message

**File:** `crates/rsg-server/src/bin/mock-scheduler.rs:153-169`

**Issue:** `--misbehave-uids` is documented as accepting "comma-separated non-negative integers or inclusive `A-B` ranges" (module doc, lines 8-9), and `malformed_uid_list_exits_2` (`tests/mock_scheduler_behaviors.rs:482-489`) asserts `"-4"` is rejected — which it is, but only by accident. `"-4".split_once('-')` matches on the *leading* `-` itself, producing `a = ""`, `b = "4"`; `a.trim().parse::<i64>()` then fails on the empty string, and the code reports `"invalid uid range \"-4\" in \"-4\""` — calling a bare negative integer an "invalid uid *range*" is confusing to a user who never typed a range. The test passes because *any* non-empty stderr + exit 2 satisfies it, not because the message is correct.

**Fix:** Explicitly reject (or explicitly support) a leading `-` before attempting the range split, e.g. check `item.starts_with('-')` first and emit `"negative uid {item:?} in {s:?} (uids must be non-negative)"`.

### IN-02: `main.rs`'s module doc still says "In Phase 1 a skeleton," unchanged since this phase only edited its imports

**File:** `crates/rsg-server/src/main.rs:1-2`

**Issue:** The binary's top-of-file doc comment ("In Phase 1 a skeleton that opens its two ZMQ sockets, reads the readiness handshake and idles") predates this phase and was left untouched by this phase's diff (only the `use` statements changed, to pull `handshake`/`transport` from the new `rsg_server` library instead of local `mod` declarations). It is harmless but slightly stale now that this same phase ships a fully working `dispatch`/`writer` pair that `main.rs` still does not use — a future reader skimming only this comment could reasonably assume `rsg-server` is still pre-handshake-only, when the surrounding crate already has a working transport/writer/dispatcher trio ready to be wired in.

**Fix:** When `main.rs` is next touched to wire in the writer/dispatcher (presumably a later phase), update this doc comment to drop the "Phase 1" framing or state the current phase boundary explicitly.

---

_Reviewed: 2026-10-06T00:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
