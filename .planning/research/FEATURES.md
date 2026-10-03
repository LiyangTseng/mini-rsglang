# Feature Research

**Domain:** Rust front half for an LLM serving stack (ingress, request-lifecycle FSM, HF tokenization, detokenization, radix prefix cache) driving an unmodified mini-sglang Python/CUDA scheduler over ZMQ + MessagePack
**Researched:** 2026-10-02
**Confidence:** HIGH for the upstream contract (read directly from mini-sglang source at commit `9a91cfaf`, 2026-05-17). MEDIUM for how competing front ends (SGLang gateway, vLLM, TGI) behave and for the benchmark design. LOW for anything that comes only from web search.

> **The constraint that drives everything else:** "matching the Python baseline" means matching what mini-sglang actually does, which is much less than the OpenAI spec. The upstream front end exposes only `/v1/chat/completions` (which also accepts a raw `prompt`), `/generate`, `/v1/models` and `/v1`. It ignores `stop`, `n`, `presence_penalty` and `frequency_penalty`. It has no `/v1/completions`, no `/health` and no `/metrics`. It always returns `finish_reason: "stop"` and returns zeroed `usage`. Anything the Rust side adds on top is a differentiator. None of it can be part of the output-parity or benchmark comparison, because the baseline cannot do it.

---

## Upstream Contract: What mini-sglang Actually Exposes (HIGH, source-verified)

The Rust front half replaces three upstream processes: the FastAPI `api_server`, the tokenizer worker and the detokenizer worker. These share one process by default (`--num-tokenizer 0`). The Rust side talks to the **scheduler rank-0 process** directly. That process stays unmodified.

### Process boundary and sockets

| Direction | Transport | Address (from `SchedulerConfig`) | Binding | Notes |
|-----------|-----------|----------------------------------|---------|-------|
| Front → scheduler | ZMQ PUSH → scheduler's PULL | `ipc:///tmp/minisgl_0{_unique_suffix}` (`zmq_backend_addr`) | Scheduler **binds** | The scheduler drains all pending messages every loop iteration. |
| Scheduler → front | Scheduler's PUSH → front's PULL | `ipc:///tmp/minisgl_1{_unique_suffix}` (`zmq_detokenizer_addr`) | The scheduler binds if `backend_create_detokenizer_link` is True. The base `SchedulerConfig` sets True. `ServerArgs` sets it to False when `num_tokenizer == 0`, and then the front binds. | A launcher shim that builds a `SchedulerConfig` directly makes the scheduler bind both sockets, so Rust only connects. That is the simplest topology. |
| Rank 0 → other ranks (TP>1) | ZMQ PUB/SUB, `minisgl_2` | — | Internal | Invisible to the front. |

- `_unique_suffix` defaults to `.pid=<pid>`. The launcher shim must set it explicitly so the Rust side knows the address.
- **"Lock-free channel" in PROJECT.md vs. reality.** The unmodified backend only speaks ZMQ IPC. Any lock-free channel has to sit inside Rust, between the ingress tasks and a dedicated ZMQ I/O task. It cannot replace the Rust↔Python hop.
- The scheduler has **no readiness handshake over ZMQ**. Upstream readiness is an `mp.Queue` ack ("Scheduler is ready"). Our launcher shim has to re-expose that, for example as a stdout line, a file, or a socket.

### Wire format (msgpack, `use_bin_type=True`)

Every message is a msgpack **map** with a `"__type__"` string discriminator. The scheduler decodes it by calling `cls(**kwargs)`. An unknown key raises `TypeError` and **kills the scheduler loop**, so the schema must match exactly.

| Message | Direction | Fields |
|---------|-----------|--------|
| `UserMsg` | → backend | `uid: int`, `input_ids: {"__type__":"Tensor","buffer":<bin, native little-endian int32>,"dtype":"torch.int32"}`, `sampling_params: SamplingParams` |
| `SamplingParams` | nested | `temperature: float` (default 0.0), `top_k: int` (-1), `top_p: float` (1.0), `ignore_eos: bool` (False), `max_tokens: int` (1024) |
| `AbortBackendMsg` | → backend | `uid: int` |
| `ExitMsg` | → backend | none (raises KeyboardInterrupt, then graceful shutdown) |
| `BatchBackendMsg` | → backend | `data: [ ...above... ]` |
| `DetokenizeMsg` | ← backend | `uid: int`, `next_token: int`, `finished: bool` |
| `BatchTokenizerMsg` | ← backend | `data: [DetokenizeMsg...]`. Sent when more than one reply is produced in a step. |

### Backend behaviors the front half must absorb (all HIGH, from `scheduler/scheduler.py`, `prefill.py`, `decode.py`)

1. **No accept/ack message.** The first sign that a request is alive is its first `DetokenizeMsg`. Queued and prefill look the same from outside. Chunked-prefill steps send nothing.
2. **Over-length prompts are silently dropped.** If `input_len >= max_seq_len`, the scheduler logs a warning and never replies. The upstream API then hangs forever. `max_seq_len = min(model max_position or override, KV capacity)` is only known after GPU init.
3. **`max_tokens` is silently clamped** to `max_seq_len - input_len`.
4. **Stopping is decided only by the backend:** the single `tokenizer.eos_token_id`, unless `ignore_eos` is set, or `max_tokens`. The final `DetokenizeMsg` has `finished=True` and **contains the EOS token**, which the detokenizer drops. **No finish reason is transmitted.** The front infers it: last token == EOS gives `stop`, otherwise `length`.
5. **Abort is fire-and-forget.** There is no confirmation. Because of overlap scheduling, **tokens can still arrive after an abort** (the in-flight batch). After an abort no `finished=True` ever arrives. Replies for unknown uids must be dropped, as upstream's `if msg.uid not in self.ack_map: continue` does.
6. **uids are owned by the front.** They must be unique for the backend's whole lifetime. A Rust restart that resets the counter to 0 while the backend is still running would collide.
7. **One `DetokenizeMsg` per request per step,** batched across requests. The front's receive path has to demultiplex them.

