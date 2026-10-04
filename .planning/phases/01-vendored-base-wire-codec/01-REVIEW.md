---
phase: 01-vendored-base-wire-codec
reviewed: 2026-10-04T04:44:35Z
depth: standard
files_reviewed: 36
files_reviewed_list:
  - .gitignore
  - Cargo.toml
  - UPSTREAM.md
  - crates/rsg-server/Cargo.toml
  - crates/rsg-server/src/handshake.rs
  - crates/rsg-server/src/main.rs
  - crates/rsg-server/src/transport.rs
  - crates/rsg-server/tests/cli.rs
  - crates/rsg-wire/Cargo.toml
  - crates/rsg-wire/src/lib.rs
  - crates/rsg-wire/tests/common/mod.rs
  - crates/rsg-wire/tests/dump.rs
  - crates/rsg-wire/tests/fixtures.rs
  - pyproject.toml
  - python/rsglang/__init__.py
  - python/rsglang/backend.py
  - python/rsglang/handshake.py
  - python/rsglang/launch.py
  - python/rsglang/sockets.py
  - python/rsglang/testing/__init__.py
  - python/rsglang/testing/fake_scheduler.py
  - python/tests/test_check_upstream.py
  - python/tests/test_handshake.py
  - python/tests/test_launch_args.py
  - python/tests/test_launch_rust_e2e.py
  - python/tests/test_topology.py
  - python/tests/test_wire_decode.py
  - requirements-mac.in
  - rust-toolchain.toml
  - scripts/bootstrap_mac_env.sh
  - scripts/check_all.sh
  - scripts/check_upstream.py
  - scripts/check_wire_decode.sh
  - scripts/gen_wire_fixtures.py
  - scripts/gpu_phase1_check.sh
  - vendor/UPSTREAM_SHA
findings:
  critical: 1
  warning: 5
  info: 9
  total: 15
status: issues_found
---

# Phase 1: Code Review Report

**Reviewed:** 2026-10-04T04:44:35Z
**Depth:** standard
**Files Reviewed:** 36
**Status:** issues_found

## Narrative Findings (AI reviewer)

## Summary

I reviewed the Phase 1 sources: the rsg-wire msgpack codec, the rsg-server skeleton (CLI, handshake, ZMQ transport), the Python launcher, scheduler wrapper, socket and handshake helpers, the integrity scripts and the tests.

The wire codec is solid. I checked the encoding rules against upstream `message/utils.py`, `core.py` and `utils/mp.py`: named maps, `__type__` first, f64 floats, `serde_bytes` buffer, and dataclass field order all match. The fixture and decode-check pipeline covers the integer and bin header widths that matter.

Most defects are in the launcher's supervision and shutdown logic:

- **Ctrl-C (CR-01).** A Ctrl-C in a terminal reliably exits with code 1 and prints a failure report. I reproduced this 3 out of 3 times.
- **Shell-mode guard (WR-01).** The guard can be bypassed by an argparse abbreviation. I reproduced this.
- **Watchdog race (WR-02).** The parent-death watchdog can miss a launcher that dies early.
- **Process group (WR-03).** The `setpgid` call detaches the launcher from the terminal's foreground group when it runs under a wrapper.
- **Handshake schema (WR-04).** The Rust handshake silently accepts a missing `eos_token_id`. I verified this with a serde probe.

Both items in `deferred-items.md` are already known and are not repeated as new findings: the killpg of pipeline siblings and the one unexplained e2e escalation. WR-03 and WR-05 are related process-group issues, but they are different failure modes.

## Critical Issues

### CR-01: Ctrl-C (SIGINT to the whole process group) makes the launcher exit 1 and report a failure

**File:** `python/rsglang/launch.py:232-246` and `python/rsglang/launch.py:261-275`
**Issue:** An interactive terminal sends SIGINT to the whole foreground process group. When the launcher is started from a shell, it leads that group, and rsg-server and the scheduler ranks are in it too.

1. rsg-server exits 0 within milliseconds (`exit_on_signal`).
2. The launcher's handler only sets `stop_requested`. Because of PEP 475, `ready_queue.get(timeout=...)` then keeps waiting for the rest of its 0.2 s or 0.5 s timeout.
3. When the wait returns, both loops run `children()` **before** they re-check `stop_requested`. They see `rsg-server exited with code 0`, call `shutdown(1)`, dump the rsg-server stderr tail as a failure, and exit 1.

