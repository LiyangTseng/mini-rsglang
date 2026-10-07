# Phase 5: Request Lifecycle & HTTP API - Pattern Map

**Mapped:** 2026-10-06
**Files analyzed:** 16 (new) + 3 (modified)
**Analogs found:** 15 / 16

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|--------------------|------|-----------|-----------------|----------------|
| `crates/rsg-server/src/main.rs` (extend) | config/CLI | request-response | `crates/rsg-server/src/main.rs` (self, existing) | exact |
| `crates/rsg-server/src/http/mod.rs` | provider (router assembly) | request-response | `crates/rsg-server/src/transport.rs` (module-with-traits shape) | role-match (no HTTP precedent exists) |
| `crates/rsg-server/src/http/routes/generate.rs` | controller | streaming | none (no HTTP code exists) — pattern comes from RESEARCH.md Pattern B | no analog |
| `crates/rsg-server/src/http/routes/chat.rs` | controller | streaming + request-response | none (no HTTP code exists) — pattern comes from RESEARCH.md Pattern C/D | no analog |
| `crates/rsg-server/src/http/routes/models.rs` | controller | request-response | `crates/rsg-server/src/handshake.rs` (serde struct + deny_unknown_fields style) | partial match |
| `crates/rsg-server/src/http/routes/health.rs` | controller | request-response | `crates/rsg-server/src/handshake.rs` (state readout) | partial match |
| `crates/rsg-server/src/http/abort_guard.rs` | utility (Drop-based guard) | event-driven | `crates/rsg-server/src/transport.rs`'s `ZmqBackendTx`/`ZmqDetokRx` split-ownership pattern (owns a resource, Send, narrow trait surface) | role-match |
| `crates/rsg-server/src/fsm/mod.rs` | service (actor) | event-driven | `crates/rsg-server/src/bin/mock-scheduler.rs`'s `run_engine` loop (single-threaded state owner over an event source) | role-match (strong) |
| `crates/rsg-server/src/fsm/state.rs` | model | CRUD (state transitions) | `crates/rsg-server/src/handshake.rs`'s `Handshake` struct (plain data + `Display`/helper impls) | role-match |
| `crates/rsg-server/src/lib.rs` (extend: `pub mod http; pub mod fsm;`) | config | — | `crates/rsg-server/src/lib.rs` (self, existing module list) | exact |
| `crates/rsg-server/tests/http_cancellation.rs` | test | event-driven | `crates/rsg-server/tests/mock_scheduler_process.rs` | exact |
| `crates/rsg-server/tests/http_errors.rs` | test | request-response | `crates/rsg-server/tests/mock_scheduler_process.rs` | exact |
| `crates/rsg-server/tests/abort_timing.rs` | test | event-driven | `crates/rsg-server/tests/mock_scheduler_process.rs` | exact |
| `crates/rsg-server/tests/observability.rs` | test | request-response | `crates/rsg-server/tests/cli.rs` | role-match |
| `crates/rsg-server/tests/stress_128.rs` | test | event-driven, batch | `crates/rsg-server/tests/common/mod.rs`'s `MockScheduler` harness (reused directly) | exact |
| `scripts/gen_api_fixtures.py` | utility (fixture generator) | file-I/O / batch | `scripts/gen_wire_fixtures.py` | exact |
| `Cargo.toml` (workspace, modify) | config | — | `Cargo.toml` (self, existing `[workspace.dependencies]` block) | exact |
| `crates/rsg-server/Cargo.toml` (modify) | config | — | `crates/rsg-server/Cargo.toml` (self, existing) | exact |

## Pattern Assignments

### `crates/rsg-server/src/main.rs` (extend for `--abort-timing`, HTTP mode)

**Analog:** `crates/rsg-server/src/main.rs` (same file, existing `Cli` struct)