### Upstream front-end behavior (the baseline to match or beat)

| Behavior | Upstream (`server/api_server.py`, `tokenizer/*.py`) | Parity implication |
|----------|------------------------------------------------------|--------------------|
| Endpoints | `POST /v1/chat/completions`, `POST /generate`, `GET /v1/models`, `GET/POST/HEAD/OPTIONS /v1` → `{"status":"ok"}` | Benchmarks must use `/v1/chat/completions`. |
| Request model | `model` (required), `prompt` or `messages` (roles system/user/assistant, `content: str`), `max_tokens=16`, `temperature=1.0`, `top_k=-1`, `top_p=1.0`, `n`, `stream`, `stop`, penalties, `ignore_eos`. Unknown fields are ignored. | Defaults differ from OpenAI: `max_tokens=16`, and `temperature=1.0` means sampling is **stochastic by default**. Parity tests must send `temperature=0`. |
| Forwarded params | `temperature`, `top_k`, `top_p`, `max_tokens`, `ignore_eos` only | `stop`, `n` and penalties are accepted but have no effect. |
| Chat template | `tokenizer.apply_chat_template(msgs, tokenize=False, add_generation_prompt=True)`, then `tokenizer.encode(prompt)` with **`add_special_tokens=True`** | Templates that already emit BOS (Llama-3) get a **double BOS**. Rust must reproduce this exactly. |
| `prompt` on the chat endpoint | Encoded raw, no template | Gives a template-free path for parity tests. |
| Streaming format | `data: {json}\n\n` with `"object":"text_completion.chunk"`, `id:"cmpl-{uid}"`, no `model` or `created`. **One chunk per generated token, even when the text delta is empty.** `role:"assistant"` rides on the first chunk. A final chunk with `finish_reason:"stop"`, then `data: [DONE]`. | TTFT in the upstream benchmark client is the time to the **first chunk**. Rust must keep one chunk per token, or the TTFT and TPOT numbers are not comparable. |
| Non-streaming | Collects all text. `finish_reason` is always `"stop"`. `usage` is all zeros. | Compare generated text only, not full response bodies. |
| Disconnect/cancel | Streaming: on disconnect, `abort_user` **sleeps 100 ms** and then sends `AbortMsg`, which goes API → tokenizer process → scheduler. Non-streaming: no cancellation, the request runs to completion. | This is a key Scenario-1 lever. Rust can abort at once over one hop, and can also cancel non-streaming requests. |
| Detokenization | `DetokenizeManager`: per-uid `decoded_ids` with `read_offset`/`surr_offset`/`sent_offset`. Each step re-decodes the window `ids[surr:]` and the window `ids[surr:read]` via `batch_decode` (`skip_special_tokens=False`). If the new text ends in U+FFFD, it holds back via `find_printable_text` (with CJK ranges). Drops a final EOS. | Port the algorithm **verbatim**. `DecodeStream` produces different chunk boundaries and edge cases. |
| Tokenizer and detokenizer process | One Python process by default handles **both** tokenization and detokenization, one ZMQ message at a time (`local_bs=1`) | Head-of-line blocking: a long prompt being tokenized stalls detokenization for every stream. This is the main host-overhead bottleneck the Rust side removes. |
| Health / metrics | None (only `/v1` and `/v1/models`) | Readiness probe for both stacks: `GET /v1/models` returns 200. |

---

## Feature Landscape

### Table Stakes (must have, or the project's core value fails)