I reproduced this 3 out of 3 times with the fake scheduler: `start_new_session=True`, then `os.killpg(pid, SIGINT)` after "handshake sent". Each run printed `rsg-server exited with code 0`, then `exit code 1`, and the process exited with `EXIT 1`.

The same race exists before ready (lines 236-242). The intended contract is that a stop signal exits 0, and the tracer test asserts this for SIGTERM. That test only signals the launcher pid, never the group, so it misses this path. Any harness that stops rust mode the way `gpu_phase1_check.sh` stops python mode (`kill -INT -- -$pgid`) will see a spurious failure.

**Fix:** Re-check the stop flag after every blocking wait and before judging child exits:
```python
        try:
            msg = ready_queue.get(timeout=_SUPERVISE_POLL_S)
        except queue.Empty:
            msg = None
        if stop_requested:
            return shutdown(0)
        ...
```
Do the same in the ready-wait loop: check `stop_requested` right after the `get` and before the `children()` scan. Add an e2e test that sends SIGINT to the launcher's process group and asserts exit 0.

## Warnings

### WR-01: The `--shell-mode` rejection can be bypassed by an abbreviation, and rust mode then runs silently with shell-mode limits

**File:** `python/rsglang/launch.py:101` and `python/rsglang/launch.py:123`
**Issue:** The guard is a literal check, `"--shell-mode" in rest`. Upstream's `parse_args` builds its parser with the default `allow_abbrev=True`, so `--shell` or `--shell-m` also turns on shell mode. Line 123 throws away the returned `run_shell` (`server_args, _ = parse_args(rest)`).

Upstream has already rewritten the arguments for shell mode by then. I verified that `parse_args([... "--shell"])` returns `run_shell=True, max_running_req=1, cuda_graph_max_bs=1, silent_output=True`. The run proceeds with one running request and CUDA graphs capped at batch size 1, which would silently corrupt any benchmark.
**Fix:**
```python
server_args, run_shell = parse_args(rest)
if run_shell:
    _log("--shell-mode is not supported with --frontend rust")
    return 2
```
Keep the cheap literal pre-check if you like, but make the parsed flag authoritative. Also add a test that uses `--shell`.

### WR-02: The parent watchdog records its parent pid too late and can miss a launcher that has already died, leaving a GPU scheduler orphaned

**File:** `python/rsglang/backend.py:61` and `python/rsglang/backend.py:75`
**Issue:** `start_parent_watchdog` reads `parent = os.getppid()` only once `run_scheduler` starts. With the `spawn` start method, that happens after a fresh interpreter has booted and unpickled `ServerArgs`. Unpickling imports `minisgl.server.args` and, through it, torch, which takes seconds.

If the launcher is SIGKILLed in that window, the child has already been reparented to init or a subreaper. `parent` is then recorded as the new parent, `os.getppid() != parent` never becomes true, and the scheduler goes on to load weights and hold the GPU forever. D-12 exists to prevent exactly that orphan. rsg-server is not affected because its stdin-EOF rule covers it.
**Fix:** Pass the launcher's pid explicitly and compare against it. On Linux you can also set the parent-death signal:
```python
# launcher: args=(rank_args, ready_queue, upstream_sha, os.getpid())
def start_parent_watchdog(launcher_pid: int, poll_interval: float = 1.0):
    if os.getppid() != launcher_pid:
        os._exit(1)          # launcher already gone before we started
    def _watch():
        while True:
            time.sleep(poll_interval)
            if os.getppid() != launcher_pid:
                os._exit(1)
    ...
```
On Linux, `prctl(PR_SET_PDEATHSIG, SIGKILL)` through ctypes, followed by the same `getppid` re-check, closes the window completely.

### WR-03: `setpgid(0, 0)` takes the launcher out of the terminal's foreground process group whenever a wrapper starts it, so Ctrl-C never reaches it

**File:** `python/rsglang/launch.py:128-129`
**Issue:** When the launcher is not a group leader, it moves itself and all later children into a new process group. That happens under any wrapper: a bash script run from a terminal, `make`, `time`, and `uv run` or `timeout` depending on whether they forward signals.

