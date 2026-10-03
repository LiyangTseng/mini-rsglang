# Architecture Research

**Domain:** Rust front half (ingress, request-lifecycle FSM, HF tokenization, radix prefix cache) for an LLM serving stack, driving an unmodified Python/CUDA mini-sglang scheduler over ZMQ + MessagePack
**Researched:** 2026-10-02
**Confidence:** HIGH for mini-sglang internals. I read the upstream source directly at `sgl-project/mini-sglang@9a91cfa` (2026-05-17). MEDIUM for comparisons with other systems: the web sources rate LOW under the classify-confidence seam, but they agree with prior knowledge of these systems. Some claims are inferred from reading code and have not been run; they are marked **(unverified)**.

---

## 0. What mini-sglang actually is (ground truth from source)

`python -m minisgl` runs `server/launch.py::launch_server`, which starts **3 + N process types**. They talk over **ZMQ PUSH/PULL sockets on `ipc:///tmp/minisgl_*` Unix-domain endpoints**. Every message is **`msgpack.packb(dict, use_bin_type=True)`**, and each dict carries a `"__type__"` tag (`message/utils.py`).

```
                 HTTP (FastAPI/uvicorn)
                        │
┌───────────────────────▼────────────────────────┐
│ API server process (main)  server/api_server.py│  FrontendManager: uid counter, uid→ack list,
│                                                │  asyncio.Event per uid, SSE generators, abort
└───────┬───────────────────────────────▲────────┘
  TokenizeMsg / AbortMsg          UserReply (incremental text)
  PUSH → ipc minisgl_4|_1         PULL ← ipc minisgl_3 (API binds)
┌───────▼───────────────────────────────┴────────┐
│ Tokenizer/Detokenizer worker(s) tokenizer/     │  num_tokenizer=0 (default): ONE process does
│   TokenizeManager  (apply_chat_template+encode)│  both tokenize and detokenize, single-threaded
│   DetokenizeManager (incremental decode state) │
└───────┬───────────────────────────────▲────────┘
  UserMsg / AbortBackendMsg        DetokenizeMsg (uid, next_token, finished)
  PUSH → ipc minisgl_0             PUSH ← from scheduler to ipc minisgl_1
┌───────▼───────────────────────────────┴────────┐
│ Scheduler process(es), one per TP rank          │  owns: PrefillManager (FIFO pending list),
│   scheduler/scheduler.py + Engine (GPU)         │  DecodeManager, TableManager, CacheManager
│   rank0 PUB→ ipc minisgl_2 → rank1..N SUB       │  (RadixPrefixCache + GPU page allocator),
└─────────────────────────────────────────────────┘  overlap scheduling, sampling, its own tokenizer (eos id)
```

### Wire contract at the scheduler boundary (the only boundary Rust must speak)

| Direction | Message `__type__` | Fields (exact) | Notes |
|-----------|--------------------|----------------|-------|
| front → scheduler | `UserMsg` | `uid:int`, `input_ids:Tensor`, `sampling_params:SamplingParams` | Tensor = `{"__type__":"Tensor","buffer":<msgpack bin, int32 LE>,"dtype":"torch.int32"}` |
| front → scheduler | `SamplingParams` (nested) | `temperature:float=0.0`, `top_k:int=-1`, `top_p:float=1.0`, `ignore_eos:bool=False`, `max_tokens:int=1024` | Decoded with `cls(**kwargs)`. **An unknown key raises TypeError and kills the scheduler process.** |
| front → scheduler | `AbortBackendMsg` | `uid:int` | The scheduler frees resources and **sends no reply**. |
| front → scheduler | `ExitMsg` | none | The scheduler raises KeyboardInterrupt and shuts down. |
| front → scheduler | `BatchBackendMsg` | `data:[...]` | Optional coalescing; unwrapped recursively. |
| scheduler → front | `DetokenizeMsg` | `uid:int`, `next_token:int`, `finished:bool` | One per running request per scheduler step. |
| scheduler → front | `BatchTokenizerMsg` | `data:[DetokenizeMsg...]` | Sent when a step produces more than one reply. |

Loading this table into Rust is the single most load-bearing artifact in the project. Example `UserMsg` as a msgpack map:

```text
{"__type__":"UserMsg","uid":7,
 "input_ids":{"__type__":"Tensor","buffer":b"\x01\x00\x00\x00...","dtype":"torch.int32"},
 "sampling_params":{"__type__":"SamplingParams","temperature":0.0,"top_k":-1,
                    "top_p":1.0,"ignore_eos":false,"max_tokens":128}}
```

