# Stack Research

**Domain:** Rust "front half" of an LLM serving stack (HTTP ingress, request-lifecycle FSM, HF tokenization + detokenization, radix prefix trie). It drives an unmodified Python/CUDA mini-sglang scheduler over ZMQ + MessagePack.
**Researched:** 2026-10-02
**Confidence:** HIGH for the upstream boundary and crate versions. MEDIUM for the ZMQ crate choice and tokenizer/template parity details.

---

## 0. Ground Truth: What Upstream mini-sglang Actually Uses (read from source)

Source: a shallow clone of `sgl-project/mini-sglang` at `main` = commit `9a91cfafe754aa85daee49998176275667eb58f2` (2026-05-17). Every fact in this section comes from reading that code. **Confidence: HIGH.**

| Item | Value | File |
|------|-------|------|
| Python | `requires-python >=3.10`. README and Dockerfile use **3.12**. The Docker base is CUDA **12.8.1** on Ubuntu 24.04 | `pyproject.toml`, `Dockerfile` |
| Key deps | `torch<2.10.0`, `transformers>=4.56.0,<=4.57.3`, `flashinfer-python>=0.5.3`, `sgl_kernel>=0.3.17.post1`, `apache-tvm-ffi`, **`pyzmq`**, **`msgpack`**, `fastapi`, `uvicorn` | `pyproject.toml` |
| Platform | **Linux only** (x86_64/aarch64). macOS is explicitly unsupported | `README.md` |
| IPC transport | **pyzmq PUSH/PULL** (plus PUB/SUB for TP-rank broadcast) over **`ipc://` Unix-domain sockets** | `utils/mp.py` |
| Wire format | `msgpack.packb(obj_dict, use_bin_type=True)` / `msgpack.unpackb(raw, raw=False)` | `utils/mp.py` |
| Message schema | A dict tagged with `"__type__": "<ClassName>"`. Fields map 1:1 to dataclass fields. A 1-D tensor becomes `{"__type__":"Tensor","buffer":<bin bytes>,"dtype":"torch.int32"}`. Decoding calls `cls(**kwargs)`, so an **extra key raises TypeError** | `message/utils.py` |
| Socket addresses | `ipc:///tmp/minisgl_0{suffix}` = backend (scheduler PULL, **scheduler binds**). `ipc:///tmp/minisgl_1{suffix}` = detokenizer (scheduler PUSH). `_2` = TP broadcast. `_3` = frontend. `_4` = tokenizer. The suffix defaults to `.pid=<pid of launcher>` and is a **dataclass field you can override** (`_unique_suffix`) | `scheduler/config.py`, `server/args.py` |
| Process topology | `api_server` (FastAPI) → tokenizer worker(s) → scheduler rank 0 → detokenizer worker → `api_server`. With `--num-tokenizer 0` (the default), one process does both tokenize and detokenize | `server/launch.py`, `tokenizer/server.py` |

**Messages the scheduler consumes** (on `minisgl_0`, encoded by `BaseBackendMsg.encoder`):
- `UserMsg{uid:int, input_ids:Tensor(int32), sampling_params:SamplingParams{temperature:float, top_k:int, top_p:float, ignore_eos:bool, max_tokens:int}}`
- `AbortBackendMsg{uid:int}`
- `BatchBackendMsg{data:[...]}`
- `ExitMsg{}`

**Messages the scheduler produces** (on `minisgl_1`):
- `DetokenizeMsg{uid:int, next_token:int, finished:bool}`
- `BatchTokenizerMsg{data:[DetokenizeMsg...]}`

The scheduler emits **raw token ids, not text**.

### How the Rust process plugs in without modifying upstream

The Rust binary replaces **three** Python processes: `api_server`, the tokenizer worker, and the detokenizer worker. It speaks exactly the wire protocol the tokenizer/detokenizer worker speaks to the scheduler:

1. **Rust PUSH → connect `ipc:///tmp/minisgl_0{suffix}`.** The scheduler binds this as PULL. Send `UserMsg` / `AbortBackendMsg` / `BatchBackendMsg`.
2. **Rust PULL ← `ipc:///tmp/minisgl_1{suffix}`.** Who binds depends on `num_tokenizer`:
   - With `num_tokenizer=0` (the default, `share_tokenizer=True`), `backend_create_detokenizer_link=False`. The **scheduler connects**, so **Rust must bind** the PULL socket.
   - With `num_tokenizer>=1`, the scheduler binds and Rust connects.
   - Pick one setting and pin it.
