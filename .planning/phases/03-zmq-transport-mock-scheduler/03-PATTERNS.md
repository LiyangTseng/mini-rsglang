# Phase 3: ZMQ Transport & Mock Scheduler - Pattern Map

**Mapped:** 2026-10-05
**Files analyzed:** 9
**Analogs found:** 9 / 9

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `crates/rsg-server/src/lib.rs` | module-root/config | request-response | `crates/rsg-server/src/main.rs` (module decls, lines 4-5) | role-match |
| `crates/rsg-server/Cargo.toml` (add `[lib]`, `proptest`/`rustc-hash` deps) | config | — | `crates/rsg-server/Cargo.toml` (itself) | exact |
| `crates/rsg-server/src/writer.rs` | service (single-writer thread) | event-driven / streaming | `crates/rsg-server/src/main.rs::spawn_stdin_reader` (lines 58-76) + `transport.rs::Transport::send_backend` | role-match |
| `crates/rsg-server/src/dispatch.rs` | service (per-uid routing table) | pub-sub / event-driven | `crates/rsg-server/src/transport.rs::recv_detok` (lines 71-84) | role-match |
| `crates/rsg-server/src/bin/mock-scheduler.rs` | service/controller (subprocess binary) | request-response / event-driven | `crates/rsg-server/src/main.rs` (whole file: CLI, signal handling, exit codes) | exact |
| `crates/rsg-server/tests/ordering_proptest.rs` | test (property-based) | event-driven | `crates/rsg-server/tests/cli.rs` (process-spawn harness, lines 1-140) | role-match |
| `crates/rsg-server/tests/dispatch_backpressure.rs` | test (integration) | event-driven | `crates/rsg-server/tests/cli.rs::Server` helper | role-match |
| `crates/rsg-server/tests/mock_scheduler_behaviors.rs` | test (integration, subprocess) | request-response | `crates/rsg-server/tests/cli.rs` (`Server::spawn`, `wait_for_log`, lines 36-116) | exact |
| `crates/rsg-server/src/main.rs` (modified: `use rsg_server::{transport, writer, dispatch}`, wire writer/dispatch into idle loop) | controller | request-response | itself (pre-existing, being extended) | exact |

## Pattern Assignments

### `crates/rsg-server/src/lib.rs` (module-root, new)

**Analog:** `crates/rsg-server/src/main.rs` lines 1-5 (module declarations) and the overall crate doc-comment style.

**Pattern to copy** — turn the existing `mod` declarations into `pub mod` and move them into a new `lib.rs`, then have `main.rs` consume the library:
```rust
// main.rs today (lines 1-14)
//! rsg-server: the Rust frontend. In Phase 1 a skeleton that opens its two ZMQ
//! sockets, reads the readiness handshake and idles (D-08).

mod handshake;
mod transport;

use std::io::BufRead;

use clap::Parser;
use tokio::signal::unix::{Signal, SignalKind, signal};
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

use transport::{Endpoint, Role, ZmqTransport};
```
New `lib.rs` should mirror this doc-comment convention and re-export:
```rust
//! rsg-server library: transport, single-writer (D-01) and per-uid dispatch
//! (D-04) modules shared by the `rsg-server` and `mock-scheduler` binaries.

pub mod dispatch;
pub mod handshake;
pub mod transport;
pub mod writer;
```
`main.rs` then changes its own `mod handshake; mod transport;` lines to `use rsg_server::{dispatch, handshake, transport, writer};`.

---

### `crates/rsg-server/Cargo.toml` (modified)

**Analog:** itself — current dependency-block style (flat `name.workspace = true` list, lines 8-15).

**Pattern to copy:**
```toml
[lib]
name = "rsg_server"
path = "src/lib.rs"

[[bin]]
name = "rsg-server"
path = "src/main.rs"

[[bin]]
name = "mock-scheduler"
path = "src/bin/mock-scheduler.rs"

[dependencies]
rustc-hash.workspace = true
# ...existing deps unchanged...

[dev-dependencies]
proptest.workspace = true
```
Add `rustc-hash = "2.1.3"` and `proptest = "1.11.0"` to the root workspace `Cargo.toml`'s `[workspace.dependencies]` table first (same place `zmq`, `clap`, `tokio` etc. are already pinned), following the existing `name = "version"` style there.

---

### `crates/rsg-server/src/writer.rs` (new, D-01/D-02/D-03)