For this project, "table stakes" means three things. Output parity with the baseline must be provable. The three benchmark scenarios must be runnable. An OpenAI client must work unchanged against either stack.

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| **Wire-exact ZMQ + msgpack client for the scheduler** (UserMsg, AbortBackendMsg, ExitMsg, batch variants; DetokenizeMsg demux) | Nothing works without it, and one bad key crashes the backend | MEDIUM | Golden-byte tests against upstream's own `serialize_type`/`msgpack.packb` output (runnable on macOS). Dedicated I/O task. Never block the async runtime on a ZMQ send, because PUSH blocks at the HWM. |
| **Launcher shim for the unmodified scheduler** (fixed `_unique_suffix`, binds both sockets, readiness signal, optional `--num-pages`/`--max-seq-len-override` pass-through) | The upstream `launch_server` always starts its own API server and tokenizer processes | LOW | Lives in our repo and imports `minisgl.server.launch._run_scheduler` / `Scheduler`. This is not a patch to upstream. The baseline should also start through the upstream entry point unchanged. |
| **`POST /v1/chat/completions`**, streaming SSE and non-streaming, with `messages` or `prompt` | The baseline endpoint, and what the benchmark client calls | MEDIUM | OpenAI-shaped chunks are fine. Keep **one SSE event per backend token** in compat mode. `role` goes in the first delta. End with `[DONE]`. |
| **`GET /v1/models`** (and `/v1` ok) | The OpenAI SDK and the upstream bench client (`get_model_name`) call it. It also serves as the readiness probe. | LOW | Return the model path as the id, as upstream does. |
| **Sampling-parameter passthrough**: `temperature`, `top_k`, `top_p`, `max_tokens`, `ignore_eos` | The only knobs the backend honors. The benchmark client sends `temperature=0`, `top_k=1` and `ignore_eos=true` via `extra_body`. | LOW | Same defaults as upstream: `max_tokens=16`, `temperature=1.0`. Accept and ignore `n`, `stop` (unless the stop-string feature is enabled), penalties, and unknown fields. Upstream tolerates them, so rejecting them would break clients. |
| **Request validation that prevents silent hangs**: prompt length vs. `max_seq_len`, `max_tokens >= 1`, non-empty input | The backend drops over-length prompts with no reply | LOW | Learn `max_seq_len` from the shim, which can report it after init, or from config/override. Return 400 instead of hanging. Also add a per-request backend-silence timeout as a backstop. |
| **HF tokenization parity** (tokenizer.json via the `tokenizers` crate, `add_special_tokens=true`) | Core Value: identical output requires identical input ids | MEDIUM | The Python fast tokenizer *is* the Rust `tokenizers` core, so raw encode parity is close to free. Risk lives in the wrapper behavior: special-token handling, `added_tokens`, and `clean_up_tokenization_spaces` on decode (a Python-only post-process: False for Qwen3, believed True for Llama-3.x, MEDIUM). Use **Qwen3-0.6B** as the canonical parity model. It has no BOS, `clean_up=False`, EOS `<|im_end|>`, and the upstream README uses it. |
| **Chat template rendering parity** (Jinja via minijinja; `add_generation_prompt=true`; HF globals `bos_token`, `eos_token`, `raise_exception`, `strftime_now`; Python-compat string methods) | Chat requests can only reach parity if the rendered prompt is byte-identical | MEDIUM-HIGH | The Qwen3 template uses `namespace`, `messages[::-1]`, `.startswith`, `.split`, `.rstrip`/`.lstrip`, `tojson` and loop vars. That needs minijinja's pycompat (`minijinja-contrib`). Template source order: `tokenizer_config.json` `chat_template`, then `chat_template.jinja`, then `chat_template.json` (the upstream Mistral fallback). Differential-test against `apply_chat_template` over a corpus. |
| **Incremental detokenization parity** (verbatim port of upstream `DetokenizeManager`, incl. the U+FFFD hold-back, `find_printable_text`, CJK ranges, EOS drop, `skip_special_tokens=false`) | Streaming text must concatenate to exactly the baseline text, and multi-byte UTF-8 (emoji, CJK, byte-fallback) must never emit broken characters | MEDIUM | `tokenizers::decode` returns lossy UTF-8 with U+FFFD, which matches Python, so the replacement-char check ports directly. Define parity on **concatenated text plus token ids**, not on chunk boundaries. Per-request state makes it trivially parallel across cores. |
| **Request-lifecycle FSM**, front-observable (see below) | PROJECT requirement and Scenario 1. Leaked state under 128 agents with cancellations is fatal. | MEDIUM | Exactly one terminal transition. Abort sent at most once, and only if a UserMsg was sent. Late tokens after a terminal state are dropped. Detokenizer and uid-map state are freed on every path. |
| **Cancellation on client disconnect** (streaming and non-streaming), with immediate `AbortBackendMsg` | Scenario 1 measures exactly this. Upstream adds a 100 ms delay and an extra process hop. | MEDIUM | Detect via drop of the response body stream (axum/hyper). Add SSE keep-alive comments so a dead client is noticed even when no tokens are flowing. Hyper's HTTP/1 disconnect detection while a body is pending must be verified in phase research. Cancelling during `Tokenizing` must guarantee the UserMsg is **never** sent afterwards. |
| **Radix prefix-cache trie, behaviorally equivalent to `RadixPrefixCache`** | PROJECT requirement | MEDIUM-HIGH | See the radix section below. **Scope caveat:** the backend keeps using its own Python radix cache, because the cache owns KV page indices and the scheduler is unmodified. The Rust trie's v1 role has to be decided explicitly. |
| **Mock backend that speaks the exact ZMQ + msgpack contract** | All front-half development happens on a Mac with no CUDA | MEDIUM | See the mock-backend section. Two flavors: a fast Rust mock for tests and load, and a Python "contract oracle" that uses upstream's own `minisgl.message` and ZMQ classes. Those import on macOS if `minisgl.kernel` is stubbed. |
| **Parity harness**: same prompts, greedy decoding, compare token ids and text, Rust front vs. upstream front on the same backend | Core Value ("output identical") | MEDIUM | Greedy decoding (`temperature=0`) is required. Cover the `prompt` path and the chat path, multi-byte outputs, long prompts (chunked prefill), and `ignore_eos` both on and off. Fixtures for tokenize, template and detokenize can be produced on the Mac using upstream Python code plus `transformers`. |
| **Benchmark harness for the 3 scenarios**, identical client and hardware for both stacks | Core Value ("reproducible harness beating baseline") | HIGH | See the benchmark section. |
| **Liveness and readiness endpoints** (`/health`, `/health/ready`, plus `/v1/models` as the shared probe) | Scenario 3 needs a precise "ready" instant, and process supervision needs a liveness check | LOW | Readiness = HTTP bound, tokenizer loaded and backend reachable. Report front-ready and backend-ready separately. |