Rules that follow from `message/utils.py` and `utils/mp.py`:
- Structs must be msgpack **maps keyed by str**. rmp-serde's default `to_vec` writes structs as arrays, which is wrong here. `buffer` must be **bin**, not an array of ints. Floats must stay floats. Because the schema has only 7 types, write the codec by hand on low-level `rmp`/`rmpv` instead of relying on serde's internally-tagged enums.
- Socket topology **when the scheduler is launched standalone with `SchedulerConfig`**. This is exactly what upstream's own `tests/core/test_scheduler.py` does. The scheduler **binds** PULL at `ipc:///tmp/minisgl_0{suffix}` and **binds** PUSH at `ipc:///tmp/minisgl_1{suffix}`, and the front **connects** to both. With `ServerArgs` and `num_tokenizer=0`, the scheduler *connects* to `minisgl_1` instead, so the front must bind. Make bind/connect configurable per endpoint.
- `suffix` defaults to `.pid=<pid>`. It is the `_unique_suffix` dataclass field and can be set to a fixed value with `dataclasses.replace`, so the Rust side can know the paths without changing upstream.

### Upstream scheduler behaviour the Rust side must account for (from `scheduler/*.py`)

1. **FIFO admission with head-of-line blocking.** `PrefillManager.schedule_next_batch` walks `pending_list` in arrival order and `break`s at the first request it can't fit. **The order of messages from Rust is the scheduler's admission order.**
2. **Overlong prompts are dropped silently.** If `len(input_ids) >= engine.max_seq_len`, the request is logged and dropped and **no reply is ever sent**. `engine.max_seq_len = min(config.max_seq_len, kv_capacity_tokens)` is only known after the engine is up. Unless Rust validates this first, such a request hangs forever.
3. **`max_tokens` is clamped server-side** to `max_seq_len - input_len` without telling the front.
4. **Abort gives no ack, and late tokens are possible.** With overlap scheduling, a `DetokenizeMsg` for an aborted uid can still arrive one step later. Upstream's Python front drops replies for unknown uids, and Rust must do the same.
5. **EOS is the single `tokenizer.eos_token_id`**, checked in the scheduler. The detokenizer then omits the EOS token from the text when `finished and next_token == eos`. Rust must not add `generation_config` EOS ids, or parity breaks.
6. **Decode batches are sorted by `uid`.** `uid` assignment (Python: monotonic from 0) affects batch order and therefore possibly numerics. Copy that scheme.
7. **The scheduler loads its own tokenizer**, but only for the EOS id. All text↔token work happens outside it.

### Where the radix cache lives

`RadixPrefixCache` (`kvcache/radix_cache.py`) is owned by `CacheManager` **inside the scheduler process**. Its keys are CPU int32 token-id tensors. Its **values are GPU tensors of KV page indices** taken from `page_table[table_idx, :cached_len]`. It is tightly coupled to:
- the GPU page free-list (`CacheManager.free_slots`). `_allocate()` calls `prefix_cache.evict()` when pages run short.
- request lifetimes. `lock_handle`/`unlock` ref-counts are tied to `Req.cache_handle` across prefill, chunked prefill, finish and abort.
- admission. `PrefillAdder._try_allocate_one` calls `match_prefix(input_ids[:len-1])` and copies the matched page indices into the request's page table.

Behaviour details any equivalence target must reproduce:
- page-aligned matching (`align_down` to `page_size`)
- `match_prefix` **does mutate** the tree, despite what its docstring says: it splits nodes and refreshes timestamps
- LRU-by-`time.monotonic_ns()` leaf eviction through `heapq`, over leaves collected by DFS
- `evict` may free more than requested
- a child key is `first token` when `page_size == 1`, otherwise a tuple of the first page

**Conclusion (HIGH):** the radix cache that actually serves traffic cannot move to the Rust front half without modifying upstream. The `UserMsg` wire schema has no field for prefix length, block table or slots. Compare TGI v3, whose Rust router owns a `RadixAllocator` (trie, free blocks and ref-counts) and sends slots to the Python shards. That only works because TGI's protocol carries the block table. See section 4 for what "Rust radix cache" can honestly mean here.

---

## 1. Standard Architecture (recommended)

### System Overview