The new group is a background group of the same session. A terminal Ctrl-C then goes only to the wrapper's group. A bash script, for example, just keeps waiting for its child, so the launcher, rsg-server and the GPU-holding scheduler keep running with no way to interrupt them from the terminal. Background groups are also stopped by SIGTTOU when they write to a terminal that has `stty tostop` set.

This is the opposite case to the known pipeline item in `deferred-items.md`. That item is about the launcher already leading the group. This one is about it not leading the group. Both come from the same "children share the launcher's group" design, so one decision should cover both.
**Fix:** Do not move the launcher itself. Start the children in their own new group, either with `process_group=0` / `start_new_session` for rsg-server and an explicit `os.setpgid` in each scheduler's bootstrap, or with one dedicated child group whose pgid the launcher records. Then `killpg` that group, not `os.getpgrp()`. The launcher stays in the terminal's foreground group and receives Ctrl-C normally.

### WR-04: The Rust handshake accepts a line with no `eos_token_id` key, which the documented contract forbids

**File:** `crates/rsg-server/src/handshake.rs:18-26`
**Issue:** The doc comment says "Every key is required (`eos_token_id` may be `null`)". serde's derive treats a missing `Option<T>` field as `None` even with `deny_unknown_fields`. I verified this with a scratch crate: `{"a":1}` deserializes to `eos_token_id: None`.

A launcher bug that drops the key would therefore be logged as `eos_token_id=null` and accepted, instead of failing with exit 2. `missing_key_is_malformed` only removes `num_pages`, so the test suite does not catch it.
**Fix:** An explicit `deserialize_with` turns off serde's implicit default for `Option`, so a missing key becomes an error:
```rust
#[serde(deserialize_with = "Option::deserialize")]
pub eos_token_id: Option<u64>,
```
Add a unit test that removes `"eos_token_id":...` and expects `Malformed`.

### WR-05: `shutdown()` always re-sends SIGINT to the group, which interrupts upstream's graceful `scheduler.shutdown()` after an external group SIGINT

**File:** `python/rsglang/launch.py:193`
**Issue:** In the CR-01 scenario, every child has already received one SIGINT. The upstream scheduler is then inside `except KeyboardInterrupt: scheduler.shutdown()`, which runs `torch.cuda.synchronize`, `sync_all_ranks()` (a barrier) and `engine.shutdown()` (`destroy_process_group`).

`shutdown()` then sends a second SIGINT to the group (`os.killpg(os.getpgrp(), SIGINT)`). That raises a new `KeyboardInterrupt` inside the handler, so cleanup is aborted halfway. With TP > 1, ranks that are still in the barrier wait for the 10 s grace period and are then SIGKILLed. Upstream's own launcher sends nothing extra on Ctrl-C.
**Fix:** Track why shutdown started. If the stop came from a signal the children have already received (a group signal), wait for the grace period first and send SIGINT only to children that are still alive, using per-pid `os.kill` instead of a second group-wide `killpg`. Keep the group signal for launcher-initiated shutdowns, such as a crash or a ready timeout.

## Info

### IN-01: Struct-level wire decoders do not validate `__type__`

**File:** `crates/rsg-wire/src/lib.rs:58-64` and `crates/rsg-wire/src/lib.rs:104-112`
**Issue:** `#[serde(tag = "__type__")]` on a **struct** is honoured when encoding but ignored when decoding. I verified that `decode::<Tensor>` accepts a map tagged `"SamplingParams"` and also a map with no tag, and that `decode::<SamplingParams>` accepts `"XamplingParams"`. The enum variants are validated, as `unknown_type_tag_is_rejected` shows, but the nested `input_ids` and `sampling_params` are not. The byte-exact re-encode fixture tests catch upstream drift, so the impact is limited to how lenient decoding is.
**Fix:** Write a small custom `Deserialize`, or add a `#[serde(rename = "__type__")] _type: TypeTag<"Tensor">`-style field that checks the value. Alternatively, document that struct tags are not checked.

### IN-02: A stdin read error is reported as "stdin EOF" and exits 3, even for a malformed (non-UTF-8) handshake