**Analog 1 (thread-ownership pattern):** `crates/rsg-server/src/main.rs::spawn_stdin_reader` (lines 56-76):
```rust
/// Read stdin on a dedicated OS thread: tokio's stdin is a blocking read that
/// cannot be cancelled and would hang runtime shutdown (RESEARCH Pattern 5).
fn spawn_stdin_reader() -> mpsc::Receiver<StdinEvent> {
    let (tx, rx) = mpsc::channel(16);
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let event = match line {
                Ok(line) => StdinEvent::Line(line),
                Err(e) => {
                    let _ = tx.blocking_send(StdinEvent::Error(e.to_string()));
                    return;
                }
            };
            if tx.blocking_send(event).is_err() {
                return;
            }
        }
        let _ = tx.blocking_send(StdinEvent::Eof);
    });
    rx
}
```
Copy the shape exactly: a `std::thread::spawn` closure owning a blocking resource (there: stdin; here: the ZMQ PUSH socket via `Transport::send_backend`), fed by a bounded `tokio::sync::mpsc::channel`, using `blocking_recv`/`blocking_send` to bridge sync and async — this is the project's `tx-zmq` convention referenced in `.claude/CLAUDE.md` and RESEARCH.md Pattern 1.

**Analog 2 (what gets sent, error propagation):** `crates/rsg-server/src/transport.rs::Transport::send_backend` (lines 64-69):
```rust
impl Transport for ZmqTransport {
    fn send_backend(&self, frame: &[u8]) -> anyhow::Result<()> {
        self.backend
            .send(frame, 0)
            .context("send on backend socket")
    }
    ...
```
The writer thread calls `transport.send_backend(&bytes)` after `rsg_wire::encode_backend(&msg)`; use `anyhow::Context` the same way (`.context("...")`) for every fallible step, matching the error-handling idiom already used throughout `transport.rs` and `main.rs`.

**Coalescing logic (bare vs. batch):** mirror upstream exactly, as flagged in RESEARCH.md Pitfall 3 — `if pending.len() == 1 { send bare } else { send BackendMsg::BatchBackendMsg { data: pending } }`. No existing Rust analog for this branch (it's new logic); the branching rule itself is sourced from `vendor/mini-sglang/python/minisgl/scheduler/io.py:124-130`.

**Generic type reused:** `rsg_wire::{BackendMsg, encode_backend}` from `crates/rsg-wire/src/lib.rs` lines 130-144, 171-173 — do not redefine these; `WriterCmd` wraps the existing `BackendMsg` variants (`UserMsg`, `AbortBackendMsg`), it does not duplicate their fields.

---

### `crates/rsg-server/src/dispatch.rs` (new, D-04/D-05/D-06)

**Analog (poll/recv loop and error handling):** `crates/rsg-server/src/transport.rs::recv_detok` (lines 71-84):
```rust
fn recv_detok(&self, timeout_ms: i64) -> anyhow::Result<Option<Vec<u8>>> {
    let ready = self
        .detok
        .poll(zmq::POLLIN, timeout_ms)
        .context("poll detokenizer socket")?;
    if ready == 0 {
        return Ok(None);
    }
    let frame = self
        .detok
        .recv_bytes(0)
        .context("recv on detokenizer socket")?;
    Ok(Some(frame))
}
```
The dispatcher thread's loop structure (poll/recv, then decode, then route) follows this same shape, but decoding must use `rsg_wire::decode_tokenizer` (lines 186-188 of `rsg-wire/src/lib.rs`) and match on its `Result` rather than `.unwrap()`, per RESEARCH.md's Security Domain section (never panic across the dispatcher thread boundary on a malformed frame — log and skip).

**Per-uid table and channel semantics:** no existing in-repo analog (this is new); use `tokio::sync::broadcast::channel(16)` keyed by `rustc_hash::FxHashMap<i64, broadcast::Sender<TokenizerMsg>>` exactly as sketched in RESEARCH.md Pattern 2 (`register`/`deregister`/`dispatch_one`). Reuse `rsg_wire::TokenizerMsg` (lines 149-158) unmodified as the channel's payload type — do not wrap it in a new struct.

**Unwrap-batch convention:** when a frame decodes to `TokenizerMsg::BatchTokenizerMsg { data }`, dispatch each element individually to its own uid's channel, mirroring upstream's `_unwrap_msg` in `vendor/mini-sglang/python/minisgl/tokenizer/server.py:24-27`.

---

### `crates/rsg-server/src/bin/mock-scheduler.rs` (new, D-08/D-09/D-10)

**Analog:** `crates/rsg-server/src/main.rs` — the entire file is the template: CLI struct via `clap::Parser`, exit-code constants, `tracing_subscriber` setup, signal handling, and socket-open error handling.

**Imports pattern** (lines 1-14):
```rust
mod handshake;
mod transport;

use std::io::BufRead;

use clap::Parser;
use tokio::signal::unix::{Signal, SignalKind, signal};
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

use transport::{Endpoint, Role, ZmqTransport};
```
`mock-scheduler.rs` instead does `use rsg_server::transport::{Endpoint, Role, ZmqTransport};` (consuming the new lib crate) and additionally pulls in `rsg_wire::{BackendMsg, TokenizerMsg, decode_backend, encode_tokenizer}`.

**CLI struct pattern** (lines 26-47):
```rust
#[derive(Parser, Debug)]
#[command(name = "rsg-server", about = "mini-rsglang Rust frontend")]
struct Cli {
    #[arg(long, value_name = "ADDR")]
    backend_addr: String,
    #[arg(long, value_enum)]
    backend_role: Role,
    #[arg(long, value_name = "ADDR")]
    detok_addr: String,
    #[arg(long, value_enum)]
    detok_role: Role,
    ...
}
```
Extend this exact shape with D-09/D-10's new flags: `--prefill-delay-ms`, `--decode-delay-ms`, `--batch-size`, repeatable `--misbehave-uids`/`--behavior` pairs (use `clap`'s `ValueEnum` for the behavior vocabulary, matching how `Role` already derives `clap::ValueEnum` in `transport.rs` lines 11-16).