### Differentiators (where the Rust front half earns its claims, or goes beyond the baseline)

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| **Decoupled tokenize and detokenize execution** (tokenization on a blocking pool, detokenization sharded per request across runtime workers) | Removes the upstream single-process head-of-line blocking. This is the mechanism behind the expected Scenario-1 and Scenario-2 wins. | MEDIUM | Upstream's knob `--num-tokenizer N` only adds tokenizer processes, with one shared detokenizer. Benchmark the baseline at its best setting for fairness. |
| **Zero-delay, single-hop abort** | Frees backend batch slots and KV sooner, so other agents get lower P99 TTFT | LOW (once the FSM exists) | Upstream is 100 ms sleep plus API → tokenizer → scheduler. Measure abort-propagation latency explicitly. |
| **Batched backend sends** (coalesce several UserMsg/Abort into one `BatchBackendMsg` per I/O tick) | Fewer syscalls and fewer scheduler `recv` calls under Scenario-2 saturation | LOW | The scheduler already handles `BatchBackendMsg`. Coalescing must not add latency: flush as soon as the channel is empty, never on a timer. |
| **Prometheus `/metrics`** (TTFT, TPOT and e2e histograms; inflight; per-state counts; cancellations; abort latency; tokenize, template and detokenize latency; ZMQ send/recv latency; RSS) | Lets the benchmark explain *why* Rust wins and catches regressions | LOW-MEDIUM | Lock-free atomics or histograms only, so metrics never perturb the benchmark. The baseline has none, so the harness must measure from the client side for both stacks. |
| **Stop strings (`stop`) implemented in the front** | OpenAI compatibility: a common request parameter the baseline silently ignores | MEDIUM | Scan the detokenized text, hold back any suffix that could be the start of a stop string, truncate at the match, then send `AbortBackendMsg` and report `finish_reason:"stop"`. A few extra backend tokens are wasted because of overlap scheduling. **Excluded from parity tests**, because the baseline ignores `stop`. Reuses the abort path. |
| **Correct `finish_reason` and real `usage`** (`stop` vs `length` inferred from final token == EOS, prompt/completion token counts) | Upstream reports `"stop"` and zeros every time, which breaks clients that check truncation | LOW | Free once the FSM tracks the final token. Optional `stream_options.include_usage`. |
| **`POST /v1/completions`** | Standard OpenAI endpoint, missing upstream | LOW | Same pipeline without the template, returning a `text_completion` object. For parity, compare it with the baseline's `/v1/chat/completions` + `prompt` path. |
| **`POST /v1/tokenize` and `/v1/detokenize`** | Lets the parity harness diff tokenizer behavior over HTTP. vLLM and SGLang offer these too. | LOW | Mostly a test-tooling feature. |
| **Front-side admission control and queue** (cap in-flight to `max_running_req`, hold the rest in Rust, cancel queued requests without touching the backend) | Makes "Queued" a real state, and queued cancellations cost the backend nothing | MEDIUM | **Off by default.** It changes scheduling versus the baseline, which forwards everything immediately, so the benchmark comparison must state which mode it uses. |
| **Shadow radix index for prefix-aware admission** (Rust trie mirrors inserted prompts, predicts cache hits, orders admitted requests or reports hit ratios) | Gives the Rust radix trie a real job without modifying the backend. Same idea as sgl-model-gateway's approximate-tree cache-aware routing. | MEDIUM | Approximate only: backend evictions are invisible. Report predicted vs. actual (actual is not observable without a backend probe), so treat it as a metric or heuristic, not a correctness feature. |
| **Fast cold start and small RSS of the front-half process** | Scenario 3 claim: milliseconds and under 500 MB | LOW-MEDIUM | Loading tokenizer.json dominates. Optionally memory-map, or pre-serialize the tokenizer and template. Avoid Python, torch and transformers in the front entirely. |
| **Backend-side probe hooks in the launcher shim** (timestamp UserMsg receipt, first token, abort receipt) | Measures abort-propagation and queueing latency in the same way for **both** stacks | MEDIUM | Done by wrapping `Scheduler._process_one_msg` and `send_result` from the shim, which is test-time instrumentation and not an upstream patch. To be fair, the baseline would also have to launch through the shim. Keep it optional and off for headline numbers. |

### Anti-Features (deliberately NOT built in v1)

| Feature | Why Requested | Why Problematic | Alternative |
|---------|---------------|-----------------|-------------|
| **Replacing the backend's Python radix cache with the Rust one** | The "Rust radix cache" requirement suggests it | The cache stores GPU KV page indices and lives inside `CacheManager` in the scheduler process. Swapping it means modifying the upstream scheduler, which breaks the "unmodified backend and baseline" decision. | Rust trie = an equivalence-proven library (differential trace replay against the Python `RadixPrefixCache` on CPU tensors), plus an optional shadow index in the front. A real swap waits for a separate milestone that relaxes "unmodified". |
| **`n > 1`, `logprobs`, `top_logprobs`, `seed`, `presence/frequency_penalty`, `stop_token_ids` enforced by the backend** | OpenAI spec completeness | The backend has no way to do these: one sequence per uid, no logprobs returned, no per-request seed, no penalties | Accept and ignore, as upstream does (document it), or return 400 with a clear message for `n>1` and `logprobs`. Never fake them. |
| **Tool calling, function-call parsing, reasoning (`<think>`) parsers, JSON mode, structured output** | Modern OpenAI servers ship them (sgl-model-gateway, vLLM) | Out of scope by PROJECT (constrained decoding deferred to v2). Large surface, and nothing in the baseline to compare against. | v2. Keep message `content` as a string, optionally also accepting an array of text parts. |
| **Multi-backend routing, load balancing, circuit breakers, retries, rate limiting** | sgl-model-gateway and TGI have them | Not needed for a single scheduler. Retries corrupt streaming semantics and benchmark numbers. | One backend per front. The shadow radix index could later back cache-aware routing. |
| **Multimodal inputs, embeddings, rerank, `/v1/responses`, conversation storage** | Feature parity with production gateways | The backend serves text causal LMs only | Not planned. |
| **Changing the SSE event granularity** (coalescing several tokens per event, dropping empty-delta events) | Fewer bytes, faster on paper | Breaks TTFT/TPOT comparability with the upstream benchmark client, which counts one event per token and takes TTFT from the first event | Compat mode keeps one event per token. A coalescing mode is allowed only as an extra, clearly labeled benchmark variant. |
| **Using `DecodeStream` instead of porting the upstream detokenizer** | Less code, and it is the HF-blessed streaming API | Different hold-back and chunking behavior, plus edge cases at the final token. Undermines parity claims. | Port `DetokenizeManager` verbatim. `DecodeStream` can be a later optimization behind a flag, validated by the parity harness. |
| **HTTP/2, gRPC, or WebSocket ingress in v1** | Seen in production gateways | No baseline counterpart. The benchmark client uses HTTP/1.1 SSE through the OpenAI SDK. | HTTP/1.1 + SSE only. |
| **In-process Python (PyO3) embedding of the scheduler** | Avoids IPC | Brings the GIL back into the front process and defeats the "GIL-free host" thesis. Also changes the baseline topology. | Keep the ZMQ process boundary, which is also upstream's boundary. |
| **Benchmarking with `--dummy-weight`** for headline numbers | No weight download needed | Garbage output, so parity cannot be shown alongside, and EOS timing differs (use `ignore_eos` anyway) | Allowed for harness smoke tests only. Headline runs use real Qwen3 weights with `ignore_eos=true` for fixed output lengths. |