3. **Launch the scheduler with a tiny Python shim that lives in this repo, not upstream.** The shim:
   - calls `minisgl.server.args.parse_args(...)`;
   - runs `dataclasses.replace(args, _unique_suffix=".rsglang")` so the socket paths are deterministic;
   - spawns `minisgl.scheduler.Scheduler(args)` per TP rank (copying the ~15-line loop in `server/launch.py::_run_scheduler`);
   - prints a ready line on stdout.

   Rust spawns the shim with `tokio::process::Command`, waits for the ready line, then connects. Rust only talks to TP rank 0. Rank 0 rebroadcasts to the other ranks over its own PUB/SUB.
4. Rust owns uid allocation, cancellation (sending `AbortBackendMsg`), and the **incremental detokenization** that the Python `DetokenizeManager` used to do.

**Architectural consequences for the roadmap (important):**
- **The real radix cache lives inside the Python scheduler** (`kvcache/radix_cache.py`, driven by `scheduler/cache.py`). Its values are GPU page indices, and it is page-aligned, LRU-evicted by `timestamp` via `heapq`, and ref-counted. The upstream wire protocol has no "use this prefix" field. **A Rust radix trie cannot replace the scheduler's KV radix cache without patching upstream.** It can still be built as specified ("behaviorally equivalent", proven by differential tests against the Python algorithm), and it is useful for:
  - (a) the **mock backend**, to simulate prefix hits and prefill cost;
  - (b) front-side prefix-hit metrics and admission decisions;
  - (c) future cache-aware routing across multiple backends (this is exactly how sglang's Rust `sgl-model-gateway`/SMG uses a radix tree).

  The roadmap and requirements should state this explicitly.
- **The "lock-free channel" constraint applies inside the Rust process only.** The process boundary is fixed by upstream to ZMQ over `ipc://`. Shared memory, iceoryx2, or raw UDS would all require patching the Python side.
- **Output parity also depends on detokenization**, not just tokenization. The Rust detokenizer must port `tokenizer/detokenize.py`:
  - the `read_offset`/`surr_offset` scheme;
  - the trailing-`"�"` check;
  - the CJK-aware `find_printable_text`;
  - dropping a final EOS token.

---

## Recommended Stack

### Core Technologies

| Technology | Version | Purpose | Why Recommended | Conf. |
|------------|---------|---------|-----------------|-------|
| Rust (stable) | **1.99.0** (2026-09-28), **edition 2024** | Language/toolchain | Current stable. Edition 2024 gives async closures and RPIT capture rules that simplify stream plumbing. Pin it with `rust-toolchain.toml` so benchmarks are reproducible | HIGH |
| `tokio` | **1.53.1** | Async runtime | The de-facto runtime. axum, hyper, reqwest and the ZMQ bridge all assume it. Use `rt-multi-thread`, `macros`, `sync`, `time`, `process`, `signal`, `net` | HIGH |
| `axum` | **0.8.9** | HTTP server / routing | Standard Rust web framework on hyper 1.x. It has a built-in `axum::response::sse::{Sse, Event, KeepAlive}`. The closest prior art, sglang's own Rust gateway (SMG), uses axum 0.8 | HIGH |
| `hyper` | **1.11.1** (transitive via axum) | HTTP/1.1 + h2 engine | Comes with axum. When a client disconnects, hyper **drops the response body stream**. That drop is the cancellation signal (see Pattern A) | HIGH |
| `tower-http` | **0.7.1** | Middleware | `trace` and `request-id` layers. Do **not** put a `timeout` layer on streaming routes | HIGH |
| `tokenizers` (HF) | **=0.22.2** (pin; latest is 0.23.2, and 1.0.0-rc.2 exists) | String → token ids, token ids → string | This is the same Rust core that Python `transformers` "fast" tokenizers wrap. **Pin to 0.22.2** because upstream pins `transformers<=4.57.3`, which pins `tokenizers>=0.22.0,<=0.23.0`, and PyPI has no 0.23.0 final, so the Python side resolves to **0.22.2**. Matching the exact core version removes one whole class of parity bugs. Use `default-features = false, features = ["onig", "esaxx_fast"]` and leave out `http` (see hf-hub) | HIGH (version) / MEDIUM (parity) |
| `minijinja` + `minijinja-contrib` | **2.24.0** both | Chat-template rendering (`apply_chat_template` equivalent) | The established Rust Jinja2 engine. `minijinja-contrib`'s `pycompat` feature supplies Python string/dict methods (`.startswith`, `.items()`, `.strip()`…) that HF templates use. SMG's `llm-tokenizer` crate uses exactly `minijinja` (`json`, `builtins`, `loop_controls`, `loader`) + `minijinja-contrib` (`pycompat`) on top of `tokenizers`. Prior art confirms the choice | HIGH (choice) / MEDIUM (parity) |
| `rmp-serde` | **1.3.1** | MessagePack encode/decode with serde | The standard serde msgpack crate. Encode with **`rmp_serde::to_vec_named`** (structs become maps with string keys, which the Python `cls(**kwargs)` decoder needs). Use `serde_bytes` for the tensor `buffer` so it is emitted as msgpack **bin**, not an int array | HIGH |
| `rmpv` | **1.3.1** | Dynamic msgpack `Value` | For golden-fixture tests, debugging dumps, and a fallback decoder for `__type__` dispatch if serde's internally tagged enums fight you | HIGH |
| `zmq` (rust-zmq, libzmq bindings) | **0.10.0** (+ `zmq-sys` 0.12, which vendors libzmq via `zeromq-src`) | Process-boundary transport to the Python scheduler | **Primary choice.** The Python peer is pyzmq, which is libzmq. Using the same C engine gives identical framing, queueing and HWM, reconnect, and ipc-file semantics, so the boundary is never the suspect in a parity or latency bug. Run it on **two dedicated OS threads** (one owns the PUSH socket, one owns the PULL socket; ZMQ sockets are not thread-safe). Bridge them to tokio with `tokio::sync::mpsc` (`blocking_recv`/`blocking_send`). Downside: the last release was 2022. That is acceptable for a thin binding over a frozen C ABI. Verify in the IPC spike | MEDIUM |
| `tokio::sync::mpsc` (in tokio) | — | In-process "lock-free" channels | It is async-aware and has `blocking_*` methods for the ZMQ threads. Internally it is a lock-free block-linked list. Use it for FSM inbox, per-request token streams, and the ZMQ bridge. One channel type everywhere means less cognitive load | HIGH |
| `tracing` + `tracing-subscriber` | **0.1.44** / **0.3.23** | Structured logs and spans | Standard. Use `env-filter` and `fmt`. Add `json` for benchmark runs. Put spans per request (`uid`) and per FSM transition | HIGH |
| `metrics` + `metrics-exporter-prometheus` | **0.24.6** / **0.18.3** | Counters and histograms at `/metrics` | Lightweight facade with a Prometheus exporter, the same pair SMG uses. Record in-server TTFT, inter-token latency, queue depth, live requests, abort counts, and ZMQ send/recv latency | HIGH |

### Supporting Libraries

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `tokio-util` | 0.7.19 | `CancellationToken`, `DropGuard` | Per-request cancellation propagation from the HTTP stream to the FSM, plus graceful shutdown |
| `tokio-stream` / `futures` | 0.1.19 / 0.3.34 | `ReceiverStream`, `Stream` combinators | Turn a per-request `mpsc::Receiver` into the SSE body stream |
| `async-stream` | 0.3.6 | `stream!{}` macro | Optional. Readable generator-style SSE streams. Hand-written `Stream` impls are fine too |
| `serde` / `serde_json` | 1.0.229 / 1.0.151 | OpenAI-compatible request/response JSON | Always |
| `serde_bytes` | 0.11.19 | Emit `Vec<u8>` as msgpack bin | Tensor `buffer` field. **Required**, or Python's `assert isinstance(buffer, bytes)` fails |
| `bytemuck` | 1.25.2 | `&[i32]` → `&[u8]` zero-copy | Building the int32 little-endian tensor buffer for `UserMsg.input_ids` |
| `hf-hub` | 1.0.0 | Resolve and download `tokenizer.json`, `tokenizer_config.json`, `chat_template.json` from the shared HF cache | Use it directly and keep the `tokenizers/http` feature off. That feature drags in `hf-hub 0.4`, so you would have two versions. Point at the same `HF_HOME` as the Python scheduler. (MEDIUM: 1.0 API not exercised) |
| `rustc-hash` (FxHashMap) / `hashbrown` | 2.1.3 / 0.17.1 | Fast maps | FSM request table keyed by `uid`, radix-node children |
| `slab` | 0.4.12 | Arena for radix-trie nodes | Hand-rolled radix trie: nodes in a slab, `usize` parent/child links, no `Rc<RefCell>` |
| `clap` | 4.6.7 (`derive`, `env`) | CLI | Server, mock-backend, and bench binaries |
| `thiserror` / `anyhow` | 2.0.21 / 1.0.104 | Errors | `thiserror` in library crates, `anyhow` in binaries |
| `mimalloc` | 0.1.52 | Global allocator | Switch it on for benchmark builds. Lower allocator overhead on the hot path, and it behaves the same on macOS and Linux. Measure the system allocator vs mimalloc in the RAM scenario. `tikv-jemallocator` 0.7.0 is the alternative on Linux |
| `nix` | 0.31.3 | Process groups and signals | Kill the Python shim **and its TP children** cleanly (`setpgid` + `killpg`) so benchmarks don't leak GPU processes |
| `sysinfo` | 0.39.6 | RSS/PSS sampling | Host-RAM measurement in the harness (scenario 3). On Linux, read `/proc/<pid>/smaps_rollup` for PSS |
| `reqwest` | 0.13.5 (`stream`, `json`, `rustls`) | HTTP client for the load generator | Custom bench harness (see below) |
| `hdrhistogram` | 7.6.0 | Latency histograms | Exact P50/P90/P99/P99.9 for TTFT, ITL and E2E in the harness |
| `rayon` | 1.12.0 | Data-parallel CPU work | Already a dependency of `tokenizers`. Only use it explicitly if you batch-encode. Otherwise keep the tokenizer on a small dedicated thread pool |
| `crossbeam-channel` | 0.5.17 | Sync MPMC | Only if you build a dedicated tokenizer thread pool (sync workers pulling jobs). Not needed for tokio↔tokio paths |

### Development Tools

| Tool | Purpose | Notes |
|------|---------|-------|
| `cargo-nextest` 0.9.146 | Test runner | Faster, process-per-test isolation. Useful because tests bind ipc sockets in `/tmp` |
| `criterion` 0.8.2 | Micro-benchmarks | Tokenizer encode/decode, chat-template render, msgpack encode/decode, radix match/insert/evict, FSM transitions. Use `harness = false`. **Divan** (0.1.21, last release 2025-04) is nicer to use but less maintained. Stick with criterion |
| `proptest` 1.11.0 | Property-based tests | Radix trie invariants (ref-count ≥ 0, `evictable + protected` sizes, page alignment) and random op sequences for differential testing against the Python reference |
| `insta` 1.49.0 | Snapshot tests | Golden token-id and template-render fixtures, plus msgpack hex snapshots of each message type |
| `uv` (Python) | Python env for the shim, fixtures, and baseline | Upstream README recommends `uv venv --python=3.12`. Use the same for reproducibility |
| `pyzmq` 27.2.0 / `msgpack` 1.2.3 (PyPI latest) | Python side of the wire | Installed via upstream's deps. Pin the exact versions in a lockfile on the GPU box |
| `tokio-console` / `console-subscriber` 0.5.0 | Async task introspection | Dev-only, behind a cargo feature. Never in benchmark builds (it adds overhead) |
| `cargo flamegraph` (flamegraph 0.6.14) / `samply` | CPU profiling | Find host overhead in the Rust path. `samply` works on macOS |
| `hyperfine` 1.20.0 | Repeated wall-clock timing | Cold-start scenario: time from process spawn to first `GET /v1/models` 200 |
| `oha` 1.16.0 | Quick HTTP load sanity checks | Smoke tests only. It cannot parse SSE token timing, so it is not the benchmark of record |
| `vllm bench serve` (`--backend openai-chat --endpoint /v1/chat/completions`) and `python -m sglang.benchmark.serving --backend sglang-oai-chat` | Third-party cross-check for scenario 2 (RPS) | Both target any OpenAI-compatible endpoint and report TTFT/ITL/E2E percentiles. Neither can **cancel requests mid-stream**, so scenario 1 needs the custom harness. Note: `sglang.bench_serving` is now a deprecated alias for `sglang.benchmark.serving` |

---

## Installation

```toml
# Cargo.toml (workspace) — key pins
[workspace.package]
edition = "2024"
rust-version = "1.99"

[workspace.dependencies]
tokio            = { version = "1.53", features = ["rt-multi-thread", "macros", "sync", "time", "process", "signal", "net"] }
tokio-util       = "0.7.19"
tokio-stream     = { version = "0.1.19", features = ["sync"] }
futures          = "0.3.34"
axum             = { version = "0.8.9", features = ["macros"] }
tower-http       = { version = "0.7.1", features = ["trace", "request-id"] }
serde            = { version = "1", features = ["derive"] }
serde_json       = "1.0.151"
serde_bytes      = "0.11.19"
rmp-serde        = "1.3.1"
rmpv             = "1.3.1"
zmq              = "0.10.0"                      # libzmq; vendored build via zmq-sys/zeromq-src
tokenizers       = { version = "=0.22.2", default-features = false, features = ["onig", "esaxx_fast"] }
minijinja        = { version = "2.24", features = ["json", "builtins", "loop_controls", "loader"] }
minijinja-contrib = { version = "2.24", features = ["pycompat", "datetime"] }
hf-hub           = { version = "1.0", features = ["tokio"] }
bytemuck         = "1.25"
rustc-hash       = "2.1"
slab             = "0.4.12"
tracing          = "0.1.44"
tracing-subscriber = { version = "0.3.23", features = ["env-filter", "fmt", "json"] }
metrics          = "0.24.6"
metrics-exporter-prometheus = "0.18.3"
clap             = { version = "4.6", features = ["derive", "env"] }
thiserror        = "2.0"
anyhow           = "1.0"
mimalloc         = "0.1.52"
nix              = { version = "0.31", features = ["signal", "process"] }
# bench harness crate
reqwest          = { version = "0.13.5", default-features = false, features = ["stream", "json", "rustls"] }
hdrhistogram     = "7.6"
sysinfo          = "0.39"

# dev
criterion = "0.8.2"
proptest  = "1.11"
insta     = "1.49"
```

```bash
cargo install cargo-nextest flamegraph hyperfine oha
# Python side (GPU box): baseline + backend are the same pinned upstream revision
uv venv --python=3.12 && source .venv/bin/activate
uv pip install -e third_party/mini-sglang   # pinned at 9a91cfa (or later chosen SHA)
# Python side (Mac, fixtures only): message classes import only torch, msgpack, numpy
uv pip install torch numpy msgpack pyzmq "transformers==4.57.3" && uv pip install --no-deps -e third_party/mini-sglang   # verify in Phase 1
```

The `rmp-serde`/`serde_bytes` choice matters because of a load-bearing wire detail: the tensor `buffer` must serialize as msgpack bin, and structs must be maps, not arrays.

```rust
#[derive(Serialize)]
#[serde(tag = "__type__")]            // emits "__type__": "UserMsg" — Python dispatches on this
enum BackendMsg {
    UserMsg { uid: u64, input_ids: TensorWire, sampling_params: SamplingParamsWire },
    AbortBackendMsg { uid: u64 },
    BatchBackendMsg { data: Vec<BackendMsg> },
}
#[derive(Serialize)]
struct TensorWire {
    #[serde(rename = "__type__")] ty: &'static str,       // "Tensor"
    #[serde(with = "serde_bytes")] buffer: Vec<u8>,       // int32 little-endian
    dtype: &'static str,                                  // "torch.int32"
}
// encode with rmp_serde::to_vec_named(&msg) — NOT to_vec (that emits arrays)
```

---

## Alternatives Considered

| Recommended | Alternative | When to Use Alternative |
|-------------|-------------|-------------------------|
| `zmq` 0.10 (libzmq) on dedicated threads | **`zeromq` 0.6.0 (zmq.rs, pure Rust, tokio-native, active: May 2026)** | If the C build of libzmq is a problem, or you want zero OS threads at the boundary. It supports PUSH/PULL over `ipc://` and is "tested against the reference implementation". Its README admits it does not implement all of ZeroMQ, so HWM, linger and reconnect semantics may differ from pyzmq's libzmq. **Hide the transport behind a small `Transport` trait** and spike both in the IPC phase. Switch if zmq.rs passes the interop and latency test |
| `zmq` + own threads | `tmq` 0.5.0 (tokio wrapper over `zmq`) | Hardly ever. It is built on libzmq's edge-triggered `ZMQ_FD` readiness, which is notoriously easy to get wrong, and its last push was 2024-10. Two plain threads are simpler and easier to reason about for latency |
| `tokio::sync::mpsc` | `flume` 0.12 / `crossbeam-channel` 0.5.17 / `kanal` 0.1.1 | `crossbeam-channel` for a sync worker pool, such as a dedicated tokenizer pool. `flume` if you want one channel usable both sync and async without tokio's `blocking_*` API. `kanal` is barely maintained (2025-03), so avoid it |
| Hand-rolled radix trie (slab arena) | `radix_trie` 0.3 | Never for this. It is a generic byte-key trie with no page alignment, no ref-count locking, no LRU-by-timestamp eviction, and no node splitting with value slices. Behavioral equivalence with `RadixPrefixCache` requires porting the algorithm directly (`_tree_walk`, `split_at`, `insert_prefix`, `evict`, `lock_handle`) |
| `minijinja` + `pycompat` | `llm-tokenizer` 1.8.0 (SMG's crate: tokenizers + minijinja + tiktoken + caching) | Use it as a **reference implementation** to read, not a dependency. It pins `tokenizers ^0.22` and `hf-hub ^0.5` and pulls in tiktoken/rayon/dashmap. The project's educational goal is to own the tokenization module |
| Custom Rust load-generator crate | sglang `benchmark.serving`, `vllm bench serve` | As independent cross-checks for scenario 2 (steady-state RPS/TTFT) and to make results credible to outsiders. They can't inject mid-stream cancellations (scenario 1) or measure cold start and RSS (scenario 3) |
| `criterion` | `divan` 0.1.21 | If you want lighter micro-bench ergonomics and accept a less active project |
| `metrics` + Prometheus | OpenTelemetry (`opentelemetry` 0.33, `tracing-opentelemetry` 0.34) | Only if you later need distributed traces across Rust and Python. Not needed for v1, and adds overhead and version churn |
| `mimalloc` | `tikv-jemallocator` 0.7 | Linux-only benchmark builds, if mimalloc shows RSS bloat in the host-RAM scenario |

---

## What NOT to Use

| Avoid | Why | Use Instead |
|-------|-----|-------------|
| Shared memory / iceoryx2 0.10 / raw Unix sockets / gRPC **at the process boundary** | Upstream's scheduler only speaks pyzmq PUSH/PULL + msgpack. Any other transport means patching upstream, which violates the "unmodified upstream" constraint | ZMQ `ipc://` + msgpack, exactly as upstream does |
| `rmp_serde::to_vec` (compact / array encoding) | It encodes structs as msgpack **arrays**. Python does `data["__type__"]` and `cls(**kwargs)` on a dict, so this crashes | `rmp_serde::to_vec_named` |
| Plain `Vec<u8>` for the tensor buffer | It serializes as an array of ints. Python asserts `isinstance(buffer, bytes)` | `#[serde(with = "serde_bytes")]` |
| Extra or renamed fields in wire structs | Python decodes with `cls(**kwargs)`, so an unknown key raises `TypeError` and kills the scheduler loop | Mirror the dataclass fields exactly. Lock them in with msgpack golden fixtures produced by upstream's own `serialize_type` |
| `tokenizers` with the `http` feature | It pulls `hf-hub 0.4` alongside the 1.0 you use directly. It also hides network I/O inside tokenizer construction, which hurts cold-start measurement | `hf-hub` 1.0 to resolve paths, then `Tokenizer::from_file` |
| Latest `tokenizers` 0.23.x / 1.0-rc without checking | The baseline Python resolves `tokenizers` 0.22.2 (because of the transformers 4.57.3 pin). A different core version risks silent id or decode drift | `=0.22.2`. Bump only together with upstream's transformers pin |
| `Rc<RefCell<Node>>` trees | Slow, leak-prone, not `Send` | Slab/arena indices |
| `axum::response::Sse` for the `/generate` endpoint | Upstream's `/generate` frames chunks as `data: <text>\n` (a **single** newline, not spec SSE). axum's `Sse` always emits `\n\n` | Raw `Body::from_stream` for `/generate`. `Sse` (or byte-identical manual framing) for `/v1/chat/completions` |
| `tower_http::timeout` on streaming routes | It kills long generations | Per-request deadlines inside the FSM, if needed at all |
| `tokio::task::spawn_blocking` as the main tokenizer path under load | The blocking pool is unbounded and shared, which makes tail latency unpredictable with 128 concurrent agents | A fixed N-thread tokenizer pool (crossbeam-channel MPMC), or inline encoding for short prompts (32-token prompts take microseconds). Decide by criterion measurement |
| `tokio-console` / OTel exporters in benchmark builds | Measurable per-task and per-span overhead skews P99 | Feature-gate them off for bench runs |
| `oha`, `wrk`, `vegeta` as benchmark of record | They don't parse the SSE token stream, so they can't measure TTFT or ITL | The custom harness. Cross-check with vllm/sglang bench tools |

---

## Stack Patterns by Variant

**Pattern A: SSE streaming + client-disconnect cancellation (axum/hyper)**
- The handler allocates a `uid`, registers `(uid, mpsc::Sender<Chunk>)` with the FSM actor, and returns a body stream built from the `Receiver`.
- Wrap the stream in a struct holding an **`AbortGuard`** whose `Drop` sends `Cancel(uid)` to the FSM unless the request already finished. Hyper drops the body when the client disconnects, so `Drop` is the cancellation hook. No polling of `is_disconnected()` is needed (upstream polls).
- Caveat: the drop happens only when hyper notices the dead connection, usually on the next write. A request still waiting in a long prefill may not be noticed until its first token. Send an SSE comment keep-alive (`KeepAlive::new().interval(…)`) if fast abort of queued requests matters for scenario 1. (MEDIUM: verify with a disconnect test.)
- The FSM turns `Cancel(uid)` into `AbortBackendMsg{uid}` on the ZMQ PUSH. It must also **ignore late `DetokenizeMsg`s for aborted uids**, exactly as upstream's `listen()` skips unknown uids.

**Pattern B: The ZMQ bridge**
- Thread **tx-zmq**: `loop { let batch = rx.blocking_recv(); coalesce any already-queued msgs into BatchBackendMsg; push.send(rmp_serde::to_vec_named(&batch)) }`.
- Thread **rx-zmq**: `loop { let raw = pull.recv_bytes(0); decode DetokenizeMsg | BatchTokenizerMsg; fsm_tx.blocking_send(Tokens(vec)) }`.
- Coalescing into `BatchBackendMsg` mirrors upstream behavior and reduces scheduler-side `recv` calls. That matters for the RPS scenario.

**Pattern C: The FSM as a single-owner actor**
- One tokio task owns `FxHashMap<u64, ReqState>` and handles `New`, `Tokens`, `Cancel`, `Finished`. No `DashMap` or locks on the hot path.
- States: `Queued → Prefill → Decode → {Finished | Cancelled}`. `Prefill→Decode` is inferred from the first `DetokenizeMsg`; TTFT is recorded there.
- The detokenizer state (`DecodeStatus`: `decoded_ids`, `read_offset`, `surr_offset`, `sent_offset`) lives in `ReqState`.

**Pattern D: Tokenization parity**
- Port these transformers 4.57.3 `apply_chat_template` behaviors:
  - `ImmutableSandboxedEnvironment(trim_blocks=True, lstrip_blocks=True)` with `loopcontrols`;
  - a custom `tojson` (`ensure_ascii=False`, `indent` support);
  - `raise_exception`, `strftime_now`;
  - render variables `messages`, `add_generation_prompt=True`, and `bos_token`/`eos_token`/`pad_token`/`unk_token` from `tokenizer_config.json`.

  In minijinja: `env.set_trim_blocks(true)`, `env.set_lstrip_blocks(true)`, `env.set_unknown_method_callback(minijinja_contrib::pycompat::unknown_method_callback)`, and register `raise_exception` and `strftime_now`.
- Upstream then calls `tokenizer.encode(prompt)` with the default `add_special_tokens=True` on the **rendered** prompt. Replicate this exactly with `encode(prompt, true)`, including any double-BOS quirk for Llama-style templates. Parity means "same as baseline", not "correct".
- Load `chat_template` from `tokenizer_config.json`, falling back to `chat_template.json`, as upstream's `load_tokenizer` does.
- Detokenize with `Tokenizer::decode(ids, skip_special_tokens=false)`. That is the `batch_decode` default upstream uses. Also apply transformers' **Python-side** `clean_up_tokenization_spaces` if `tokenizer_config.json` sets it; this step is not in the Rust core. Do **not** use `tokenizers::DecodeStream` for parity: its chunking algorithm differs from upstream's offset scheme. It is fine for a non-parity fast path.
- Golden fixtures: generate `(input → ids)` and `(ids stream → incremental strings)` with Python on the GPU box, or on the Mac with `transformers==4.57.3`. Check them in, and run `insta` snapshots on Mac CI.

**Pattern E: Mock backend (Mac, no GPU)**
- A Rust binary, `mock-scheduler`, binds and connects the *same* ipc addresses and speaks the *same* msgpack. It emits deterministic tokens with configurable prefill and decode delays, and uses the Rust radix trie to model prefix hits.
- Also run a ~50-line **Python mock scheduler** that imports upstream's real `minisgl.message` classes. This is a cross-language wire conformance test on the Mac before touching a GPU.

**If the GPU box runs TP>1:**
- Change nothing in Rust. Rank 0 rebroadcasts over its own `minisgl_2` PUB/SUB. The shim must replicate the multi-rank spawn loop and only wait for rank 0's ready signal.

---

## Benchmark Harness (stack for the three RFC scenarios)

| Scenario | Tool | Measures |
|----------|------|----------|
| 1. 128 concurrent agents, dynamic requests and cancellations, P99 TTFT | **Custom Rust harness**. reqwest streaming plus a hand-written `data:` line parser (`eventsource-stream` is unmaintained since 2022 and trivial to replace). A seeded RNG decides think time and abort-after-N-tokens per agent. hdrhistogram collects TTFT, ITL and E2E, and the harness records the client-observed abort time | P50/P90/P99 TTFT and abort-to-quiescence. Same binary, same seed against both servers |
| 2. 32-token short-prompt saturation, RPS | Custom harness (closed-loop at concurrency C and open-loop Poisson), **cross-checked with** `vllm bench serve --backend openai-chat` and `python -m sglang.benchmark.serving --backend sglang-oai-chat` | Max sustainable RPS at a TTFT SLO. Third-party numbers make the claim credible |
| 3. Cold start + host RAM | `hyperfine` + harness. Spawn the process, poll `GET /v1/models`, and measure (a) front-half ready and (b) end-to-end ready, including scheduler weight load. Sample PSS across the **whole process tree** (Python baseline = api_server + tokenizer/detokenizer + scheduler; Rust = rust-front + scheduler) | Cold-start ms and host RAM (PSS). Report the front-half-only delta separately, because weight loading dominates end-to-end time |
| Parity (standard inference) | Upstream's own `benchmark/online/bench_simple.py` (uses `AsyncOpenAI` against `:1919/v1`) run unchanged against both servers, plus a greedy-decoding (`temperature=0`) output-diff script | ±2% throughput reference and byte-identical greedy output |

The Rust server should listen on the same default port (**1919**) and expose the same routes (`/generate`, `/v1`, `/v1/chat/completions`, `/v1/models`) so upstream's client and benchmark scripts run unmodified against it.

---

## Version Compatibility

| Package A | Compatible With | Notes |
|-----------|-----------------|-------|
| `tokenizers =0.22.2` (Rust) | Python `tokenizers 0.22.2` resolved by `transformers<=4.57.3` (upstream pin) | Bump both together. PyPI has no `0.23.0` final, so `<=0.23.0` resolves to 0.22.2 |
| `tokenizers 0.22.2` `http` feature | `hf-hub ^0.4.1` | Reason to keep `http` off and use `hf-hub 1.0` directly |
| `axum 0.8.9` | `hyper 1.x`, `tower 0.5.3`, `tower-http 0.7.1` | SMG still uses tower-http 0.6. Both work with axum 0.8 |
| `reqwest 0.13.5` | `hyper 1.x` | The 0.13 TLS feature names changed vs 0.12 (use `rustls`). Check feature names when copying older snippets |
| `zmq 0.10.0` → `zmq-sys 0.12` | builds libzmq from source (`zeromq-src`) with `cc` | Works on macOS arm64 and Linux. Needs a C/C++ toolchain. Interop target: pyzmq 27.x (bundles libzmq 4.3.x) |
| `zeromq 0.6.0` | tokio 1.x (`tokio-runtime` feature), MSRV 1.85 | Alternative transport. `ipc-transport` is Unix-only |
| `minijinja 2.24` | `minijinja-contrib 2.24` | Keep the versions in lockstep |
| `rmp-serde 1.3.1` | `rmp 0.8.15`, `rmpv 1.3.1` | Internally tagged enums (`#[serde(tag="__type__")]`) decode through serde's buffered `Content`. Verify `BatchTokenizerMsg{data:[DetokenizeMsg…]}` round-trips. Fall back to `rmpv::Value` dispatch if not |
| upstream mini-sglang `9a91cfa` | Python 3.12, CUDA 12.8, torch <2.10, Linux only | Pin the SHA as a git submodule. Re-verify the message schema (§0) on any bump |

---

## Sources

- **Primary source, read directly (HIGH):** `github.com/sgl-project/mini-sglang` @ `9a91cfa` (2026-05-17). Files: `pyproject.toml`, `Dockerfile`, `README.md`, `docs/structures.md`, `python/minisgl/utils/mp.py`, `message/{backend,frontend,tokenizer,utils}.py`, `server/{launch,args,api_server}.py`, `scheduler/{config,io,scheduler,cache}.py`, `tokenizer/{server,tokenize,detokenize}.py`, `kvcache/radix_cache.py`, `utils/hf.py`, `benchmark/online/bench_simple.py`
- **crates.io registry API (HIGH)**, queried 2026-10-02, for every Rust crate version above. Feature flags were read from the version metadata of `tokenizers 0.23.2`, `zeromq 0.6.0`, `minijinja-contrib 2.24.0` and `zmq 0.10.0`. Dependency lists came from `tokenizers 0.22.2` and `llm-tokenizer 1.3.2`
- **static.rust-lang.org stable channel manifest (HIGH):** rustc 1.99.0, 2026-09-28
- **PyPI JSON API (HIGH):** `tokenizers` releases (no 0.23.0 final), `transformers` latest 5.18.0, `pyzmq` 27.2.0, `msgpack` 1.2.3
- **`huggingface/transformers` v4.57.3 `setup.py` (HIGH):** `tokenizers>=0.22.0,<=0.23.0`, `jinja2>=3.1.0`
- **`sgl-project/sglang` `sgl-model-gateway/Cargo.toml` (HIGH, prior art):** axum 0.8, tokio, tower-http, metrics + metrics-exporter-prometheus, tracing, criterion, and its `llm-tokenizer` crate (tokenizers + minijinja + minijinja-contrib/pycompat)
- **`sgl-project/sglang` `python/sglang/benchmark/serving.py` (HIGH):** backends `sglang-oai` and `sglang-oai-chat`; `bench_serving` is a deprecated alias
- **docs.rs `tokenizers::DecodeStream` (MEDIUM):** `step(id) -> Result<Option<String>>`, returns `None` on incomplete UTF-8
- **docs.vllm.ai `vllm bench serve` (MEDIUM):** `openai-chat` backend, `--max-concurrency`, `--percentile-metrics`. No cancellation feature documented
- **github.com/zeromq/zmq.rs README (MEDIUM):** socket types, tcp + ipc transports, "does not implement all of ZeroMQ's feature set"
- **Web search (LOW–MEDIUM, not authoritative):** minijinja pycompat and HF chat-template feature list; axum SSE disconnect behavior (the drop-based cancellation is from general hyper semantics and must be verified by test)

---
*Stack research for: Rust front half (ingress / FSM / tokenizer / radix) over unmodified mini-sglang*
*Researched: 2026-10-02*
