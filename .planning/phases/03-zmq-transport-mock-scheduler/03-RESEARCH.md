# Phase 3: ZMQ Transport & Mock Scheduler - Research

**Researched:** 2026-10-05
**Domain:** Rust-side ZMQ transport ordering/dispatch, property-based concurrency testing, subprocess-based mock backend
**Confidence:** MEDIUM-HIGH

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**Single ordered writer (WIRE-03)**
- **D-01:** All outgoing messages to the scheduler (`UserMsg`, `AbortBackendMsg`, and any coalesced `BatchBackendMsg`) go through one single-writer component, per the project's existing `tx-zmq` thread pattern (`.claude/CLAUDE.md` §Stack Patterns). The exact internal mechanism (dedicated OS thread fed by a channel, vs. an async task owning the socket) is Claude's discretion — the user-facing guarantee is "one path, FIFO, no second writer can interleave," not the specific thread model.
- **D-02:** The ordering guarantee is proven by a **property-based test (proptest)**, generating random interleavings of concurrent submit/abort calls across many uids, asserting the scheduler never observes an abort before its own submit for any uid. Reversible.
- **D-03:** The ordering property test runs **end-to-end through the real transport and a real `mock-scheduler` subprocess** (not an isolated in-memory stand-in for the writer). Reversible (a faster isolated unit test can be added later).

**Reply routing & slow-consumer backpressure**
- **D-04:** Replies are dispatched to in-flight requests via a **per-uid bounded channel**: a dispatcher reads frames off the detokenizer socket and routes each `DetokenizeMsg`/`BatchTokenizerMsg` entry to the channel registered for its `uid`. Replies for unknown/deregistered uids (late tokens after an abort) are dropped without error.
- **D-05:** Each per-uid channel is **bounded with a small fixed capacity (around 16)**. When a slow consumer leaves its channel full, the **oldest buffered token for that uid is dropped** rather than blocking the dispatcher. Costly to reverse — Phase 5's FSM builds directly on this per-uid channel API.
- **D-06:** Every dropped token is counted and surfaced — a per-uid dropped-token counter plus a `tracing` warning — so tests (and later Phase 6 parity debugging) can distinguish "we intentionally dropped a backed-up token" from "the backend sent something wrong."
- **D-07:** The channel bound (~16) is a **fixed constant for Phase 3**, not an exposed CLI/config knob.

**Mock scheduler (MOCK-01)**
- **D-08:** `mock-scheduler` is a **standalone subprocess binary**, spawned like the real backend over real `ipc://` sockets. It reuses `rsg-wire`'s `BackendMsg`/`TokenizerMsg` types and the bind/connect role convention from Phase 1's `transport.rs`. Costly to reverse — Phase 5/6/7 all spawn this same binary in place of the real backend.
- **D-09:** Misbehavior scenarios are configured via **CLI flags at spawn** (no scenario-file format). Behaviors apply to **specific uids via uid-range flags** (e.g. `--misbehave-uids 3,7 --behavior late-abort-token`). Batched replies use a **fixed `--batch-size N` flag** (accumulate up to N, flush on a short timer if fewer are ready). No randomized/jittered batch timing in this phase.
- **D-10:** Prefill/decode timing is **minimal**: fixed, uniform `--prefill-delay-ms` and `--decode-delay-ms` flags applied to every request.
- **D-11:** Richer timing configurability (seeded-random distributions, per-uid overrides, replay profiles) is **explicitly deferred** to whichever of Phase 5/7 needs it.

### Claude's Discretion
- The exact single-writer mechanism (dedicated thread + channel vs. async task owning the socket) — D-01 fixes the guarantee, not the implementation.
- Internal module/crate layout for `mock-scheduler` (e.g. a new `crates/mock-scheduler` binary crate vs. a module inside an existing crate) and for the per-uid dispatch table.
- Exact proptest case count, shrinking configuration, and how many concurrent simulated callers the ordering test uses.
- The precise `--behavior` flag vocabulary and value syntax for `mock-scheduler`, as long as it covers the three behaviors named in MOCK-01.

### Deferred Ideas (OUT OF SCOPE)
- **Richer mock-scheduler timing** (seeded random delay distributions, per-uid delay overrides, replayable scenario profiles) — revisit when Phase 5 (cancellation stress test) or Phase 7 (benchmark harness) plans actually specify what they need.
- **Configurable per-uid channel bound** — add the knob only when a later phase's stress/benchmark scenario needs to tune it.
- No Rust radix cache or prefix-hit modeling in the mock (deferred to v2 project-wide).
- Benchmark-grade timing realism deferred until Phase 5/7 need it.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| WIRE-03 | Rust exchanges messages with the scheduler over ZMQ `ipc://` through a single ordered writer, so an abort can never overtake its own submit | §Architecture Patterns (single-writer thread), §Code Examples (writer sketch, proptest+tokio pattern), §Common Pitfalls (mpsc ordering, early-send races) |
| MOCK-01 | One minimal Rust mock scheduler speaks the same wire protocol, runs end to end on a Mac, and reproduces late-tokens-after-abort / silently-dropped-overlong-prompt / batched-reply backend behaviors | §Architecture Patterns (mock-scheduler CLI/behaviors), §Code Examples (mock-scheduler sketch), §Package Legitimacy Audit |
</phase_requirements>

## Summary

Phase 3 adds two new pieces of production/test Rust code on top of Phase 1's `ZmqTransport`: (1) a single-writer component that serializes all outgoing `UserMsg`/`AbortBackendMsg`/`BatchBackendMsg` traffic through one path, and (2) a per-uid reply dispatcher with bounded, drop-oldest backpressure. Both are proven against a new `mock-scheduler` subprocess binary that speaks the exact same wire protocol as the real backend.

The two hardest technical questions this research resolves: **what data structure gives "bounded, drop-oldest, with a count of what was dropped" without hand-rolling a ring buffer** (`tokio::sync::broadcast::channel` — verified via official docs to overwrite the oldest value at capacity and report the exact dropped count via `RecvError::Lagged(n)`), and **how `mock-scheduler` can be spawned reliably from Rust integration tests via `CARGO_BIN_EXE_<name>`** (confirmed working in-repo today for same-package binaries; cross-package behavior is not documented, so the safe, zero-new-dependency choice is to add `mock-scheduler` as a second binary in `rsg-server`'s own `src/bin/`, backed by a new `rsg-server` library target so both `main.rs` and `mock-scheduler.rs` can share `transport.rs`'s `Endpoint`/`Role`/`ZmqTransport` types).