```
┌──────────────────────────────── mini-rsglang (one Rust process, tokio) ─────────────────────────────┐
│                                                                                                     │
│  INGRESS (axum)                        LIFECYCLE                          BACKEND CLIENT (actor)    │
│  ┌──────────────────┐   submit()   ┌─────────────────────┐  Cmd mpsc  ┌──────────────────────────┐ │
│  │ /generate        │─────────────▶│ RequestHandle (FSM)  │───────────▶│ owns ZMQ PUSH + PULL     │ │
│  │ /v1/chat/compl.  │              │ per-request task:    │            │ owns uid→Sender routing  │ │
│  │ /v1/models       │◀─SSE stream──│  tokenize→admit→     │◀──Event────│ encodes UserMsg/Abort    │ │
│  │ (same API/SSE    │  (Drop=cancel)│  dispatch→stream→    │  mpsc per  │ decodes DetokenizeMsg,   │ │
│  │  framing as      │              │  detokenize→finish   │  request   │ fans out by uid,         │ │
│  │  upstream)       │              └──────┬──────▲────────┘            │ drops unknown uids       │ │
│  └──────────────────┘                     │      │                     └───────┬──────────▲───────┘ │
│                         ┌─────────────────▼──┐ ┌─┴──────────────────┐          │          │         │
│                         │ TOKENIZER service  │ │ ADMISSION queue    │          │          │         │
│                         │ HF `tokenizers` +  │ │ (pass-through by   │          │          │         │
│                         │ chat template;     │ │ default; optional  │          │          │         │
│                         │ blocking pool      │ │ prefix-aware order │          │          │         │
│                         │ IncrementalDecoder │ │ via SHADOW RADIX)  │          │          │         │
│                         └────────────────────┘ └────────────────────┘          │          │         │
└────────────────────────────────────────────────────────────────────────────────┼──────────┼─────────┘
                       msgpack over ZMQ ipc://  (PUSH UserMsg/Abort/Exit)        │          │ (PULL DetokenizeMsg)
┌────────────────────────────────────────────────────────────────────────────────▼──────────┴─────────┐
│ EITHER: unmodified upstream Scheduler (GPU box), started by our Python launcher shim               │
│ OR:     rsg-mock-scheduler (Mac), same sockets, same messages, emulated scheduler semantics         │
└─────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

Effect on topology: Rust replaces **the API server process and the tokenizer/detokenizer process(es)**. Each request then goes through **2 IPC hops instead of 4**: Rust→scheduler and scheduler→Rust, where upstream does API→tok→sched→detok→API. Upstream also has 2 extra Python processes that each import torch and transformers. This is the real, structural source of host-overhead wins on TTFT, RPS and RAM. The scheduler loop itself is unchanged and stays the throughput ceiling.

### Component Responsibilities

| Component | Responsibility | Typical Implementation |
|-----------|----------------|------------------------|
| **rsg-wire** | Exact msgpack encode/decode of the 7 upstream message types; golden fixtures | Hand-rolled codec on `rmp`/`rmpv`; zero I/O; property tests plus cross-language fixtures |
| **rsg-transport** (BackendClient actor) | Owns both ZMQ sockets. Single ordered writer and single reader. uid→`mpsc::Sender<BackendEvent>` routing table. Coalesces sends into `BatchBackendMsg`. Drops replies for unknown uids and counts them | One tokio task with `select!` over a command channel and socket recv. Async ZMQ (`zeromq` crate, ipc transport) or libzmq bindings on a dedicated thread |
| **rsg-tokenizer** | (a) encode: chat template → `encode(add_special_tokens=true)` → `Vec<u32>`, matching `transformers`. (b) `IncrementalDecoder`: a port of `DetokenizeManager` covering `surr_offset`/`read_offset`, U+FFFD hold-back, `find_printable_text`, CJK rules and EOS omission | HF `tokenizers` crate plus a Jinja engine for chat templates. Encode runs on a blocking pool; decode runs inline per request |
| **rsg-lifecycle** | Request FSM, uid allocation (monotonic from 0), validation (`max_seq_len`, `max_tokens` clamp), cancellation policy, admission queue, metrics (TTFT, ITL) | Plain enum FSM plus one tokio task per request. No shared mutable state except the actor channels |
| **rsg-radix** | Token-id radix trie: match / insert / evict / lock / unlock, page-aligned, LRU. Behaviourally equivalent to upstream `RadixPrefixCache` | Arena-allocated nodes (`Vec<Node>` plus indices), a generic value type, an injectable logical clock |
| **rsg-server** | HTTP/SSE with the upstream API surface and byte framing, OpenAI chat, config, startup handshake, child-process supervision | axum + tokio; `Drop` guard on the SSE body triggers cancel |
| **rsg-mock-scheduler** | GPU-free stand-in that speaks the same wire protocol and emulates the scheduler semantics in §0 | Rust binary: step loop with configurable prefill/decode latency and deterministic token generator |
| **python shim** (`rsglang_shim`) | Starts the **unmodified** upstream `Scheduler` as a library with the same CLI flags as the baseline, a fixed `_unique_suffix`, and a readiness handshake reporting `max_seq_len`, `eos_token_id`, `page_size`, `max_running_req` and `num_pages` | ~100 lines. Uses `minisgl.server.args.parse_args` and the `_run_scheduler` pattern, modelled on upstream `tests/core/test_scheduler.py` |

## 2. Recommended Project Structure

```
mini-rsglang/
├── Cargo.toml                    # workspace
├── crates/
│   ├── rsg-wire/                 # message types + msgpack codec + golden fixture tests
│   ├── rsg-transport/            # BackendClient actor, Backend trait, ZMQ impl, in-proc mock impl
│   ├── rsg-tokenizer/            # encode, chat template, IncrementalDecoder
│   ├── rsg-radix/                # trie; no tokio/zmq deps; differential-test friendly
│   ├── rsg-lifecycle/            # FSM, uid alloc, validation, admission, cancellation
│   ├── rsg-server/               # axum app + main binary (`mini-rsglang`)
│   ├── rsg-mock-scheduler/       # binary speaking the upstream wire protocol
│   └── rsg-bench/                # load generator: 128-agent cancel, 32-tok RPS, cold start
├── python/
│   ├── rsglang_shim/             # launch_scheduler.py: upstream used as a library, never patched
│   └── conformance/
│       ├── wire_fixtures.py      # emit/verify msgpack bytes using upstream minisgl.message
│       ├── radix_diff.py         # replay op traces on upstream RadixPrefixCache (CPU, stubbed kernel/clock)
│       ├── tokenizer_parity.py   # transformers encode/apply_chat_template/batch_decode corpora
│       └── py_front_vs_mock.py   # run upstream API server + tokenizer worker against rsg-mock-scheduler
├── third_party/mini-sglang/      # git submodule pinned (e.g. 9a91cfa) — baseline + backend
└── bench/                        # scenario configs, result schema, reports
```

### Structure Rationale

- **rsg-wire is separate and has no dependencies.** It is the riskiest contract: a schema mistake kills the remote scheduler process. It needs golden tests that can run on the Mac before anything else exists.
- **rsg-radix has no async or I/O dependencies.** It is a pure data structure whose correctness is defined by differential testing against Python. Keeping it isolated lets it be built in parallel and, later, wrapped by PyO3 (§4, option C) without bringing tokio along.
- **The mock scheduler is a binary, not just a trait.** The highest-value tests check behaviour on the real sockets and in real messages: ordering, late tokens, silent drops. A trait mock alone can't exercise these.
- **`python/conformance` runs on macOS.** `minisgl.message` needs only torch (CPU), numpy and msgpack. It does not need flashinfer or sgl-kernel. **(unverified: check the import chain on macOS in phase 1.)**

## 3. Architectural Patterns

### Pattern 1: Single-owner backend actor (ordered writer and router in one task)

**What:** One task owns the PUSH socket, the PULL socket and the `uid → Sender` map. Request tasks send `Cmd::Submit{uid, ids, params, events_tx}` and `Cmd::Abort{uid}` on one MPSC channel.
**Why:** ZMQ sockets are not thread-safe. Doing *insert route, then send UserMsg* in one task guarantees a route exists before any token can arrive. Sending Abort on the same FIFO guarantees it reaches the scheduler after its UserMsg. That covers the "lock-free channel" in PROJECT.md: lock-free intra-process MPSC, with the cross-process link fixed by upstream as ZMQ.
**Trade-offs:** The single task could become a bottleneck. In practice it won't: about 128 requests × ~100 steps/s is roughly 13k small messages/s, and the scheduler already batches replies per step.

```rust
loop {
    tokio::select! {
        Some(cmd) = cmd_rx.recv() => match cmd {
            Cmd::Submit { uid, ids, params, tx } => { routes.insert(uid, tx); pending.push(UserMsg{uid, ids, params}); }
            Cmd::Abort { uid } => { routes.remove(&uid); pending.push(AbortBackendMsg{uid}); }
        },
        Ok(frame) = pull.recv() => for m in wire::decode_replies(&frame)? {
            match routes.get(&m.uid) { Some(tx) => { let _ = tx.try_send(m.into()); if m.finished { routes.remove(&m.uid); } }
                                       None => metrics.late_after_abort += 1 }
        },
    }
    if !pending.is_empty() { push.send(wire::encode_batch(&pending.drain(..))).await?; }
}
```

### Pattern 2: Per-request task owns all per-request state (FSM + IncrementalDecoder)

**What:** Each HTTP request runs one task holding `RequestState` and that request's own `IncrementalDecoder`. Detokenization happens there, in parallel across tokio workers. In upstream, by contrast, one Python process detokenizes everything serially.
**FSM (as observable from the front):**

```
Received ──tokenize ok──▶ Tokenized ──validate ok──▶ Queued(admission) ──Submit sent──▶ Dispatched
   │ err                     │ too long/err              │ cancel                         │ first DetokenizeMsg
   ▼                         ▼                           ▼                                ▼
 Failed(4xx)              Failed(4xx)              Cancelled(no abort needed)        Decoding ──finished──▶ Finished
                                                                                      │ cancel        (stream ends)
                        Dispatched/Decoding ──cancel──▶ Cancelling(Abort sent; route removed; late tokens dropped) ──▶ Cancelled