---

## Request Lifecycle FSM (front-observable)

The front cannot see "prefill" directly, because the backend sends no state events. Map the PROJECT state names onto what the front can actually observe:

```
                 validate fail ─────────────────────────► Rejected (4xx, terminal)
Received ──► Tokenizing ──► [Queued]* ──► Submitted ──► Decoding ──► Finished{Eos | Length | StopString}
   │             │             │            │  (UserMsg sent;  (first DetokenizeMsg)
   │             │             │            │   backend queued/prefill — opaque)
   └─────────────┴─────────────┴────────────┴──────────────┴──► Cancelled{ClientGone | StopString | Timeout}
                                                                 Failed{BackendSilent | BackendGone | Internal}
* Queued exists only when front-side admission control is enabled.
```

| PROJECT name | Front state | Entry event | On cancel |
|--------------|-------------|-------------|-----------|
| queued | Tokenizing / Queued / Submitted | request accepted | Before Submitted: drop locally, never send UserMsg. After: send one `AbortBackendMsg`. |
| prefill | Submitted (awaiting first token) | UserMsg written to the socket | Abort. Late tokens are possible and are ignored. |
| decode | Decoding | first DetokenizeMsg | Abort. Late tokens are ignored. |
| finished | Finished | `finished=True` received, or stop-string match | — |
| cancelled | Cancelled | client disconnect, stop-string abort, timeout | — |

Invariants to property-test (loom and proptest against the mock): exactly one terminal state. Abort sent at most once and only after a UserMsg. No UserMsg after cancellation. Every state map is empty after quiescence. uids are never reused for the life of the backend process; start the counter from a random or time-based base so a Rust restart cannot collide with in-flight backend requests.

---

## Radix Cache: Upstream Semantics to Reproduce (HIGH, `kvcache/radix_cache.py`, `scheduler/cache.py`)

| Operation | Upstream behavior | Equivalence notes |
|-----------|-------------------|-------------------|
| Child key | `page_size==1`: the first token id. Otherwise: a tuple of the first `page_size` ids. | Default page_size=1. The trtllm backend forces 16/32/64. Support both. |
| `match_prefix(ids)` | Walks from the root. Compares with `fast_compare_key` (`std::mismatch`). **Aligns match length down to page_size.** On a partial match it **splits the node** (the new parent inherits `ref_count` and timestamp), so match *mutates* tree structure even though its doc says otherwise. Updates the timestamp of every visited node to a single `tic` (LRU). Returns a handle `(cached_len, node)`. | The scheduler calls it on `input_ids[:len-1]`, always leaving at least one token to compute. |
| `insert_prefix(ids, indices)` | Truncates to `align_down(len, page_size)`. Walks the tree (which may split). If not fully present, attaches **one** new leaf holding the remaining suffix, with `timestamp=now`. `evictable_size += len`. Returns `(prefix_len_already_cached, handle(insert_len, node))`, and the caller frees the duplicate indices `[old_cached, prefix_len)`. | Values are opaque KV slot indices. In Rust use `u32`, passed through unchanged. |
| `lock_handle(h, unlock)` | Walks node → root. Increments or decrements `ref_count`. On 0↔1 transitions moves `length` between `evictable_size` and `protected_size`. The root is permanently `ref_count=1`. | Refcounts are per node on the path, not per handle. |
| `evict(size)` | Asserts `size <= evictable_size`. Collects **all** unreferenced leaves by a full DFS (O(n) per call). Heap ordered by timestamp (oldest first). Pops whole nodes, so it **may over-evict**. When a parent becomes a leaf with ref 0, it is pushed onto the heap. Returns the concatenated values. | Equal-timestamp ties are broken by heap and DFS order, which depends on dict insertion order. Use an injected logical clock in tests and compare evicted *sets and sizes*. Exact order only when timestamps are distinct. |
| `size_info` | `(evictable_size, protected_size)` | The scheduler's `available_size = evictable + free_slots*page_size`. |
| `reset` / `check_integrity` | `NotImplementedError` / no-op | Do not over-build. |
| Alternative | `--cache naive` (`NaivePrefixCache`). The upstream README's online benchmark uses `--cache naive`. | Benchmark configs must state the cache type, because radix hits change TTFT for agent workloads. |

