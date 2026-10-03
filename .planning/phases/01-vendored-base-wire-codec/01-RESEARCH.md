# Phase 1: Vendored Base & Wire Codec - Research

**Researched:** 2026-10-03
**Domain:** Vendoring a pinned Python repo, a Python process launcher with a readiness handshake, and a byte-exact Rust msgpack codec against the upstream Python encoder
**Confidence:** HIGH. Every open question in CONTEXT.md was answered from upstream source at `9a91cfa`, read this session. Most were also confirmed by running code on this Mac.

> **How to read the tags.**
> - **[VERIFIED: ran]**: confirmed by running code on this Mac this session. The output is quoted next to the claim.
> - **[VERIFIED: file:lines]**: read from upstream source this session. The line numbers refer to the upstream repo root at `9a91cfa`, which was cloned to `$TMPDIR/msg-up` (outside the repo).
> - **[CITED]**: taken from official docs.
> - **[ASSUMED]**: not verified. These need confirmation.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

#### Vendor layout & tracking
- **D-01:** Vendor the **whole** upstream repo at `9a91cfa`: `python/`, `benchmark/`, `tests/`, docs, `pyproject.toml`, `LICENSE`, README. Do not cherry-pick. This keeps `benchmark/online/bench_simple.py` and `tests/core/test_scheduler.py` available for later phases.
- **D-02:** The vendored tree lives at `vendor/mini-sglang/`. Our code lives outside it:
  - Rust crates in a cargo workspace at the repo root or under `crates/`.
  - Our Python launcher and helpers under `python/` (package `rsglang`).
  - The backend is installed with `uv pip install -e vendor/mini-sglang`.
  - **Reversibility:** costly. Every import path, install command and later plan references this location.
- **D-03:** Modifications are tracked in `UPSTREAM.md` plus a **check script**:
  - `UPSTREAM.md` records the source repo URL, the commit `9a91cfa`, and every modified vendored file with the reason for the change.
  - The script fetches pristine `9a91cfa` (git clone/archive at that SHA) and diffs it against `vendor/mini-sglang/`. It fails if any differing file is missing from `UPSTREAM.md`'s modified list.
- **D-04:** The same check script **enforces the frozen Python frontend**. It fails if any file under the frontend paths differs from pristine, even if the file is listed. Frontend paths include at least `python/minisgl/server/api_server.py`, `python/minisgl/tokenizer/` and `python/minisgl/message/`; the researcher confirms the exact set from source. The only exception is an entry explicitly marked as a shared backend fix that applies to both modes.
- The expected outcome for Phase 1 is **zero modified vendored files** (see D-09).

#### Launcher shape & topology
- **D-05:** The single launch command is a **Python launcher**: `python -m rsglang.launch --frontend python|rust <upstream server args>`.
  - `--frontend python`: run upstream's own launch path (`minisgl.server.launch`) **unmodified**, forwarding the args.
  - `--frontend rust`: the launcher starts the backend (scheduler rank processes) itself and also spawns the Rust binary.
  - Both modes reuse upstream's argument parsing (`minisgl.server.args`).
- **D-06:** Socket names are pinned by the **launcher, which sets a per-run `_unique_suffix`** on the scheduler config. The launcher computes the full `ipc://` addresses and passes them to Rust. It removes stale `/tmp/minisgl_*` socket files on start.
- **D-07:** Bind/connect roles in rust mode **mirror what upstream's Python mode produces for the same ServerArgs**. The scheduler process must run identically in both modes, for a fair comparison. If upstream's Python detokenizer binds `minisgl_1`, Rust binds it; if it connects, Rust connects. The Rust side makes bind vs connect **configurable per endpoint**. The researcher must pin the exact roles from source (`server/launch.py`, `scheduler/config.py`, `utils/mp.py`, `backend_create_detokenizer_link`, default `num_tokenizer`).
- **D-08:** In Phase 1, `--frontend rust` starts a **Rust skeleton binary**. It:
  - parses CLI flags and connects or binds its sockets per D-07;
  - reads the handshake from stdin;
  - logs the received values (`tracing`);
  - idles until it receives SIGTERM/SIGINT or stdin EOF.
  It serves no HTTP yet.

#### Readiness handshake
- **D-09:** The handshake is produced by a **launcher-side wrapper** around the **unmodified** upstream `Scheduler`. Our scheduler-process entry function (modelled on upstream `tests/core/test_scheduler.py` and the `_run_scheduler` pattern in `server/launch.py`) does three things:
  - constructs the Scheduler;
  - reads `max_seq_len`, `eos_token_id`, `page_size`, `max_running_req` and `num_pages` from the scheduler, its config or the model config after init;
  - reports them on the ready queue.
  No vendored file is patched for the handshake. This meets BASE-03's "same backend code" because the `Scheduler` class is byte-identical in both modes. Python mode does not need the handshake.
- **D-10:** Channel: **one JSON line written to the Rust process's stdin** once the backend is ready. Rust is spawned **in parallel** with backend startup, not after it, so its startup (and later its tokenizer load) overlaps weight loading. Rust must not send anything to the scheduler before the handshake line arrives.
  - **Reversibility:** costly. The handshake JSON format becomes a contract between the launcher and the Rust binary that later phases build on. Changes are cheap while both sides are in this repo, so it is not one-way.
- **D-11:** Payload split. **Static** information goes as **CLI args at spawn**:
  - full ipc addresses;
  - the bind/connect role per endpoint;
  - the model path / HF id;
  - the run id / suffix.
  **Dynamic** information goes in the **stdin handshake**:
  - `max_seq_len`, `eos_token_id`, `page_size`, `max_running_req`, `num_pages`;
  - the **upstream SHA**.
  Rust **refuses to start**, exiting non-zero with a clear error, when the handshake SHA differs from the SHA its wire fixtures were generated from (`9a91cfa`). This turns a silent ZMQ hang into a loud error.
- **D-12:** The launcher owns failure handling:
  - A configurable ready timeout applies.
  - All children (scheduler ranks, including any TP children, and the Rust binary) run in one process group (`setpgid`).
  - If any child exits or the timeout expires, the launcher `killpg`s the whole group, prints the tail of the failing child's stderr, and exits non-zero.
  - As a backstop, **Rust treats stdin EOF as fatal**: if the launcher dies (even by `kill -9`), Rust exits and is not orphaned.

#### Golden fixtures & decoder check
- **D-13:** Golden fixtures are generated **on the Mac** in a `uv` env (Python 3.12) with **CPU torch** and msgpack. The generator uses upstream's real `minisgl.message` classes and `serialize_type` from `vendor/mini-sglang/`, then `msgpack.packb(..., use_bin_type=True)`.
  - The package `__init__` may pull in CUDA-only deps (`sgl_kernel`, `flashinfer`). If it does, the fallback is to load the message modules **by file path**, bypassing the package `__init__`. Never patch the vendored code for this.
  - The researcher verifies which path is needed.
- **D-14:** Fixtures are **committed** to the repo as binary files plus a readable manifest, so `cargo test` runs without Python. A Python check **regenerates them and diffs** against the committed set, and fails on any difference. It runs alongside the D-03 check, so a vendored-code change can't silently stale the fixtures.
- **D-15:** WIRE-02 runs as a **Rust dump followed by a pytest decode**:
  1. A Rust test or example writes each encoded message (for every fixture case) to a temp directory.
  2. pytest decodes each file with upstream's **real** decoder (`cls(**kwargs)` path). It asserts there is no error and that re-encoding with the Python encoder reproduces the bytes exactly.
  One command or script runs both steps.
- **D-16:** The fixture case matrix has four parts:
  - **Base:** one case per upstream message type (all 7).
  - **Batch variants:** `BatchTokenizerMsg` with 1 entry and with N entries; a bare `DetokenizeMsg`; `BatchBackendMsg` mixing `UserMsg` and `AbortBackendMsg`.
  - **Numeric width boundaries:** integers at msgpack width switch points, namely 127/128, 255/256, 65535/65536, -1 (`top_k=-1`), and -32/-33. Floats must encode as **float64** like Python's default. Rust wire structs use `f64`, never `f32`, because `f32` would also change sampling values such as `top_p=0.9`. Use booleans both ways.
  - **Tensor length boundaries:** `input_ids` of 1 token; 63 and 64 tokens (bin8→bin16); 16383 and 16384 tokens (bin16→bin32). Check that dtype is the string `"torch.int32"` and the byte order is little-endian.

### Claude's Discretion
- Rust workspace and crate layout (e.g. `crates/rsg-wire` with no I/O dependencies, plus a skeleton binary crate), and the `rust-toolchain.toml` pin.
- Hand-rolled `rmp` encoding vs `rmp-serde` with `to_vec_named`. Either is fine as long as the fixtures pass byte-for-byte, including map key order.
- The Rust ZMQ crate for the skeleton (`zmq` vs `zeromq`). The Phase 1 skeleton only needs to connect; the full transport decision belongs to Phase 3 (WIRE-03). Keep it behind a small interface.
- Script and command names (e.g. `scripts/check_upstream.py`, `just`/Makefile targets), the fixture file format, and the manifest format.
- Python env management details (uv lockfile layout for Mac vs GPU box).