```

"Prefill" can't be observed from outside the scheduler. Pending and prefilling look the same to the front until the first token arrives, so model `Dispatched` as "queued or prefilling in the backend". TTFT is measured as HTTP arrival → first `DetokenizeMsg`.
**Trade-offs:** One task per request costs ~KBs each. That is negligible at 128 concurrent requests and fine at 10k.

### Pattern 3: RAII cancellation from the HTTP body

**What:** The SSE stream holds a `CancelGuard`. axum drops the body when the client disconnects, and `Drop` sends `Cmd::Abort` if the state is `Dispatched` or `Decoding`. Nothing is sent if the state is earlier: drop the work locally and never send UserMsg.
**Contrast with upstream:** Python only notices a disconnect when the next chunk is yielded (`request.is_disconnected()`), then sleeps 0.1 s, then aborts through the tokenizer process (2 hops). Rust notices at once and uses 1 hop. Expect visibly faster KV reclamation in Scenario 1.
**Warning:** an abort that arrives while the request is in the in-flight **prefill** batch under overlap scheduling appears to lead to a double free or double unlock in upstream `_process_last_data`. The abort frees `table_idx` and unlocks the handle, then `cache_req(finished=False)` runs on the same req. **(unverified: inferred from code reading. Reproduce on GPU.)** Faster aborts make this race more likely. Provide an abort policy switch: `immediate` (default, baseline-like) or `defer-until-first-token` (safe).

### Pattern 4: Same-wire mock (the Mac seam)

**What:** `rsg-mock-scheduler` binds the same `ipc://` endpoints and implements the semantics in §0:
- FIFO with HOL blocking and a `max_running_req` cap
- one `BatchTokenizerMsg` per step
- an optional one-step-late reply after abort, emulating overlap
- silent drop of overlong prompts
- finish on `max_tokens` or eos
- a deterministic token stream, such as replaying the prompt tokens cyclically or a seeded PRNG over the vocab
- configurable per-token prefill cost and per-step decode latency