**CLI flag pattern** (lines 24-45):
```rust
#[derive(Parser, Debug)]
#[command(name = "rsg-server", about = "mini-rsglang Rust frontend")]
struct Cli {
    #[arg(long, value_name = "ADDR")]
    backend_addr: String,
    #[arg(long, value_enum)]
    backend_role: Role,
    // ...
}
```
Follow this exactly for the new `--abort-timing immediate|deferred` flag (RESEARCH.md's `AbortTiming` `clap::ValueEnum` sketch) and any new `--http-addr`/`--backend-timeout-ms` flags — same `#[arg(long, value_name = ...)]` / `#[arg(long, value_enum)]` conventions, default via `default_value_t`.

**Exit-code convention** (lines 15-21):
```rust
const EXIT_OK: i32 = 0;
const EXIT_STARTUP: i32 = 1;
const EXIT_BAD_HANDSHAKE: i32 = 2;
const EXIT_STDIN_EOF: i32 = 3;
```
`mock-scheduler.rs` adds `EXIT_BAD_FRAME: i32 = 4` the same way (next free number, doc-commented one line above). Phase 5 should allocate the next free exit code similarly if the HTTP server needs its own distinguishable failure (e.g. HTTP listener bind failure) rather than reusing `EXIT_STARTUP` silently — reuse `EXIT_STARTUP` unless a new failure mode needs separate diagnosis.

**Tracing-init pattern** (lines 96-102, identical in `mock-scheduler.rs` lines 78-84):
```rust
tracing_subscriber::fmt()
    .with_writer(std::io::stderr)
    .with_ansi(false)
    .with_env_filter(
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
    )
    .init();
```
Copy verbatim into the HTTP server binary's `main()`.

**Socket-then-handshake-then-serve sequencing** (lines 114-170): open transport before touching stdin; `_transport` kept alive with a leading-underscore binding and a comment on why nothing is sent before handshake. The HTTP server's `axum::serve(...)` should start only after handshake success (mirrors `tokio::select!` gating pattern at lines 138-168) — store the parsed `Handshake` (for `max_seq_len`) in shared `AppState` at that point.

---

### `crates/rsg-server/src/fsm/mod.rs` (new — FSM actor)

**Analog:** `crates/rsg-server/src/bin/mock-scheduler.rs`'s `run_engine` (lines 145-220ish) — a single-owner loop over an `FxHashMap`-shaped table of in-flight requests (`Running` struct), draining an event source and advancing due timers.

**Core actor-loop pattern** (lines 63-73, 136-149):
```rust
/// One in-flight request the engine is emitting echo tokens for.
struct Running {
    prompt_ids: Vec<i32>,
    total: i64,
    emitted: i64,
    due: Instant,
}

enum ProcessOutcome {
    Continue,
    Exit(i32),
}

fn run_engine(
    transport: &ZmqSchedulerTransport,
    prefill_delay: Duration,
    decode_delay: Duration,
) -> i32 {
    // (1) wait for a frame or next due time
    // (2) take one step at clock `now`
    // (3) drain every available frame, process with arrival time `now`
    // (4) advance every request whose due time <= now
}
```
Mirror this shape for the FSM actor: replace `BTreeMap<uid, Running>` with `FxHashMap<u64, ReqState>` (per CLAUDE.md/RESEARCH.md Pattern G), replace the ZMQ-frame event source with an `mpsc::Receiver<FsmEvent>` inbox (`New`, `Tokens`, `Cancel`, `Finished`), and keep the same "single owner, no locks" discipline. The "check uid still active before acting" guard RESEARCH.md calls out (LIFE-01's exactly-one-terminal-state guarantee) should follow the same defensive-match style `mock-scheduler.rs` uses when it encounters an already-unknown/removed uid (grep `run_engine`/`ProcessOutcome` usage for the exact idiom during implementation — same file, lines past 150, not re-read here to avoid duplicate range).

**Thread-handoff pattern for actor-on-its-own-thread vs. tokio task** (lines 75-134, `main()`):
```rust
let (tx, rx) = tokio::sync::oneshot::channel();
std::thread::Builder::new()
    .name("mock-engine".to_string())
    .spawn(move || {
        let code = run_engine(&transport, prefill_delay, decode_delay);
        let _ = tx.send(code);
    })
    .expect("spawn mock-engine thread");
let code = rx.await.unwrap_or(EXIT_STARTUP);
```
The FSM itself should be a plain `tokio::task::spawn`'d async task (not an OS thread — it has no blocking ZMQ calls directly, those live in tx-zmq/rx-zmq threads per CLAUDE.md), but this snippet is the project's canonical "hand a result back across a thread/task boundary via a oneshot" idiom if the FSM needs to report a terminal exit code.

---

### `crates/rsg-server/src/fsm/state.rs` (new — `ReqState`, lifecycle enum)

**Analog:** `crates/rsg-server/src/handshake.rs`'s `Handshake` struct — plain data struct with a small set of helper methods (`eos_display`, `to_json_line`), `Debug, Clone, PartialEq, Eq` derives, doc comments stating the exact schema/invariant.

**Plain-data-with-helpers pattern** (lines 20-50):
```rust
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Handshake {
    pub handshake_version: u32,
    pub upstream_sha: String,
    pub max_seq_len: u64,
    // ...
}

impl Handshake {
    pub fn eos_display(&self) -> String { /* ... */ }
}
```
Model `ReqState`/the lifecycle enum the same way: a `#[derive(Debug, Clone, PartialEq, Eq)]` enum `LifecycleState { Queued, Prefill, Decode, Finished, Cancelled, Failed }` plus a `ReqState` struct holding `uid`, current state, TTFT `Instant` (recorded at `Prefill -> Decode`, per RESEARCH.md Pattern G), and the abort-timing mode. Put invariants in doc comments the way `handshake.rs` documents "every key is required."

**Error-enum-with-Display pattern** (lines 52-76):
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandshakeError {
    Malformed(String),
    UnsupportedVersion { got: u32, expected: u32 },
    ShaMismatch { got: String, expected: String },
}
impl fmt::Display for HandshakeError { /* match + write! per variant */ }
impl std::error::Error for HandshakeError {}
```
Use the same shape for any FSM-internal error type (e.g. invalid state transition attempted).

---

### `crates/rsg-server/src/http/abort_guard.rs` (new — Drop-based cancellation)

**Analog:** `crates/rsg-server/src/transport.rs`'s split-socket halves (`ZmqBackendTx`, `ZmqDetokRx`) — small `Send` structs that own exactly one resource and expose a narrow trait.

**Resource-owning-struct-with-narrow-trait pattern** (lines 113-125):
```rust
/// The backend-sending half of a split [`ZmqTransport`]. Owns the PUSH socket.
pub struct ZmqBackendTx {
    _ctx: zmq::Context,
    backend: zmq::Socket,
}

impl BackendSink for ZmqBackendTx {
    fn send_backend(&self, frame: &[u8]) -> anyhow::Result<()> {
        self.backend.send(frame, 0).context("send on backend socket")
    }
}
```
`AbortGuard` should follow the same minimalism: a struct owning `uid`, a `CancellationToken`, an `mpsc::Sender<FsmEvent>`, and a `finished: bool` flag (per RESEARCH.md's Pattern A sketch), with the `Drop` impl as the only behavior beyond construction — no extra methods beyond what's needed, matching this codebase's preference for small single-purpose wrapper types.

---

### `crates/rsg-server/src/http/routes/generate.rs`, `chat.rs`, `models.rs`, `health.rs` (new — no in-repo analog)

No existing HTTP/axum code exists anywhere in the workspace (confirmed: `grep -r axum crates/` returns nothing; RESEARCH.md's own Wave-0 Gaps list states this explicitly). These four files have **no codebase analog** — build them directly from RESEARCH.md's verbatim-quoted upstream patterns:

- **`generate.rs`**: RESEARCH.md "Pattern B" (single-`\n` framing, `Body::from_stream`, not `axum::response::Sse`) and "Code Examples > SSE framing for `/generate`".
- **`chat.rs`**: RESEARCH.md "Pattern C" (streaming, real SSE via `axum::response::sse::{Sse, Event}`, `"object": "text_completion.chunk"` literal, `id: f"cmpl-{uid}"`) and "Pattern D" (non-streaming, hardcoded `usage: {0,0,0}`, `finish_reason: "stop"`, `id: f"chatcmpl-{uid}"`) and "Pattern F" (request-struct defaults).
- **`models.rs`**: RESEARCH.md "Pattern E" (`ModelCard`/`ModelList`, `/v1` multi-method route via `any()`/chained methods).
- **`health.rs`**: no upstream precedent (API-02 free design space) — `/health` = always-200 once listener is up; `/health/ready` = 200 only after handshake parsed, gated on the same `Handshake` struct `handshake.rs` already defines (reuse `Handshake` directly, don't redefine readiness state).

For serde request-struct field-default conventions (Pattern F), mirror `handshake.rs`'s `#[serde(deny_unknown_fields)]` / `#[serde(deserialize_with = "Option::deserialize")]` precision — but note upstream's Pydantic models are **not** `deny_unknown_fields` (extra JSON fields are typically ignored by Pydantic unless configured otherwise); do not copy `deny_unknown_fields` onto the OpenAI-compatible request structs unless RESEARCH.md's Pattern F confirms upstream rejects extra fields (it doesn't address this — treat as permissive/default serde behavior, i.e. omit `deny_unknown_fields` on these particular structs, unlike `Handshake`).

---

### `scripts/gen_api_fixtures.py` (new — API golden fixtures, D-02)

**Analog:** `scripts/gen_wire_fixtures.py`

**CLI/exit-code/docstring pattern** (lines 1-15):
```python
#!/usr/bin/env python3
"""Generate the golden msgpack wire fixtures from upstream's own encoder (D-13, D-14, D-16).
...
Usage:
  scripts/gen_wire_fixtures.py            write fixtures/wire/*.msgpack and manifest.json
  scripts/gen_wire_fixtures.py --out DIR  write them to DIR instead
  scripts/gen_wire_fixtures.py --check    regenerate into a temp dir and byte-diff with fixtures/wire

Exit codes: 0 ok, 1 fixtures differ (--check), 2 environment error.
"""
```
Copy this header shape: `gen_api_fixtures.py` should document its own exit codes (0 ok / 1 fixtures differ / 2 env error), support `--out DIR` and `--check`, and state which Rust file (`crates/rsg-server/tests/...`) must stay in step with the case table — matching `gen_wire_fixtures.py`'s closing line `"The case table must stay in step with crates/rsg-wire/tests/common/mod.rs."`.

**Vendored-import-only pattern** (lines 49-56):
```python
def _load_upstream():
    """Import upstream's classes from the vendored tree, and nowhere else."""
    sys.path.insert(0, str(VENDOR_PY))
    try:
        import msgpack
        import numpy
        import torch
        import minisgl
        from minisgl.core import SamplingParams
```
`gen_api_fixtures.py` must likewise import `api_server.py`'s actual route handlers/response models from `vendor/mini-sglang/python/minisgl/server/api_server.py` (or spawn the real Python frontend process against `mock-scheduler`, per D-02: "golden fixtures captured from a **live** Python-frontend run"), never hand-transcribe the response shape.

**`--check` byte-diff pattern** (function `check(committed: Path)`, line 267 header seen; read `CHECKED_MANIFEST_KEYS` at line 44):
```python
CHECKED_MANIFEST_KEYS = ("upstream_sha", "interpretation", "type_tags", "cases")
```
`gen_api_fixtures.py` needs its own `CHECKED_MANIFEST_KEYS`-equivalent **plus** an explicit per-endpoint normalized/excluded-JSON-path list (RESEARCH.md Pitfall 2: `created` fields in `/v1/models` and non-streaming chat-completions are live timestamps) — extend this pattern with a `NORMALIZED_FIELDS` dict keyed by endpoint, not a blanket exclusion, so new nondeterministic fields must be added deliberately (same spirit as `CHECKED_MANIFEST_KEYS` being an explicit tuple, not "diff everything").

---

### `crates/rsg-server/tests/http_cancellation.rs`, `http_errors.rs`, `abort_timing.rs`, `stress_128.rs` (new test files)

**Analog:** `crates/rsg-server/tests/mock_scheduler_process.rs` + `crates/rsg-server/tests/common/mod.rs`

**Subprocess-harness reuse pattern** (`tests/common/mod.rs` lines 19-125):
```rust
pub struct MockScheduler {
    pub backend_addr: String,
    pub detok_addr: String,
    child: Child,
    stdin: Option<ChildStdin>,
    stdout_lines: Arc<Mutex<Vec<String>>>,
    stderr_lines: Arc<Mutex<Vec<String>>>,
}

impl MockScheduler {
    pub fn spawn(extra_args: &[&str]) -> MockScheduler { /* unique ipc addrs via AtomicUsize COUNTER */ }
    pub fn wait_ready(&mut self) -> Handshake { /* poll stdout_lines with 20s deadline */ }
    pub fn frontend(&self) -> ZmqTransport { /* opens the Connect/Bind mirror of mock's roles */ }
    pub fn wait_for_log(&mut self, needle: &str) -> String { /* poll stderr_lines with 20s deadline */ }
}
```
All four new integration test files should spawn `MockScheduler` exactly this way (same `mod common;` import, same 20s-deadline polling idiom for anything asynchronous) and additionally spawn the new `rsg-server` HTTP binary as a second subprocess, following `tests/cli.rs`'s own `Server`-harness convention (not re-read here — same project, same subprocess-harness family; reuse its `Child`/`Stdio::piped()`/line-polling idiom rather than inventing a new one). `stress_128.rs` additionally needs the "randomized abort-after-N-tokens logic living in the test driver itself" per CONTEXT.md D-04 — implement this as plain Rust `rand`-seeded logic in the test file, not as a new `mock-scheduler` flag.

**Unique-address-per-test pattern** (lines 17, 34-37):
```rust
static COUNTER: AtomicUsize = AtomicUsize::new(0);
let n = COUNTER.fetch_add(1, Ordering::SeqCst);
let pid = std::process::id();
let backend_addr = format!("ipc:///tmp/rsgm-{pid}-{n}-0");
```
Reuse verbatim for any new ipc or TCP port allocation the HTTP tests need (e.g. a unique `127.0.0.1:0`-bound HTTP listener port per test, to run tests in parallel without collisions).

---

### `crates/rsg-server/tests/observability.rs` (new — `/health`, `/health/ready`, `/metrics`)

**Analog:** `crates/rsg-server/tests/cli.rs` (process-level CLI/exit-code assertions) — closest available role-match since there's no existing HTTP-response-assertion test to copy from. Use `reqwest` (already in RESEARCH.md's Standard Stack via axum's transitive deps, or add directly) against the spawned HTTP server, following the same subprocess-spawn-then-poll idiom as `tests/common/mod.rs`.

---

## Shared Patterns

### CLI argument conventions
**Source:** `crates/rsg-server/src/main.rs` lines 24-45, `crates/rsg-server/src/bin/mock-scheduler.rs` lines 36-61
**Apply to:** All new/modified CLI surfaces (`--abort-timing`, any new HTTP-bind-address or timeout flags)
```rust
#[arg(long, value_name = "ADDR")]
backend_addr: String,
#[arg(long, value_enum)]
backend_role: Role,
#[arg(long, default_value_t = 0)]
prefill_delay_ms: u64,
```

### Tracing initialization
**Source:** `crates/rsg-server/src/main.rs` lines 96-102 (identical in `mock-scheduler.rs` lines 78-84)
**Apply to:** Any new binary entry point (the HTTP server binary, if it is a separate `bin/` target rather than `main.rs` itself extended)
```rust
tracing_subscriber::fmt()
    .with_writer(std::io::stderr)
    .with_ansi(false)
    .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
    .init();
```

### Exit-code allocation
**Source:** `crates/rsg-server/src/main.rs` lines 14-21, extended in `mock-scheduler.rs` line 26
**Apply to:** Any new distinguishable failure mode in the HTTP server (bind failure, FSM panic, etc.) — allocate the next free integer, document it with a one-line comment above the const, same as existing code.

### Plain-data struct + helper methods, not builder patterns
**Source:** `crates/rsg-server/src/handshake.rs` lines 20-50
**Apply to:** `ReqState`, lifecycle enum, any new wire-adjacent or config struct — `#[derive(Debug, Clone, PartialEq, Eq, ...)]`, doc comments stating the exact invariant/schema, small `impl` blocks with one-purpose helper methods (`eos_display`, `to_json_line`) rather than a fluent builder.

### Subprocess-harness test infrastructure
**Source:** `crates/rsg-server/tests/common/mod.rs` (full file, 150+ lines read)
**Apply to:** All five new integration test files — reuse `MockScheduler::spawn`, `wait_ready`, `frontend`, `wait_for_log` directly via `mod common;`, and extend the same pattern (unique-address `AtomicUsize` counter, `Arc<Mutex<Vec<String>>>` line buffers, 20s-deadline polling loops) for the new HTTP-server subprocess harness rather than inventing a different subprocess-management style.

### Fixture-generation discipline (golden-file byte parity)
**Source:** `scripts/gen_wire_fixtures.py` (docstring + `CHECKED_MANIFEST_KEYS` + `check()` function)
**Apply to:** `scripts/gen_api_fixtures.py` — explicit `--out`/`--check` CLI surface, explicit exit codes documented in the module docstring, explicit allow-list of what gets diffed (extend with a per-endpoint normalized-fields list for non-deterministic `created` timestamps per Pitfall 2), import real upstream code rather than hand-transcribing response shapes.

## No Analog Found

| File | Role | Data Flow | Reason |
|------|------|-----------|--------|
| `crates/rsg-server/src/http/routes/generate.rs` | controller | streaming | No axum/HTTP code exists anywhere in the workspace yet; build directly from RESEARCH.md's verbatim-quoted upstream `api_server.py` Pattern B and axum's own `Body::from_stream` API (not independently verified against docs.rs this session — flagged `[ASSUMED]` in RESEARCH.md) |
| `crates/rsg-server/src/http/routes/chat.rs` | controller | streaming + request-response | Same — RESEARCH.md Patterns C/D/F are the only available reference |
| `crates/rsg-server/src/http/routes/models.rs` | controller | request-response | Same — RESEARCH.md Pattern E |
| `crates/rsg-server/src/http/routes/health.rs` | controller | request-response | No upstream equivalent at all (API-02 is new, Rust-only design space); only `handshake.rs`'s readiness state is reusable as the `/health/ready` gate |

## Metadata

**Analog search scope:** `crates/rsg-server/src/` (all files), `crates/rsg-server/tests/` (all files), `crates/rsg-wire/src/lib.rs`, `scripts/gen_wire_fixtures.py`, workspace `Cargo.toml`
**Files scanned:** 11 tracked Rust/Python files directly read this session (`main.rs`, `handshake.rs`, `transport.rs`, `rsg-wire/src/lib.rs` head, `bin/mock-scheduler.rs` head, `tests/common/mod.rs` head, `scripts/gen_wire_fixtures.py` head), plus `git ls-files` confirming no other HTTP/axum-related tracked file exists anywhere in the repo
**Pattern extraction date:** 2026-10-06
**Tracked-source gate:** all analog paths above verified via `git ls-files` to be tracked source (not gitignored mirrors); no `.gsd/capabilities/` or other install-mirror paths were used as analogs