The property-based ordering test (D-02/D-03) needs `proptest` (a locked choice, confirmed clean on crates.io) driving real async code; since no mature `proptest`+`tokio` glue crate exists, the test body builds its own `tokio::runtime::Runtime` and calls `.block_on()` per generated case — a known community pattern, not an official API, so it should be spiked early.

**Primary recommendation:** Give `rsg-server` a `lib.rs` exposing `transport` (and the new `writer`/`dispatch` modules) as public API; add `mock-scheduler` as a second same-package binary (`src/bin/mock-scheduler.rs`) reusing those types; implement the per-uid reply channel with `tokio::sync::broadcast::channel(16)` instead of a hand-rolled ring buffer; implement the single writer as a dedicated OS thread (the existing `tx-zmq` pattern) fed by a `tokio::sync::mpsc` channel; and drive the D-02 ordering proptest with a manually-constructed `tokio::runtime::Runtime` per case against a spawned `mock-scheduler` subprocess.

## Architectural Responsibility Map

This project's tiers are not a generic web-app stack; they follow the project's own frontend/backend split (`.claude/CLAUDE.md` Constraints: "Rust owns ingress, lifecycle FSM, tokenization, detokenization; Python/CUDA owns scheduler, weights, batching loop, kernels, KV cache").

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Single ordered writer (submit/abort serialization) | Rust Frontend (ingress/transport) | ZMQ IPC boundary | Ordering must be enforced before bytes leave the Rust process; the wire format itself carries no ordering concept — a `BackendMsg` frame is just bytes on a PUSH socket. |
| Per-uid reply routing / dispatch | Rust Frontend (ingress/transport) | — | Uid demuxing is pure frontend-side bookkeeping; the backend has no concept of "routing" — it just emits `DetokenizeMsg{uid,...}` and trusts the peer to sort it out. |
| Slow-consumer backpressure isolation | Rust Frontend (ingress/transport) | — | Must be enforced entirely in-process; the backend has no visibility into how fast Rust's internal per-request consumers drain their channels. |
| Wire message coalescing into `BatchBackendMsg`/`BatchTokenizerMsg` | Rust Frontend (ingress/transport) | ZMQ IPC boundary | Coalescing happens on the sending side before the wire, mirroring upstream's own tokenizer-side (`tokenize_worker`) and scheduler-side (`_reply_tokenizer_rank0`) coalescing. |
| Mock scheduler (misbehavior simulation) | Python/CUDA Backend tier (stand-in) | Rust test harness | `mock-scheduler` deliberately impersonates the real backend's role at the IPC boundary for GPU-free development — it is a backend-tier stand-in, not frontend logic, even though it happens to be written in Rust. |

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard | Conf. |
|---------|---------|---------|---------------|-------|
| `zmq` | 0.10.0 (already pinned, workspace `Cargo.toml:22`) [VERIFIED: /Users/li-yangtseng/Codes/mini-rsglang/.claude/worktrees/sparkling-juggling-shore/Cargo.toml:22] | Transport to/from `mock-scheduler` and (later) the real scheduler | Already settled in Phase 1; Phase 3 builds on `ZmqTransport`, doesn't revisit the crate choice | HIGH |
| `tokio` (`sync`, `rt-multi-thread`, `macros`, `signal`, `time` features) | 1.53.1 (already pinned, workspace `Cargo.toml:19`) [VERIFIED: /Users/li-yangtseng/Codes/mini-rsglang/.claude/worktrees/sparkling-juggling-shore/Cargo.toml:19] — `"tokio = { version = \"1.53.1\", features = [\"rt-multi-thread\", \"macros\", \"signal\", \"sync\", \"time\"] }"` | `mpsc` for the writer's inbox, `broadcast` for per-uid reply channels — both ship under the existing `sync` feature, no feature-flag change needed | Already in the stack; `broadcast`/`mpsc` are both part of `tokio::sync` | HIGH |
| `rmp-serde` + `rsg-wire` | 1.3.1 (already pinned) [VERIFIED: /Users/li-yangtseng/Codes/mini-rsglang/.claude/worktrees/sparkling-juggling-shore/Cargo.toml:14] | Encode/decode `BackendMsg`/`TokenizerMsg` on both the writer and `mock-scheduler` | Byte-verified against upstream in Phase 1; Phase 3 does no wire-format work, only reuse | HIGH |
| `proptest` | 1.11.0 [VERIFIED: crates.io registry — published 2017-06-18, 4.1M weekly downloads, repo github.com/proptest-rs/proptest] | D-02's property-based ordering test | Locked by CONTEXT.md D-02 (user decision, not explored as an alternative) | HIGH (legitimacy) / MEDIUM (async-integration ergonomics, see Pitfalls) |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `rustc-hash` (`FxHashMap`) | 2.1.3 [VERIFIED: crates.io registry — `max_stable_version: "2.1.3"`, published 2026-07-02, owned by `rust-lang` org, 18.3M weekly downloads] | Per-uid dispatch table (`FxHashMap<i64, broadcast::Sender<TokenizerMsg>>`) | Already the project-wide recommendation (`.claude/CLAUDE.md` §Supporting Libraries: "FSM request table keyed by `uid`, radix-node children"); Phase 3 is the first phase to actually need a uid-keyed map |
| `clap` (`derive`) | 4.6.7 (already pinned) [VERIFIED: /Users/li-yangtseng/Codes/mini-rsglang/.claude/worktrees/sparkling-juggling-shore/Cargo.toml:13] | `mock-scheduler`'s own CLI (`--misbehave-uids`, `--behavior`, `--batch-size`, `--prefill-delay-ms`, `--decode-delay-ms`) | Follows the existing `rsg-server` CLI convention (`crates/rsg-server/src/main.rs`) per D-09/the canonical refs |
| `tracing` | 0.1.44 (already pinned) | `tracing::warn!` on every dropped token (D-06) | Already in the stack |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `tokio::sync::broadcast::channel(16)` for the per-uid drop-oldest channel | Hand-rolled `Mutex<VecDeque<T>>` ring buffer + `Notify` | The hand-rolled version gives exact single-consumer semantics but reimplements what `broadcast` already does (bounded ring buffer, overwrite-oldest, `Lagged(n)` drop count) — see §Don't Hand-Roll. Only revisit if a future phase needs true multi-consumer fan-out per uid, which `broadcast` already supports for free. |
| `mock-scheduler` as a second binary inside `rsg-server` (`src/bin/`) | A separate `crates/mock-scheduler` workspace member crate | A separate crate reads as architecturally cleaner (mock-scheduler conceptually impersonates the *backend*, not the frontend) but requires either duplicating `transport.rs`'s types or exposing them from a shared library crate, and cross-package `CARGO_BIN_EXE_<name>` resolution is not confirmed by the Cargo book (§Common Pitfalls). Revisit if a later phase's test suite lives in a crate that cannot depend on `rsg-server`. |
| Manual `tokio::runtime::Runtime::new().block_on()` per proptest case | `proptest_async` crate | `proptest_async` hardcodes `async-std` as its executor today, not `tokio` [CITED: web search, cross-checked — MEDIUM]; adding it would mean either patching it or running two async runtimes in one test binary. Manual `block_on` is more verbose but has zero extra dependencies and zero risk of executor mismatch. |

