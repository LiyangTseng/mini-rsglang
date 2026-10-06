# Phase 1: Vendored Base & Wire Codec - Pattern Map

**Mapped:** 2026-10-03
**Files analyzed:** 20 (new; zero vendored files modified)
**Analogs found:** 14 / 20 (all analogs are upstream mini-sglang @ 9a91cfa, cited by their future path under `vendor/mini-sglang/`; read from pristine clone `$TMPDIR/msg-up`)

> The repo has no source code yet. No in-repo analogs exist. All analogs are upstream files, which become git-tracked once vendored (tracked-source gate satisfied after the vendoring task). Line numbers refer to upstream @ 9a91cfa.

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|---|---|---|---|---|
| `vendor/mini-sglang/**` | vendored tree | file-I/O | (git archive of 9a91cfa) | n/a |
| `vendor/UPSTREAM_SHA` | config | — | none | none |
| `UPSTREAM.md` | config/doc | — | none (schema in RESEARCH Pattern 2) | none |
| `scripts/check_upstream.py` | utility | file-I/O / batch | none | none |
| `pyproject.toml` (root, `rsglang`) | config | — | `vendor/mini-sglang/pyproject.toml` | role-match |
| `python/rsglang/launch.py` | entrypoint/launcher | process-spawn, request-response (ready queue) | `vendor/mini-sglang/python/minisgl/server/launch.py` (`launch_server`/`start_subprocess`, L40-100) + `python/minisgl/__main__.py` | exact |
| `python/rsglang/backend.py` (`run_scheduler`, `extract_handshake`) | service (process entry) | event-driven (ready queue) | `vendor/mini-sglang/python/minisgl/server/launch.py` `_run_scheduler` L16-37 | exact |
| `python/rsglang/handshake.py` | utility | transform (dict→JSON line) | none | none |
| `python/rsglang/sockets.py` | utility | transform (addresses/roles) | `vendor/mini-sglang/python/minisgl/scheduler/config.py` L8-41 | role-match |
| `python/rsglang/testing/fake_scheduler.py` | test fixture | streaming (ZMQ) | `vendor/mini-sglang/tests/core/test_scheduler.py` L16-60 + `python/minisgl/utils/mp.py` | exact |
| `python/tests/test_launcher*.py` | test | process integration | `vendor/mini-sglang/tests/core/test_scheduler.py` | role-match |
| `python/tests/test_wire_decode.py` (WIRE-02) | test | transform | `vendor/mini-sglang/tests/misc/test_serialize.py` L22-35 | exact |
| `scripts/gen_wire_fixtures.py` | utility | file-I/O / transform | `vendor/mini-sglang/tests/misc/test_serialize.py` + `utils/mp.py` L24-26 | role-match |
| `scripts/check_wire_decode.sh` | utility | batch | none | none |
| `fixtures/wire/*.msgpack`, `manifest.json` | data | — | generated | n/a |
| `Cargo.toml`, `rust-toolchain.toml` | config | — | none | none |
| `crates/rsg-wire/src/lib.rs` | model + codec | transform | `vendor/mini-sglang/python/minisgl/message/{backend,tokenizer}.py`, `message/utils.py`, `core.py` L15-21 | exact (semantic) |
| `crates/rsg-wire/tests/{fixtures.rs,dump.rs}` | test | file-I/O | none in-repo; RESEARCH Fixture design | none |
| `crates/rsg-server/src/main.rs` | entrypoint bin | event-driven (stdin, signals) + ZMQ bind/connect | `vendor/mini-sglang/python/minisgl/utils/mp.py` (bind-vs-connect by `create`) | partial |

## Pattern Assignments

### `python/rsglang/backend.py` (process entry, ready queue)

**Analog:** `vendor/mini-sglang/python/minisgl/server/launch.py` lines 16-37. Clone verbatim; replace the string ack with a handshake dict. Do NOT import `_run_scheduler`.
```python
def _run_scheduler(args: ServerArgs, ack_queue: mp.Queue[str]) -> None:
    import torch
    from minisgl.scheduler import Scheduler

    with torch.inference_mode():
        scheduler = Scheduler(args)
        scheduler.sync_all_ranks()

        if args.tp_info.is_primary():
            ack_queue.put("Scheduler is ready")      # -> put({"kind":"ready", **extract_handshake(...)})

        if args.silent_output:
            logging.disable(logging.INFO)

        try:
            scheduler.run_forever()
        except KeyboardInterrupt:
            logger = init_logger(__name__)
            if args.tp_info.is_primary():
                print()
                logger.info("Scheduler exiting gracefully...")
            scheduler.shutdown()
```
Handshake attribute paths (RESEARCH §Handshake): `scheduler.engine.max_seq_len`, `scheduler.eos_token_id` (may be None), `scheduler.cache_manager.page_size` (read after init), `args.max_running_req`, `scheduler.engine.num_pages`. Add `try/except BaseException` that puts `{"kind":"error","rank":r,"traceback":...}`, plus a ppid-watch daemon thread. Make the `Scheduler` factory injectable for Mac tests.

