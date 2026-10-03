# Pitfalls Research

**Domain:** Rust "front half" (ingress, request-lifecycle FSM, HF tokenization/detokenization, radix prefix cache) driving an unmodified Python/CUDA mini-sglang backend over ZMQ + MessagePack
**Researched:** 2026-10-02
**Confidence:** HIGH for upstream-behavior pitfalls (verified by reading mini-sglang source at HEAD `9a91cfa`, 2026-05-17); MEDIUM for ecosystem and benchmarking pitfalls (web sources plus established practice); LOW where marked.

> **Phase labels used below.** No ROADMAP exists yet, so these are the suggested phases. The roadmapper should map them onto the real phases.
> - **P1 Boundary**: pin upstream, launcher shim, msgpack/ZMQ protocol, golden fixtures, mock backend, baseline profiling spike
> - **P2 Tokenizer**: tokenization, chat template, and incremental detokenization parity
> - **P3 Lifecycle/Ingress**: HTTP/SSE ingress, request FSM, cancellation, backpressure
> - **P4 Radix**: Rust radix trie plus a differential harness against the Python implementation
> - **P5 GPU E2E**: end-to-end output parity on the remote GPU box
> - **P6 Benchmarks**: three-scenario harness, methodology, reporting

---

## Ground Truth: What the Upstream Boundary Actually Is

All of these were verified in the source. Every pitfall below rests on them.

