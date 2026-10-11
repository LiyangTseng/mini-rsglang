## Project

**mini-rsglang**

mini-sglang with its Python frontend replaced by an optimized Rust frontend. The repo is built on top of `sgl-project/mini-sglang` (MIT): its Python code is vendored in at a pinned commit, and the frontend processes — API server, tokenizer, detokenizer — are rewritten in Rust (concurrent ingress, request-lifecycle FSM, Hugging Face tokenization and detokenization). The Python/CUDA backend (scheduler, engine, KV cache, kernels) stays in Python and is shared by both frontends. The original Python frontend is kept frozen as the baseline, so the same repo can launch either `--frontend python` or `--frontend rust` and measure how much the Rust frontend improves each scenario. It is for the author as a learning-and-proof project.

**Core Value:** Serving through the Rust frontend produces output identical to the Python frontend on the same backend, and a reproducible benchmark harness quantifies the Rust frontend's improvement in each of the three host-overhead-bound scenarios.

### Constraints

- **Architecture**: Rust owns ingress, lifecycle FSM, tokenization, detokenization; Python/CUDA owns scheduler, weights, batching loop, kernels, KV cache (including the radix cache)
- **Fair comparison**: both frontends run against the same vendored backend; backend changes must apply to both modes; the Python frontend stays frozen
- **IPC**: the existing ZMQ + MessagePack wire format; Rust must match it byte-for-byte (an extra key crashes the scheduler)
- **Environment**: must be developable and testable without a GPU (mock backend) — dev machine is a Mac
- **Verification**: performance claims are measured on Linux on identical hardware; projections stay projections until measured
- **License**: mini-sglang is MIT; keep its copyright notice and LICENSE

## Upstream ground truth (read from source)

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
- `UserMsg{uid:int, input_ids:Tensor(int32), sampling_params:SamplingParams{temperature:float, top_k:int, top_p:float, ignore_eos:bool, max_tokens:int}}`
- `AbortBackendMsg{uid:int}`
- `BatchBackendMsg{data:[...]}`
- `ExitMsg{}`
- `DetokenizeMsg{uid:int, next_token:int, finished:bool}`
- `BatchTokenizerMsg{data:[DetokenizeMsg...]}`

## How the Rust process plugs in without modifying upstream

- **The real radix cache lives inside the Python scheduler** (`kvcache/radix_cache.py`, driven by `scheduler/cache.py`). Its values are GPU page indices, and it is page-aligned, LRU-evicted by `timestamp` via `heapq`, and ref-counted. The upstream wire protocol has no "use this prefix" field. **A Rust radix trie cannot replace the scheduler's KV radix cache without patching upstream.**
- **The "lock-free channel" constraint applies inside the Rust process only.** The process boundary is fixed by upstream to ZMQ over `ipc://`. Shared memory, iceoryx2, or raw UDS would all require patching the Python side.
- **Output parity also depends on detokenization**, not just tokenization. The Rust detokenizer ports `tokenizer/detokenize.py`.

For the full stack research (crate choices, alternatives, version pins), see
`.planning/research/STACK.md`.

## Planning history

Phases 1–7 were planned and executed with GSD; that record lives in
`.planning/` and is **read-only history** (see `.planning/README.md`).
GSD is retired for this repo: do not run `/gsd-*` commands and do not add new
files under `.planning/`. New work is tracked as GitHub issues and plan docs
under `docs/plans/`.

`.planning/STATE.md` is still read by `python/tests/test_parity_report.py`,
and requirement IDs such as `BENCH-01` or `PAR-02` in code comments refer to
`.planning/REQUIREMENTS.md`, so do not move or delete `.planning/`.

## Agent skills

### Issue tracker

Issues live in GitHub Issues for `LiyangTseng/mini-rsglang` (via `gh` CLI). See `docs/agents/issue-tracker.md`.

### Triage labels

Default five-role vocabulary (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: root `CONTEXT.md` + `docs/adr/`. See `docs/agents/domain.md`.