**Also:** the in-process `Backend` trait implementation (`InProcMock`) supports fast FSM unit tests with `tokio::time::pause()`.
**Payoff:** `py_front_vs_mock.py` can run upstream's own API server and tokenizer worker against the same mock. The callback passed to `run_api_server` starts only `tokenize_worker`. That gives a **GPU-free, apples-to-apples A/B of front-half host overhead on the Mac**, the cleanest test of the RFC's central claim. **(unverified: `minisgl.utils` imports `arch.py`/`torch_utils`. Check that these import on macOS with CPU torch.)**

## 4. The radix cache question: what a "Rust radix cache" can honestly mean

| Option | What it is | Upstream unmodified? | Serves real traffic? | Verdict |
|--------|------------|----------------------|----------------------|---------|
| **A. Behavioural-equivalence crate** | `rsg-radix` built to the `BasePrefixCache` semantics. Differential-tested by replaying identical op traces (match/insert/lock/unlock/evict) through upstream `RadixPrefixCache` on CPU. The harness stubs `minisgl.kernel.fast_compare_key` in Python and injects a counter for `time.monotonic_ns` plus a fake global ctx for `page_size` | Yes | No | **Required.** This literally satisfies the PROJECT requirement. |
| **B. Shadow / advisory cache in the front** | Rust mirrors the scheduler's likely cache contents: insert prompt prefixes at first token and request+output at finish, size-bounded by `num_pages × page_size` from the handshake. Uses: predicted prefix-hit metrics per request; optional **prefix-aware admission ordering** (an LPM-style policy in the Rust queue; since upstream is FIFO, the Rust send order *is* the scheduler's policy); future multi-scheduler cache-aware routing in the style of sgl-router | Yes | Advisory only | **Recommended, with LPM off by default.** Reordering changes batch composition, so parity runs must use pass-through. |
| **C. In-scheduler plug-in via the public registry** | upstream exposes `SUPPORTED_CACHE_MANAGER` (a runtime `Registry`). Our launcher shim can `register("rs-radix")` a thin Python `BasePrefixCache` adapter backed by a PyO3 build of `rsg-radix` (token keys read zero-copy from CPU tensors; values held as opaque `PyObject` GPU-tensor handles), then start the scheduler with `cache_type="rs-radix"` | **Yes** (uses an extension point, no source patch) | **Yes** | **Stretch phase.** It is the only honest way for a Rust radix to serve traffic. Gains will be small: radix ops run per request, not per token, except `evict`'s full-tree DFS under memory pressure. The project boundary also changes from "msgpack only" to "msgpack + in-process plug-in". This needs an explicit decision. |
| D. Replace the cache across the wire, TGI-style | Rust owns allocation and sends slots | **No.** The wire has no slot fields | n/a | **Rejected.** It violates the constraint against forking or patching upstream. |

**Implication for benchmarks:** in options A and B, Rust radix code is never on the TTFT or RPS hot path. Reports must not credit any of the three scenario wins to the radix cache. Under option B with LPM enabled, a win comes from a *scheduling-policy change*. It would be a separate, labelled experiment, not a host-overhead result.

**Equivalence definition (needed before coding):**
- same `cached_len` from every `match_prefix`
- same `InsertResult.cached_len`
- same `size_info` (evictable/protected) after every op
- the same *set of evicted value indices* for each `evict(n)`