| Fact | Where | Consequence |
|------|-------|-------------|
| Transport is **ZMQ PUSH/PULL over `ipc:///tmp/minisgl_{0..4}.pid=<launcher-pid>`**. It is not a "lock-free channel." | `utils/mp.py`, `scheduler/config.py`, `server/args.py` | Rust has to speak ZMQ. "Lock-free" can only describe Rust-internal queues. |
| Messages are msgpack **maps** tagged with the Python class name (`"__type__": "UserMsg"`). Tensors are encoded as `{"__type__":"Tensor","buffer":<bin>,"dtype":"torch.int32"}`. | `message/utils.py` | Rust has to emit maps (not rmp-serde's default arrays), `bin` (not int arrays), and exact class names. |
| Deserialization is `cls(**kwargs)`. | `message/utils.py` | **Any extra field crashes the scheduler** with a TypeError. Rust cannot add `seed`, `stop`, or any other field to `SamplingParams`. |
| Scheduler pulls `UserMsg` / `AbortBackendMsg` / `ExitMsg` / `BatchBackendMsg` and pushes **one `DetokenizeMsg(uid, next_token, finished)` per token**, batched per step. | `scheduler/io.py`, `scheduler.py` | Rust takes over the API server, tokenizer, and detokenizer processes, including incremental detokenization. |
| **No abort acknowledgement.** Abort for an unknown or finished uid is silently ignored. | `scheduler.py::_process_one_msg` | The FSM has to tolerate late tokens after abort and must never wait for an ack. |
| Input with `len > max_seq_len` is **dropped silently with no reply**. `max_tokens` is silently clamped. | `scheduler.py::_process_one_msg` | A naive Rust FSM waits forever for these requests and leaks a slot. |
| `max_seq_len = min(model max, KV num_tokens)` is computed at runtime from GPU memory. | `engine/engine.py` | Rust can't know the limit statically. It needs the shim to report it, or a watchdog. |
| The radix cache lives **inside the scheduler** and maps tokens to KV page indices. | `scheduler/cache.py`, `kvcache/radix_cache.py` | A Rust radix trie **cannot replace it** without patching upstream. It can only mirror it or be tested against it. |
| The upstream launcher spawns the API server, tokenizers, and scheduler together. Readiness goes through a `multiprocessing.Queue`. The bind/connect roles depend on `num_tokenizer`. | `server/launch.py` | "Unmodified upstream" still needs **our own Python launcher shim** that imports `Scheduler`. |
| Tokenize = `apply_chat_template(tokenize=False, add_generation_prompt=True)` then `encode(prompt)` with default `add_special_tokens=True`. | `tokenizer/tokenize.py` | Rust has to reproduce this exact two-step sequence, including its quirks such as a possible double BOS. |
| Detokenize = surr/read-offset `batch_decode` (default `skip_special_tokens=False`), a U+FFFD holdback, and a word-boundary holdback (`find_printable_text`). EOS is dropped only when `finished && token == eos_token_id`. | `tokenizer/detokenize.py` | Chunk boundaries are heuristic. Parity has to be defined on the final text, and the slicing is by Python code-point length. |
| Upstream supports **Linux only**: `sgl-kernel`, `flashinfer`, and `minisgl.kernel` AOT-compiled via tvm-ffi. | `README.md`, `kernel/radix.py` | The scheduler can't run on macOS even CPU-only. Python `RadixPrefixCache` imports a compiled kernel (`fast_compare_key`). |
| Version pins: `transformers>=4.56.0,<=4.57.3`, `torch<2.10.0`. Package version is a static `0.1.0`. | `pyproject.toml` | Pin by **git SHA** and an exact lockfile. `pip install minisgl==0.1.0` pins nothing. |
| Churn: protocol/core files changed in about 11 commits from Dec 2025 to Feb 2026 (cancellation added 2026-02-17). Radix timestamp semantics changed 2026-05-10 (#124). | upstream git log | The boundary is an internal API that is still moving. |

---

## Critical Pitfalls

### Pitfall 1: Treating "unmodified upstream" as a stable public API

**What goes wrong:**
The Rust front half is written against whatever upstream looks like today. A later `git pull` renames a message class, adds a required field, or changes bind/connect roles. The scheduler then dies with `KeyError`/`TypeError` on the first message. Because ZMQ PUSH just queues, the Rust side sees a silent hang, not an error.

**Why it happens:**
mini-sglang's process boundary is internal plumbing (`message/*.py`, `utils/mp.py`), not a documented protocol. Its serialization is Python-reflective (`__type__` = class name, `cls(**kwargs)`). There's no schema, version field, or handshake. The ZMQ addresses embed the launcher's PID (`.pid=<os.getpid()>`), and readiness is signalled over `multiprocessing.Queue`, which Rust can't read.

**How to avoid:**
- Pin upstream to an exact **commit SHA** (submodule) plus a lockfile for `transformers`, `tokenizers`, `torch`, `flashinfer`, `sgl_kernel`, and `pyzmq`. Treat any bump as a deliberate milestone task.
- Write a **thin Python launcher shim** (`rsglang_backend.py`) that imports upstream `Scheduler`/`ServerArgs` unmodified. It sets a deterministic `_unique_suffix`, pins `num_tokenizer` so bind/connect roles are known (the scheduler binds `minisgl_0`; who binds `minisgl_1` depends on `backend_create_detokenizer_link`), and signals readiness on a channel Rust can see (stdout line, ZMQ, or a file). Record in PROJECT.md that "unmodified" means **no patches to upstream files**. A launcher of our own is allowed.
- **Golden protocol fixtures:** a Python script at the pinned SHA uses upstream's own `serialize_type` + `msgpack.packb(use_bin_type=True)` to emit byte fixtures for every message type, including batch wrappers and the Tensor encoding. Rust tests must round-trip them byte-for-byte or semantically. CI regenerates fixtures at the pinned SHA and fails on any diff.
- Watch scheduler liveness: child-process exit, plus a timeout on the first token after the first send. A dead scheduler should surface as a 503, not a hang.

**Warning signs:**
Rust sends and nothing comes back. Scheduler stderr shows `KeyError: 'UserMsg'` or `TypeError: __init__() got an unexpected keyword argument`. Leftover `/tmp/minisgl_*` sockets have the wrong PID suffix. Fixture diff appears after a submodule bump.

**Phase to address:** P1 Boundary. This is the first thing built. Re-verify at every upstream bump.

---

### Pitfall 2: msgpack encoding that "looks right" but isn't upstream's encoding

**What goes wrong:**
rmp-serde serializes structs as **arrays** by default. `Vec<u8>` is serialized as an array of ints unless `serde_bytes` is used. Rust may send `input_ids` as a msgpack array instead of a Tensor map with `bin` LE int32 bytes. It may also send `temperature` as an int or add an extra field. Each of these crashes the scheduler process or makes it misbehave.

**Why it happens:**
Developers test Rust-to-Rust round trips (always pass), or test against a hand-written mock that is more lenient than upstream's `cls(**kwargs)`.

**How to avoid:**
Use `rmp_serde::to_vec_named` / `StructMapConfig`, `#[serde(with = "serde_bytes")]`, an explicit `__type__` tag field, and explicit little-endian `i32` packing with `dtype: "torch.int32"`. Validate against the golden fixtures (Pitfall 1) and against **upstream's own Python decoder** running in the mock (Pitfall 9). Handle both single messages and `Batch*Msg` wrappers on receive: the scheduler sends a bare `DetokenizeMsg` when a step has one reply and a `BatchTokenizerMsg` otherwise.

**Warning signs:**
The mock accepts a message that upstream's `BaseBackendMsg.decoder` rejects. `ValueError: Cannot deserialize type` appears in scheduler logs. Prompts arrive as float64 or the wrong length.

**Phase to address:** P1 Boundary.

---

### Pitfall 3: Tokenizer parity failure (Rust `tokenizers` vs Python `AutoTokenizer`)

**What goes wrong:**
Rust loads `tokenizer.json` from the model repo and calls `encode(text, true)`. The ids differ from Python for some inputs. Prefix-cache hits, output, and parity tests then all diverge. The usual sources:
1. **Load-time overrides.** transformers patches the backend tokenizer at load from `tokenizer_config.json`: `add_bos_token`/`add_eos_token` rewrite the post-processor for Llama-family fast tokenizers, and `legacy`/`add_prefix_space` change the pre-tokenizer or normalizer. Added-token flags also matter. So the on-disk `tokenizer.json` is not the effective tokenizer.
2. **Slow-only repos.** If a repo has only `tokenizer.model` (SentencePiece), AutoTokenizer converts it to a fast tokenizer at load time. Rust has nothing to load.
3. **Chat template rendering.** Python uses Jinja2 inside transformers' sandboxed environment (`trim_blocks`, `lstrip_blocks`, a custom `tojson`, `raise_exception`, `strftime_now`). minijinja without `minijinja-contrib` pycompat fails on `.startswith`, `.split`, `.strip`, `[::-1]`, and similar, or renders whitespace differently. Template lookup order also matters: `tokenizer_config.json` `chat_template`, `chat_template.jinja`, and upstream's own Mistral fallback to `chat_template.json` via `hf_hub_download`. So do default variables: Qwen3's `enable_thinking` is left undefined, which means thinking is on.
4. **Double BOS is upstream behavior.** Upstream renders the template (which already contains BOS for Llama-3) and then `encode`s with `add_special_tokens=True`, which may add a second BOS. Rust must **replicate this, not fix it**, or parity breaks.
5. **Version skew.** The Rust `tokenizers` crate version differs from the Python `tokenizers` wheel that transformers 4.57.x pulls in. Normalizer, pre-tokenizer, or added-token-split behavior has changed between releases.

**Why it happens:**
"Python's fast tokenizer *is* the Rust library" is only half true. transformers adds a Python layer on top, and that layer has behavior of its own.

**How to avoid:**
- **Export the effective tokenizer from Python.** At the pinned versions, run `AutoTokenizer.from_pretrained(...)` the same way upstream's `load_tokenizer` does, then dump `tok.backend_tokenizer.to_str()` and `tok.chat_template` (resolved string), `eos_token_id`, `bos_token`, `clean_up_tokenization_spaces`, and special-token maps into an artifact bundle. Rust loads that bundle, not the raw repo files.
- Pin the Rust `tokenizers` crate to the **same version** as the Python `tokenizers` wheel in the baseline lockfile.
- Use minijinja with `minijinja-contrib` pycompat, `trim_blocks`/`lstrip_blocks` matching transformers, and `raise_exception`/`strftime_now`/`tojson` shims. Compile the template once and cache it.
- **Differential corpus test, runnable on the Mac.** transformers and CPU torch install on macOS. Run upstream's `TokenizeManager` (or an equivalent call sequence) over a corpus: ASCII, CJK, emoji, combining marks, RTL, whitespace runs, literal special-token strings typed by users (`"<|im_end|>"` in user content), empty strings, very long inputs, and multi-turn chat with system/tool roles. Assert **exact id equality**. Gate merges on it.
- Start with **Qwen3-0.6B**, upstream's own benchmark model: `bos_token: null`, `add_bos_token: false`, `clean_up_tokenization_spaces: false`, so the fewest traps. Add a Llama-3.x model second, specifically to exercise BOS and cleanup behavior.

**Warning signs:**
Off-by-one prompt lengths. The first token differs only on chat requests. Radix hit rates differ between baseline and Rust for identical workloads. Divergence only on non-ASCII input.

**Phase to address:** P2 Tokenizer, with the corpus harness built at the start of P2 before any optimization. Re-run on the GPU box in P5.

---

### Pitfall 4: Incremental detokenization and UTF-8 / multibyte split bugs

**What goes wrong:**
A port of upstream's `DetokenizeManager` produces panics, garbled characters, duplicated or lost text, or different streamed text:
- Python `len()` counts **code points** and Rust `str::len()` counts **bytes**. A literal port of `read_str[len(surr_str):]` and `output_str[s.sent_offset:]` slices at the wrong place, or **panics** on a non-char boundary for CJK or emoji.
- Byte-level BPE splits one character across tokens, so a partial decode yields U+FFFD. Upstream holds back text that ends in `"�"`. If a model legitimately emits U+FFFD, the stream stalls until the next token.
- Upstream assumes `decode(surr)` is a prefix of `decode(surr + read)`. That's false for SentencePiece leading-space (`▁`) handling and some byte-fallback cases. Python silently slices garbage. Rust panics or diverges.
- `find_printable_text` holds back the trailing partial word (anything after the last space) unless the text ends in `\n` or a CJK character. Chunk boundaries therefore depend on this heuristic. With tokenizers' own `DecodeStream`, the final text is the same but the **chunks differ**.
- On finish, text still held behind a trailing U+FFFD can be dropped permanently. That's an upstream quirk that parity tests will hit.
- `clean_up_tokenization_spaces` (applied by transformers' Python `decode`, not by the Rust core) removes spaces before punctuation for models whose config sets it, reportedly Llama-3.x (MEDIUM confidence). Rust `tokenizers::decode` doesn't do this.
- EOS handling: upstream drops only `eos_token_id` and only when `finished`. Other stop tokens (for example `<|endoftext|>` vs `<|im_end|>`) are streamed as text because `skip_special_tokens=False`.

**Why it happens:**
The algorithm looks like simple string slicing, and the bugs only show up on non-ASCII text or particular token splits.

**How to avoid:**
- Track offsets in **token-index space**, compute string deltas with char-boundary-safe operations, and never index `&str` with offsets that came from another string. Write the port as a pure function `(state, token, finished) -> (state', delta)` so it can be property-tested.
- Define parity at two levels. **(a) Required:** the concatenated final text equals Python's for the same token-id stream. **(b) Optional:** chunk-boundary equality, only if the Rust port replicates `find_printable_text` exactly. Pick (a) as the gate.
- **Mac-runnable differential test:** feed identical recorded token-id streams (including ones that split multi-byte characters, plus random-id fuzzing over the vocab) into upstream's `DetokenizeManager` and the Rust port. Compare outputs. Add `proptest`: for any id sequence, the concatenated deltas equal `decode(all_ids)` modulo the documented holdback and EOS rules.
- Apply `clean_up_tokenization_spaces` in Rust when the exported config says so, using transformers' exact replacement list.

**Warning signs:**
`byte index N is not a char boundary` panics. Text has doubled spaces or missing first characters. Tests pass on English and fail on Chinese or emoji. The Rust stream ends shorter than `tokenizer.decode(ids)`.

**Phase to address:** P2 Tokenizer.

---

### Pitfall 5: Cancellation races (late tokens, uid reuse, leaked slots, double release)

**What goes wrong:**
Under Scenario 1 (128 agents with dynamic cancellations):
- **Late tokens after abort.** There's no ack. With overlap scheduling, the in-flight batch still produces a `DetokenizeMsg` for the aborted uid (possibly with `finished=true`), and messages already queued in ZMQ also arrive. An FSM that deletes state on abort either panics on an unknown uid or, worse, delivers the text to a new request if **uids are reused**.
- **Abort overtakes submit.** Rust might use more than one PUSH socket, or separate tasks for tokenized submits and aborts. An `AbortBackendMsg` can then reach the scheduler before its `UserMsg`. The scheduler ignores the abort and runs the request to `max_tokens`, wasting GPU time and skewing the benchmark.
- **Abort during tokenization** (before the `UserMsg` is sent). The request must be cancelled locally and never sent.
- **Abort after finish.** This is harmless upstream, but the Rust FSM must not transition Finished to Cancelled or release its own permit or semaphore twice.
- **Silent drop.** Over-length inputs never get a reply, so the request sits in Prefill forever.
- **Upstream race under heavy abort (MEDIUM/LOW, unverified on GPU).** In overlap mode an abort is processed (`_free_req_resources`) while that request's batch is still in flight. `_process_last_data` may then `cache_req` or free it again, and `finished_reqs` only guards against the previous step. Upstream's own frontend delays aborts by `asyncio.sleep(0.1)` and only notices disconnects between chunks, so the window rarely opens. A fast Rust frontend opens it much more often. `CacheManager.check_integrity()` raises on idle if pages leaked.
- **Refcount underflow in the Rust radix mirror.** Unlocking a cache handle twice on the cancel path makes `u32` wrap in release builds.

**Why it happens:**
Cancellation is a distributed-systems problem: two processes, no ack, overlap pipelining. It gets modelled as a local state flip.

**How to avoid:**
- Use **monotonic u64 uids, never reused.** Keep a tombstone set (or a uid watermark) so late messages for dead uids are dropped silently, the same way upstream's `if msg.uid not in ack_map: continue` does.
- A **single ordered writer task** owns the one PUSH socket. Submits and aborts both go through it, which guarantees FIFO order.
- Make the FSM explicit and exhaustive: `Queued -> Tokenizing -> Submitted(Prefill) -> Decoding -> {Finished | Cancelled}`. Add a `CancelRequested` substate that still drains late tokens, and make terminal transitions idempotent. Use a typed enum with `match`, not booleans. Release every per-request resource (semaphore permit, channel sender, radix-mirror lock) in exactly one place, preferably an RAII guard whose `Drop` does it.
- Pre-validate prompt length against the backend's `max_seq_len` (reported by the shim at startup) and add a no-first-token watchdog.
- **Cancellation stress test on the mock and on the GPU.** Run random abort timing (before submit, during prefill, mid-decode, at the finishing token, after finish) for N thousand requests. Afterwards assert that FSM live count == 0, the radix mirror's refcounts are all zero, and the scheduler's idle `check_integrity` didn't raise. If the upstream race reproduces, document it. Don't patch upstream. Optionally rate-limit abort timing, but only if the baseline does the same.

**Warning signs:**
Memory or live-request count grows over a long Scenario 1 run. Text from a cancelled stream appears in another stream. GPU throughput stays high after all clients disconnect. Scheduler logs `Cannot evict` or `integrity check failed`. Requests hang at exactly the context limit.

**Phase to address:** P3 Lifecycle/Ingress (design and mock stress test). Repeat in P5 GPU E2E against the real scheduler.

---

### Pitfall 6: Unbounded queues and wrong backpressure topology

**What goes wrong:**
- One ZMQ PULL reader task fans tokens out to per-request SSE senders. If those channels are **unbounded**, a slow or stalled client buffers tokens without limit and memory grows. If they're **bounded with blocking `send().await`**, one slow client blocks the single reader and **head-of-line blocks every other stream**. That causes exactly the P99 TTFT blow-up Scenario 1 is meant to show Rust avoids.
- libzmq sockets are not thread-safe, and blocking `send`/`recv` on a tokio worker stalls the runtime. ZMQ PUSH blocks or returns `EAGAIN` at SNDHWM (default 1000) when the scheduler is slow or not yet bound.
- No admission control at the front means everything piles into the scheduler's own unbounded `pending_list`, and TTFT grows without bound under overload.
- Tokenization is CPU-heavy (long prompts, chat templates). Running it inline in async tasks starves the reactor. Tokenizers' internal rayon parallelism then oversubscribes cores alongside tokio workers.

**Why it happens:**
Tokio examples use `unbounded_channel` for convenience, and "lock-free" is read as "unbounded."

**How to avoid:**
- Per-request output is **append-only text**, so coalesce instead of blocking or dropping. Each request gets a small bounded channel (or a `Mutex<String>` + `Notify`). The reader does `try_send`, and when that fails it appends to a pending buffer flushed on the next notify. The reader never awaits a client.
- Dedicate OS threads (or `spawn_blocking`) to libzmq sockets, one owner per socket, bridged to tokio with bounded channels. Alternatively evaluate the pure-Rust async `zeromq` crate, but verify its `ipc://` interop against pyzmq first (MEDIUM confidence on its maturity).
- Run tokenization on a bounded rayon or blocking pool with an explicit size, and set `TOKENIZERS_PARALLELISM`/`RAYON_NUM_THREADS` deliberately.
- Add admission control (a semaphore on in-flight requests) as a **separately reported** feature. For like-for-like benchmarks, leave it off or set it to the baseline's effective limit (see Pitfall 11).

**Warning signs:**
RSS climbs during runs with slow consumers. One stalled `curl` raises every other stream's inter-token latency. tokio-console shows long poll times. CPU sits at 100% on one core while P99 rises.

**Phase to address:** P3 Lifecycle/Ingress (topology decided in P1 Boundary when the IPC pump is designed).

---

### Pitfall 7: Expecting the Rust radix cache to change backend behavior ("radix equivalence" when KV ownership stays in Python)

**What goes wrong:**
The team builds a Rust radix trie and expects prefix-cache hits or eviction to improve. But the **real** cache lives inside the Python scheduler and holds KV page indices. Every `UserMsg` carries the full `input_ids`, and the scheduler does its own `match_prefix`/`insert_prefix`/`evict`. A Rust trie in the front half can't free or pin KV pages and can't observe the scheduler's evictions, since no eviction events cross the boundary. A "shadow" trie drifts from reality within minutes. If it's used for routing or admission it makes wrong decisions, and it buys no benchmark win.

**Why it happens:**
The RFC lists the radix cache as a front-half module, but in mini-sglang it is inseparable from KV memory management, which is explicitly out of scope.

**How to avoid:**
- State the v1 goal precisely in REQUIREMENTS: **"A standalone Rust radix trie that is behaviorally equivalent to `RadixPrefixCache` under a differential op-trace harness."** It is not used on the serving hot path unless a hot-path use case is defined (for example, prefix-hit estimation for logging or future cache-aware routing). Don't claim it contributes to benchmark wins.
- Semantics to replicate exactly:
  - page-size alignment (`align_down`)
  - `key_fn` (the first token if `page_size == 1`, otherwise a tuple of the first page)
  - matching on `input_ids[: len-1]`
  - `split_at` inheriting `ref_count` and the timestamp, then refreshing to the walk's `tic` (changed upstream in #124)
  - root `ref_count = 1`
  - `evictable_size`/`protected_size` accounting
  - eviction collects only leaves with refcount 0, re-pushes a parent that becomes a leaf, and **may overshoot** the requested size
- **Ties:** upstream uses `heapq` with `__lt__` on `timestamp` only, and every node touched in one walk shares the same `tic`. The eviction order among ties therefore depends on DFS collection order plus heap internals. Either replicate that order exactly (same DFS stack order, a heapq-compatible binary heap) or specify the tie-breaker and compare **evicted sizes plus invariants** instead of exact node sets.
- **Clock:** upstream uses `time.monotonic_ns()`. The differential harness must monkeypatch it to a logical clock in Python and inject the same clock in Rust.
- **Harness portability:** Python `RadixPrefixCache` imports `minisgl.kernel.fast_compare_key` (a tvm-ffi AOT C++ module) and `get_global_ctx()`, and `minisgl.kernel/__init__` imports pynccl and Triton. On macOS, inject a pure-Python stub module for `minisgl.kernel` via `sys.modules` and set a minimal global context. This is test-only and doesn't modify upstream. Otherwise run the harness on the Linux box.
- In Rust, use an arena or slotmap with indices for parent/child links, not `Rc<RefCell<>>` cycles. Use checked refcount arithmetic and panic on underflow.

**Warning signs:**
Someone proposes "send only the uncached suffix" (which requires an upstream change). Radix work starts appearing in benchmark attribution. Differential tests are flaky on eviction (a tie-order problem). Tests pass only with `page_size=1`.

**Phase to address:** P4 Radix (scope statement in REQUIREMENTS before P4 begins).

---

### Pitfall 8: Output-parity testing that nondeterminism makes flaky or meaningless

**What goes wrong:**
"Identical output to the Python baseline" is tested by firing the same concurrent workload at both stacks and diffing text. The tests fail randomly, or someone loosens the comparison until it proves nothing.

**Why it happens:**
- The default chat endpoint uses `temperature = 1.0` (`api_server.py`). The benchmark client explicitly sends `0.0`.
- Non-greedy sampling uses FlashInfer with only a global `torch.manual_seed(42)`, so there's no per-request seed. A seed field can't be added without crashing the scheduler (Pitfall 2).
- Greedy (`argmax`) is deterministic only for a **fixed batch composition**. The logits from RMSNorm, matmul, and attention kernels change with batch size, CUDA-graph padding, and chunked-prefill splits (Thinking Machines: batch non-invariance, not atomics). The Rust front half changes arrival timing, which changes the batches, which in bf16 legitimately changes greedy outputs after some token.
- Prefix-cache state from earlier runs changes the prefill split, which also changes numerics.

**How to avoid:**
Use a **parity ladder** with an explicit gate at each level:
1. **Tokenization ids exact** (Mac, no GPU). Hard gate.
2. **Detokenization text exact** for identical id streams (Mac). Hard gate.
3. **Protocol exact.** The `UserMsg` bytes the Rust side sends decode, via upstream's decoder, to the same `input_ids`/`SamplingParams` as the Python tokenizer produces. Hard gate.
4. **End-to-end greedy, sequential** (one request at a time, `temperature=0`, `top_k=-1`, `top_p=1.0`, fresh backend process, same `max_tokens`) on the GPU. Text must match exactly. Hard gate.
5. **End-to-end greedy, concurrent.** Report the exact-match rate and the first-divergence token index distribution against the baseline under the same load. Not a gate; divergence after a shared prefix is expected.
6. Sampling (`temperature>0`): distribution-level sanity checks only, such as length distribution and perplexity. Never exact-text checks.

**Warning signs:**
Parity tests pass locally and fail on CI. Mismatches only appear when concurrency > 1. Someone proposes "fuzzy" or BLEU-based parity for level 4.

**Phase to address:** Levels 1–3 in P2 Tokenizer and P1 Boundary. Levels 4–6 in P5 GPU E2E. The ladder itself should be written into REQUIREMENTS so "identical output" has an operational definition.

---

### Pitfall 9: The mock backend is more forgiving than the real scheduler

**What goes wrong:**
The whole Rust front half is developed on the Mac against a mock that decodes leniently, replies to every request, emits tokens only before abort, never batches replies, never drops or clamps, and answers instantly. Everything passes, then breaks on the first real GPU run.

**Why it happens:**
Upstream can't run on macOS, so the mock is written from a reading of the code rather than from the code.

**How to avoid:**
- Write the mock **in Python, reusing upstream's `minisgl.message` and `minisgl.utils.mp` modules** (they import only torch CPU, numpy, msgpack, and pyzmq, all installable on macOS). That way it decodes with upstream's exact `cls(**kwargs)` strictness and replies with upstream's exact encoder.
- Make the mock **adversarial by configuration**:
  - batched `BatchTokenizerMsg` replies of varying size
  - one or two late tokens after an abort (simulating the overlap batch)
  - silent drop when the prompt exceeds a configured `max_seq_len`
  - `max_tokens` clamping
  - EOS emission with `finished=true`
  - configurable per-step latency (prefill proportional to length, decode per step)
  - bursty stalls
- A mock-conformance checklist, derived from the Ground Truth table above, is reviewed at every upstream bump.
- Optionally add a Rust in-process mock for fast unit tests. The Python mock is the contract mock.

**Warning signs:**
The first GPU run surfaces issues the mock never produced (hangs on long prompts, panics on unknown uids). The mock's code has no imports from `minisgl`.

**Phase to address:** P1 Boundary (built together with the protocol). Adversarial modes are extended in P3.

---

### Pitfall 10: Host overhead is not the bottleneck, so the wins don't materialize

**What goes wrong:**
After all the work, Scenario 1/2 numbers are within noise of the baseline, or only better against a strawman configuration.

**Why it happens:**
- mini-sglang already runs tokenization and detokenization in **separate processes** and uses **overlap scheduling**, which hides scheduler CPU time behind the GPU.
- The Python scheduler loop is untouched: batch building, Python radix ops, msgpack decode plus dataclass construction under the GIL on the scheduler side. The Rust front half doesn't remove that cost; the boundary's Python half is unchanged.
- With larger models (14B/32B) GPU time dominates every scenario.

Where Rust plausibly wins:
- With default `num_tokenizer=0`, **one Python process does both tokenize and detokenize** for all streams. New-prompt tokenization queues behind per-token detokenization of 128 streams.
- FastAPI/uvicorn per-chunk SSE overhead.
- Four ZMQ hops (API, tokenizer, scheduler, detokenizer, API) shrink to two (Rust, scheduler, Rust).
- The deliberate 100 ms abort delay and lazy disconnect detection upstream.
- Front-half RSS: each Python front-half process imports torch and transformers under `spawn`, so there's no copy-on-write sharing.

**How to avoid:**
- **Spike first, in P1:** on the GPU box, run Scenario 1/2 load against the *baseline* and profile every process (`py-spy record --subprocesses`, per-process CPU%). Quantify time spent in API server, tokenizer, and detokenizer vs scheduler vs GPU idle gaps (Nsight Systems/nvtx: upstream has `nvtx_annotate`). If front-half processes are not near saturation and the GPU shows no idle bubbles caused by input starvation, rescope the claims **before** building P3–P4.
- Pick the regime deliberately and disclose it: a small model (Qwen3-0.6B, upstream's own benchmark model), short prompts and outputs, high concurrency. Optionally add `--dummy-weight` runs that isolate host overhead, labelled as such.
- Phrase success criteria as "measured delta with CI under stated conditions," not as RFC projections such as 30% TTFT or 20–40% RPS. PROJECT.md already says this; keep it enforced in REQUIREMENTS.

**Warning signs:**
The profile shows the scheduler process at 100% CPU and the front half idle. GPU utilization is already ~100% in the baseline. Gains appear only against `num_tokenizer=0`.

**Phase to address:** P1 Boundary (spike), with formal measurement in P6 Benchmarks. **The roadmap should put this spike before P3/P4 investment.**

---

### Pitfall 11: Benchmark methodology mistakes

**What goes wrong:**
The numbers are unreproducible, biased toward Rust, or simply wrong. Specific traps:
- **Coordinated omission.** Scenario 1 ("128 agents") is inherently closed-loop, since agents wait for responses. Under a stall, the closed-loop generator stops offering load and hides the tail. Scenario 2 (RPS saturation) measured closed-loop just reports the client's own concurrency limit.
- **Upstream's client as judge.** In `benchmark/client.py`, `tics[0]` is taken **after `chat.completions.create()` returns**, i.e. after response headers. Its "TTFT" is the first SSE chunk even though upstream's first chat chunk can carry only `role` and no content (and `find_printable_text` can make early `incremental_output` empty). A Rust server that sends headers at a different moment, or skips empty chunks, shifts "TTFT" without any real change. Trace replay measures from the actual send time, not the scheduled time.
- **Client on the same host** competes for exactly the CPU cores whose overhead is being measured. A Python `openai` async client parsing 128 streams can itself become the bottleneck.
- **Unfair baseline:** default `num_tokenizer=0` vs a multi-threaded Rust tokenizer pool; differing uvloop/logging/env; different `max_running_requests`, CUDA-graph batch size, `memory_ratio`, or model revision; a different `tokenizers` version.
- **Cache state leakage:** upstream `RadixPrefixCache.reset()` raises `NotImplementedError`. Running the baseline then Rust on the same backend process gives the second run a warm prefix cache.
- **Warmup:** the first requests pay for FlashInfer JIT, lazy imports, allocator growth, and an empty radix cache. That's fine to discard for steady-state scenarios. **Not** discarding it is the whole point of Scenario 3.
- **P99 from too few samples:** 128 agents × a few requests gives fewer than 20 samples above P99. Single runs also carry thermal and GPU-clock variance.
- **Survivorship bias:** Rust admission control or timeouts rejecting requests lowers the P99 of the requests that survived.

**How to avoid:**
- Use a **purpose-built load generator** (Rust or Go; `oha`/`vegeta`-style or a custom tokio client). Validate its headroom first by pointing it at the instant-reply mock and confirming it sustains well above the target rate. Report latency from the **scheduled** send time. Scenario 2 uses open-loop Poisson arrivals at several rates and reports the throughput-latency curve and knee, not a single number. Scenario 1 stays closed-loop by definition, but log offered load and include think-time.
- **One TTFT definition, applied identically to both stacks:** from request-send (or scheduled) time to the first SSE event whose `delta.content` is non-empty. Also report time-to-headers separately.
- Pin CPU cores (`taskset`/cgroups: scheduler, front half, client on disjoint cores), lock GPU clocks (`nvidia-smi -lgc`) where permitted, and run the client on separate cores at minimum. Optionally run a second configuration with the client on another host.
- Compare against the **best-configured baseline** (`--num-tokenizer` swept over 0/1/2/4) as well as the default. Report both. Keep identical upstream SHA, flags, env, and lockfile, recorded in each result's metadata.
- **Restart backend processes between runs.** Interleave A/B runs (ABAB…, at least 5 repetitions), report medians with bootstrap CIs, and collect at least about 1000 requests per configuration for P99. Include errors, timeouts, and rejections in the denominator.
- **Cold start (Scenario 3):** define it as phases: process exec, HTTP listening, tokenizer loaded, backend-ready signal, first token. The Rust claim applies to **front-half readiness** (and optionally front-half restart with a warm backend). End-to-end cold start is dominated by torch import, CUDA context, weight load, and CUDA-graph capture in the unchanged Python scheduler, so "millisecond cold start" end-to-end is not achievable and must not be claimed. **Host RAM:** measure PSS (`/proc/<pid>/smaps_rollup`) summed over front-half processes only (Rust binary vs Python API server + tokenizer + detokenizer processes), with the scheduler excluded from both or included in both.

**Warning signs:**
Rust's P99 TTFT improves but the error count rises. Results flip between runs. The client process sits at 100% CPU. "TTFT" is under a millisecond (it's measuring headers or empty chunks). The baseline was run once, at the start of the day.

**Phase to address:** P6 Benchmarks, but the **TTFT definition, load-generator choice, and baseline-config matrix** should be fixed in P1 so the spike (Pitfall 10) already uses them.

---

### Pitfall 12: Developing on macOS and inferring Linux behavior

**What goes wrong:**
Latency, memory, or IPC conclusions drawn on the Mac don't hold on the Linux GPU box, or the Mac setup fails in ways Linux doesn't.

**Why it happens / specifics:**
- No CUDA, and upstream refuses macOS entirely. Only the mock runs locally (Pitfall 9).
- `ipc://` UNIX socket path limit is **104 bytes on macOS vs 108 on Linux**. `/tmp` symlinks to `/private/tmp`, and `$TMPDIR` paths are long. Stale socket files remain after crashes.
- The default `ulimit -n` on macOS (often 256) is hit by 128 agents plus client sockets plus ZMQ fds, causing `EMFILE` that never happens on Linux.
- Allocators differ (macOS libmalloc vs glibc ptmalloc), and RSS semantics differ (compressed memory, `phys_footprint`). Timer coalescing and App Nap inflate latency on laptops.
- Architectures differ (aarch64 Mac vs likely x86_64 box). Builds of `onig`/C dependencies differ. Both are little-endian, but encode tensor bytes explicitly as LE anyway.

**How to avoid:**
Use the Mac for correctness only (unit, differential, and mock-integration tests). Make **every performance and memory claim on Linux** with the same binary build profile. Use a fixed, short ZMQ socket directory configurable by environment variable. Raise `ulimit -n` in dev scripts. Choose one global allocator explicitly (for example mimalloc or jemalloc) and use it on both platforms so the allocator is not a hidden variable. Run CI on Linux for the mock-integration suite as well as macOS.

**Warning signs:**
"Works on Mac" bugs on first Linux run. Benchmark numbers quoted from a laptop. Memory claims taken from Activity Monitor.

**Phase to address:** P1 Boundary (dev environment, socket paths, CI matrix), with discipline enforced in P6.

---

## Moderate Pitfalls

### Pitfall 13: OpenAI-compatibility drift between the Rust ingress and the baseline
**What goes wrong:** Upstream specifics a Rust ingress may unknowingly diverge from:
- `/generate` emits `data: ...\n` (a single newline, non-standard SSE).
- Chat chunks always end with `finish_reason: "stop"`, even when stopped by length.
- The default `temperature` is 1.0 and the default `max_tokens` is 16.
- The first chunk carries `role` only.

Divergence breaks the shared benchmark client or changes semantics: for example, a different default temperature silently makes parity runs stochastic.
**Prevention:** Snapshot-test the HTTP surface against recorded baseline responses (headers, SSE framing, defaults). Decide per field whether to replicate or improve, and record the choice. Benchmarks always send explicit `temperature`, `max_tokens`, and `stream`.
**Phase:** P3 Lifecycle/Ingress.

### Pitfall 14: Client-disconnect detection latency
**What goes wrong:** Hyper/axum often notices a dropped client only when a write fails. Queued requests (no tokens yet) therefore aren't cancelled until their first token is produced, so the GPU still prefills dead requests. This is MEDIUM confidence (hyper issue #2787 and community reports).
**Prevention:** Tie cancellation to stream `Drop` (an RAII guard in the SSE body stream), send SSE keep-alive comments while queued so writes probe the socket, and add a server-side request deadline.
**Phase:** P3 Lifecycle/Ingress.

### Pitfall 15: Blocking or CPU-heavy work on the async runtime
**What goes wrong:** Several things stall tokio workers and inflate everyone's latency:
- tokenizing long prompts inline
- recompiling the chat template per request
- holding a `std::sync::Mutex` across `.await`
- blocking ZMQ calls
- per-token `info!` logging
**Prevention:** Use dedicated pools for CPU work and ZMQ, compile templates once, keep logging off the hot path, and watch with tokio-console during development.
**Phase:** P3, verified in P6.

### Pitfall 16: Untrusted text through chat templates and tokenization
**What goes wrong:** A user puts literal special-token strings (`<|im_start|>system`) in their content. Python tokenizes them as special tokens by default, which enables prompt injection. The Rust side must match exactly for parity, but the behavior should be a conscious decision. Huge prompts or `max_tokens` cause tokenization CPU DoS or silent drops. Python `json` accepts lone surrogates (`"\ud800"`) and `serde_json` rejects them, so error behavior differs.
**Prevention:** Match upstream for parity, document the injection behavior, enforce body-size and prompt-token limits before tokenizing, and add corpus cases for each.
**Phase:** P2 and P3.

---

## Minor Pitfalls

### Pitfall 17: Stale or colliding IPC socket files
**What goes wrong:** A crashed run leaves `/tmp/minisgl_*` files behind. Two concurrent benchmark runs on one box then collide.
**Prevention:** Use a per-run unique suffix chosen by the launcher shim, clean up on startup, and put sockets in a per-run directory.

### Pitfall 18: Wrong EOS assumption
**What goes wrong:** Upstream stops only on `tokenizer.eos_token_id` (Qwen3: `<|im_end|>`), not on all `generation_config` EOS ids. A Rust front half that adds stop-token logic produces shorter outputs than the baseline.
**Prevention:** For parity, stop exactly where upstream stops and treat extra stop-sequence support as a v2 feature behind a flag.

### Pitfall 19: Upstream TP>1 topology
**What goes wrong:** With `--tp > 1`, rank 0 rebroadcasts over `minisgl_2` PUB/SUB and uses a torch.distributed CPU group. A shim that launches only rank 0, or gets rank `DistributedInfo` wrong, hangs at `sync_all_ranks`.
**Prevention:** Keep v1 at TP=1 unless needed. If TP>1 is required, reuse upstream's per-rank launch pattern verbatim in the shim.

---

## Technical Debt Patterns

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| Load raw `tokenizer.json` from the HF repo instead of the Python-exported effective tokenizer | Skips the export step | Silent parity breaks on models with load-time overrides | Never for the parity gate. Fine for exploratory spikes. |
| Hand-written Rust mock instead of a Python mock reusing `minisgl.message` | No Python on the dev box | Mock drifts from upstream strictness, so bugs show up only on the GPU | Only as an additional fast unit-test mock |
| Use `tokenizers::DecodeStream` instead of porting upstream's detokenizer | Less code, robust UTF-8 handling | Different chunk boundaries; final-text parity still needs checking | Acceptable if parity is defined on final text (recommended) and documented |
| Unbounded tokio channels everywhere | Simple code | Memory blowup and hidden backpressure. Breaks Scenario 1 claims. | Never on per-request output paths |
| Run the upstream benchmark client unmodified | Zero harness work | TTFT measured from headers, Python client bottleneck, coordinated omission | Only for a smoke test, never for reported numbers |
| Rust radix trie as a "shadow" of the scheduler's cache | Looks like integration | Drifts without eviction events; misleading decisions | Never without an explicit sync mechanism (which needs upstream changes, out of scope) |
| Pin upstream by pip version | Easy install | `0.1.0` never changes, so effectively unpinned | Never. Pin by SHA. |

## Integration Gotchas

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| pyzmq ↔ Rust ZMQ | Bind/connect role mismatch (both connect, so silent hang). PID-suffixed addresses guessed wrong. | Shim fixes `_unique_suffix` and `num_tokenizer`. Rust reads addresses from the shim's handshake. |
| Python msgpack ↔ rmp-serde | Struct-as-array, ints instead of `bin`, extra fields | `to_vec_named`, `serde_bytes`, golden fixtures, upstream decoder in the mock |
| transformers ↔ `tokenizers` crate | Assuming identical behavior; version skew | Export effective tokenizer and template from Python. Pin crate version = wheel version. |
| Jinja2 ↔ minijinja | Missing pycompat methods, different whitespace control, `tojson` differences | minijinja-contrib pycompat, match `trim_blocks`/`lstrip_blocks`, render-diff corpus vs Python |
| Upstream scheduler readiness | Sending before the scheduler binds (messages queue to HWM, then block) | Wait for the shim's readiness signal. Bound the send queue and time out. |
| Upstream silent drops | Waiting forever on over-length prompts | Pre-validate against the shim-reported `max_seq_len`, plus a watchdog |

## Performance Traps

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| Single ZMQ reader blocked by one slow SSE client | All streams' ITL spikes together | Non-blocking fan-out with per-request text coalescing | One slow client at any concurrency |
| Tokenization on tokio workers | P99 TTFT rises with prompt length across all requests | Dedicated bounded CPU pool | Long prompts (over ~4k tokens) or chat-heavy load |
| Global `Mutex<HashMap<uid, State>>` held across awaits | Lock contention, tail latency | Sharded map or actor-per-request, no locks across `.await` | Roughly 128+ concurrent with per-token updates |
| Per-token JSON + SSE + log allocations | High CPU in the front half at high decode rates | Pre-sized buffers, no hot-path logging | ~128 streams × hundreds of tok/s |
| Benchmark client on shared cores | Both stacks look alike (the client is the bottleneck) | Core pinning, validated client headroom | Scenario 2 saturation |
| Radix trie with `Rc<RefCell>` and recursion | Leaks or stack overflow on deep tries | Arena indices, iterative walks | Very long shared prefixes |

## Security Mistakes

| Mistake | Risk | Prevention |
|---------|------|------------|
| Exposing the ingress without body or prompt-length limits | Tokenization CPU DoS, memory exhaustion | Byte and token limits before tokenizing. Bounded in-flight requests. |
| IPC sockets in world-writable `/tmp` | A local user injects msgpack into the scheduler (aborts or forged requests) | Per-run directory with 0700 permissions (local-only threat, LOW priority for a learning project) |
| Literal special tokens in user content parsed as control tokens | Prompt/role injection | Match upstream for parity but document it. Optional v2 escaping flag. |
| Logging full prompts and outputs at info level | Data leakage plus a perf hit | Log ids and lengths only by default |

## UX Pitfalls

| Pitfall | User Impact | Better Approach |
|---------|-------------|-----------------|
| A hung request on over-length input (copied from upstream's silent drop) | Client waits forever | Return 400 with a clear message (document it as a deliberate divergence from baseline) |
| `finish_reason` always `"stop"` | Clients can't tell truncation from completion | Replicate for parity mode. Return correct `"length"` in a documented improved mode. |
| Empty-content first chunks | Clients and benchmarks miscount TTFT | Skip empty-content chunks or define TTFT on non-empty content (both stacks the same) |
| No 503 when the backend died | Silent hangs | Liveness-aware health endpoint and fast-fail |

## "Looks Done But Isn't" Checklist

- [ ] **Protocol:** often missing batch-wrapper decoding and the Tensor dtype string. Verify by round-tripping every golden fixture, including `BatchTokenizerMsg` with 1 vs N entries.
- [ ] **Launcher shim:** often missing deterministic addresses and a readiness signal. Verify that a cold start with no sleeps never sends before ready.
- [ ] **Tokenizer:** often tested only with plain text. Verify the chat-template path, multi-turn, system and tool roles, literal special tokens, CJK and emoji, and Llama-3 BOS behavior.
- [ ] **Detokenizer:** often tested only on ASCII. Verify fuzzed id streams that split multi-byte characters, final text vs `decode(all_ids)`, and EOS-drop semantics.
- [ ] **Cancellation:** often tested only mid-decode. Verify abort before submit, during tokenization, during prefill, at the finishing token, after finish, and a double abort. Check zero leaked FSM entries afterwards.
- [ ] **Backpressure:** often tested only with fast clients. Verify that one deliberately stalled client doesn't change other streams' ITL.
- [ ] **Radix:** often tested only with `page_size=1` and no ties. Verify `page_size>1`, split refresh semantics (#124), eviction ties, and refcount invariants after the cancellation stress run.
- [ ] **GPU parity:** often run once. Verify sequential greedy exact match across at least 100 diverse prompts on a fresh backend.
- [ ] **Benchmarks:** often a single run with upstream's client. Verify the validated load generator, the TTFT definition, the baseline config matrix, ABAB runs, CIs, and errors in the denominator.
- [ ] **Cold start and RAM:** often end-to-end and ambiguous. Verify phase-decomposed timings and PSS of front-half processes only.

## Recovery Strategies

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| Upstream protocol drift after a bump | LOW (if fixtures exist) / HIGH (if not) | Revert the pin, regenerate fixtures, diff, and update Rust types and the mock together |
| Tokenizer parity break on a new model | MEDIUM | Re-export the effective tokenizer and template. Find the first diverging corpus case and bisect normalizer, pre-tokenizer, post-processor, and template. |
| Detokenizer panics in production | LOW | Switch to a char-boundary-safe delta (or `DecodeStream`), add the failing ids to the fuzz corpus |
| Cancellation leaks discovered on the GPU | MEDIUM | Add tombstones, a single ordered writer, and RAII release. Reproduce on the adversarial mock first. |
| Upstream double-free under heavy aborts | MEDIUM | Document it, reproduce minimally, and report upstream. The benchmark must not crash, so if necessary add a documented abort-coalescing delay to the Rust side and apply the same policy to the baseline comparison. |
| Wins don't materialize | HIGH (if found in P6) / LOW (if found in the P1 spike) | Rescope claims to measured regimes (small model, short prompts, front-half RSS and startup). Report honestly. |
| Benchmark numbers invalid | MEDIUM | Rerun with the fixed methodology. The results harness should make reruns one command. |

## Pitfall-to-Phase Mapping

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| 1 Unstable upstream boundary | P1 Boundary | Golden fixtures regenerated at the pinned SHA in CI. The shim launches unmodified upstream. |
| 2 msgpack encoding mismatch | P1 Boundary | Upstream decoder (inside the mock) accepts every Rust message |
| 3 Tokenizer parity | P2 Tokenizer | Exact-id corpus test on Mac (Qwen3 plus one Llama-3.x) |
| 4 Detokenization / UTF-8 | P2 Tokenizer | Differential and proptest vs upstream `DetokenizeManager`. No panics on the fuzz corpus. |
| 5 Cancellation races | P3 (mock), P5 (GPU) | Stress run ends with zero live requests, zero refcounts, and no scheduler integrity error |
| 6 Backpressure | P1 (topology), P3 | A stalled-client test leaves other streams' ITL unchanged. RSS stays bounded. |
| 7 Radix equivalence scope | P4 Radix (scope in REQUIREMENTS) | Differential op-trace harness with a logical clock, `page_size` 1 and >1 |
| 8 Parity nondeterminism | P1–P2 (levels 1–3), P5 (levels 4–6) | Parity ladder gates pass; concurrent mismatch rate is reported |
| 9 Lenient mock | P1 Boundary, P3 | The mock imports `minisgl.message`. Adversarial modes are covered by tests. |
| 10 Host overhead not the bottleneck | P1 spike, P6 | Baseline profile shows front-half saturation before P3/P4 investment |
| 11 Benchmark methodology | P1 (definitions), P6 | Validated client headroom, ABAB runs with CIs, best-config baseline, cold start by phase |
| 12 macOS vs Linux | P1, P6 | Linux CI job. All claims measured on the Linux box. |
| 13 OpenAI-compat drift | P3 | HTTP snapshot tests vs recorded baseline |
| 14 Disconnect detection | P3 | Disconnect while queued prevents prefill (verified via mock logs) |
| 15 Blocking on the runtime | P3, P6 | tokio-console shows no long polls under load |
| 16 Untrusted input | P2, P3 | Corpus cases plus limit tests |

## Sources

- **mini-sglang source at `9a91cfa` (2026-05-17), read directly. HIGH.** Files: `python/minisgl/message/{backend,frontend,tokenizer,utils}.py`, `utils/mp.py`, `utils/hf.py`, `tokenizer/{server,tokenize,detokenize}.py`, `scheduler/{scheduler,io,cache,config,prefill,decode,table}.py`, `kvcache/radix_cache.py`, `kernel/radix.py`, `engine/sample.py`, `server/{launch,args,api_server}.py`, `benchmark/client.py`, `pyproject.toml`, `README.md`, `docs/features.md`. Git history: https://github.com/sgl-project/mini-sglang (protocol churn Dec 2025–Feb 2026; cancellation #16 on 2026-02-17; radix split timestamp refresh #124 on 2026-05-10).
- Qwen3-0.6B `tokenizer_config.json` (no BOS, `clean_up_tokenization_spaces: false`, `eos_token: <|im_end|>`, `enable_thinking` template). HIGH. https://huggingface.co/Qwen/Qwen3-0.6B/raw/main/tokenizer_config.json
- Thinking Machines, "Defeating Nondeterminism in LLM Inference" (batch non-invariance). MEDIUM. https://thinkingmachines.ai/blog/defeating-nondeterminism-in-llm-inference
- Coordinated omission and open vs closed load models. MEDIUM. https://www.scylladb.com/2021/04/22/on-coordinated-omission/ ; https://grafana.com/docs/k6/v2.3.x/using-k6/scenarios/concepts/open-vs-closed.md
- rmp-serde struct-as-array default and `to_vec_named`/`StructMapConfig`. MEDIUM. https://docs.rs/rmp-serde ; https://github.com/3Hren/msgpack-rust/issues/114
- HF tokenizers `DecodeStream` (incremental decode, UTF-8 holdback). MEDIUM. https://huggingface.co/docs/tokenizers/v0.23.2/api/decoders
- transformers Llama fast-tokenizer added-token, `legacy`, and BOS issues; `clean_up_tokenization_spaces` deprecation history. MEDIUM. https://github.com/huggingface/transformers/pull/24042 ; https://github.com/huggingface/transformers/issues/31187
- minijinja and minijinja-contrib pycompat for HF chat templates. MEDIUM. https://github.com/mitsuhiko/minijinja ; https://docs.rs/crate/minijinja/2.24.0
- SGLang Model Gateway (Rust tokenization in front of SGLang, a prior example of this architecture). MEDIUM. https://lmsysorg.mintlify.app/docs/advanced_features/sgl_model_gateway
- Hyper client-disconnect detection. MEDIUM/LOW. https://github.com/hyperium/hyper/issues/2787 ; https://users.rust-lang.org/t/axum-sse-and-backpressure/133061
- LOW-confidence items flagged inline: the upstream overlap-scheduling double-free under abort (inferred from code reading, not reproduced), Llama-3.x `clean_up_tokenization_spaces: true`, and the maturity of the pure-Rust `zeromq` crate's `ipc://` interop.

---
*Pitfalls research for: Rust front half for an LLM serving stack driving unmodified mini-sglang*
*Researched: 2026-10-02*