**Exit-code constants pattern** (lines 16-23):
```rust
const EXIT_OK: i32 = 0;
const EXIT_STARTUP: i32 = 1;
const EXIT_BAD_HANDSHAKE: i32 = 2;
const EXIT_STDIN_EOF: i32 = 3;
```
`mock-scheduler` should define its own small exit-code set the same way (e.g. `EXIT_OK = 0`, `EXIT_STARTUP = 1`) for consistency, per the canonical refs in CONTEXT.md.

**Signal + socket-open error handling pattern** (lines 78-93, 129-136):
```rust
fn install_signal(kind: SignalKind, name: &str) -> Signal {
    match signal(kind) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("failed to install {name} handler: {e}");
            std::process::exit(EXIT_STARTUP);
        }
    }
}
...
let _transport = match ZmqTransport::open(&backend, &detok) {
    Ok(t) => t,
    Err(e) => {
        tracing::error!("failed to open sockets: {e:#}");
        std::process::exit(EXIT_STARTUP);
    }
};
tracing::info!("sockets ready");
```
Reuse verbatim. The RESEARCH.md readiness-signal recommendation (Pitfall 1/A1) says to emit a `tracing::info!("sockets ready")`-style log line for the test harness to wait on — this is already the exact line `main.rs` emits, so `mock-scheduler` should emit an equivalent line (e.g. `"mock-scheduler ready"`) at the same point in startup.

---

### `crates/rsg-server/tests/ordering_proptest.rs`, `dispatch_backpressure.rs`, `mock_scheduler_behaviors.rs` (new)

**Analog:** `crates/rsg-server/tests/cli.rs` (full file; key excerpt lines 36-78, 94-141).