### `python/rsglang/launch.py` (launcher)

**Analog:** `vendor/mini-sglang/python/minisgl/server/launch.py` lines 40-69 (imports L1-10).
```python
import multiprocessing as mp
from dataclasses import replace
from minisgl.distributed import DistributedInfo
from minisgl.utils import init_logger
...
server_args, run_shell = parse_args(sys.argv[1:], run_shell)   # L44; from minisgl.server.args
mp.set_start_method("spawn", force=True)                        # L52
world_size = server_args.tp_info.size
ack_queue: mp.Queue[str] = mp.Queue()
for i in range(world_size):
    new_args = replace(server_args, tp_info=DistributedInfo(i, world_size))
    mp.Process(target=_run_scheduler, args=(new_args, ack_queue),
               daemon=False, name=f"minisgl-TP{i}-scheduler").start()
```
Rust mode: same loop with `target=rsglang.backend.run_scheduler`, skip the tokenizer-worker spawns (L71-100), first `replace(server_args, _unique_suffix=".rsg=<pid>")`. Python mode: `os.execv(sys.executable, [sys.executable, "-m", "minisgl", *rest])`, matching upstream entry `python/minisgl/__main__.py` L1-5 (`launch_server()`). Launcher flags via `argparse.ArgumentParser(allow_abbrev=False).parse_known_args()`; reject `--shell-mode` in rust mode. Process group / killpg / timeout per RESEARCH Launcher steps 4-9 (no upstream analog; upstream has none).

### `python/rsglang/sockets.py` (addresses and roles)

**Analog:** `vendor/mini-sglang/python/minisgl/scheduler/config.py` lines 8-41.
```python
def _get_pid_suffix() -> str:
    return f".pid={os.getpid()}"
_unique_suffix: str = field(default_factory=_get_pid_suffix)
def zmq_backend_addr(self):     return "ipc:///tmp/minisgl_0" + self._unique_suffix
def zmq_detokenizer_addr(self): return "ipc:///tmp/minisgl_1" + self._unique_suffix
```
Do not re-derive strings: read `server_args.zmq_backend_addr` / `zmq_detokenizer_addr` after the `replace`. Role rule: `detok_role = "connect" if server_args.backend_create_detokenizer_link else "bind"`; backend role always `connect`. Unlink only `/tmp/minisgl_{0..4}<suffix>`. Warning: `SchedulerConfig.backend_create_detokenizer_link` returns `True` (L39-41); always use `ServerArgs`.

### `python/rsglang/testing/fake_scheduler.py` and launcher integration tests

**Analog:** `vendor/mini-sglang/tests/core/test_scheduler.py` lines 16-52 (process + queue + upstream ZMQ wrappers).
```python
@torch.inference_mode()
def scheduler(config: SchedulerConfig, queue: mp.Queue) -> None:
    scheduler = Scheduler(config)
    queue.put(None)
    ...
mp.set_start_method("spawn", force=True)
q = mp.Queue(); p = mp.Process(target=scheduler, args=(config, q)); p.start(); q.get()
send_backend = ZmqPushQueue(config.zmq_backend_addr, create=False, encoder=BaseBackendMsg.encoder)
recv_backend = ZmqPullQueue(config.zmq_detokenizer_addr, create=False, decoder=BaseTokenizerMsg.decoder)
```
The fake must mirror the real scheduler's roles: `ZmqPullQueue(zmq_backend_addr, create=True, decoder=BaseBackendMsg.decoder)` and `ZmqPushQueue(zmq_detokenizer_addr, create=args.backend_create_detokenizer_link, encoder=BaseTokenizerMsg.encoder)` (scheduler/io.py L36-45). It exposes `engine.max_seq_len`, `eos_token_id`, `cache_manager.page_size`, `engine.num_pages`, `sync_all_ranks`, `run_forever`, `shutdown` so `extract_handshake` runs unchanged. Upstream tests use `@call_if_main` scripts; ours should be plain pytest functions (`testpaths=["python/tests"]`, `norecursedirs` includes `vendor`).

### `python/tests/test_wire_decode.py` (WIRE-02) and `scripts/gen_wire_fixtures.py`

**Analog:** `vendor/mini-sglang/tests/misc/test_serialize.py` lines 1-35, plus the packing settings in `python/minisgl/utils/mp.py` L24-26 / L66-68.
```python
from minisgl.core import SamplingParams
from minisgl.message import BatchBackendMsg, UserMsg
from minisgl.message.utils import serialize_type, deserialize_type
t = torch.tensor([1, 2, 3], dtype=torch.int32)
u = BatchBackendMsg([UserMsg(uid=0, input_ids=t, sampling_params=SamplingParams())])
result = u.decoder(u.encoder())
```
```python
event = msgpack.packb(self.encoder(obj), use_bin_type=True)   # encode (mp.py:25)
return self.decoder(msgpack.unpackb(event, raw=False))        # decode (mp.py:68)
```
Decode test: `BaseBackendMsg.decoder` / `BaseTokenizerMsg.decoder` (backend.py L17-19, tokenizer.py L17-19) on `msgpack.unpackb(raw, raw=False)`, then assert `msgpack.packb(serialize_type(obj), use_bin_type=True) == raw`. Generator: always pass float literals (`temperature=0.0`) since an int encodes as `00`. Imports work directly (namespace package); no file-path loading.