### Deferred Ideas (OUT OF SCOPE)
- **Capture real upstream message sequences as extra fixtures.** Do this in Phase 6, on the GPU box, while running real traffic for PAR-01.
- **Empty `input_ids`.** Upstream's `torch.frombuffer` path likely rejects an empty buffer. Treat it as a Phase 5 validation rule (Rust must never send an empty prompt), not as a codec fixture.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| BASE-01 | mini-sglang @ `9a91cfa` is vendored with its MIT LICENSE and copyright notice; `UPSTREAM.md` records the source commit and every modified file | Full SHA, tree hash and file inventory (121 entries, 1 symlink); `git archive` vendoring recipe; offline tree-hash check plus a pristine-diff check; exact frozen-path set (§Frozen-Frontend Path Set) |
| BASE-02 | One launch command starts the shared backend with either `--frontend python` (frozen original) or `--frontend rust` | Python mode: `os.execv(python -m minisgl …)`. Rust mode: replicate `start_subprocess`'s rank loop with our wrapper and spawn the Rust skeleton. `parse_args` and `ServerArgs` import on the Mac, so the launcher is testable there (§Launcher Architecture) |
| BASE-03 | The backend reports a readiness handshake (max_seq_len, eos_token_id, page_size, max_running_req); both frontends use the same backend code | Exact attribute paths on a constructed `Scheduler` with no patching. `page_size` must be read **after** init because of the TRTLLM override. The handshake JSON schema is defined, and the SHA comes from a single-source file (§Handshake) |
| WIRE-01 | The Rust codec produces bytes identical to upstream's encoder for all 7 message types | Ran on this Mac: `rmp_serde::to_vec_named` with `#[serde(tag="__type__")]` and `serde_bytes` reproduces Python bytes for all 6 boundary messages plus the nested `SamplingParams` and Tensor, with matching int widths and bin8/16/32 boundaries (§Wire Codec) |
| WIRE-02 | Every message Rust sends decodes through the real Python decoder | Prototype ran: a Rust dump of 15 cases (tensor lengths and int boundaries) went through upstream `BaseBackendMsg.decoder` and the Python re-encode. Result: `15 of 15` byte-identical (§WIRE-02 Flow) |
</phase_requirements>

## Project Constraints (from CLAUDE.md)

Directives from `./CLAUDE.md` and `./.claude/CLAUDE.md` that the planner must honor:

- **Orchestration:** the Opus orchestrator does architecture, decomposition and review. Implementation, multi-file edits and test runs go to Sonnet sub-agents. Plans should be split into small, delegable tasks.
- **GSD workflow:** file changes happen only inside GSD commands (`/gsd-execute-phase` for this phase).
- **Issue tracker:** local markdown under `.scratch/<feature>/`. Domain docs go in a single root `CONTEXT.md` plus `docs/adr/`, created lazily.
- **Architecture:** Rust owns ingress, FSM, tokenization and detokenization. Python/CUDA owns the scheduler, weights, batching and KV cache.
- **Fair comparison:** both frontends run against the same vendored backend. The Python frontend stays frozen.
- **IPC:** use the existing ZMQ plus MessagePack wire format byte-for-byte. One extra key crashes the scheduler. [VERIFIED: ran, `TypeError AbortBackendMsg.__init__() got an unexpected keyword argument 'extra'`]
- **Environment:** must be developable and testable on a Mac with no GPU.
- **Verification:** performance claims are measured on Linux. Phase 1 makes no performance claims.
- **License:** keep mini-sglang's MIT LICENSE and its copyright notice.
- **Stack pins (CLAUDE.md):** Rust stable 1.99.0 with edition 2024 via `rust-toolchain.toml`; `rmp-serde` 1.3.1 with `to_vec_named`, never `to_vec`; `serde_bytes` for the tensor buffer; `tokio` 1.53.1; `tracing` 0.1.44 / `tracing-subscriber` 0.3.23; `clap` 4.6.7; `thiserror` in libraries and `anyhow` in binaries; `nix` 0.31.3 for process groups; `uv` with Python 3.12.

## Summary

Every open research question was answered from source, and the risky parts were run on this Mac. The main correction to prior research and to D-13: **no file-path loading fallback is needed.** `minisgl` has no package `__init__.py`; it is a PEP 420 namespace package. On macOS with CPU torch 2.9.1, the following all import without `flashinfer`, `sgl_kernel`, `fastapi` or `tvm_ffi` installed: `minisgl.message`, `minisgl.core`, `minisgl.utils`, `minisgl.scheduler`, `minisgl.engine`, `minisgl.server.args` and `minisgl.server.launch`. Upstream's real `parse_args` also runs, provided `--dtype` is given explicitly. A full `uv pip install -e` of the vendored package fails on macOS because `sgl-kernel` ships only manylinux wheels. Use `uv pip install --no-deps -e vendor/mini-sglang` on the Mac and the full install on the GPU box. [VERIFIED: ran]

The second main finding: **`rmp-serde` 1.3.1 is byte-exact without a hand-rolled codec.** `rmp_serde::to_vec_named` on internally tagged enums and structs (`#[serde(tag = "__type__")]`), with `#[serde(with = "serde_bytes")]` on the tensor buffer, produced exactly the bytes of upstream's `msgpack.packb(serialize_type(m), use_bin_type=True)`. This held for UserMsg, AbortBackendMsg, ExitMsg, BatchBackendMsg, DetokenizeMsg and BatchTokenizerMsg. rmp-serde also decoded Python's bytes back into the same Rust values. Python and rmp choose the same minimal width at every integer boundary tested, for signed and unsigned types alike. Both emit float64 for `f64`. bin8/bin16/bin32 switch at 63/64 and 16383/16384 tokens on both sides. `serialize_type` puts `__type__` **first**, then the dataclass fields in declaration order. A prototype of the WIRE-02 flow (Rust dumps files, upstream decoder reads them, Python re-encodes) came back `15 of 15` identical. [VERIFIED: ran]

Third: the **topology in default Python mode** (`num_tokenizer=0`) is as follows. The scheduler **binds** PULL on `minisgl_0` and **connects** PUSH to `minisgl_1`. The shared tokenizer/detokenizer worker **binds** PULL on `minisgl_1` and **connects** to `_0` and `_3`. The API server binds `_3` and connects to `_1`. `_4` is unused and `_2` exists only when TP>1. In rust mode, Rust therefore **connects `_0` and binds `_1`**. The launcher should derive Rust's `_1` role from `server_args.backend_create_detokenizer_link` (scheduler binds → Rust connects) so that any `--num-tokenizer` value stays mirrored. **Do not** build the scheduler from a bare `SchedulerConfig` the way `tests/core/test_scheduler.py` does: there, `backend_create_detokenizer_link` is hard-coded `True`, so the scheduler binds `_1` and the topology differs from Python mode. Use `ServerArgs`. [VERIFIED: file:lines + ran]

**Primary recommendation:**
- Vendor with `git archive 9a91cfa | tar -x -C vendor/mini-sglang`. Gate it with an offline tree-hash check (`02d3e4ad…`) plus a pristine-diff check script.
- Build `rsg-wire` on `rmp-serde` 1.3.1 `to_vec_named`, with a Python fixture generator that imports upstream directly.
- Build the launcher as `python -m rsglang.launch`. Python mode `exec`s `python -m minisgl`. Rust mode runs our own `run_scheduler` wrapper (a clone of `_run_scheduler` plus the handshake) through upstream's `ServerArgs`, and spawns a tokio skeleton.
- Test the whole rust-mode path on the Mac with an injectable fake scheduler that uses upstream's real `ZmqPullQueue`/`ZmqPushQueue`.

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Vendored upstream tree and its integrity | Repo / build tooling (`vendor/`, `scripts/check_upstream.py`) | Git (tree hash) | Pure file-integrity concern. No runtime code |
| Launch orchestration, process group, timeout | Python launcher (`python/rsglang/launch.py`) | OS (pgid, signals) | Must import upstream `parse_args`/`ServerArgs` and multiprocessing, so it lives in Python |
| Readiness handshake extraction | Python scheduler-process wrapper (`python/rsglang/backend.py`) | Upstream `Scheduler` (read-only) | Values exist only after `Scheduler(args)` is built inside the rank-0 process |
| Handshake transport | Launcher → Rust stdin (one JSON line) | — | D-10 |
| Wire encode/decode | Rust `rsg-wire` (no I/O) | Python fixture generator (oracle) | Pure data transform. The oracle is upstream's own encoder |
| Socket bind/connect | Rust skeleton binary (`rsg-server`) | — | D-07/D-08. Roles come in as CLI args |
| Fixture freshness and decode conformance | Python scripts + pytest | Rust tests (dump) | D-14/D-15 |

## Standard Stack