**Installation:**
```bash
# workspace Cargo.toml — add two new workspace dependencies
# proptest = "1.11.0"       (dev-dependency, D-02)
# rustc-hash = "2.1.3"      (regular dependency, per-uid dispatch table)

cargo add --workspace proptest --dev --package rsg-server
cargo add --workspace rustc-hash --package rsg-server
```

**Version verification:** both versions above were confirmed live against the crates.io registry API this session (`https://crates.io/api/v1/crates/<name>`), not from training-data recall alone.

## Package Legitimacy Audit

| Package | Registry | Age | Downloads | Source Repo | Verdict | Disposition |
|---------|----------|-----|-----------|-------------|---------|-------------|
| `proptest` | crates | ~9 yrs (since 2017-06-18) | 4.1M/week | github.com/proptest-rs/proptest | OK | Approved |
| `rustc-hash` | crates | ~8 yrs (since 2018-05-24) | 18.3M/week | github.com/rust-lang/rustc-hash | OK | Approved |

**Packages removed due to [SLOP] verdict:** none.
**Packages flagged as suspicious [SUS]:** none.

Both packages were checked via `gsd-tools query package-legitimacy check --ecosystem crates <pkg>`, both returned `OK` with no reasons flagged, and both are cross-confirmed by `.claude/CLAUDE.md`'s own project-wide stack recommendation (`proptest` 1.11.0 as a dev tool, `rustc-hash` 2.1.3 as a supporting library) — so they also satisfy the stricter "discovered via project documentation + registry-confirmed" bar, not just a bare WebSearch hit.

## Architecture Patterns

### System Architecture Diagram

```
                        (D-01/D-02/D-03 — this phase)
   many async callers                                         mock-scheduler
   (future Phase 5 FSM;                                        (subprocess)
   Phase 3 tests call                                      ┌─────────────────┐
   submit()/abort()                                        │ PULL minisgl_0   │◄──┐
   directly)                                                │ (backend, bind)  │   │
        │                                                   └────────┬─────────┘   │
        │ WriterCmd::Submit{uid,...}                                  │ decode      │  ipc://
        │ WriterCmd::Abort{uid}                                       │ BackendMsg  │  sockets
        ▼                                                             ▼             │
  tokio::sync::mpsc (bounded)                              [behavior engine:        │
        │                                                   late-abort-token,       │
        ▼                                                   drop-overlong,          │
  ┌──────────────────┐   coalesce pending ──►  BackendMsg   batch-size N]           │
  │ writer thread     │   into bare msg (n=1)                   │                   │
  │ ("tx-zmq")        │   or BatchBackendMsg (n>1)               │ encode            │
  │ owns PUSH socket  │ ───────────────────────────────────────► │ TokenizerMsg      │
  └──────────────────┘                                            ▼                  │
                                                             ┌──────────────────┐     │
                                                             │ PUSH minisgl_1    │────►│
                                                             │ (detok, connect)  │ ipc://
                                                             └──────────────────┘
        ┌──────────────────┐   decode TokenizerMsg,
        │ dispatcher thread │   unwrap BatchTokenizerMsg,
        │ ("rx-zmq")        │   look up uid in FxHashMap
        │ owns PULL socket  │◄──────────────────────────────────── (PULL minisgl_1, bind)
        └─────────┬─────────┘
                   │ tx.send(DetokenizeMsg)   (never blocks; drops oldest if full)
                   ▼
        FxHashMap<uid, broadcast::Sender<TokenizerMsg>>
                   │
         ┌─────────┴──────────┐
         ▼                    ▼
  per-uid broadcast::Receiver  per-uid broadcast::Receiver   ... (one per in-flight request)
  (consumed by Phase 5's        (unknown uid → map lookup
   future FSM; Phase 3 tests     misses → frame silently
   read it directly)             dropped, no crash — D-04)
```

### Recommended Project Structure

```
crates/
├── rsg-wire/                      # unchanged — wire codec (Phase 1)
└── rsg-server/
    ├── Cargo.toml                 # gains a [lib] target; mock-scheduler becomes a
    │                              # same-package [[bin]] (autodiscovered via src/bin/)
    ├── src/
    │   ├── lib.rs                 # NEW — `pub mod transport; pub mod writer; pub mod dispatch;`
    │   ├── main.rs                # rsg-server binary — `use rsg_server::{transport, writer, dispatch};`
    │   ├── transport.rs           # unchanged (Phase 1) — now re-exported via lib.rs
    │   ├── handshake.rs           # unchanged (Phase 1)
    │   ├── writer.rs              # NEW — single-writer thread (D-01/D-02), WriterCmd, WriterHandle
    │   ├── dispatch.rs            # NEW — per-uid dispatch table (D-04/D-05/D-06), DispatchHandle
    │   └── bin/
    │       └── mock-scheduler.rs  # NEW — D-08/D-09/D-10, `use rsg_server::transport::{...}`
    └── tests/
        ├── cli.rs                 # unchanged (Phase 1) — black-box rsg-server subprocess tests
        ├── ordering_proptest.rs   # NEW — D-02/D-03, spawns mock-scheduler, drives writer directly
        └── dispatch_backpressure.rs  # NEW — D-04/D-05/D-06 unit/integration tests
```