### `crates/rsg-wire/src/lib.rs` (wire types + codec)

**Analog (schema source of truth):** `vendor/mini-sglang/python/minisgl/message/backend.py` L22-41, `message/tokenizer.py` L22-31, `core.py` L15-21; encoding rules in `message/utils.py` L20-35:
```python
if isinstance(self, torch.Tensor):
    serialized["__type__"] = "Tensor"
    serialized["buffer"] = self.numpy().tobytes()
    serialized["dtype"] = str(self.dtype)
    return serialized
serialized["__type__"] = self.__class__.__name__     # __type__ FIRST
for k, v in self.__dict__.items():                   # then fields in declaration order
    serialized[k] = _serialize_any(v)
```
Decoder `cls(**kwargs)` (utils.py L63-69) means no extra keys, ever. Rust form: RESEARCH Pattern 3 (verified byte-exact): `#[serde(tag="__type__")]` enums `BackendMsg {UserMsg, AbortBackendMsg, ExitMsg {}, BatchBackendMsg}` and `TokenizerMsg {DetokenizeMsg, BatchTokenizerMsg}`, structs `Tensor` (`serde_bytes` buffer, dtype `"torch.int32"`) and `SamplingParams` (`temperature: f64, top_k: i64, top_p: f64, ignore_eos: bool, max_tokens: i64`), `rmp_serde::to_vec_named`. `pub const UPSTREAM_SHA = include_str!("../../../vendor/UPSTREAM_SHA")` (trim). No I/O deps; `thiserror` for errors.

### `crates/rsg-server/src/main.rs` (skeleton)

**Analog (partial):** `vendor/mini-sglang/python/minisgl/utils/mp.py` L12-22, L54-64: one socket per wrapper, bind-or-connect chosen by a flag.
```python
self.socket = self.context.socket(zmq.PUSH)
self.socket.bind(addr) if create else self.socket.connect(addr)
```
Mirror as CLI `--backend-addr/--backend-role connect`, `--detok-addr/--detok-role bind|connect` behind a small `Transport` trait (`zmq` 0.10). Stdin on dedicated `std::thread` → tokio mpsc; `select!` with `ctrl_c()`/SIGTERM. Exit codes: signal 0, malformed/SHA mismatch 2, stdin EOF 3 (RESEARCH Pattern 5). `anyhow`, `tracing`, `clap` derive.

## Shared Patterns

### Wire packing settings (apply to: fixture generator, WIRE-02 test, fake scheduler, rsg-wire)
**Source:** `vendor/mini-sglang/python/minisgl/utils/mp.py` L25 and L68: `use_bin_type=True` on pack, `raw=False` on unpack. Rust equivalent: `to_vec_named` + `serde_bytes`.

### Spawn + ready queue (apply to: launch.py, backend.py, fake scheduler tests)
**Source:** `server/launch.py` L52-69 and `tests/core/test_scheduler.py` L36-40: `mp.set_start_method("spawn", force=True)`, `mp.Queue`, `daemon=False`, readiness only from `args.tp_info.is_primary()`.

### Argument parsing (apply to: launch.py both modes)
**Source:** `server/launch.py` L42-44: `from minisgl.server.args import parse_args`; `server_args, _ = parse_args(argv, run_shell)`. On Mac pass `--dtype` explicitly.

### SHA single source (apply to: launcher, rsg-wire, manifest, tests)
`vendor/UPSTREAM_SHA` = `9a91cfafe754aa85daee49998176275667eb58f2`; a test asserts equality of all three consumers.

## No Analog Found

| File | Role | Data Flow | Reason / use instead |
|---|---|---|---|
| `scripts/check_upstream.py` | utility | file-I/O | RESEARCH Pattern 1-2 (tree hash `02d3e4ad...`, `git ls-files --cached --others --exclude-standard`, Tier A/B paths) |
| `UPSTREAM.md` | doc | — | table `| Path | Reason | Shared backend fix (yes/no) |`, empty in Phase 1 |
| `python/rsglang/handshake.py` | utility | transform | RESEARCH §Handshake JSON schema (`handshake_version: 1`) |
| `scripts/check_wire_decode.sh` | utility | batch | RESEARCH WIRE-02 Flow (cargo test dump → pytest with `DUMP_DIR`) |
| `crates/rsg-wire/tests/*.rs` | test | file-I/O | RESEARCH Fixture design (a)/(b)/(c) |
| `Cargo.toml`, `rust-toolchain.toml` | config | — | RESEARCH Recommended Project Structure (resolver 3, edition 2024, 1.99.0) |

## Metadata

**Analog search scope:** upstream `python/minisgl/{server,message,utils,scheduler,tokenizer}`, `tests/`, at `$TMPDIR/msg-up` (9a91cfa). Repo itself has no source.
**Files scanned:** 12
**Pattern extraction date:** 2026-10-03