**Differential test strategy:** run upstream `RadixPrefixCache` on CPU int32 tensors on the Mac, with `minisgl.kernel` stubbed (`fast_compare_key` replaced by a pure-Python mismatch) and a `Context(page_size)` installed via `set_global_ctx`. Replay random traces of match, lock, insert, unlock and evict. After every step, assert identical `cached_len`, `size_info`, inserted prefix lengths and evicted sets.

---

## Mock Backend Capabilities (needed for GPU-free development)

| Capability | Why | Complexity |
|------------|-----|------------|
| Binds or connects the same two ZMQ IPC sockets and decodes the exact msgpack schema, **rejecting unknown keys** as the real scheduler effectively does by crashing | Contract fidelity | LOW |
| Continuous-batching emulation: pending queue, `max_running_req` cap, one step per tick emitting one `DetokenizeMsg` per running request, batched as `BatchTokenizerMsg` | Realistic demux and backpressure | MEDIUM |
| Configurable timing: prefill cost = f(uncached input tokens, chunk size), decode step = f(batch size), jitter, deterministic seed | Scenario rehearsal, tail-latency tests | MEDIUM |
| Deterministic output generators: echo input, scripted id sequences (multi-byte UTF-8 split across tokens, CJK, emoji, byte-fallback, special tokens), EOS at position k, `ignore_eos` and `max_tokens` honored, final message includes the EOS token | Detokenizer and FSM tests | LOW |
| Upstream-faithful quirks: silently drop `input_len >= max_seq_len`, clamp `max_tokens`, no abort ack, **emit 0–2 late tokens after abort** (overlap scheduling), ignore unknown-uid aborts, `ExitMsg` triggers shutdown | Proves the front handles real backend edge cases | LOW-MEDIUM |
| Fault injection: stall (no replies), crash/disconnect, slow consumer (HWM pressure) | Timeout, backend-gone and backpressure paths | LOW |
| Readiness signal identical to the launcher shim's | Same startup code path as the real backend | LOW |
| Python "contract oracle" mode: tiny script using upstream `minisgl.message` and `ZmqPullQueue`/`ZmqPushQueue` (imports on macOS with torch CPU + pyzmq + msgpack, `minisgl.kernel` stubbed) | Proves byte-compatibility with the **real** upstream codec without a GPU | LOW |
| Optional internal shadow radix to emulate cache-hit-dependent prefill cost | Makes Scenario-1 agent workloads (shared system prompt, growing history) behave realistically | MEDIUM |

---

## Benchmark Harness Features

General requirements, applying to all scenarios:

| Feature | Why | Complexity |
|---------|-----|------------|
| **One load generator for both stacks**, written in Rust (tokio + hyper/reqwest, SSE parser) or a pinned, verified-fast tool, on separate pinned cores | A Python asyncio client saturates before the server at 128 streams or high RPS and would hide the difference. The upstream `minisgl.benchmark.client` (OpenAI SDK) is kept as a cross-check, not as the headline client. | MEDIUM |
| Identical request bodies to `/v1/chat/completions` (`temperature=0`, `top_k=1`, `ignore_eos=true`, fixed `max_tokens`), same model, same backend flags (`--cache radix|naive`, `--max-running-requests`, page size) | Fairness | LOW |
| TTFT = first SSE event (also record first non-empty content), TPOT, e2e, P50/P90/P99/max, RPS, tokens/s, error and timeout counts | Matches upstream `process_benchmark_results` | LOW |
| Warm-up phase, steady-state window, ≥3 repetitions, confidence intervals, seeded workloads, raw per-request JSONL output | Reproducibility | MEDIUM |
| Run manifest: git SHAs (ours plus pinned upstream), GPU/driver/CUDA, model revision, flags, baseline config incl. `--num-tokenizer` | "Reproducible" requirement | LOW |
| Baseline tuned to its best config (try `--num-tokenizer 0` and `N`) | Avoid a straw-man comparison | LOW |
| Parity gate before benchmarking (greedy output identical on a fixed prompt set) | Do not benchmark a front half that produces different output | LOW |

| Scenario | Workload | Metrics | Harness-specific features |
|----------|----------|---------|---------------------------|
| **1. 128 concurrent agents, dynamic requests/cancellations → P99 TTFT** | 128 closed-loop agents. Multi-turn sessions with a shared system prompt and a growing history (radix-friendly), think time between turns. A seeded fraction of requests is cancelled by client disconnect: before the first token, after k tokens, or after t ms. | P99/P50 TTFT of **non-cancelled** requests, TPOT, cancellation-to-abort latency (internal metrics plus optional shim probe), goodput | Real TCP disconnect (drop the connection, not just stop reading). Per-agent session state. Cancel-timing distribution recorded per request. Report the cancelled fraction actually achieved. |
| **2. 32-token short-prompt saturation → RPS** | Prompts of exactly 32 tokens (upstream `generate_prompt` technique: re-encode until the length matches), small fixed output (e.g. 1–16 tokens, `ignore_eos`). Concurrency sweep and/or open-loop Poisson arrival-rate sweep. | Max sustainable RPS under a latency SLO (e.g. P99 e2e), the RPS-vs-latency curve, CPU utilization of front-half processes | Open-loop generator (avoids coordinated omission), saturation detection, CPU accounting per process |
| **3. Serverless cold start + host RAM** | (a) Front-half-only cold start with the backend already warm, or with the mock. (b) Full-stack cold start (dominated by weight load and CUDA graph capture in both stacks). | Process spawn → `/v1/models` 200. Spawn → first token. RSS/PSS (peak and steady state, idle and under load) **per process, split into front-half and backend groups** | Process-tree sampler (`/proc/<pid>/smaps_rollup` PSS on Linux), cold page cache option (drop caches) vs. warm, repeated N times. **The "ms cold start, under 500 MB" claim only applies to the front-half group.** The baseline's front group is the uvicorn API process plus the tokenizer/detokenizer process(es), each importing torch and transformers. |