### Pattern 1: Single ordered writer as a dedicated thread (D-01)

**What:** One OS thread owns the PUSH socket exclusively. All callers (test code now; Phase 5's FSM later) send `WriterCmd` values into a shared `tokio::sync::mpsc::Sender`, cloned per caller. The thread drains the channel, coalescing into a `BatchBackendMsg` exactly as upstream does — a bare message when there is exactly one pending item, a `Batch*Msg` wrapper only when there are two or more [VERIFIED: /Users/li-yangtseng/Codes/mini-rsglang/.claude/worktrees/sparkling-juggling-shore/vendor/mini-sglang/python/minisgl/scheduler/io.py:124-130] — `"if num_reply == 1: self._send_into_tokenizer.put(reply[0]) elif num_reply > 1: self._send_into_tokenizer.put(BatchTokenizerMsg(data=reply))"` and mirrored on the tokenizer's own outgoing side [VERIFIED: /Users/li-yangtseng/Codes/mini-rsglang/.claude/worktrees/sparkling-juggling-shore/vendor/mini-sglang/python/minisgl/tokenizer/server.py:99-101] — `"if len(batch_output.data) == 1: batch_output = batch_output.data[0] send_backend.put(batch_output)"`.

**When to use:** Any time outgoing frames to the scheduler socket are produced from more than one logical caller.

**Example (sketch, not verified against a compiled build — illustrative only):**
```rust
// writer.rs
pub enum WriterCmd {
    Submit { uid: i64, input_ids: Tensor, sampling_params: SamplingParams },
    Abort { uid: i64 },
}

pub struct WriterHandle(tokio::sync::mpsc::Sender<WriterCmd>);

impl WriterHandle {
    pub async fn submit(&self, uid: i64, input_ids: Tensor, sp: SamplingParams) -> anyhow::Result<()> {
        self.0.send(WriterCmd::Submit { uid, input_ids, sampling_params: sp }).await?;
        Ok(())
    }
    pub async fn abort(&self, uid: i64) -> anyhow::Result<()> {
        self.0.send(WriterCmd::Abort { uid }).await?;
        Ok(())
    }
}

pub fn spawn_writer(transport: impl Transport + 'static) -> WriterHandle {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<WriterCmd>(1024);
    std::thread::spawn(move || {
        loop {
            let Some(first) = rx.blocking_recv() else { break };
            let mut pending = vec![to_backend_msg(first)];
            while let Ok(cmd) = rx.try_recv() {
                pending.push(to_backend_msg(cmd));
            }
            let msg = if pending.len() == 1 {
                pending.pop().unwrap()
            } else {
                BackendMsg::BatchBackendMsg { data: pending }
            };
            let bytes = rsg_wire::encode_backend(&msg).expect("encode");
            transport.send_backend(&bytes).expect("send");
        }
    });
    WriterHandle(tx)
}
```
Ordering is preserved because a single `Sender`'s sequential `.send()` calls are delivered to the one `Receiver` in the order they were sent [CITED: docs.rs/tokio — tokio::sync::mpsc is an MPSC queue; a given sender's calls are sequenced relative to itself because the second `.await` cannot begin until the first completes — MEDIUM].

### Pattern 2: Per-uid drop-oldest reply channel via `tokio::sync::broadcast` (D-04/D-05/D-06)

**What:** Instead of hand-rolling a bounded ring buffer, use one `tokio::sync::broadcast::channel(16)` per uid. Register: `let (tx, rx) = broadcast::channel(16); table.insert(uid, tx); return rx` (the caller keeps `rx`; the dispatcher keeps `tx` in the table). Dispatch: `if let Some(tx) = table.get(&uid) { let _ = tx.send(msg); }` — a missing or already-dropped entry is a no-op, directly implementing "drop replies for unknown/deregistered uids without crashing" (D-04). Consume: the per-request consumer loops on `rx.recv().await`; on `Err(RecvError::Lagged(n))` it increments a per-uid dropped-token counter and emits `tracing::warn!(uid, dropped = n, "detokenize backlog dropped tokens")` (D-06), then calls `recv()` again to resume from the oldest still-retained value.

**When to use:** Any bounded, single-writer/single-reader (or single-writer/multi-reader) channel in this codebase that needs "drop the oldest, tell me how many" semantics instead of backpressure-blocks-the-writer semantics.