Exact eviction order depends on heap tie-breaking among equal timestamps: splits inherit the timestamp, and every node touched in one walk shares a `tic`. Either replicate the Python DFS collection order and `heapq` sift exactly, or define equivalence modulo ties and test both ways.

## 5. Data Flow

### Request flow (streaming chat completion, happy path)

```
client POST /v1/chat/completions (stream=true)
  → [rsg-server] parse/validate JSON (same pydantic defaults as upstream: max_tokens=16, temperature=1.0, ...)
  → [rsg-lifecycle] uid = next_uid(); state=Received; spawn request task; return SSE body (CancelGuard armed)
  → [rsg-tokenizer, blocking pool] apply_chat_template(messages, add_generation_prompt=True) → encode(add_special_tokens=True) → Vec<u32>
  → [rsg-lifecycle] validate len < max_seq_len (from handshake) else 400; clamp max_tokens; state=Queued
  → [admission] pass-through (default) | prefix-aware ordering (opt-in, via shadow radix)
  → [rsg-transport actor] routes.insert(uid, tx); encode UserMsg{uid, int32 LE bin, SamplingParams}; PUSH (ipc minisgl_0)
  → [upstream Scheduler] FIFO admit → radix match → prefill → sample → DetokenizeMsg(s) per step → PUSH (ipc minisgl_1)
  → [rsg-transport actor] PULL; decode Batch/Detokenize; route by uid (unknown uid → drop)
  → [request task] state=Decoding on first; IncrementalDecoder.push(token, finished) → incremental &str
  → [rsg-server] SSE chunk with upstream framing ("data: {json}\n\n"; /generate uses "data: {text}\n")
  → on finished: final chunk with finish_reason "stop", then "data: [DONE]"; state=Finished; guard disarmed
```

### Cancellation flow

```
client disconnects → axum drops body → CancelGuard::drop
  ├─ state ∈ {Received, Tokenized, Queued}  → mark Cancelled locally; never send UserMsg
  └─ state ∈ {Dispatched, Decoding}         → Cmd::Abort(uid) → actor removes route, PUSH AbortBackendMsg
                                              (policy: immediate | defer-until-first-token)
                                            → scheduler frees req + KV (no ack) → late DetokenizeMsg dropped + counted
```

### Startup / control flow

```
mini-rsglang (Rust) starts → binds HTTP early (cold-start metric = HTTP-ready) → spawns python shim (or attaches to existing)
  → shim: parse_args(same flags as baseline) → replace(_unique_suffix=".rsg-<id>") → spawn TP scheduler(s) → wait ready
  → shim writes handshake JSON {endpoints, max_seq_len, eos_token_id, page_size, num_pages, max_running_req} to stdout/fd
  → Rust connects/binds ZMQ, loads tokenizer from same model_path, flips /health to ready
shutdown: Rust sends ExitMsg → waits child exit → kills on timeout
```

### Key Data Flows

1. **Tokens in:** text → Rust tokenizer → `Vec<u32>` → int32 LE bytes in the msgpack bin. Zero-copy is not possible across processes, but there is one encode, and there is no Python process in the path.
2. **Tokens out:** one `DetokenizeMsg` per request per scheduler step goes to the actor, then over a per-request bounded channel to the per-request decoder and out as SSE. Backpressure: if a client reads slowly, the per-request channel fills. Use `try_send` and buffer in the request task's unbounded local Vec so the actor never blocks on one slow client.
3. **Cancellation:** HTTP drop → guard → actor → Abort on the same FIFO as Submit, which guarantees ordering.

## 6. Scaling Considerations

| Scale (concurrent requests) | Architecture adjustments |
|-------|--------------------------|
| 1–128 (project target) | Design as described: one actor, one task per request, tokenizer on a blocking pool. Nothing more needed. |
| 128–2k | Coalesce sends (already in the actor). Cap queued requests in admission so admission doesn't grow without bound. Pre-size tokenizer pool threads to cores. Watch ZMQ HWM: the default SNDHWM is 1000 messages. |
| 2k+ / multi-GPU replicas | Several schedulers mean several actors, and a cache-aware router uses the shadow radix per backend (the sgl-router pattern). That is v2. |

### Scaling Priorities

1. **First bottleneck: the Python scheduler loop.** It is unchanged and caps decode throughput, which is why the realistic target for standard inference is parity. Front-half gains show up where host overhead dominates: short prompts at high RPS, many streams, and cancellation churn.
2. **Second bottleneck: tokenization of long prompts and chat-template rendering.** It is CPU-bound, so keep it off the async workers.

## 7. Anti-Patterns

### Anti-Pattern 1: Claiming the Rust radix cache speeds up serving
**What people do:** build a Rust trie in the front and attribute TTFT wins to it.
**Why it's wrong:** the scheduler's own Python radix cache still makes every admission and eviction decision, so the Rust trie is not on the hot path.
**Do this instead:** follow options A, B and C in §4. Report radix results as equivalence and microbenchmarks, and keep them separate from the end-to-end scenarios.