### Core (Rust)
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| Rust toolchain | 1.99.0 (stable, 2026-09-28), edition 2024 | Language | [VERIFIED: static.rust-lang.org stable manifest: `version = "1.99.0 (b940084d7 2026-09-28)"`]. Local rustup has 1.95.0; `rust-toolchain.toml` installs 1.99 automatically |
| `rmp-serde` | 1.3.1 (pulls `rmp` 0.8.15) | msgpack encode/decode | [VERIFIED: ran, byte-exact for all boundary types; crates.io max_stable 1.3.1] |
| `serde` (derive) | 1.0.229 | Derive | [VERIFIED: cargo resolved] |
| `serde_bytes` | 0.11.19 | Emit `Vec<u8>` as msgpack bin | [VERIFIED: ran. Without it: `81a6627566666572920102` (array of ints); with it: `c4…` bin] |
| `zmq` (rust-zmq) | 0.10.0 (+ `zmq-sys` 0.12.0, bundled libzmq 4.3.4, statically linked) | Skeleton bind/connect | [VERIFIED: ran. Builds on macOS arm64 in ~24 s cold. Interop with pyzmq 27.2.0 / libzmq 4.3.5 through upstream's own `ZmqPullQueue`/`ZmqPushQueue` over `ipc://`. Rust received the 51-byte `DetokenizeMsg`] |
| `tokio` | 1.53.1 (`rt-multi-thread`, `macros`, `signal`, `sync`, `time`) | Skeleton runtime (signals) | Project standard (CLAUDE.md) |
| `clap` | 4.6.7 (`derive`, `env`) | Skeleton CLI | Project standard |
| `serde_json` | 1.0.151 | Parse the handshake line | Project standard |
| `tracing` / `tracing-subscriber` | 0.1.44 / 0.3.23 (`env-filter`, `fmt`) | Logging the handshake values | Project standard |
| `anyhow` / `thiserror` | 1.0.104 / 2.0.21 | Errors (bin / lib) | Project standard |

### Supporting (Rust dev)
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `rmpv` | 1.3.1 | Dynamic msgpack values | Debug dumps and negative tests (unknown `__type__`) in `rsg-wire` tests |
| `hex` | 0.4.3 | Readable diffs in failing tests | Dev-dep only |

### Python (Mac fixture/test env and GPU env)
| Package | Version (verified installed this session) | Purpose |
|---------|-----------|---------|
| Python | 3.12.12 (via `uv venv --python 3.12`) | Matches upstream README/Dockerfile 3.12 [VERIFIED: Dockerfile:3 `ARG PYTHON_VERSION=3.12`] |
| `torch` | 2.9.1 (CPU on Mac) | Required by `minisgl.message`. Upstream pin `torch<2.10.0` [VERIFIED: pyproject.toml:28] |
| `numpy` | 2.5.3 | Used by `message/utils.py` |
| `msgpack` | 1.2.3 | The encoder of record |
| `pyzmq` | 27.2.0 (libzmq 4.3.5) | Upstream's `utils/mp.py` (fake scheduler) |
| `transformers` | 4.57.3 (resolves `tokenizers` 0.22.2) | Needed at import time by `minisgl.utils.hf` (pulled in by `minisgl.utils` → `server.args`) [VERIFIED: hf.py:8] |
| `pytest` | 9.1.1 | Python tests |

**Installation (Mac dev env):**
```bash
uv venv --python 3.12 .venv
uv pip install "torch==2.9.1" numpy "msgpack==1.2.3" "pyzmq==27.2.0" "transformers==4.57.3" pytest
uv pip install --no-deps -e vendor/mini-sglang   # full deps fail on macOS: sgl-kernel is manylinux-only
uv pip install --no-deps -e .                    # our rsglang package (root pyproject)
```
**Installation (GPU box):** `uv venv --python=3.12 && uv pip install -e vendor/mini-sglang && uv pip install torch-c-dlpack-ext && uv pip install -e .` [VERIFIED: Dockerfile:29-32 does `uv pip install -e .` then `uv pip install torch-c-dlpack-ext`]. A C/C++ toolchain is also needed for `zmq-sys` (`build-essential`). [ASSUMED for Linux; verified on macOS with Xcode CLT]

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `rmp-serde` derive | Hand-rolled `rmp` writer (prior ARCHITECTURE.md suggestion) | Unnecessary: rmp-serde was verified byte-exact. Revisit only if Phase 3/5 profiling shows decode overhead from serde's buffered `Content` for internally tagged enums |
| `zmq` 0.10 | `zeromq` 0.6.0 (pure Rust, updated 2026-05-04) | Phase 3 decides (WIRE-03). Phase 1 uses `zmq` because the interop was verified. Keep it behind a tiny trait |

**Version verification:** crates.io API queried this session: `zmq 0.10.0 (2022-11-04)`, `zmq-sys 0.12.0`, `zeromq 0.6.0 (2026-05-04)`, `rmp-serde 1.3.1 (2025-12-23)`, `serde_bytes 0.11.19`, `tokio 1.53.1`, `tracing 0.1.44`, `tracing-subscriber 0.3.23`, `clap 4.6.7`, `serde_json 1.0.151`, `anyhow 1.0.104`, `thiserror 2.0.21`, `nix 0.31.3`.

## Package Legitimacy Audit

| Package | Registry | Age / last release | Downloads | Source Repo | Verdict | Disposition |
|---------|----------|-----|-----------|-------------|---------|-------------|
| rmp-serde | crates | 1.3.1, 2025-12 | 136M | github.com/3Hren/msgpack-rust | OK | Approved |
| serde_bytes | crates | 0.11.19 | 211M | github.com/serde-rs/bytes | OK | Approved |
| serde | crates | 1.0.229 | — | serde-rs | OK | Approved |
| zmq | crates | 0.10.0, 2022-11 | 7.2M | github.com/erickt/rust-zmq | OK | Approved (old release, thin binding; interop verified) |
| zeromq | crates | 0.6.0, 2026-05 | 2.9M | github.com/zeromq/zmq.rs | OK | Not used in Phase 1 |
| tokio, tracing, tracing-subscriber, clap, serde_json, anyhow | crates | current | very high | official orgs | OK | Approved |
| msgpack | PyPI | 1.2.3 | n/a | github.com/msgpack/msgpack-python | SUS (`too-new`, `unknown-downloads`) | Flagged. Declared by upstream pyproject.toml:26 and installed and imported this session |
| pyzmq | PyPI | 27.2.0 | n/a | github.com/zeromq/pyzmq | SUS (`unknown-downloads`) | Flagged. Upstream dep, pyproject.toml:31 |
| torch | PyPI | 2.9.1 | n/a | github.com/pytorch/pytorch | SUS (`too-new`, `unknown-downloads`) | Flagged. Upstream dep, pyproject.toml:28 |
| numpy | PyPI | 2.5.3 | n/a | github.com/numpy/numpy | SUS (`too-new`, `unknown-downloads`, `no-repository`) | Flagged. Imported by upstream message/utils.py:5 |
| transformers | PyPI | 4.57.3 (pinned) | n/a | github.com/huggingface/transformers | SUS (`too-new`, `unknown-downloads`) | Flagged. Upstream dep, pyproject.toml:29 |
| pytest | PyPI | 9.1.1 | n/a | github.com/pytest-dev/pytest | SUS (`unknown-downloads`) | Flagged. Upstream dev dep, pyproject.toml:43 |

**Packages removed due to [SLOP] verdict:** none.
**Packages flagged as suspicious [SUS]:** msgpack, pyzmq, torch, numpy, transformers, pytest. These are the seam's verdicts. The reasons are registry-metadata signals: PyPI exposes no download counts, and "too-new" refers to the latest release date. Every one of these names is declared by upstream's `pyproject.toml`, which was read this session. The planner should add **one** `checkpoint:human-verify` task before creating the Python env lockfile, confirming these exact names and pins.

## Architecture Patterns

### System Architecture Diagram

```
                       python -m rsglang.launch --frontend {python|rust} <upstream args>
                                              │
                         ┌────────────────────┴─────────────────────┐
                 --frontend python                          --frontend rust
                         │                                          │
          os.execv(sys.executable, -m minisgl, args)     parse launcher flags (allow_abbrev=False)
          (upstream process tree, byte-identical)        upstream parse_args(rest) → ServerArgs
                         │                               replace(_unique_suffix=run suffix)
                 api_server binds _3                     unlink /tmp/minisgl_{0..4}<suffix>
                 tok/detok worker binds _1               setpgid(0,0) if not leader
                 scheduler binds _0, connects _1                     │
                                                 ┌──────────────────┼──────────────────────┐
                                                 ▼                  ▼                      ▼
                                   Popen(rust skeleton,     mp.Process(rsglang.backend   (TP>1: more ranks,
                                   stdin=PIPE, CLI addrs    .run_scheduler, rank r)       same wrapper)
                                   + roles)                         │
                                        │                  Scheduler(args) [unmodified]
                                   bind _1 (PULL)          sync_all_ranks()
                                   connect _0 (PUSH)       rank0: queue.put(handshake dict)
                                   wait for stdin line              │
                                        │                           ▼
                                        │            launcher: ready_queue.get(timeout, poll children)
                                        │◀──── one JSON line ───────┘
                                   check upstream_sha == fixture SHA (else exit≠0)
                                   tracing::info!(handshake values)
                                   idle until SIGINT/SIGTERM (exit 0) or stdin EOF (exit≠0)

   failure path: any child exits / timeout ─▶ launcher ignores signals, killpg(group), prints stderr tail, exit≠0
```

### Recommended Project Structure
```
Cargo.toml                    # [workspace] resolver = "3", members = ["crates/*"]; edition 2024 in workspace.package
rust-toolchain.toml           # channel = "1.99.0", components = ["rustfmt", "clippy"]
crates/
├── rsg-wire/                 # message types + rmp-serde codec; NO I/O deps; UPSTREAM_SHA const
│   └── tests/{fixtures.rs, dump.rs}
└── rsg-server/               # Phase 1 skeleton bin; later phases grow it into the HTTP server
pyproject.toml                # project "rsglang", package-dir python/; [tool.pytest.ini_options] testpaths=["python/tests"], norecursedirs includes "vendor"
python/
├── rsglang/{__init__.py, launch.py, backend.py, handshake.py, sockets.py}
├── rsglang/testing/fake_scheduler.py   # Mac-only fake used by integration tests
└── tests/
fixtures/wire/                # *.msgpack + manifest.json (committed)
scripts/{check_upstream.py, gen_wire_fixtures.py, check_wire_decode.sh}
vendor/
├── UPSTREAM_SHA              # one line: 9a91cfafe754aa85daee49998176275667eb58f2 (outside the vendored tree)
└── mini-sglang/              # git archive of 9a91cfa, untouched
UPSTREAM.md
```

### Pattern 1: Vendoring by `git archive` plus a tree-hash fast check
**What:** Extract exactly the 121 tracked entries. One of them is a symlink: `.dockerignore` is mode `120000`. Commit them. If the vendored tree is unmodified, `git rev-parse HEAD:vendor/mini-sglang` equals upstream's root tree hash.
**Facts:** full SHA `9a91cfafe754aa85daee49998176275667eb58f2`, commit date `Sun May 17 20:37:42 2026 +0800`, title `[Fix] Stabilize decode batch request order across TP ranks (#113)`, root tree `02d3e4ad34ec00c88f549fd9d287a4588958d824`. Modes: `120 100644`, `1 120000`. [VERIFIED: ran `git rev-parse '9a91cfa^{tree}'`, `git ls-tree -r`]
```bash
git clone --filter=blob:none https://github.com/sgl-project/mini-sglang "$TMP/up"
git -C "$TMP/up" archive 9a91cfafe754aa85daee49998176275667eb58f2 | tar -x -C vendor/mini-sglang
# after commit, offline proof of zero modifications:
test "$(git rev-parse HEAD:vendor/mini-sglang)" = 02d3e4ad34ec00c88f549fd9d287a4588958d824
```
Never use `cp -L` or `rsync -L`: either would turn the `.dockerignore` symlink into a file and change the tree hash.

### Pattern 2: Check script (D-03/D-04)
1. Get pristine content, either by `git archive` of the SHA from a clone or from the GitHub tarball of the SHA.
2. Collect the vendored file set with `git ls-files --cached --others --exclude-standard -z -- vendor/mini-sglang`. This view catches uncommitted edits and ignores the `python/minisgl.egg-info` that the editable install creates [VERIFIED: ran; the vendored `.gitignore` already ignores `*.egg-info/` and `__pycache__/`].
3. Compare content and symlink-ness per path. Each differing, added or removed path must appear in the `UPSTREAM.md` modified table. Tier A frozen paths fail even when listed. Tier B paths fail unless the row says `shared-backend-fix: yes`.
4. `--offline` mode: the tree-hash comparison only (HEAD must be clean under `vendor/`).
5. `UPSTREAM.md` should use a machine-parseable table: `| Path | Reason | Shared backend fix (yes/no) |`. For Phase 1 the table is empty.

### Pattern 3: Rust wire types (verified byte-exact)
```rust
// Source: throwaway crate run this session against upstream bytes ($TMPDIR/wiretest), rmp-serde 1.3.1
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(tag = "__type__", rename = "Tensor")]
pub struct Tensor { #[serde(with = "serde_bytes")] pub buffer: Vec<u8>, pub dtype: String } // dtype = "torch.int32"

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(tag = "__type__", rename = "SamplingParams")]
pub struct SamplingParams { pub temperature: f64, pub top_k: i64, pub top_p: f64, pub ignore_eos: bool, pub max_tokens: i64 }

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(tag = "__type__")]
pub enum BackendMsg {             // variant names ARE the wire tags
    UserMsg { uid: i64, input_ids: Tensor, sampling_params: SamplingParams },
    AbortBackendMsg { uid: i64 },
    ExitMsg {},                   // braced empty variant → {"__type__":"ExitMsg"} (0x81 map)
    BatchBackendMsg { data: Vec<BackendMsg> },
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(tag = "__type__")]
pub enum TokenizerMsg {
    DetokenizeMsg { uid: i64, next_token: i64, finished: bool },
    BatchTokenizerMsg { data: Vec<TokenizerMsg> },
}
pub fn encode<T: Serialize>(m: &T) -> Vec<u8> { rmp_serde::to_vec_named(m).expect("infallible for these types") }
```
Field names and order must match the dataclasses exactly. Struct field order is the map key order, which byte-exactness depends on. Use `rename` on struct-level tags so that a Rust rename cannot silently change the wire tag. (The run above used the struct name as the tag; adding `rename` is a recommendation and was not part of the run.) The run used `i64` for ints. `u32`/`u64` produce identical bytes for non-negative values [VERIFIED: ran, `u32 128 cc80` = `i64 128 cc80`], so either choice is fine.

### Pattern 4: Scheduler-process wrapper with handshake (D-09)
Clone `_run_scheduler` exactly and add the handshake. Do not import `_run_scheduler`, because it cannot report the values.
```python
# python/rsglang/backend.py — mirrors upstream server/launch.py:16-37
def run_scheduler(args, ready_queue, upstream_sha: str) -> None:
    import torch
    from minisgl.scheduler import Scheduler            # factory overridable for Mac tests (see Pitfall 8)
    with torch.inference_mode():
        scheduler = Scheduler(args)
        scheduler.sync_all_ranks()
        if args.tp_info.is_primary():
            ready_queue.put({"kind": "ready", **extract_handshake(scheduler, args, upstream_sha)})
        if args.silent_output:
            logging.disable(logging.INFO)
        try:
            scheduler.run_forever()
        except KeyboardInterrupt:
            scheduler.shutdown()

def extract_handshake(scheduler, args, upstream_sha):
    return {
        "upstream_sha": upstream_sha,
        "max_seq_len": int(scheduler.engine.max_seq_len),          # engine.py:67, NOT args.max_seq_len
        "eos_token_id": scheduler.eos_token_id,                    # scheduler.py:70; may be None for some tokenizers
        "page_size": int(scheduler.cache_manager.page_size),       # cache.py:25; post-TRTLLM-override value
        "max_running_req": int(args.max_running_req),              # engine/config.py:20; TableManager uses it, scheduler.py:58
        "num_pages": int(scheduler.engine.num_pages),              # engine.py:55 (excludes the +1 dummy page)
    }
```
Wrap the body in `try/except BaseException` that puts `{"kind": "error", "rank": r, "traceback": …}` on the queue before re-raising. The launcher can then print the real cause, not just "child exited".

### Pattern 5: Rust skeleton stdin handling
Read stdin on a **dedicated `std::thread`** that sends `Line(String)` and then `Eof` over a tokio mpsc channel. `main` `select!`s over that channel, `ctrl_c()` and `signal(SignalKind::terminate())`. Exit with `std::process::exit`. Tokio's async stdin cannot be cancelled: "For technical reasons, `stdin` is implemented by using an ordinary blocking read on a separate thread, and it is impossible to cancel that read. This can make shutdown of the runtime hang until the user presses enter." [CITED: docs.rs/tokio/1.53.1/tokio/io/struct.Stdin.html]

Exit codes:
- SIGINT/SIGTERM → 0.
- stdin EOF before or after the handshake → non-zero (for example 3), with the error "launcher went away".
- Malformed handshake or SHA mismatch → non-zero (for example 2), with a message naming both SHAs.

### Anti-Patterns to Avoid
- **Building the scheduler from a bare `SchedulerConfig`** (as `tests/core/test_scheduler.py:28-34` does). `SchedulerConfig.backend_create_detokenizer_link` returns `True` [VERIFIED: scheduler/config.py:39-41], so the scheduler would bind `_1`, unlike Python mode. Always use `ServerArgs` from `parse_args`.
- **Reading `page_size` before `Scheduler(args)` returns, or from a pickled copy in the launcher.** `_adjust_config` overrides it in place to 64 on TRTLLM (SM100) GPUs [VERIFIED: engine.py:227-229 `override("page_size", 64)`].
- **Reading `args.max_seq_len`.** That is the model limit (`rotary_config.max_position`, engine/config.py:43-47). The scheduler enforces `engine.max_seq_len = min(config.max_seq_len, num_tokens)` (engine.py:67).
- **`rmp_serde::to_vec`.** It emits arrays [VERIFIED: ran, `to_vec (compact) abort: 92af41626f72744261636b656e644d736707`].
- **Plain `Vec<u8>` for `buffer`.** It becomes an int array [VERIFIED: ran].
- **`f32` anywhere on the wire.** `0.9f32` becomes `ca3f666666`, whereas Python's float64 is `cb3feccccccccccccd` [VERIFIED: ran].
- **Deleting `/tmp/minisgl_*` by glob.** That kills other running servers' sockets. Delete only this run's `/tmp/minisgl_{0..4}<suffix>`.

## Frozen-Frontend Path Set (D-04)

Derived from the import graph at `9a91cfa` [VERIFIED: grep of imports and files read this session]:

**Tier A: frontend-only. Always fails if it differs, with no exception possible, because no backend code imports these:**
- `python/minisgl/server/api_server.py` (FastAPI frontend, imported only by `launch.py:41`)
- `python/minisgl/server/launch.py` (the Python-mode process launcher)
- `python/minisgl/server/__init__.py`, `python/minisgl/__main__.py`, `python/minisgl/shell.py` (entry points)
- `python/minisgl/tokenizer/` (`__init__.py`, `server.py`, `tokenize.py`, `detokenize.py`; imported only by `launch.py:50`)
- `LICENSE` (attribution; BASE-01)

**Tier B: shared wire/config contract. Fails unless the row is marked `shared-backend-fix: yes`, and any change also requires regenerating fixtures:**
- `python/minisgl/message/` (all 5 files): the wire schema, imported by both the scheduler (`scheduler/io.py:6`, `scheduler.py:8`) and the frontend
- `python/minisgl/core.py`: defines `SamplingParams`, whose field order is part of the wire bytes
- `python/minisgl/utils/mp.py`: the msgpack and ZMQ settings for both sides
- `python/minisgl/scheduler/config.py`: socket addresses and `_unique_suffix`
- `python/minisgl/server/args.py`: `ServerArgs` topology properties and `parse_args`, used by both modes
- `python/minisgl/utils/hf.py`: `load_tokenizer`, used by the frontend for tokenization and by the scheduler for `eos_token_id`

**Tier C (recommended, discretionary): measurement tools frozen for fairness:**
- `benchmark/`, `python/minisgl/benchmark/` (`bench_simple.py` imports `minisgl.benchmark.client`)

## Socket Topology (D-07), pinned from source

Default `ServerArgs` has `num_tokenizer: int = 0` [VERIFIED: server/args.py:18] → `share_tokenizer` is `return self.num_tokenizer == 0` [VERIFIED: args.py:21-23].

| Endpoint | Address (args.py / scheduler/config.py) | Python mode, `num_tokenizer=0` | Rust mode (mirror) |
|----------|---------|--------------------------------|--------------------|
| `_0` backend | `"ipc:///tmp/minisgl_0" + self._unique_suffix` (config.py:25) | Scheduler rank0 PULL **binds**: `ZmqPullQueue(config.zmq_backend_addr, create=True, …)` (io.py:36-40). Tokenizer worker PUSH **connects**: `ZmqPushQueue(backend_addr, create=False, …)` (tokenizer/server.py:43) | Rust PUSH **connects** |
| `_1` detokenizer | `"ipc:///tmp/minisgl_1" + self._unique_suffix` (config.py:29) | Scheduler PUSH `create=config.backend_create_detokenizer_link` (io.py:41-45) = `return not self.share_tokenizer` (args.py:41-43) = False → **connects**. Detok worker PULL `create=create` (server.py:45) with `"create": server_args.tokenizer_create_addr` (launch.py:81) = `return self.share_tokenizer` (args.py:37-39) = True → **binds**. API server PUSH to `zmq_tokenizer_addr` (= `_1` when shared, args.py:29-32) with `create=config.frontend_create_tokenizer_link` (api_server.py:438-440) = False → connects | Rust PULL **binds** |
| `_2` broadcast | config.py:33 | Only if TP>1: rank0 PUB binds (io.py:52-54); other ranks SUB connect (io.py:58-62) | Not touched by Rust |
| `_3` frontend | `"ipc:///tmp/minisgl_3" + self._unique_suffix` (args.py:27) | API server PULL binds (api_server.py:433-435); worker PUSH connects (server.py:44) | Unused (internal to Rust) |
| `_4` tokenizer | `"ipc:///tmp/minisgl_4" + …` (args.py:33) | Unused when `num_tokenizer=0` | Unused |

**Launcher rule:** `detok_role = "connect" if server_args.backend_create_detokenizer_link else "bind"`; `backend_role = "connect"` always. [VERIFIED: ran on Mac: `share True tok_create True backend_create_detok False front_create_tok False`; with `num_tokenizer=2`: `ipc:///tmp/minisgl_4.pid=… True False`]

**Suffix:** default `_unique_suffix: str = field(default_factory=_get_pid_suffix)` → `f".pid={os.getpid()}"` (config.py:8-11, 21). `dataclasses.replace(server_args, _unique_suffix=".rsg=abc123")` works, and the value survives the later `replace(..., tp_info=...)` per rank [VERIFIED: ran: `ipc:///tmp/minisgl_0.rsg=abc123 …`, `suffix survives replace: .rsg=abc123`]. Keep the suffix short. The sun_path limit is 104 bytes on macOS. [ASSUMED: 108 on Linux]

**libzmq behavior** [VERIFIED: ran, pyzmq 27.2.0]:
- A hard exit leaves the socket file behind (`stale exists after hard exit: True`).
- A re-bind over a stale file succeeds.
- `connect` to a missing ipc path does not error; libzmq retries.

Starting Rust and the scheduler in parallel is therefore safe in either order. Stale-file cleanup is hygiene, not a correctness requirement. Remove this run's files on exit as well as on start.

## Wire Codec: the 7 types and exact bytes

**Upstream defines 10 concrete message dataclasses** [VERIFIED: message/__init__.py:1-3]:
- backend: `BatchBackendMsg`, `ExitMsg`, `UserMsg`, `AbortBackendMsg`
- tokenizer: `BatchTokenizerMsg`, `DetokenizeMsg`, `TokenizeMsg`, `AbortMsg`
- frontend: `BatchFrontendMsg`, `UserReply`

Only 6 of them cross the scheduler boundary that Rust replaces. `TokenizeMsg`, `AbortMsg`, `UserReply` and `BatchFrontendMsg` are internal to the Python frontend.

**Recommended reading of "7 message types":** the 6 boundary messages plus the nested tagged dataclass `SamplingParams`. That is every `cls(**kwargs)`-decoded class on the boundary. `Tensor` is an 8th `__type__` tag, but it is a special-cased encoding (utils.py:24-29, 55-61), not a class. The fixture matrix covers all 8 tags whichever way "7" is counted. [ASSUMED: interpretation; see Open Questions]

Fields, verbatim:
- backend.py:22-41: `data: List[BaseBackendMsg]`; `ExitMsg … pass`; `uid: int`, `input_ids: torch.Tensor  # CPU 1D int32 tensor`, `sampling_params: SamplingParams`; `AbortBackendMsg … uid: int`
- tokenizer.py:22-31: `data: List[BaseTokenizerMsg]`; `uid: int`, `next_token: int`, `finished: bool`
- core.py:15-21: `temperature: float = 0.0`, `top_k: int = -1`, `top_p: float = 1.0`, `ignore_eos: bool = False`, `max_tokens: int = 1024`

**Encoder** [VERIFIED: message/utils.py:20-35]: `serialized["__type__"] = self.__class__.__name__` comes first, then `for k, v in self.__dict__.items(): serialized[k] = _serialize_any(v)`. Dataclass `__init__` assigns fields in declaration order, so the key order is `__type__`, then the fields. Tensor: `"__type__": "Tensor"`, `"buffer": self.numpy().tobytes()`, `"dtype": str(self.dtype)`. Packing is `msgpack.packb(self.encoder(obj), use_bin_type=True)` (mp.py:25).

**Decoder** [VERIFIED: utils.py:52-69]: `cls = cls_map[type_name]` with `cls_map = globals()` of the defining module, then `cls(**kwargs)`. `SamplingParams` resolves because backend.py:7 and tokenizer.py:6 import it.

Observed key order [VERIFIED: ran]: `keys ['__type__', 'uid', 'input_ids', 'sampling_params'] ['__type__', 'buffer', 'dtype'] ['__type__', 'temperature', 'top_k', 'top_p', 'ignore_eos', 'max_tokens']`

Integer, float and bin widths, Python msgpack 1.2.3 vs rmp-serde 1.3.1 (identical) [VERIFIED: ran both]:

| Value | Python `msgpack.packb` | rmp-serde `i64` |
|-------|------------------------|-----------------|
| 127 / 128 | `7f` / `cc80` | `7f` / `cc80` |
| 255 / 256 | `ccff` / `cd0100` | `ccff` / `cd0100` |
| 65535 / 65536 | `cdffff` / `ce00010000` | same |
| 2^32 | `cf0000000100000000` | same |
| -1 / -32 / -33 | `ff` / `e0` / `d0df` | same |
| -128 / -129 | `d080` / `d1ff7f` | same |
| 0.9 (f64) | `cb3feccccccccccccd` | same (f32 would be `ca3f666666`) |
| tensor 63 / 64 tok | buffer header `c4` / `c5` | same |
| tensor 16383 / 16384 tok | `c5` / `c6` | same; total msg lengths 65577 / 65583 equal |

A full message, verified identical in both languages: `AbortBackendMsg(uid=7)` = `82a85f5f747970655f5faf41626f72744261636b656e644d7367a375696407`; `ExitMsg()` = `81a85f5f747970655f5fa7457869744d7367`.

**Python-side caveat:** `SamplingParams(temperature=0)` (an int) encodes as `00`, not float64 [VERIFIED: ran, `int-as-temp 00`]. The fixture generator must always pass float literals (`0.0`, `1.0`). Upstream's API server gets floats from pydantic `temperature: float = 1.0` (api_server.py:73).

**Rust decode is lenient:** rmp-serde ignores unknown keys [VERIFIED: ran, `extra key decode -> Ok(())`] and rejects an unknown `__type__` with "unknown variant". That is acceptable for the decode direction. The decode→re-encode byte equality catches schema drift.

### WIRE-02 Flow (D-15), verified prototype
1. The `rsg-wire` integration test `dump` writes `<case>.msgpack` for every case into `$DUMP_DIR`.
2. pytest runs `BaseBackendMsg.decoder(msgpack.unpackb(raw, raw=False))`, or `BaseTokenizerMsg.decoder` for tokenizer messages, then re-encodes with `msgpack.packb(serialize_type(obj), use_bin_type=True)` and asserts `== raw`.

The prototype output: `15 of 15` OK, including `user_len16384.msgpack n=16384 dtype=torch.int32 … hdr=c6`. [VERIFIED: ran]

### Fixture design (D-14/D-16), recommended
- `scripts/gen_wire_fixtures.py` builds the case table, using only upstream classes plus `serialize_type` plus `msgpack.packb(..., use_bin_type=True)`. It writes `fixtures/wire/<case>.msgpack` and `fixtures/wire/manifest.json`. The manifest holds `upstream_sha`, the generator versions (python, torch, msgpack, numpy), and per case `{name, file, top_type, sha256, len, summary}`. The summary is a JSON rendering with buffers shown as length plus the first ids. With `--check`, the script regenerates into a tempdir and byte-diffs.
- Rust tests:
  - (a) For every fixture: decode → re-encode → equals the file. This proves encoder parity with no duplicated case table.
  - (b) For the 7 base cases plus the batch variants: hand-construct the Rust value → encode → equals the file. This guards against symmetric decode/encode bugs.
  - (c) Assert `rsg_wire::UPSTREAM_SHA == manifest.upstream_sha == vendor/UPSTREAM_SHA`.
- Int boundaries live in fields that can carry them: `uid` and `max_tokens` (positive) and `top_k` (negative boundaries). The codec never validates semantics, so `top_k=-33` is legitimate as a codec fixture.

## Handshake (BASE-03)

The JSON line, recommended schema (field names come from D-11; `handshake_version` is an additive recommendation):
```json
{"handshake_version":1,"upstream_sha":"9a91cfafe754aa85daee49998176275667eb58f2","max_seq_len":40960,"eos_token_id":151645,"page_size":1,"max_running_req":256,"num_pages":123456}
```

| Field | Source on the constructed scheduler (verbatim) | Notes |
|-------|-------------------------------|-------|
| `max_seq_len` | `self.max_seq_len = min(config.max_seq_len, num_tokens)` (engine.py:67), as `scheduler.engine.max_seq_len` | The scheduler drops requests where `max_output_len = max_seq_len - input_len` is `<= 0` (scheduler.py:177-183) |
| `eos_token_id` | `self.eos_token_id = self.tokenizer.eos_token_id` (scheduler.py:70) | Qwen3-0.6B: `eos 151645 '<|im_end|>'` [VERIFIED: ran]. Use JSON `null` if None, and make the Rust side `Option<u32>` |
| `page_size` | `self.page_size = page_size` (cache.py:25), as `scheduler.cache_manager.page_size`, built from the post-`_adjust_config` `config.page_size` (scheduler.py:59-61) | Default `page_size: int = 1` (engine/config.py:25). It becomes 64 on SM100 with TRTLLM |
| `max_running_req` | `max_running_req: int = 256` (engine/config.py:20), read as `args.max_running_req` | Not overridden by `_adjust_config` (engine.py:218-233) |
| `num_pages` | `self.num_pages = self._determine_num_pages(init_free_memory, config)` (engine.py:55) | The KV pool allocates `num_pages + 1` including a dummy page (engine.py:59) |

`Scheduler` does not keep the config (`# self.config = config`, scheduler.py:73). The wrapper uses its own `args` reference, which is the same object `Engine` mutated. Qwen3-0.6B expected values: `max_seq_len = min(40960, num_pages*page_size)` (`max_pos 40960` [VERIFIED: ran]; `max_position=config.max_position_embeddings` [VERIFIED: models/config.py:77]).

**SHA single source:** `vendor/UPSTREAM_SHA` (one line). The launcher reads it. `rsg-wire` exposes `pub const UPSTREAM_SHA: &str = include_str!("../../../vendor/UPSTREAM_SHA")` (trimmed at use). The fixture manifest records it. A test asserts all three are equal. Compare the full 40-character SHA.

## Launcher Architecture (BASE-02)

**Python mode:** `os.execv(sys.executable, [sys.executable, "-m", "minisgl", *forwarded])`. This is upstream's documented command (README.md:119 `python -m minisgl --model "Qwen/Qwen3-0.6B"`). The resulting process tree is the baseline's, with zero launcher overhead. That matters for Phase 7. `multiprocessing` spawn skips re-running `minisgl/__main__.py` in children, which is why upstream's own `assert __name__ == "__main__"` (`__main__.py:3`) works. [ASSUMED: from CPython spawn semantics; upstream relies on it]

**Rust mode, step by step:**
1. Launcher flags (`--frontend`, `--rust-bin`, `--ready-timeout`, `--rust-log`) are parsed with `argparse.ArgumentParser(allow_abbrev=False).parse_known_args()`. The rest is forwarded to upstream `parse_args(rest)`. `allow_abbrev=False` stops our parser from swallowing upstream flags by prefix.
2. Reject `--shell-mode` in rust mode. It forces `max_running_req=1` and `silent_output` (args.py:230-234) and has no meaning there.
3. `server_args = replace(server_args, _unique_suffix=f".rsg={os.getpid()}")`. Unlink this suffix's `/tmp/minisgl_{0..4}` files.
4. Process group: if `os.getpgrp() != os.getpid()`, call `os.setpgid(0, 0)`. Run from a non-interactive shell, the launcher is not a group leader: `launcher pid 64251 pgrp before 64249`. After the call, both mp-spawn and Popen children inherit the group: `('mp-child', 64253, 64251)`, `popen-child 64251` [VERIFIED: ran].
5. `subprocess.Popen([rust_bin, --backend-addr, …, --backend-role connect, --detok-addr, …, --detok-role bind|connect, --model, …, --run-id, suffix], stdin=PIPE, stderr=<log file>)`.
6. `mp.set_start_method("spawn", force=True)`. For each rank: `mp.Process(target=rsglang.backend.run_scheduler, args=(replace(server_args, tp_info=DistributedInfo(i, world)), q, sha), daemon=False)`. This mirrors launch.py:52-69.
7. Wait: poll `q.get(timeout=0.5)` in a loop, checking `p.is_alive()` for each rank, `rust.poll()`, and the deadline.
8. On ready: `rust.stdin.write(json.dumps(hs) + "\n"); flush()`. Keep stdin open; closing it means shutdown.
9. Supervise: on any child exit, the timeout, or SIGINT/SIGTERM, set SIGINT/SIGTERM to `SIG_IGN` **at that moment, not earlier**, then `os.killpg(pg, SIGINT)`, wait about 10 s, then `SIGKILL`. Print the stderr tail or the error from the queue, unlink the sockets, and exit non-zero. Ignored dispositions are inherited across exec, which is why they must be set only at shutdown.

**Default ready timeout:** 900 s, configurable. It must cover first-run HF download, weight load and CUDA graph capture. [ASSUMED: value]

**Scheduler-side backstop (recommended):** inside `run_scheduler`, a daemon thread polls `os.getppid()` and `os._exit(1)`s when the parent changes. This is the Python equivalent of Rust's stdin-EOF rule, so a `kill -9` of the launcher does not leak GPU processes. It lives in our wrapper, not in vendored code.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| msgpack codec | Custom byte writer | `rmp-serde` `to_vec_named` + `serde_bytes` | Verified byte-exact, including decode of nested tagged enums |
| Upstream argument parsing | Re-declaring upstream flags | `minisgl.server.args.parse_args` | Imports and runs on the Mac [VERIFIED: ran] |
| Scheduler ZMQ in the Mac fake | Raw pyzmq | Upstream `minisgl.utils.ZmqPullQueue`/`ZmqPushQueue` | Exactly upstream's socket and msgpack settings |
| Fixture encoder | A Python re-implementation of `serialize_type` | Upstream `minisgl.message.utils.serialize_type` | It is the oracle by definition |
| Vendored copy | `cp -R` | `git archive <sha> \| tar -x` | Exact tracked set, symlink preserved, nothing extra |
| Integrity check | Per-file checksums list | Git tree hash `02d3e4ad…` plus a pristine diff | One comparison, nothing to maintain |

## Common Pitfalls

### Pitfall 1: Standalone `SchedulerConfig` flips the `_1` role
**What goes wrong:** the scheduler binds `_1`, Rust also binds it, and the two never connect. Or the rust-mode scheduler differs from Python mode.
**Why:** `SchedulerConfig.backend_create_detokenizer_link` is `True` (config.py:39-41), while `ServerArgs` overrides it to `not share_tokenizer` (args.py:41-43).
**How to avoid:** always build `ServerArgs` through `parse_args`, and derive Rust roles from the property.
**Warning signs:** `Address already in use` on the `_1` bind, or the scheduler never sees a peer.

### Pitfall 2: `page_size` read too early
**What goes wrong:** the handshake reports 1 on a Blackwell box where the scheduler actually uses 64.
**How to avoid:** read `scheduler.cache_manager.page_size` after construction.
**Warning signs:** a mismatch with the `"Page size is overridden to 64 for TRTLLM backend"` log line (engine.py:229).

### Pitfall 3: Full editable install on the Mac
**What goes wrong:** `uv pip install -e vendor/mini-sglang` fails with "Wheels are available for `sgl-kernel` (v0.3.21) on the following platforms: `manylinux2014_aarch64`, `manylinux2014_x86_64`" [VERIFIED: ran].
**How to avoid:** use `--no-deps` plus the explicit Mac requirement list.

### Pitfall 4: pytest collecting vendored tests
**What goes wrong:** a bare `pytest` at the repo root collects `vendor/mini-sglang/tests/kernel/*`, which imports CUDA kernels, and fails. The vendored `pyproject.toml` `addopts` also requires `pytest-cov` (pyproject.toml:111-117).
**How to avoid:** the root `pyproject.toml` sets `testpaths = ["python/tests"]` and `norecursedirs = ["vendor", "target", ".venv"]`. Always run pytest from the repo root so the root config is the inifile.

### Pitfall 5: `--dtype auto` needs the network on the Mac
`parse_args` calls `cached_load_hf_config(model_path)` when `dtype == "auto"` (args.py:251-254), which goes to HF over the network. Mac tests must pass `--dtype bfloat16`. [VERIFIED: ran; `parse_args([... "--dtype","bfloat16","--page-size","16"])` → `OK 16 256 0 .pid=54970`]

### Pitfall 6: Spawn children re-import the main module
With `spawn`, children import the parent's main module as `__mp_main__`. `rsglang/launch.py` must keep all side effects under `if __name__ == "__main__":`. The scheduler target must be a module-level function in an importable module (`rsglang.backend.run_scheduler`). [ASSUMED: CPython multiprocessing semantics]

### Pitfall 7: Tokio stdin blocks shutdown
See Pattern 5. Use a std thread plus `process::exit`. [CITED: tokio docs]

### Pitfall 8: The Mac cannot construct a real `Scheduler`
`Engine.__init__` uses CUDA (engine.py:31-39). To test the full rust-mode path on the Mac, `run_scheduler` should resolve its scheduler class from an env var, for example `RSGLANG_SCHEDULER_FACTORY=rsglang.testing.fake_scheduler:FakeScheduler`, defaulting to `minisgl.scheduler:Scheduler`. The fake:
- exposes `engine.max_seq_len`, `engine.num_pages`, `eos_token_id`, `cache_manager.page_size`, `sync_all_ranks()`, `run_forever()`, `shutdown()`;
- in `run_forever`, opens `ZmqPullQueue(args.zmq_backend_addr, create=True, …)` and `ZmqPushQueue(args.zmq_detokenizer_addr, create=args.backend_create_detokenizer_link, …)`, which copies io.py:36-45, and exits on `ExitMsg`.

### Pitfall 9: Vendored `.gitignore` and symlink
The vendored `.gitignore` ignores `*.json`, `build/` and `dist/` under `vendor/mini-sglang/`. The upstream tree has no tracked JSON, so this is harmless [VERIFIED: ran `git ls-files`]. Never put our fixtures or manifests under `vendor/`. `.dockerignore` is a symlink; preserve it.

### Pitfall 10: Fixture generator uses int literals for float fields
See the Python-side caveat above. Use `0.0`, not `0`.

## Code Examples

### Fixture generator core
```python
# Source: verified pattern ($TMPDIR/fx/gen.py, run this session against vendored-equivalent upstream)
import msgpack, torch
from minisgl.core import SamplingParams
from minisgl.message import (UserMsg, AbortBackendMsg, ExitMsg, BatchBackendMsg,
                             DetokenizeMsg, BatchTokenizerMsg)
from minisgl.message.utils import serialize_type

def enc(m) -> bytes:
    return msgpack.packb(serialize_type(m), use_bin_type=True)   # == utils/mp.py:25 for BaseBackendMsg

def ids(n):  # deterministic, int32, little-endian on all supported hosts (sys.byteorder == 'little')
    return torch.tensor([(i * 7919) % 151936 for i in range(n)], dtype=torch.int32)

base = {
    "user": UserMsg(uid=7, input_ids=ids(3),
                    sampling_params=SamplingParams(temperature=0.0, top_k=-1, top_p=1.0,
                                                   ignore_eos=False, max_tokens=128)),
    "abort": AbortBackendMsg(uid=7),
    "exit": ExitMsg(),
    "batch_backend_mixed": BatchBackendMsg(data=[UserMsg(uid=1, input_ids=ids(3),
                                  sampling_params=SamplingParams()), AbortBackendMsg(uid=2)]),
    "detok": DetokenizeMsg(uid=7, next_token=151645, finished=True),
    "batch_tok_1": BatchTokenizerMsg(data=[DetokenizeMsg(uid=1, next_token=5, finished=False)]),
    "batch_tok_n": BatchTokenizerMsg(data=[DetokenizeMsg(uid=i, next_token=i+4, finished=i % 2 == 0)
                                           for i in range(1, 6)]),
}
```

### WIRE-02 decode check (pytest)
```python
# Source: verified prototype ($TMPDIR/fx/check.py → "15 of 15")
from minisgl.message import BaseBackendMsg, BaseTokenizerMsg
def roundtrip(raw: bytes, decoder) -> None:
    obj = decoder(msgpack.unpackb(raw, raw=False))      # real cls(**kwargs) path
    assert msgpack.packb(serialize_type(obj), use_bin_type=True) == raw
```

### Topology derivation (launcher)
```python
# Source: args.py:25-47, config.py:23-33 (verified); behavior verified by running on Mac
def rust_endpoints(sa) -> dict:
    return {
        "backend_addr": sa.zmq_backend_addr,  "backend_role": "connect",
        "detok_addr":   sa.zmq_detokenizer_addr,
        "detok_role":   "connect" if sa.backend_create_detokenizer_link else "bind",
    }
```

## State of the Art / Corrections to Prior Research

| Prior statement | Correction | Evidence |
|-----------------|------------|----------|
| "Whether `minisgl.message` imports on macOS is unverified"; D-13 file-path fallback | Imports work. So do `scheduler`, `engine`, `server.args` and `server.launch`. No fallback is needed | [VERIFIED: ran, module import loop, no errors; `flashinfer False`, `sgl_kernel False`] |
| ARCHITECTURE.md: "write the codec by hand on rmp/rmpv instead of serde's internally-tagged enums" | rmp-serde internally tagged enums are byte-exact and decode correctly | [VERIFIED: ran, all `match true`, `decoded bb eq true`] |
| Prior research: upstream as a git submodule | Vendored via `git archive` (D-01/D-02) | Decision |
| (not covered) | `page_size` can be overridden at runtime (TRTLLM → 64) | engine.py:227-229 |
| test_scheduler.py as the startup template | Copy its *startup pattern* only. Its `SchedulerConfig` gives the wrong `_1` role for rust mode | config.py:39-41 vs args.py:41-43 |

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | "7 message types" = the 6 boundary messages + `SamplingParams` (Tensor is an encoding) | Wire Codec | Low: the matrix covers all 8 tags anyway. Only the wording in docs/tests changes |
| A2 | Rust 1.99.0 compiles `zmq` 0.10 / `zmq-sys` and the workspace (verified on 1.95.0 only) | Standard Stack | Low: if it fails, pin 1.95 temporarily |
| A3 | The GPU Linux box has a C/C++ toolchain for `zmq-sys`'s bundled libzmq build | Standard Stack | Medium: the build fails, so install `build-essential` |
| A4 | Ready timeout default of 900 s is enough for first-run download plus CUDA graph capture | Launcher | Low: it is configurable |
| A5 | CPython spawn skips `pkg.__main__` re-execution and imports the main module as `__mp_main__` | Launcher, Pitfall 6 | Low: upstream already relies on it |
| A6 | Linux sun_path limit is 108 bytes | Topology | Low: suffixes are short |
| A7 | A `getppid()`-polling watchdog in the scheduler wrapper is acceptable (not a vendored change) | Launcher | Low |
| A8 | The `handshake_version` field and the `{"kind": "ready"/"error"}` queue envelope are acceptable additions to D-11 | Handshake | Low: additive, and both ends are in this repo |

## Open Questions (RESOLVED)

1. **The exact meaning of "7 message types" in ROADMAP/REQUIREMENTS.**
   - Known: there are 6 boundary messages, plus `SamplingParams` and Tensor as nested tags.
   - Recommendation: adopt A1 in test names and docs, and cover all 8 tags in fixtures.
   - RESOLVED (planning, plan 01-04): "7 message types" = UserMsg, AbortBackendMsg, ExitMsg, BatchBackendMsg, DetokenizeMsg, BatchTokenizerMsg, SamplingParams; Tensor is the 8th `__type__` tag. The fixture matrix has a standalone `base_` case for all 8 tags, so WIRE-01 holds under either reading.
2. **GPU type on the GPU box (SM90 vs SM100).**
   - This decides whether the expected handshake `page_size` is 1 or 64. It only affects the manual check's expected values, not the code.
   - RESOLVED (planning, plan 01-05): `scripts/gpu_phase1_check.sh` prints the GPU name and accepts `page_size` 1 or 64; the handshake value is read after scheduler construction, so the code is correct on either.
3. **Whether to use `zeromq` (pure Rust) for the skeleton instead.**
   - Recommendation: no. Phase 1 uses the verified `zmq` behind a trait, and Phase 3 decides.
   - RESOLVED (planning, plan 01-02): Phase 1 uses `zmq` 0.10 behind the `Transport` trait in `crates/rsg-server/src/transport.rs`; the transport crate decision stays with Phase 3 (WIRE-03).

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| uv | Python envs | ✓ | 0.9.2 | — |
| Python 3.12 | Fixture gen, launcher tests | ✓ (fetched by uv) | 3.12.12 | — |
| rustup / cargo | Rust build | ✓ | 1.95.0 local; 1.99.0 installed via rust-toolchain.toml | — |
| C/C++ toolchain (Xcode CLT) | `zmq-sys` bundled libzmq | ✓ (build succeeded) | Apple clang | — |
| git | Vendoring, check script | ✓ | 2.39.3 | — |
| Network to github.com | Pristine fetch in the check script | ✓ this session | — | `--offline` tree-hash mode |
| Network to huggingface.co | Only for `--dtype auto` / real model runs | ✓ this session | — | Pass `--dtype bfloat16` in Mac tests |
| `timeout` (GNU coreutils) | — | ✗ on macOS | — | Python-side timeouts in tests. Don't use `timeout` in scripts |
| CUDA GPU | Criteria 2/3 real run | ✗ (Mac) | — | GPU box manual verification (below) |

**Missing dependencies with no fallback:** none for Mac work. Criteria 2 and 3 (the real backend) need the GPU box.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | `cargo test` (Rust 1.99, built-in harness) + pytest 9.1.1 |
| Config file | none yet. Wave 0 creates the root `Cargo.toml` workspace and the root `pyproject.toml` `[tool.pytest.ini_options]` |
| Quick run command | `cargo test -p rsg-wire && uv run pytest python/tests -x -q -m "not slow"` |
| Full suite command | `cargo test --workspace && uv run pytest python/tests -q && python scripts/gen_wire_fixtures.py --check && scripts/check_wire_decode.sh && python scripts/check_upstream.py` |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| BASE-01 | Vendored tree == pristine 9a91cfa; LICENSE present; UPSTREAM.md lists every diff; frozen tiers enforced | integration (script) | `python scripts/check_upstream.py` (network) / `python scripts/check_upstream.py --offline` | ❌ Wave 0 |
| BASE-01 | The check script itself detects an unlisted edit and a listed-but-frozen edit | unit | `uv run pytest python/tests/test_check_upstream.py -x` (runs against a temp copy) | ❌ Wave 0 |
| BASE-02 | Python mode builds the exact `python -m minisgl <args>` argv (exec injected) | unit | `uv run pytest python/tests/test_launch_args.py -x` | ❌ Wave 0 |
| BASE-02 | Rust mode end to end on the Mac: launcher + fake scheduler (real upstream ZMQ queues) + real Rust skeleton; Rust logs the handshake | integration | `cargo build -p rsg-server && RSGLANG_SCHEDULER_FACTORY=rsglang.testing.fake_scheduler:FakeScheduler uv run pytest python/tests/test_launch_rust_e2e.py -x` | ❌ Wave 0 |
| BASE-02 | Failure handling: a fake scheduler crash → killpg, non-zero exit, stderr tail; ready timeout; launcher SIGKILL → Rust exits on stdin EOF | integration | same file, separate tests | ❌ Wave 0 |
| BASE-02/03 | Real GPU run: Python mode serves `/v1/chat/completions`; rust mode logs real values | manual (GPU) | see the GPU checklist below | n/a |
| BASE-03 | `extract_handshake` reads the right attributes (SimpleNamespace fakes); JSON schema; SHA mismatch → Rust exit≠0 | unit | `uv run pytest python/tests/test_handshake.py -x` + `cargo test -p rsg-server` | ❌ Wave 0 |
| BASE-03 | Topology: Rust roles derived from `ServerArgs` (num_tokenizer 0 → bind `_1`, N>0 → connect) | unit | `uv run pytest python/tests/test_topology.py -x` (real upstream `parse_args`) | ❌ Wave 0 |
| WIRE-01 | Every fixture: Rust decode→encode == bytes; hand-built base cases encode == bytes; SHA consistency | unit | `cargo test -p rsg-wire` | ❌ Wave 0 |
| WIRE-01 | Fixtures are fresh against the vendored code | integration | `python scripts/gen_wire_fixtures.py --check` | ❌ Wave 0 |
| WIRE-02 | Rust-encoded messages decode through the real decoder and re-encode identically | integration | `scripts/check_wire_decode.sh` (cargo test dump → pytest) | ❌ Wave 0 |

### GPU-only manual checklist (criteria 2/3)
1. Build: `cargo build --release -p rsg-server`. Env: the GPU install line above.
2. `python -m rsglang.launch --frontend python --model Qwen/Qwen3-0.6B`, then `curl -s localhost:1919/v1/chat/completions -H 'content-type: application/json' -d '{"model":"x","messages":[{"role":"user","content":"hi"}],"max_tokens":16}'` returns a completion.
3. `python -m rsglang.launch --frontend rust --model Qwen/Qwen3-0.6B --rust-bin target/release/<bin>` produces a Rust log line with `max_seq_len` (≤ 40960), `eos_token_id=151645`, `page_size` (1, or 64 on SM100), `max_running_req=256`, `num_pages>1`, and the SHA.
4. `kill -9 <launcher pid>`: the Rust process and the scheduler exit, and `nvidia-smi` shows no leftover process.
5. `python scripts/check_upstream.py` passes on the GPU box checkout. That is the proof that the Python frontend is unchanged.

### Sampling Rate
- **Per task commit:** the quick run command.
- **Per wave merge:** the full suite command.
- **Phase gate:** the full suite is green on the Mac and the GPU checklist is signed off, before `/gsd-verify-work`.

### Wave 0 Gaps
- [ ] Root `Cargo.toml` (workspace) + `rust-toolchain.toml` (1.99.0)
- [ ] Root `pyproject.toml` for `rsglang`, with pytest `testpaths`/`norecursedirs` (Pitfall 4)
- [ ] Root `.gitignore`: add `target/`, `.venv/`, `__pycache__/`, `*.egg-info/`
- [ ] `python/tests/conftest.py`: tmp suffix fixture, Rust binary path fixture, cleanup of `/tmp/minisgl_*<suffix>`
- [ ] `python/rsglang/testing/fake_scheduler.py`
- [ ] Mac env bootstrap script (uv venv + `--no-deps -e vendor/mini-sglang`)

## Security Domain

`security_enforcement` is on (ASVS L1). This phase has no network listener; HTTP arrives in Phase 5. The attack surface is local.

### Applicable ASVS Categories
| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | No endpoints in Phase 1 |
| V3 Session Management | no | — |
| V4 Access Control | no (local IPC only) | Upstream ipc sockets in `/tmp`; permissions unchanged from upstream |
| V5 Input Validation | yes | Rust: strict serde parse of the handshake (`deny_unknown_fields` on the handshake struct is fine; it is not the wire) plus a SHA equality check; clap-validated CLI. Python: argparse; forwarded args validated by upstream `parse_args` |
| V6 Cryptography | no | sha256 in the manifest is integrity, not security |
| V10/V14 Supply chain & config | yes | Upstream pinned by full SHA plus a tree-hash check. Crates pinned in `Cargo.lock`, Python pins in a lockfile. Human-verify checkpoint for the PyPI SUS verdicts |

### Known Threat Patterns
| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Glob-deleting other users' or other runs' `/tmp/minisgl_*` sockets | DoS | Unlink only this run's exact 5 paths. `/tmp` sticky bit already blocks other users' files |
| Predictable socket path squatting by another local user | Spoofing/DoS | libzmq bind fails loudly if it cannot replace the file. Accept for a single-user dev or GPU box, and document |
| Vendored code silently altered | Tampering | Tree hash `02d3e4ad…` plus the pristine-diff check in the phase gate |
| Orphaned GPU processes after a launcher crash | DoS (resource) | Process group plus killpg, Rust stdin-EOF exit, scheduler `getppid` watchdog |

## Sources

### Primary (HIGH confidence)
- `github.com/sgl-project/mini-sglang` @ `9a91cfafe754aa85daee49998176275667eb58f2`, cloned and read this session: `message/{__init__,utils,backend,tokenizer,frontend}.py`, `core.py`, `utils/{__init__,mp,hf}.py`, `env.py`, `server/{__init__,launch,args,api_server}.py`, `scheduler/{__init__,config,io,scheduler,cache,table}.py`, `engine/{__init__,config,engine}.py`, `distributed/info.py`, `models/config.py`, `tokenizer/{__init__,server,tokenize,detokenize}.py`, `__main__.py`, `shell.py`, `tests/core/test_scheduler.py`, `tests/misc/test_serialize.py`, `pyproject.toml`, `Dockerfile`, `README.md`, `LICENSE`, `.gitignore`, `.dockerignore`
- Code run this session on macOS arm64: uv venv (Python 3.12.12, torch 2.9.1, numpy 2.5.3, msgpack 1.2.3, pyzmq 27.2.0, transformers 4.57.3); throwaway cargo crates `wiretest` (rmp-serde 1.3.1) and `zmqspike` (zmq 0.10.0); Qwen3-0.6B tokenizer and config from HF
- crates.io API (crate versions) and `static.rust-lang.org/dist/channel-rust-stable.toml` (Rust 1.99.0)

### Secondary (MEDIUM confidence)
- docs.rs tokio 1.53.1 `io::Stdin` (cancellation warning)

### Tertiary (LOW confidence)
- None used for decisions.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH. Versions come from registries, and codec and ZMQ behavior were run.
- Architecture (topology, handshake attributes, launcher): HIGH. Read from source with line citations, and topology and parse_args were run on the Mac. The real-GPU handshake values are confirmed only by the manual checklist.
- Pitfalls: HIGH. Most were reproduced (`to_vec` arrays, f32, int temperature, full install failure, stale sockets, pgid).

**Research date:** 2026-10-03
**Valid until:** 2026-11-02 (stable: upstream is pinned by SHA, so only crate and PyPI versions drift)