**Subprocess-spawn + unique-address pattern** (lines 36-62):
```rust
static COUNTER: AtomicUsize = AtomicUsize::new(0);
...
fn spawn() -> Server {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let pid = std::process::id();
    let backend = format!("ipc:///tmp/rsgc-{pid}-{n}-0");
    let detok = format!("ipc:///tmp/rsgc-{pid}-{n}-1");
    let mut child = Command::new(env!("CARGO_BIN_EXE_rsg-server"))
        .args([...])
        .env("RUST_LOG", "info")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn rsg-server");
    ...
}
```
The new test files spawn `mock-scheduler` the same way, substituting `env!("CARGO_BIN_EXE_mock-scheduler")` (same-package binary target, per RESEARCH.md Pitfall 2/Don't-Hand-Roll table) and using a similarly-unique `ipc:///tmp/rsgm-<pid>-<n>-{0,1}` address scheme to avoid cross-test collisions.

**Readiness wait-for-log pattern** (lines 94-116):
```rust
/// Wait until a stderr line contains `needle`; panics after 20 s.
fn wait_for_log(&mut self, needle: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(l) = self.lines.lock().unwrap().iter().find(|l| l.contains(needle)) {
            return l.clone();
        }
        if Instant::now() > deadline {
            let _ = self.child.kill();
            panic!("timed out waiting for {needle:?}; stderr:\n{}", self.stderr());
        }
        thread::sleep(Duration::from_millis(20));
    }
}
```
Reuse this verbatim (copy the `Server`/stderr-capture-thread/`wait_for_log` helper into a shared test-support module or duplicate it per RESEARCH.md's Pitfall 1 recommendation) to block on `mock-scheduler`'s own readiness line before sending the first frame.

**Exit/signal pattern** (lines 118-141) — reuse `signal()`/`wait_exit()` verbatim for any test that needs to terminate the `mock-scheduler` subprocess cleanly.

**proptest+tokio glue** — no in-repo analog; use the RESEARCH.md Code Examples skeleton (manual `tokio::runtime::Runtime::new().block_on()` per generated case, `proptest::collection::vec(op_strategy(), 1..200)`), since no existing test file in this repo drives property-based async tests yet.

---

### `crates/rsg-server/src/main.rs` (modified)

**Analog:** itself. Change `mod handshake; mod transport;` to `use rsg_server::{dispatch, handshake, transport, writer};` once `lib.rs` exists (see lib.rs section above), and wire `writer::spawn_writer`/`dispatch::DispatchTable` into the existing idle loop (lines 172-189) following the same `tokio::select!` arm style already used for `stdin.recv()`/`sigint.recv()`/`sigterm.recv()`.

## Shared Patterns

### Error handling (anyhow::Context)
**Source:** `crates/rsg-server/src/transport.rs` lines 64-104 (every fallible ZMQ call wrapped in `.context("...")` / `.with_context(|| ...)`).
**Apply to:** `writer.rs`, `dispatch.rs`, `mock-scheduler.rs` — every socket/encode/decode call site.

### Tracing/logging convention
**Source:** `crates/rsg-server/src/main.rs` lines 106-114, 136, 172 (`tracing::info!`/`tracing::error!`/`tracing::warn!` with structured fields, e.g. `tracing::info!(backend_addr = %cli.backend_addr, ...)`).
**Apply to:** All new files. In particular D-06's dropped-token signal must use `tracing::warn!(uid, dropped = n, "...")` structured-field style, not a bare string.

### CLI convention (clap derive + exit codes)
**Source:** `crates/rsg-server/src/main.rs` lines 16-47.
**Apply to:** `mock-scheduler.rs`'s own `Cli` struct and exit-code constants.

### Endpoint/Role bind-connect convention
**Source:** `crates/rsg-server/src/transport.rs` lines 11-32, 87-104 (`Endpoint { addr, role }`, `open_socket` matching on `Role::Bind`/`Role::Connect`).
**Apply to:** `mock-scheduler.rs` must reuse `rsg_server::transport::{Endpoint, Role, ZmqTransport}` unmodified — do not redefine a parallel type.

### Wire type reuse (no redefinition)
**Source:** `crates/rsg-wire/src/lib.rs` lines 126-188 (`BackendMsg`, `TokenizerMsg`, `encode_backend`, `decode_backend`, `encode_tokenizer`, `decode_tokenizer`).
**Apply to:** `writer.rs`, `dispatch.rs`, `mock-scheduler.rs` all consume these types directly; none of them define new wire structs (the 8-tag schema in `WIRE_TYPE_TAGS`, lines 33-42, is frozen per RESEARCH.md Anti-Patterns).

### Subprocess test harness (spawn/readiness/exit)
**Source:** `crates/rsg-server/tests/cli.rs` lines 1-186 (`Server` struct: `spawn`, `write`, `wait_for_log`, `signal`, `wait_exit`).
**Apply to:** `ordering_proptest.rs`, `dispatch_backpressure.rs`, `mock_scheduler_behaviors.rs`.

## No Analog Found

| File | Role | Data Flow | Reason |
|------|------|-----------|--------|
| (none) | — | — | All 9 files have at least a role-match analog in-repo; the only genuinely new logic (bare-vs-batch coalescing branch, `broadcast`-based per-uid table, proptest+tokio glue, mock-scheduler misbehavior CLI) is covered by RESEARCH.md's Code Examples/Architecture Patterns sections instead, as noted inline above. |

## Metadata

**Analog search scope:** `crates/rsg-server/src/`, `crates/rsg-server/tests/`, `crates/rsg-wire/src/lib.rs`
**Files scanned:** `transport.rs`, `main.rs`, `handshake.rs`, `tests/cli.rs`, `rsg-wire/src/lib.rs`, `Cargo.toml` (workspace + `rsg-server`)
**Pattern extraction date:** 2026-10-05