---

## Feature Dependencies

```
Wire-exact ZMQ/msgpack codec
    └──requires──> Launcher shim (fixed addresses, readiness)        [real backend]
    └──requires──> Mock backend (same contract)                       [Mac dev]
                        └──enhanced by──> Python contract oracle (upstream codec)

Tokenizer parity ──┬──> Chat template parity ──┐
                   └──> Detokenizer parity ────┤
                                               ▼
Request-lifecycle FSM ──requires──> codec + tokenizer + detokenizer
    ├──> /v1/chat/completions (stream + non-stream) ──> /v1/models, /health
    ├──> Disconnect cancellation ──> Stop strings (reuse abort path)
    ├──> Correct finish_reason/usage
    ├──> /v1/completions, /v1/tokenize (cheap once the pipeline exists)
    └──> Front-side admission queue (optional)

Radix trie (library) ──requires──> differential oracle (upstream RadixPrefixCache on CPU)
    └──enhances──> Shadow prefix index ──enhances──> admission ordering / metrics / mock prefill cost

Parity harness ──requires──> FSM + endpoints + real backend (GPU) ──gates──> Benchmark harness
Benchmark harness ──requires──> load generator + launcher shim + metrics/RSS sampler
Prometheus /metrics ──enhances──> Benchmark harness (explains wins)

Front-side admission queue ──conflicts──> strict baseline-equivalent scheduling (must be labeled)
SSE token coalescing ──conflicts──> TTFT/TPOT comparability (compat mode forbids it)
Swapping backend radix cache ──conflicts──> "unmodified upstream" decision
```

### Dependency Notes

- **The codec comes first.** Every other component, and both backends (mock and real), sit behind it. The Python contract oracle lets it be proven byte-exact on the Mac before any GPU time.
- **Tokenizer, template and detokenizer parity can be proven entirely on the Mac**, by generating fixtures from upstream's own Python `TokenizeManager`/`DetokenizeManager` with `transformers` 4.56–4.57.3 (upstream's pinned range).
- **Stop strings depend on cancellation.** They are the same `AbortBackendMsg` path, plus text hold-back in the detokenizer output.
- **The radix trie is independent of the request path.** It can be built and proven in parallel. Its integration into the request path is only via the optional shadow index.
- **The parity gate comes before benchmarks.** A faster front half with different output proves nothing.

---

## MVP Definition

### Launch With (v1)

- [ ] Wire-exact ZMQ/msgpack codec, launcher shim, readiness signal: the foundation, and it de-risks the IPC boundary first, as PROJECT states.
- [ ] Mock backend (Rust) plus Python contract oracle: lets the whole front half be built on macOS.
- [ ] HF tokenization, chat template and detokenizer parity (Qwen3-0.6B canonical; verbatim `DetokenizeManager` port): Core Value parity.
- [ ] Request-lifecycle FSM with disconnect cancellation (streaming and non-streaming) and immediate abort: Scenario 1.
- [ ] `/v1/chat/completions` (stream and non-stream, `prompt` or `messages`), `/v1/models`, `/v1`, `/health`, `/health/ready`, sampling passthrough with upstream defaults, length validation and backend-silence timeout: baseline API surface plus hang prevention.
- [ ] Radix trie library, differentially proven against upstream `RadixPrefixCache` (page_size 1 and >1): PROJECT requirement, scoped honestly.
- [ ] Parity harness on the GPU machine (greedy, token ids and text): Core Value.
- [ ] Benchmark harness for the three scenarios with the Rust load generator, RSS/PSS sampler and run manifest: Core Value.
- [ ] Minimal `/metrics` (TTFT histogram, inflight, cancellations, abort latency): needed to explain results. Low cost.

### Add After Validation (v1.x)

- [ ] Stop strings: once parity and benchmark numbers exist. Trigger: client compatibility requests.
- [ ] Correct `finish_reason` and `usage`, `stream_options.include_usage`: trivial, but changes response bodies relative to the baseline.
- [ ] `/v1/completions`, `/v1/tokenize`, `/v1/detokenize`: cheap API breadth.
- [ ] Shadow radix index for prefix-aware admission ordering and cache-hit metrics: gives the Rust trie a live role.
- [ ] Front-side admission control (labeled benchmark variant).
- [ ] Shim probes for backend-side abort/queue latency.

### Future Consideration (v2+)

- [ ] Constrained decoding / structured-output FSM: explicitly v2 in PROJECT.
- [ ] Swapping the backend's radix cache for the Rust one (requires relaxing "unmodified upstream").
- [ ] `DecodeStream`-based detokenizer behind a flag, SSE coalescing variant.
- [ ] Tool calling, reasoning parsers, multi-backend cache-aware routing.

---

## Feature Prioritization Matrix