**Verified semantics (not assumed):**
- "The provided capacity is rounded up to the next power of two; that rounded size is the number of messages the ring buffer can hold." [CITED: https://docs.rs/tokio/latest/tokio/sync/broadcast/index.html — MEDIUM] — 16 is already a power of two, so D-07's constant needs no adjustment.
- "If a value is sent when the channel is at capacity, the oldest value currently held by the channel is overwritten." [CITED: https://docs.rs/tokio/latest/tokio/sync/broadcast/index.html — MEDIUM]
- `RecvError::Lagged` "carries the number of messages that were dropped before the receiver's cursor and are therefore no longer available," and receiving it "does not close or disconnect the receiver" — the next `recv()` resumes from the oldest retained value. [CITED: https://docs.rs/tokio/latest/tokio/sync/broadcast/index.html — MEDIUM]

**Example (sketch, illustrative):**
```rust
// dispatch.rs
pub struct DispatchTable {
    table: rustc_hash::FxHashMap<i64, tokio::sync::broadcast::Sender<TokenizerMsg>>,
}

impl DispatchTable {
    pub fn register(&mut self, uid: i64) -> tokio::sync::broadcast::Receiver<TokenizerMsg> {
        let (tx, rx) = tokio::sync::broadcast::channel(16);
        self.table.insert(uid, tx);
        rx
    }
    pub fn deregister(&mut self, uid: i64) {
        self.table.remove(&uid); // existing Receiver still drains what's buffered, then closes
    }
    pub fn dispatch_one(&self, uid: i64, msg: TokenizerMsg) {
        if let Some(tx) = self.table.get(&uid) {
            let _ = tx.send(msg); // Err means the receiver already dropped — drop silently
        }
        // else: fully unknown uid (never registered, or already deregistered) — drop silently (D-04)
    }
}
```

### Pattern 3: `mock-scheduler` misbehavior CLI (D-08/D-09/D-10)

**What:** `mock-scheduler` binds/connects its sockets using the same `Endpoint`/`Role` convention `rsg-server` already uses [VERIFIED: /Users/li-yangtseng/Codes/mini-rsglang/.claude/worktrees/sparkling-juggling-shore/crates/rsg-server/src/transport.rs:27-32] — `"pub struct Endpoint { pub addr: String, pub role: Role, }"`. Its CLI (illustrative flag vocabulary — Claude's discretion per CONTEXT.md) accumulates `UserMsg`/`AbortBackendMsg` frames, applies per-uid configured misbehavior, and replies with `DetokenizeMsg` or `BatchTokenizerMsg` using the exact same unwrap convention upstream uses for incoming batches [VERIFIED: /Users/li-yangtseng/Codes/mini-rsglang/.claude/worktrees/sparkling-juggling-shore/vendor/mini-sglang/python/minisgl/tokenizer/server.py:24-27] — `"def _unwrap_msg(msg: BaseTokenizerMsg) -> List[BaseTokenizerMsg]: if isinstance(msg, BatchTokenizerMsg): return msg.data return [msg]"`.

```
mock-scheduler \
  --backend-addr ipc:///tmp/rsgm-0 --backend-role bind \
  --detok-addr   ipc:///tmp/rsgm-1 --detok-role   connect \
  --prefill-delay-ms 5 --decode-delay-ms 2 \
  --batch-size 4 \
  --misbehave-uids 3,7 --behavior late-abort-token \
  --misbehave-uids 9   --behavior drop-overlong
```

- `late-abort-token`: after receiving `AbortBackendMsg{uid}` for a flagged uid, emit one (or N) more `DetokenizeMsg{uid, finished:false}` before actually stopping — reproduces the suspected upstream abort-during-prefill behavior noted in `.planning/STATE.md`'s Phase 6 blocker.
- `drop-overlong`: silently never reply to `UserMsg{uid}` for a flagged uid — reproduces upstream's silent-hang-on-overlong-prompt behavior that motivates LIFE-04.
- `--batch-size N`: accumulate up to N pending `DetokenizeMsg` (across uids) before flushing one `BatchTokenizerMsg`; flush early on a short timer if fewer than N are ready.

### Anti-Patterns to Avoid
- **Hand-rolling a drop-oldest ring buffer with `Mutex<VecDeque<T>>` + `Notify`:** `tokio::sync::broadcast` already provides this exact semantic, with the dropped-count signal built in (`Lagged(n)`). See §Don't Hand-Roll.
- **Adding a new `__type__` wire message for "mock-scheduler readiness":** the wire schema is closed at 8 tags, byte-verified against upstream in Phase 1 [VERIFIED: /Users/li-yangtseng/Codes/mini-rsglang/.claude/worktrees/sparkling-juggling-shore/crates/rsg-wire/src/lib.rs:33-42] — `"pub const WIRE_TYPE_TAGS: [&str; 8] = [ \"UserMsg\", \"AbortBackendMsg\", \"ExitMsg\", \"BatchBackendMsg\", \"DetokenizeMsg\", \"BatchTokenizerMsg\", \"SamplingParams\", \"Tensor\", ];"`. Any process-level readiness signal `mock-scheduler` needs for test synchronization must happen outside this wire protocol (e.g. a stdout/stderr log line the test waits for, mirroring the existing `wait_for_log` helper in `crates/rsg-server/tests/cli.rs:95-116`), never as a 9th message type.
- **Dropping a `broadcast::Receiver` immediately after creating the channel:** once all receivers of a `broadcast` channel are dropped, the `Sender` is permanently closed and every future `send()` returns `Err` — the one `Receiver` returned from `register()` must be held by the per-request consumer for the channel's whole lifetime.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Bounded, drop-oldest, count-what-was-dropped per-uid channel (D-05/D-06) | `Mutex<VecDeque<T>>` + `Notify` ring buffer | `tokio::sync::broadcast::channel(16)` | Already implements "overwrite oldest at capacity" and reports the exact drop count via `RecvError::Lagged(n)` [CITED: docs.rs/tokio] — a hand-rolled version would have to reimplement and separately test both behaviors |
| Property-based concurrency ordering proof (D-02/D-03) | A custom interleaving-generator/scheduler harness | `proptest` generating the *scenario* (uid count, per-uid submit/abort timing/ordering), executed against the real async writer + real subprocess | Locked decision (D-02); `proptest`'s shrinking also helps minimize a failing interleaving down to the smallest reproducing case |
| Locating a sibling workspace binary from an integration test | Manual path arithmetic from `std::env::current_exe()`, or adding `escargot`/`assert_cmd` | `env!("CARGO_BIN_EXE_<name>")` for a binary in the *same* package | Already the proven, zero-dependency pattern in this exact repo [VERIFIED: /Users/li-yangtseng/Codes/mini-rsglang/.claude/worktrees/sparkling-juggling-shore/crates/rsg-server/tests/cli.rs:42] — `"Command::new(env!(\"CARGO_BIN_EXE_rsg-server\"))"`. Cross-package resolution tools only earn their keep once a *different* crate's tests need to spawn `mock-scheduler`, which is not Phase 3's problem. |

**Key insight:** every "hand-roll or not" decision in this phase resolves the same way — the project's existing stack (`tokio::sync`, Cargo's own binary-resolution convention) already has a narrow, well-documented primitive that matches the locked decision's exact semantics; building a custom version would mean re-deriving and re-testing behavior the dependency already guarantees.

## Runtime State Inventory

Not applicable — Phase 3 is new code (a new writer/dispatcher module and a new `mock-scheduler` binary), not a rename/refactor/migration of existing runtime state.

## Common Pitfalls

### Pitfall 1: Early-send races against a not-yet-bound `mock-scheduler`
**What goes wrong:** A ZMQ PUSH socket's `connect()` call succeeds immediately even if the peer hasn't bound yet (libzmq queues/retries under the hood); a test that spawns `mock-scheduler` and the writer "at the same time" and immediately starts sending can pass most of the time and then flake once `mock-scheduler`'s bind is slow (e.g. under CI load).
**Why it happens:** ZMQ's connect-before-bind tolerance is a feature for production resilience, but it hides a race in tests that assume strict ordering of "mock-scheduler is listening" before "first frame sent."
**How to avoid:** Have `mock-scheduler` emit an explicit readiness line (stdout or `tracing` to stderr) once its sockets are open, and have the test block on that line before sending the first frame — exactly the `wait_for_log` pattern already proven in `crates/rsg-server/tests/cli.rs:95-116`.
**Warning signs:** Intermittent test failures where the first few submitted uids are missing from `mock-scheduler`'s observed receive order, but only on some runs.

### Pitfall 2: `CARGO_BIN_EXE_<name>` cross-package ambiguity
**What goes wrong:** If `mock-scheduler` is split into its own workspace crate and referenced only as a dev-dependency, `env!("CARGO_BIN_EXE_mock-scheduler")` inside `rsg-server`'s integration tests may or may not resolve, depending on Cargo version and build graph — the Cargo book documents the variable without explicitly scoping it to same-package-only or workspace-wide [CITED: https://doc.rust-lang.org/cargo/reference/environment-variables.html — ambiguous wording, MEDIUM].
**Why it happens:** The only confirmed-working case, both in official docs and in this repo's own precedent, is a binary target that lives in the *same package* as the test.
**How to avoid:** Keep `mock-scheduler` as a second `[[bin]]`/`src/bin/` target inside the `rsg-server` package for Phase 3 (see §Architecture Patterns, Recommended Project Structure). If a later phase's test suite lives in a different crate and needs to spawn it, resolve that with `escargot` or a documented path convention at that time — don't solve an unconfirmed future need now.
**Warning signs:** `env!("CARGO_BIN_EXE_mock-scheduler")` fails to compile ("environment variable not defined") in a crate other than the one that declares the `mock-scheduler` binary target.

### Pitfall 3: Dropping a message's wire-shape distinction (bare vs. batch)
**What goes wrong:** Sending a 1-element `BatchBackendMsg{data:[x]}` instead of the bare `x` (or vice versa for a single detokenize reply) is still valid msgpack and will decode fine against `rsg-wire`'s own round-trip tests, but it diverges from upstream's own encoder behavior, which is a latent parity bug waiting to surface once a real backend (not `mock-scheduler`) is in the loop.
**Why it happens:** `rsg-wire`'s `BatchBackendMsg`/`BatchTokenizerMsg` variants happily accept a one-element `data` vec; nothing in the type system forces "wrap only when count > 1."
**How to avoid:** Mirror the exact upstream branching verified in `scheduler/io.py:124-130` and `tokenizer/server.py:83-85/99-101/106-108` — `if count == 1: send bare; elif count > 1: send Batch*Msg`.
**Warning signs:** A WIRE-02-style decode-through-Python test added later for this new coalescing logic fails only on batches of exactly size 1.

### Pitfall 4: Treating `broadcast::Sender::send`'s `Ok` as "delivered, not dropped"
**What goes wrong:** `Sender::send()` on a `tokio::sync::broadcast` channel returns `Ok(receiver_count)` as long as at least one receiver exists — it returns `Ok` even when the send caused an older, unread value to be silently overwritten. Code that treats `Ok` from the *dispatcher's* `send()` call as proof nothing was dropped will never observe D-06's required drop signal, because the drop is only visible to the *consumer* on its next `recv()` as `Err(RecvError::Lagged(n))`.
**Why it happens:** The drop-detection point and the send point are on opposite ends of the channel by design.
**How to avoid:** Implement the per-uid dropped-token counter and `tracing::warn!` in the *consumer's* `recv()` loop (on `Lagged`), not in the dispatcher's `send()` call site.
**Warning signs:** A test that asserts "dropped-token counter increments when the dispatcher calls `send()` on a full channel" never fails because the dispatcher-side code path never sees the drop at all.

## Code Examples

### Proptest driving async code against a real subprocess (D-02/D-03)

```rust
// tests/ordering_proptest.rs — illustrative skeleton, not a verified compiled example
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })] // count is Claude's discretion
    #[test]
    fn abort_never_precedes_its_own_submit(
        ops in proptest::collection::vec(op_strategy(), 1..200)
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap(); // manual block_on — no mature proptest+tokio glue crate exists
        rt.block_on(async {
            let mock = spawn_mock_scheduler().await;           // real subprocess, D-03
            let writer = spawn_writer(open_transport(&mock));  // real transport, D-03
            run_ops_concurrently(&writer, ops).await;          // many tokio::spawn tasks issuing submit()/abort()
            let received_order = mock.drain_observed_order().await; // read from mock-scheduler's stdout log lines
            assert_no_abort_before_its_submit(&received_order);
        });
    }
}
```
This pattern — manually building a `Runtime` and calling `.block_on()` inside each `proptest!` case body — is the standard community workaround for combining `proptest` with async code, since no actively-maintained `proptest`+`tokio` glue crate exists today (`proptest_async` targets `async-std`, not `tokio`) [CITED: web search, cross-checked — MEDIUM]. Spike this early (Wave 0) to confirm acceptable test wall-clock time, since each case spins up a fresh runtime and a fresh subprocess.

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|---------------|--------|
| N/A | N/A | N/A | This phase's domain (ZMQ transport ordering, bounded drop-oldest channels, property-based concurrency testing) has no recent upstream API churn relevant here — `tokio::sync::broadcast` and `proptest`'s core API have been stable for years. |

**Deprecated/outdated:** none identified for this phase's scope.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | Success Criterion 1's "receives its readiness handshake" refers to a process-level readiness signal from `mock-scheduler` (e.g. a log line the test waits for), not a new ZMQ wire message and not necessarily Phase 1's stdin-based `Handshake` JSON (`BASE-03`) — since `mock-scheduler` is spawned directly by the test, not via the Python launcher that relays that handshake. | Architecture Patterns (Anti-Patterns), Pitfall 1 | If wrong, the planner might build a 9th wire message type (breaking the frozen 8-tag schema) or route the test through the Python launcher unnecessarily (adding Python-process coupling to a Phase 3 goal that CONTEXT.md scopes as Mac/GPU-free). Low-to-medium risk; flag for discuss-phase/plan-check confirmation. |
| A2 | Recommended crate layout — giving `rsg-server` a `lib.rs` and adding `mock-scheduler` as a same-package `src/bin/` target — is a *recommendation*, not a locked requirement; CONTEXT.md explicitly leaves module/crate layout to Claude's discretion. | Architecture Patterns, §Alternatives Considered | Low risk: an equally valid separate-crate layout exists; a planner choosing it just needs to also solve the `CARGO_BIN_EXE_<name>` cross-package resolution problem it reintroduces (Pitfall 2). |
| A3 | `tokio::sync::broadcast::channel(16)` per uid is the right primitive for D-05/D-06's semantics, and remains a reasonable foundation for Phase 5's FSM (per D-05's "costly to reverse" note) even though Phase 5 isn't planned yet. | Architecture Patterns (Pattern 2) | Medium risk given D-05's explicit "costly to reverse" flag — but the chosen primitive's public API surface (`Sender`/`Receiver`/`recv`/`Lagged`) is narrow and already matches every stated requirement, minimizing lock-in risk relative to a custom type. |
| A4 | The proptest+tokio "manual `Runtime::block_on` per case" pattern is sound for this test's needs (performance, shrinking behavior with real subprocess I/O). | Common Pitfalls, Code Examples | Medium risk: this is a cross-checked web-search community pattern (MEDIUM confidence), not official `proptest` or `tokio` documentation. Recommend spiking it in Wave 0 before committing the full ordering-test design. |
| A5 | `rustc-hash`/`FxHashMap` is an acceptable choice for the per-uid dispatch table (uids in Phase 3 originate only from the Rust side itself and from `mock-scheduler` echoing them back — not from an untrusted external client at this layer), so `FxHashMap`'s non-DoS-resistant hashing is not a concern here. | Standard Stack (Supporting) | Low risk for Phase 3; would need re-examination if a later phase ever keys this same table directly off attacker-controlled input. |

## Open Questions

1. **What exactly synchronizes the writer/test with `mock-scheduler`'s startup before the first frame is sent?**
   - What we know: ZMQ PUSH `connect()` succeeds even before the peer binds; the existing `cli.rs` test harness already has a proven "wait for a log line" pattern.
   - What's unclear: Whether criterion 1's "receives its readiness handshake" implies something more formal (e.g. a one-shot acknowledgment message) than a log-line wait.
   - Recommendation: Default to the log-line-wait pattern (A1); raise during `/gsd-plan-phase` or a plan-check pass if the planner wants a stronger guarantee.

2. **Does the per-uid dispatch table need an explicit `deregister()` call, or does dropping the `Receiver` suffice?**
   - What we know: Once all receivers of a `broadcast::Sender` are dropped, the sender is closed and further `send()`s return `Err` (silently dropped, satisfying D-04) — but the `FxHashMap` entry itself would still leak memory for the lifetime of the process unless explicitly removed.
   - What's unclear: Whether Phase 3's own scope requires active table cleanup, or whether it's acceptable to leave that to Phase 5 (which will own request lifecycle termination).
   - Recommendation: Implement `deregister(uid)` now (cheap, a few lines) since D-04's "unknown/deregistered uids" language already implies the table must support removal; don't defer a trivial piece of the stated contract.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust toolchain | All of Phase 3 | ✓ | 1.99.0 [VERIFIED: /Users/li-yangtseng/Codes/mini-rsglang/.claude/worktrees/sparkling-juggling-shore/rust-toolchain.toml:2] | — |
| `zmq` (libzmq via `zmq-sys`/`zeromq-src`) | Transport + `mock-scheduler` sockets | ✓ (already building on this Mac per Phase 1) | 0.10.0 | — |
| `cargo` / workspace resolver 3 | Build, test | ✓ | — [VERIFIED: /Users/li-yangtseng/Codes/mini-rsglang/.claude/worktrees/sparkling-juggling-shore/Cargo.toml:2] | — |
| GPU / real Python scheduler | Not needed this phase | n/a | — | `mock-scheduler` is the explicit GPU-free substitute (project goal) |

**Missing dependencies with no fallback:** none.
**Missing dependencies with fallback:** none — this phase was explicitly designed to need no GPU/Linux backend.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | `cargo test` (plain `#[test]`/integration tests), workspace-wide — no `cargo-nextest` wired in yet despite `.claude/CLAUDE.md` recommending it |
| Config file | none — `scripts/check_all.sh` [VERIFIED: /Users/li-yangtseng/Codes/mini-rsglang/.claude/worktrees/sparkling-juggling-shore/scripts/check_all.sh:14-15] — `"step 1 \"cargo test --workspace\" \ncargo test --workspace"` |
| Quick run command | `cargo test -p rsg-server ordering_proptest` / `cargo test -p rsg-server dispatch_backpressure` |
| Full suite command | `scripts/check_all.sh` |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| WIRE-03 | Scheduler never observes an abort before its own submit, under concurrent load | property (proptest) + integration (real subprocess) | `cargo test -p rsg-server --test ordering_proptest` | ❌ Wave 0 |
| WIRE-03 | Replies route to the correct in-flight uid; unknown uids dropped without crash; slow consumer on one uid doesn't stall others | integration | `cargo test -p rsg-server --test dispatch_backpressure` | ❌ Wave 0 |
| MOCK-01 | `mock-scheduler` reproduces late-tokens-after-abort, silently-dropped-overlong-prompt, batched-reply behaviors, one test per behavior | integration (spawns `mock-scheduler` subprocess) | `cargo test -p rsg-server --test mock_scheduler_behaviors` | ❌ Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p rsg-server`
- **Per wave merge:** `scripts/check_all.sh`
- **Phase gate:** Full suite green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] Add `rsg-server/src/lib.rs` exposing `transport` (and new `writer`/`dispatch` modules) — needed before any of the new integration tests can `use rsg_server::...`
- [ ] Add `mock-scheduler` as `rsg-server/src/bin/mock-scheduler.rs` — needed before `env!("CARGO_BIN_EXE_mock-scheduler")` resolves in tests
- [ ] `cargo add proptest --dev -p rsg-server` and `cargo add rustc-hash -p rsg-server` — neither crate is in the workspace yet
- [ ] `tests/ordering_proptest.rs`, `tests/dispatch_backpressure.rs`, `tests/mock_scheduler_behaviors.rs` — none exist yet
- [ ] Spike the manual `Runtime::block_on`-per-proptest-case pattern (A4) for acceptable wall-clock time before committing to the full test design

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | No | The `ipc://` Unix-domain socket boundary has no application-level auth concept; access control is the filesystem permissions on the socket file, already fixed by Phase 1's design. |
| V3 Session Management | No | No session concept at this transport layer. |
| V4 Access Control | No | Same-machine, same-user local IPC; no cross-principal access control applies within Phase 3's scope. |
| V5 Input Validation | Yes | Every frame off the detokenizer socket is attacker/peer-controlled bytes (even if the "attacker" here is just a misbehaving `mock-scheduler` or, later, a buggy real scheduler) and must decode through `Result`-returning `rsg_wire::decode_tokenizer`, never `.unwrap()`, mirroring the existing `WireError` handling [VERIFIED: /Users/li-yangtseng/Codes/mini-rsglang/.claude/worktrees/sparkling-juggling-shore/crates/rsg-wire/src/lib.rs:44-55] — `"pub enum WireError { Encode(...), Decode(...), TensorDtype{...}, TensorLength{...} }"`. |
| V6 Cryptography | No | No cryptography at this boundary; `ipc://` is a local Unix socket, not a network transport. |

### Known Threat Patterns for this stack

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Malformed/truncated msgpack frame on the detokenizer PULL socket causes a panic and kills the whole dispatcher thread | Denial of Service | Match on `rsg_wire::decode_tokenizer`'s `Result` in the dispatcher loop; log and skip the bad frame, never propagate a panic across the thread boundary — consistent with `rsg-wire`'s own `unknown_type_tag_is_rejected` test already proving the decoder returns `Err` rather than panicking [VERIFIED: /Users/li-yangtseng/Codes/mini-rsglang/.claude/worktrees/sparkling-juggling-shore/crates/rsg-wire/src/lib.rs:374-385]. |
| A uid collision/replay (a misbehaving or buggy backend sends `DetokenizeMsg` for a uid it was never given) is treated as a routed reply | Spoofing (weak form, same-trust-boundary) | D-04 already requires dropping replies for unknown/deregistered uids without error — this doubles as the mitigation; no extra work needed beyond implementing D-04 as specified. |

## Sources

### Primary (HIGH confidence)
- `crates/rsg-wire/src/lib.rs` (read this session) — wire type definitions, `WIRE_TYPE_TAGS`, `WireError`
- `crates/rsg-server/src/transport.rs` (read this session) — `Transport` trait, `Endpoint`/`Role`, `ZmqTransport`
- `crates/rsg-server/src/main.rs`, `crates/rsg-server/src/handshake.rs` (read this session) — CLI/exit-code/handshake conventions
- `crates/rsg-server/tests/cli.rs` (read this session) — proven `CARGO_BIN_EXE_<name>` same-package subprocess-test pattern, `wait_for_log` readiness pattern
- `vendor/mini-sglang/python/minisgl/scheduler/io.py`, `vendor/mini-sglang/python/minisgl/tokenizer/server.py`, `vendor/mini-sglang/python/minisgl/utils/mp.py` (read this session) — upstream coalescing (bare-vs-batch) behavior
- `Cargo.toml`, `crates/rsg-server/Cargo.toml`, `crates/rsg-wire/Cargo.toml`, `rust-toolchain.toml` (read this session) — pinned versions
- `.planning/phases/03-zmq-transport-mock-scheduler/03-CONTEXT.md`, `.planning/REQUIREMENTS.md`, `.planning/STATE.md`, `.planning/ROADMAP.md` (read this session)
- crates.io registry API (`https://crates.io/api/v1/crates/proptest`, `.../rustc-hash`), queried this session — version/legitimacy confirmation
- `gsd-tools query package-legitimacy check` — `proptest` and `rustc-hash` both `OK`

### Secondary (MEDIUM confidence)
- `https://docs.rs/tokio/latest/tokio/sync/broadcast/index.html` (WebFetch, this session) — capacity/overwrite/`Lagged` semantics
- `https://doc.rust-lang.org/cargo/reference/environment-variables.html` and `.../cargo-targets.html` (WebFetch, this session) — `CARGO_BIN_EXE_<name>` scope, `src/bin/` library access
- WebSearch, cross-checked (proptest+tokio community pattern; `proptest_async` targeting `async-std`)

### Tertiary (LOW confidence)
- None retained — all WebSearch findings above were corroborated by a second source (official docs or this repo's own code) before being used as a recommendation.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — every new dependency (`proptest`, `rustc-hash`) is both a locked/pre-recommended choice and independently confirmed live on the crates.io registry this session.
- Architecture: MEDIUM-HIGH — the single-writer and per-uid-channel patterns are grounded in this repo's own existing code and tokio's official docs; the exact crate-layout recommendation (lib+bin same-package) is Claude's discretion per CONTEXT.md, not a locked requirement.
- Pitfalls: MEDIUM — most are derived from verified official-doc semantics (broadcast, CARGO_BIN_EXE) rather than from having actually run the new code (which doesn't exist yet).

**Research date:** 2026-10-05
**Valid until:** 30 days (stable Rust/tokio/proptest APIs; no fast-moving dependencies in this phase's scope)