**File:** `crates/rsg-server/src/main.rs:64-66` and `crates/rsg-server/src/main.rs:159-162`
**Issue:** `lines()` returns `InvalidData` for a non-UTF-8 line. The reader turns that into `StdinEvent::Error`, which logs "launcher went away (stdin EOF)" and exits with `EXIT_STDIN_EOF` (3) instead of `EXIT_BAD_HANDSHAKE` (2). The diagnosis is wrong.
**Fix:** Before the handshake, map `ErrorKind::InvalidData` to `EXIT_BAD_HANDSHAKE` with a "malformed handshake line" message, and keep exit 3 for real I/O errors and EOF.

### IN-03: The upstream SHA is included separately in two crates

**File:** `crates/rsg-server/src/handshake.rs:16` and `crates/rsg-wire/src/lib.rs:27`
**Issue:** rsg-server does not depend on rsg-wire, so both crates `include_str!` `vendor/UPSTREAM_SHA` on their own. The comment says the expected SHA is the one "the Rust wire fixtures were generated from", but rsg-server never reads the codec's constant.
**Fix:** In a later phase, when rsg-server depends on rsg-wire, use `rsg_wire::UPSTREAM_SHA`.

### IN-04: Socket-suffix validation and unlinking have small gaps

**File:** `python/rsglang/sockets.py:17-35`
**Issue:**
- `_SUFFIX_RE.match` with `$` accepts a trailing `"\n"`.
- `unlink_run_sockets` tolerates only `FileNotFoundError`. A stale `/tmp/minisgl_*` file owned by another user, for example left by an earlier `sudo` run with a reused pid, raises `PermissionError` from the `finally` in `run_rust_mode`. That hides the real return code or exception.
**Fix:** Use `_SUFFIX_RE.fullmatch(suffix)`. Catch `PermissionError` in the unlink loop and log it.

### IN-05: The `--rust-log` default always overrides the user's `RUST_LOG`

**File:** `python/rsglang/launch.py:60-61` and `python/rsglang/launch.py:148`
**Issue:** The default is the literal `"info"`, so an exported `RUST_LOG=debug` is silently replaced.
**Fix:** `default=None`, and set `RUST_LOG` only when `--rust-log` is given (or when `RUST_LOG` is unset).

### IN-06: `resolve_rust_bin` prefers a possibly stale release build, and its error message is misleading

**File:** `python/rsglang/launch.py:65-78`
**Issue:** If `target/release/rsg-server` exists, it wins over a freshly built `target/debug/rsg-server`. The handshake SHA check does not detect code staleness. When an explicit `--rust-bin` path is missing, the message still says "run: cargo build -p rsg-server".
**Fix:** Log which binary was chosen. Name the explicit path in the error. Consider picking the newer of the two by mtime.

### IN-07: The rsg-server CLI tests wait a fixed 100 ms for the stderr drain thread

**File:** `crates/rsg-server/tests/cli.rs:140-141`
**Issue:** The assertions on `s.stderr()` right after `wait_exit` (for example `contains("handshake rejected")`) rely on a sleep. That can flake under parallel `cargo test` load.
**Fix:** Keep the drain thread's `JoinHandle` in `Server` and join it after the child exits. The pipe reaches EOF once the process is gone.

### IN-08: The e2e cleanup signals pids and pgids after they may have been reaped

**File:** `python/tests/test_launch_rust_e2e.py:114-128`
**Issue:** `os.killpg(self.proc.pid, SIGKILL)` and `os.kill(pid, SIGKILL)` run on remembered ids after the processes have usually exited and been reaped. If the pid is reused, the test kills an unrelated process or group.
**Fix:** Signal only while `self.proc.poll() is None`. For children, check that the process still belongs to the run's group (`os.getpgid(pid) == self.proc.pid`) before killing it.

### IN-09: `bootstrap_mac_env.sh --relock` hardcodes Apple Silicon

**File:** `scripts/bootstrap_mac_env.sh:30-31`
**Issue:** `--python-platform aarch64-apple-darwin` produces a lock that is wrong for Intel Macs.
**Fix:** Derive the platform from `uname -m`, or document that only arm64 is supported.

---

_Reviewed: 2026-10-04T04:44:35Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