| Feature | User Value | Implementation Cost | Priority |
|---------|------------|---------------------|----------|
| ZMQ/msgpack codec + launcher shim | HIGH | MEDIUM | P1 |
| Mock backend + Python contract oracle | HIGH | MEDIUM | P1 |
| Tokenizer + chat template parity | HIGH | MEDIUM-HIGH | P1 |
| Detokenizer parity (verbatim port) | HIGH | MEDIUM | P1 |
| Lifecycle FSM + disconnect cancellation | HIGH | MEDIUM | P1 |
| `/v1/chat/completions` SSE + non-stream, `/v1/models`, health | HIGH | MEDIUM | P1 |
| Length validation + backend-silence timeout | HIGH | LOW | P1 |
| Radix trie + differential oracle | MEDIUM | MEDIUM-HIGH | P1 |
| Parity harness | HIGH | MEDIUM | P1 |
| Benchmark harness (3 scenarios, Rust loadgen, RSS sampler) | HIGH | HIGH | P1 |
| Minimal `/metrics` | MEDIUM | LOW | P1 |
| Batched backend sends | MEDIUM | LOW | P2 |
| Stop strings | MEDIUM | MEDIUM | P2 |
| Correct finish_reason / usage | MEDIUM | LOW | P2 |
| `/v1/completions`, `/v1/tokenize`, `/v1/detokenize` | LOW-MEDIUM | LOW | P2 |
| Shadow radix index for admission | MEDIUM | MEDIUM | P2 |
| Front-side admission queue | MEDIUM | MEDIUM | P3 |
| Shim backend probes | MEDIUM | MEDIUM | P3 |
| Tool calling / structured output / multi-backend routing | LOW (for this project) | HIGH | P3 / v2 |

---

## Competitor Feature Analysis

| Feature | mini-sglang (baseline, HIGH) | SGLang / sgl-model-gateway (Rust) (MEDIUM/LOW) | vLLM OpenAI server (MEDIUM) | TGI router (Rust) (MEDIUM) | Our Approach |
|---------|------------------------------|-----------------------------------------------|-----------------------------|----------------------------|--------------|
| OpenAI endpoints | `/v1/chat/completions` (+`prompt`), `/v1/models` | chat, completions, embeddings, tokenize/detokenize, responses, conversations | chat, completions, embeddings, tokenize/detokenize | chat, completions, plus native `/generate(_stream)` | chat + models (v1); completions and tokenize in v1.x |
| Streaming | SSE, one event per token, non-standard `object` | SSE, OpenAI-spec deltas | SSE, OpenAI-spec | SSE | SSE, OpenAI-spec chunks, one event per token in compat mode |
| Sampling params honored | temperature, top_k, top_p, max_tokens, ignore_eos | Broad (penalties, n, logprobs, seed, stop) | Broad | Broad, validated | Same as backend. Stop strings implemented in the front (v1.x). |
| Stop strings | Ignored | Yes | Yes (with text hold-back) | Yes | Front-side, via abort (v1.x) |
| Cancellation | Streaming only. 100 ms delay, 2 hops. | Tokio cancellation, upstream abort | Abort on disconnect | Abort on disconnect | Streaming and non-streaming, immediate, 1 hop |
| Tokenization location | Python process shared with detokenization | In-process Rust (gRPC mode), cached | Python (multi-process frontend options) | In-process Rust, validation workers | In-process Rust, blocking pool |
| Chat templates | transformers Jinja2 | Rust Jinja-style (HF-compatible) | transformers Jinja2 | minijinja | minijinja + pycompat, differential-tested |
| Incremental detokenization | surr/read offsets + U+FFFD hold-back | Buffered incremental | Incremental (fast path based on tokenizers' DecodeStream) | Incremental | Verbatim upstream algorithm (parity) |
| Prefix cache in front | None (cache lives in scheduler) | Approximate radix tree for cache-aware routing, periodic LRU eviction | None in front | None | Equivalence-proven trie library plus optional shadow index |
| Health / metrics | None | liveness, readiness, health_generate, Prometheus (40+ metrics) | `/health`, `/metrics` | `/health`, `/info`, `/metrics` | `/health`, `/health/ready`, `/metrics` (focused set) |

---

## Sources

- **Primary (HIGH):** `sgl-project/mini-sglang` source at commit `9a91cfafe754aa85daee49998176275667eb58f2` (2026-05-17), read directly: `server/api_server.py`, `server/launch.py`, `server/args.py`, `message/{backend,frontend,tokenizer,utils}.py`, `utils/mp.py`, `tokenizer/{server,tokenize,detokenize}.py`, `kvcache/{base,radix_cache}.py`, `kernel/{radix.py,csrc/src/radix.cpp}`, `scheduler/{scheduler,io,cache,prefill,decode,config}.py`, `engine/{config,sample}.py`, `core.py`, `env.py`, `benchmark/client.py`, `README.md`, `docs/features.md`, `docs/structures.md`, `pyproject.toml`.
- Qwen3-0.6B `tokenizer_config.json` and `generation_config.json` on Hugging Face (fetched): EOS `<|im_end|>`, `clean_up_tokenization_spaces=false`, no BOS, generation EOS list `[151645, 151643]`. The backend only stops on the single tokenizer EOS. (HIGH)
- [SGLang Model Gateway docs](https://docs.sglang.io/advanced_features/sgl_model_gateway.html): endpoints, Rust tokenization, cache-aware routing with approximate radix tree, health and Prometheus metrics. (LOW per the classify-confidence seam: summarized by a fetch model, and consistent with prior knowledge.)
- [HF tokenizers decoders / DecodeStream docs](https://huggingface.co/docs/tokenizers/main/api/decoders): incremental `step()` decoding exists in the Rust core. (LOW per the seam; used only to justify *not* depending on it for parity.)
- vLLM, TGI and minijinja feature claims come from prior knowledge (MEDIUM) and should be re-verified in phase research if a design depends on them.
- `clean_up_tokenization_spaces=true` for Llama-3.x is unverified (gated repo). Treat it as MEDIUM, and choose Qwen3 as the canonical model to avoid the issue.

---
*Feature research for: Rust front half of an LLM serving stack driving unmodified mini-sglang*
*Researched: 2026-10-02*