### Anti-Pattern 2: serde-derived msgpack with defaults
**What people do:** `#[derive(Serialize)]` plus `rmp_serde::to_vec`.
**Why it's wrong:** structs come out as arrays, `Vec<u8>` comes out as an array of ints, and a stray field crashes the remote scheduler via `cls(**kwargs)`.
**Do this instead:** write the codec by hand and pin it with golden bytes produced by upstream's own `serialize_type` and `msgpack.packb`.

### Anti-Pattern 3: Shared `Mutex<HashMap<uid, ...>>` touched by HTTP handlers and the socket reader
**Why it's wrong:** it creates races between registering a route and receiving the first token, and between abort and the original send. It also adds lock contention on the per-token path.
**Do this instead:** route through a single-owner actor (Pattern 1).

### Anti-Pattern 4: Changing admission order or uid scheme in parity runs
**Why it's wrong:** upstream admission is FIFO and decode batches are sorted by uid. Different ordering changes batch composition and can change greedy outputs through floating-point non-associativity.
**Do this instead:** run parity with pass-through admission and monotonic-from-0 uids. Check token identity first with serial requests, then under identical submission order.

### Anti-Pattern 5: Forking the launcher
**Do this instead:** use the upstream `Scheduler`/`parse_args` as a library from our own shim. upstream's `tests/core/test_scheduler.py` already demonstrates driving a standalone scheduler over ZMQ.

## 8. Integration Points

### External Services

| Service | Integration Pattern | Notes |
|---------|---------------------|-------|
| upstream mini-sglang scheduler | ZMQ PUSH/PULL over `ipc://` plus msgpack (§0 table) | Pin the commit. Any upstream message-schema change is a breaking change, so keep it in the wire golden tests. |
| HF Hub / local model dir | Load `tokenizer.json`, `tokenizer_config.json` (chat_template, clean_up_tokenization_spaces), and `chat_template.json` as a fallback, as upstream's `load_tokenizer` does | Rust must find the template in the same file upstream reads it from. |
| Benchmark clients | Same HTTP routes and SSE framing as upstream | Lets one load generator hit both servers unchanged. |

### Internal Boundaries

| Boundary | Communication | Notes |
|----------|---------------|-------|
| rsg-server ↔ rsg-lifecycle | direct calls + `mpsc` event stream per request | The HTTP layer has no backend knowledge. |
| rsg-lifecycle ↔ rsg-tokenizer | `spawn_blocking`/rayon for encode; inline `IncrementalDecoder` | The decoder is per request with no shared state. |
| rsg-lifecycle ↔ rsg-transport | `Cmd` MPSC (Submit/Abort/Exit) → actor; per-request `mpsc` back | One FIFO keeps Submit before Abort. |
| rsg-transport ↔ rsg-wire | pure fns `encode(&[Msg]) -> Bytes`, `decode(&[u8]) -> Vec<Reply>` | No I/O in rsg-wire. |
| rsg-lifecycle ↔ rsg-radix (shadow) | sync calls inside the admission component | Advisory only, behind a feature flag. |
| Rust ↔ python shim | child process; handshake JSON; `ExitMsg`/SIGTERM | The shim only uses upstream public modules. |

## 9. Suggested Build Order (dependency-driven)

```
P1 wire codec + golden fixtures (Mac) ─┬─▶ P2 transport actor + mock scheduler ─▶ P3 lifecycle FSM + cancellation ─┐
                                       │                                                                         ├─▶ P5 HTTP ingress/SSE ─▶ P6 shim + handshake + Py-front-vs-mock A/B (Mac)
P4 tokenizer encode + IncrementalDecoder parity (parallel, Mac) ────────────────────────────────────────────────┘                                   │
P7 radix crate + differential harness (parallel, Mac) ─▶ P8 shadow radix (metrics, opt-in LPM)                                                     ▼
                                                                                                                         P9 GPU E2E parity (remote) ─▶ P10 benchmark harness, 3 scenarios
                                                                                                                                                       ▶ P11 (stretch) radix plug-in via registry
```

1. **P1 Wire protocol.** This de-risks the boundary first, as PROJECT.md requires. Exit criterion: Rust-encoded bytes round-trip through upstream `BaseBackendMsg.decoder`, and upstream-encoded `DetokenizeMsg`/`BatchTokenizerMsg` decode in Rust.
2. **P2 Transport + mock scheduler.** Depends on P1. Exit criterion: submit, stream, abort, late tokens and silent drops all behave correctly over real ipc sockets on the Mac.
3. **P3 Lifecycle FSM.** Depends on P2; it can start earlier against `InProcMock`. Exit criterion: 128 concurrent tasks with random cancellations, no leaked routes, and every state transition covered by tests.
4. **P4 Tokenizer.** Independent, so start it alongside P1. It has the biggest parity risk: chat-template rendering, `clean_up_tokenization_spaces`, and the U+FFFD/`find_printable_text` streaming rules.
5. **P5 HTTP ingress.** Depends on P3 and P4. Byte-compatible routes and framing.
6. **P6 Shim + A/B on Mac.** Depends on P5. The Python front against the mock gives the first real host-overhead numbers without a GPU.
7. **P7/P8 Radix.** Independent pure-data-structure work. Integrate as a shadow after P5.
8. **P9 GPU parity.** Depends on P6. First contact with the real scheduler: abort race, `max_seq_len` handshake, EOS behaviour.
9. **P10 Benchmarks.** The harness can be developed against the mock from P6 onward. Final numbers need P9.
10. **P11 Stretch.** Registry plug-in for the radix (option C). Needs an explicit scope decision.

