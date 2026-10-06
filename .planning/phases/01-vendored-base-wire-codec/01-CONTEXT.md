# Phase 1: Vendored Base & Wire Codec - Context

**Gathered:** 2026-10-03
**Status:** Ready for planning

<domain>
## Phase Boundary

This phase delivers four things:
- A pinned, attributed copy of mini-sglang @ `9a91cfa` in the repo.
- One Python launch command that runs the shared backend with either the frozen Python frontend or a Rust frontend skeleton.
- A readiness handshake delivered to the Rust frontend: `max_seq_len`, `eos_token_id`, `page_size`, `max_running_req` (plus extras listed below).
- A Rust msgpack codec that is byte-exact with upstream's encoder for all 7 message types. It is proven by golden fixtures and by upstream's real Python decoder, and the check runs on the Mac.

Requirements: BASE-01, BASE-02, BASE-03, WIRE-01, WIRE-02.

Not in this phase:
- ZMQ transport ordering and the single-writer design (WIRE-03, Phase 3).
- The mock scheduler (Phase 3).
- Tokenizer (Phase 4).
- HTTP API (Phase 5).

The Phase 1 Rust binary only connects, receives the handshake and logs it.

</domain>

<decisions>
## Implementation Decisions

### Vendor layout & tracking
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

### Launcher shape & topology
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

### Readiness handshake
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

### Golden fixtures & decoder check
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

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Project scope & requirements
- `.planning/ROADMAP.md` §Phase 1: goal and the 5 success criteria.
- `.planning/REQUIREMENTS.md`: BASE-01..03 and WIRE-01..02, plus WIRE-03 and MOCK-01 for what is *not* in this phase.
- `.planning/PROJECT.md`: Constraints (IPC byte-exactness, fair comparison, frozen frontend, MIT license) and Key Decisions.
- `.claude/CLAUDE.md`: the stack pins (tokio, rmp-serde 1.3.1, serde_bytes, zmq 0.10 / zeromq 0.6, Rust 1.99 edition 2024) and the §0 ground truth on the upstream wire format and socket addresses.

### Research (upstream read at 9a91cfa)
- `.planning/research/SUMMARY.md`: recommended approach, the launcher-shim pattern, and open Phase 1 questions (bind/connect, macOS import).
- `.planning/research/ARCHITECTURE.md`: socket topology standalone vs with `ServerArgs`, the `rsg-wire` crate role, and the shim role.
- `.planning/research/PITFALLS.md`: Pitfall 1 (unstable upstream boundary, golden fixtures), the msgpack encoding pitfalls (named maps, `serde_bytes`, Tensor dtype, batch wrappers), and the integration-gotchas table.
- `.planning/research/STACK.md`: crate versions and the rmp-serde tagged-enum caveat.

### Upstream source (after vendoring, under `vendor/mini-sglang/`)
- `python/minisgl/message/{backend,tokenizer,frontend,utils}.py`: message dataclasses, `serialize_type`, and the decoder (`cls(**kwargs)`).
- `python/minisgl/utils/mp.py`: ZMQ socket wrappers and the msgpack pack/unpack settings.
- `python/minisgl/server/{launch,args}.py`: process topology, ready signalling, `_unique_suffix`, `num_tokenizer`.
- `python/minisgl/scheduler/{config,scheduler,io}.py`: socket addresses, bind roles, and where the handshake values live.
- `tests/core/test_scheduler.py`: the pattern for starting a standalone scheduler.
- `LICENSE`: the MIT notice, which must be kept.

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- The repo has no code yet; only `.planning/` exists. Everything in this phase is new.
- Upstream assets to reuse without modification:
  - `serialize_type` and the message classes, as the fixture generator and decoder oracle.
  - `minisgl.server.args` parsing, reused by the launcher.
  - The `_run_scheduler` and `test_scheduler.py` startup pattern, reused by the handshake wrapper.

### Established Patterns
- Upstream signals scheduler readiness over a `multiprocessing.Queue`. The Python launcher can receive readiness the same way, then forward the handshake to Rust over stdin.

### Integration Points
- `vendor/mini-sglang/` is installed editable into the uv env. The launcher in `python/rsglang/` imports from it.
- The Rust skeleton binary is spawned by the launcher. Its interface is CLI flags (static config) plus one stdin JSON line (handshake).

### Open questions for research
- Can the four handshake values (and `num_pages`) be read from a constructed upstream `Scheduler`, its config, or the model config, without patching? Where exactly does each one live?
- What are the exact bind/connect roles for each of `minisgl_0`..`_4` in upstream's default Python mode?
- Does `import minisgl.message` work on macOS with CPU torch, or is the file-path loading fallback (D-13) needed?
- What map key order does `serialize_type` produce (is `__type__` first or last)? How do rmp's signed and unsigned integer width choices compare with Python msgpack's?
- What exactly are the 7 message types (confirm against `message/*.py`)?

</code_context>

<specifics>
## Specific Ideas

- "Same backend code" (BASE-03 / criterion 3) means the same `Scheduler` class. The handshake wrapper sits in our launcher, not in vendored code.
- Cold-start friendliness drove D-10. Rust and backend start in parallel, so front-half work overlaps weight loading.
- The SHA check in D-11 exists specifically to make wire-version mismatches fail loudly rather than hang silently.

</specifics>

<deferred>
## Deferred Ideas

- **Capture real upstream message sequences as extra fixtures.** Do this in Phase 6, on the GPU box, while running real traffic for PAR-01.
- **Empty `input_ids`.** Upstream's `torch.frombuffer` path likely rejects an empty buffer. Treat it as a Phase 5 validation rule (Rust must never send an empty prompt), not as a codec fixture.

</deferred>

---

*Phase: 01-vendored-base-wire-codec*
*Context gathered: 2026-10-03*