## 10. Comparison with other front halves

| System | Front-half processes | Boundary / serialization | Who tokenizes / detokenizes | Who owns the prefix cache | Lesson for us |
|--------|----------------------|--------------------------|-----------------------------|----------------------------|---------------|
| **mini-sglang** (HIGH, source) | API server + tokenizer/detokenizer worker(s) | ZMQ ipc PUSH/PULL + msgpack with `__type__` tags | separate Python worker process | scheduler (`RadixPrefixCache` + GPU page allocator) | Replace API + tokenizer processes; the boundary is narrow (7 message types). |
| **SGLang full** (MEDIUM, prior knowledge) | main process with TokenizerManager; separate Detokenizer process | ZMQ + pickled Python objects | TokenizerManager / DetokenizerManager | scheduler (RadixCache) | The same split at larger scale. Pickle would be far harder to speak from Rust; mini-sglang's msgpack is what makes this project feasible. |
| **sgl-model-gateway (sgl-router), gRPC mode** (LOW→MEDIUM, docs) | Rust gateway | gRPC to SRT workers, **token ids** on the wire | **Rust, in-process** (plus reasoning and tool parsers) | the worker. The router keeps only an **approximate per-worker radix tree, char-level**, for cache-aware routing (`cache_threshold` 0.3, `balance_abs_threshold` 64, `balance_rel_threshold` 1.5, `eviction_interval_secs` 120, `max_tree_size`) | The closest analogue. It validates "Rust tokenizes, worker keeps the real cache", and the shadow radix (option B) is exactly its routing tree. |
| **vLLM V1** (MEDIUM, docs + prior) | API server process(es) | ZMQ to EngineCore process(es); msgpack (msgspec) | API-server side (input processor; output processor/detokenizer) | EngineCore KVCacheManager (hash-based block prefix caching) | The industry standard puts tokenize/detokenize in the front and the cache next to the allocator, the same split we're inheriting. |
| **TGI v3** (MEDIUM, source + prior) | Rust router (validation, tokenization, queue, batching) | gRPC over Unix sockets to Python shards | Rust router | **Rust router** (`RadixAllocator`: trie + free blocks + ref-counts; sends slots/prefix length to the shards) | A Rust-owned radix is only possible when the protocol carries block tables. mini-sglang's doesn't, so option D is out. |

## Sources

- mini-sglang source, `sgl-project/mini-sglang@9a91cfafe754` (2026-05-17), read directly: `server/launch.py`, `server/api_server.py`, `server/args.py`, `utils/mp.py`, `message/{backend,frontend,tokenizer,utils}.py`, `tokenizer/{server,tokenize,detokenize}.py`, `scheduler/{scheduler,io,config,cache,prefill,decode,table}.py`, `kvcache/{radix_cache,base,__init__}.py`, `utils/{registry,hf}.py`, `engine/config.py`, `tests/core/test_scheduler.py`, `tests/misc/test_serialize.py`, `pyproject.toml`. HIGH.
- [SGLang Model Gateway docs](https://docs.sglang.io/docs/advanced_features/sgl_model_gateway.md): cache-aware policy parameters, char-level approximate tree, gRPC mode with in-process Rust tokenizer. Seam LOW, consistent with prior knowledge, so MEDIUM.
- [TGI `backends/v3/src/radix.rs`](https://github.com/huggingface/text-generation-inference/blob/main/backends/v3/src/radix.rs): `RadixAllocator` owns `free_blocks`, `allocations` and the `RadixTrie`, with LRU leaf eviction and ref-counts. MEDIUM.
- [vLLM architecture overview](https://docs.vllm.ai/en/latest/design/arch_overview/): API server ↔ engine core over ZMQ. MEDIUM. The msgspec/msgpack detail comes from prior knowledge and was not re-verified (LOW).
- [zmq.rs (`zeromq` crate)](https://github.com/zeromq/zmq.rs): PUSH/PULL, tcp + ipc (Unix), tokio runtime, "does not implement all of ZeroMQ's feature set". MEDIUM. The STACK research should confirm it against libzmq ipc peers.

---
*Architecture research for: Rust front half driving the unmodified mini-sglang scheduler*
*Researched: 2026-10-02*
