# Phase 6: GPU End-to-End Parity - Pattern Map

**Mapped:** 2026-10-06
**Files analyzed:** 6
**Analogs found:** 6 / 6

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `scripts/parity_check.py` | utility/CLI driver | request-response (HTTP + side-channel capture) | `scripts/baseline_profile.py` | exact (same `discover`/`run`/`validate` subcommand shape, same project) |
| `python/rsglang/parity/sidecar.py` (new module, mirrors `profiling/sidecar.py`) | model/schema | file-I/O (JSON read/write + validation) | `python/rsglang/profiling/sidecar.py` | exact (same schema_version/meta/validated-writer pattern) |
| `scripts/gpu_phase6_parity.sh` | utility/test (human-run GPU wrapper) | request-response + process lifecycle | `scripts/gpu_phase1_check.sh` (secondary: `scripts/gpu_phase2_profile.sh`) | exact (identical `record()`/`cleanup()`/`trap`/PASS-FAIL convention) |
| `scripts/<health-watcher>.sh` (D-12 process-health watcher) | utility (process/event monitoring) | event-driven (poll liveness/GPU listing) | `scripts/gpu_phase1_check.sh` (`alive()`, `on_gpu()`, `gpu_pids()`) | exact (reuses same helper functions) |
| ids-capture debug hook in `vendor/mini-sglang/python/minisgl/scheduler/scheduler.py` | service (backend instrumentation, shared-fix) | event-driven (per-message log on existing `reply` loop) | `vendor/mini-sglang/python/minisgl/scheduler/scheduler.py:144-167` (`_process_last_data`) itself — modify in place | exact (same file, additive log line only) |
| Rust-side ids-capture debug dump (new code in `rsg-server` consuming `TokenizerMsg::DetokenizeMsg`) | service/middleware (debug sink) | streaming (per-uid token stream tap) | `crates/rsg-wire/src/lib.rs:146-158` (`TokenizerMsg` enum) + `crates/rsg-server/src/transport.rs` (`DetokSource::recv_detok`) | role-match (new consumer of an existing decoded stream) |
| `python/tests/test_parity_check.py` | test | request-response (unit, Mac-testable) | `python/tests/test_baseline_profile.py` | exact |
| `python/tests/test_<health-watcher>.py` (bash-sourcing Mac test) | test | event-driven (bash helper sourcing) | `python/tests/test_gpu_check_script.py` | exact |
| `docs/benchmarks/parity-report.json` | config/data output | file-I/O (sidecar write) | `docs/benchmarks/baseline-profile.json` | exact |
| `docs/benchmarks/parity-report.md` | documentation output | file-I/O | `docs/benchmarks/baseline-profile.md` | exact |

## Pattern Assignments

### `scripts/parity_check.py` (CLI driver, request-response)

**Analog:** `scripts/baseline_profile.py`

**Imports pattern** (lines 26-39):
```python
from __future__ import annotations

import argparse
import json
import os
import shutil
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "python"))

from rsglang.profiling import analysis, hook, procs, scenarios, session, sidecar  # noqa: E402
```
For `parity_check.py`, mirror this exactly but import a new `rsglang.parity` package (e.g. `from rsglang.parity import corpus, diff, sidecar, session`) instead of `rsglang.profiling`.

**Subcommand shape** (lines 44-128): three-verb pattern `discover` / `run` / `validate`, each `sub.add_parser(...)` with its own flags (`--model`, `--port`, `--timeout`, `--out`, `--work-dir`, `--server-cmd` override for Mac tests). `parity_check.py` should use the same verbs: `discover` (connectivity smoke against `mock-scheduler` for both frontend binaries), `run` (the real 100-prompt sweep — sequential Python-then-Rust per D-04/Pattern 4 from RESEARCH.md), `validate` (schema-check the written JSON, `--require-gpu` flag).

**Validate pattern** (line 124-126):
```python
validate = sub.add_parser("validate", help="Validate a baseline-profile.json sidecar")
validate.add_argument("file", metavar="FILE", type=Path)
validate.add_argument("--require-gpu", action="store_true")
```
Copy directly for `parity-report.json`.

**Exit code convention** (docstring lines 19-23): `0` OK, `1` measurement failure, `2` environment error. Reuse exactly: `1` for any token-id/text mismatch (D-04 zero tolerance) or malformed sidecar; `2` for missing GPU/model/env (e.g. gated Llama checkpoint unavailable).

---

### `python/rsglang/parity/sidecar.py` (new, schema/model, file-I/O)

**Analog:** `python/rsglang/profiling/sidecar.py`

**Header/module docstring convention** (lines 1-7):
```python
"""Sidecar schema, provenance metadata, and a validated atomic writer for
BENCH-01's docs/benchmarks/baseline-profile.json (D-13/D-14).

Standard library only. ...
"""
```
Mirror for the parity sidecar, citing D-07 instead of D-13/D-14.

**Constants pattern** (lines 23-27):
```python
SCHEMA_VERSION = 1
GENERATED_BY = "scripts/baseline_profile.py"
ROLES = ("api_server", "scheduler", "tokenizer")
SCENARIOS = ("s1_cancel", "s2_saturation", "s3_coldstart")
CANONICAL_OUT = "docs/benchmarks/baseline-profile.json"
```
For parity: `SCHEMA_VERSION = 1`, `GENERATED_BY = "scripts/parity_check.py"`, `CANONICAL_OUT = "docs/benchmarks/parity-report.json"`, and a `_META_KEYS` tuple extended with `models` (plural — Qwen3-0.6B + Llama-3.2-1B-Instruct, per D-02/D-03) instead of a single `model` key.

**Provenance capture** (lines 67-91): `_git_commit(repo_root)` and `_git_dirty(repo_root)` via `subprocess.run(["git", "-C", str(repo_root), "rev-parse", "HEAD"], ...)` with a `(OSError, subprocess.TimeoutExpired)` catch returning `None`. Copy verbatim — same `meta.git_commit`/`meta.git_dirty` fields the parity report needs per the `docs/benchmarks/{name}.{md,json}` sidecar convention (Pattern 2 in RESEARCH.md).

**Validated atomic writer**: `SidecarError(ValueError)` wrapping a list of validation error strings (lines 59-64) — reuse the same error-aggregation shape for per-prompt schema violations (missing `token_ids`, mismatched lengths, etc.) in the parity sidecar's `validate_sidecar()`.

---

### `scripts/gpu_phase6_parity.sh` (human-run GPU wrapper)

**Analog:** `scripts/gpu_phase1_check.sh`

**Header/usage convention** (lines 1-32):
```bash
#!/usr/bin/env bash
# Phase N GPU verification: ... A human runs it once at the end of the phase
# and signs off on the PASS/FAIL lines.
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/gpu_phaseN_check.sh [--model ...] [--help]
...
EOF
}
```

**Argument parsing loop** (lines 39-47):
```bash
while [ $# -gt 0 ]; do
  case "$1" in
    --model) MODEL="${2:?--model needs a value}"; shift 2 ;;
    --port) PORT="${2:?--port needs a value}"; shift 2 ;;
    --timeout) TIMEOUT="${2:?--timeout needs a value}"; shift 2 ;;
    --help|-h) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done
```
Add `--llama-model` for D-02's Llama-3.2-1B-Instruct comparison target and `--concurrency` for D-10's fixed concurrent-load point.

**Result-recording + cleanup-trap pattern** (lines 60-75):
```bash
declare -a RESULTS=()
declare -a STARTED_PGIDS=()

record() {  # record <step> <PASS|FAIL> <detail>
  RESULTS+=("$2 step $1: $3")
  echo "$2 step $1: $3"
}

cleanup() {
  local pgid
  for pgid in "${STARTED_PGIDS[@]+"${STARTED_PGIDS[@]}"}"; do
    kill -9 -- "-$pgid" 2>/dev/null || true
  done
}
trap cleanup EXIT
```
Copy verbatim — this is the exact per-criterion PASS/FAIL + orphan-safety-net convention Phase 6's own script must follow (RESEARCH.md Pattern 3).

**Session-spawning helper** (lines 77-104, `start_session()`): reuse unmodified to launch both frontends under `setsid` for the sequential Python-then-Rust sweeps (D-04) and for launching Phase 5's stress tool (D-11).

**Liveness/GPU-listing helpers** (lines 106-114):
```bash
alive() { kill -0 "$1" 2>/dev/null; }
gpu_pids() { nvidia-smi --query-compute-apps=pid --format=csv,noheader | tr -d ' '; }
on_gpu() {  # on_gpu <pid> -> 0 listed, 1 not listed, 2 nvidia-smi failed
  local out
  out="$(gpu_pids)" || { echo "nvidia-smi failed" >&2; return 2; }
  grep -qx "$1" <<<"$out"
}
```
These three functions are also the direct basis for the separate D-12 health-watcher script (see below) — do not duplicate logic, factor into a shared sourced file if both scripts need it, following the project's existing "Mac helper tests source this file" convention (see `test_gpu_check_script.py` header).

---

### `scripts/<health-watcher>.sh` (D-12 process-health watcher)

**Analog:** `scripts/gpu_phase1_check.sh` (same helpers as above: `alive`, `on_gpu`, `gpu_pids`)

Per RESEARCH.md Pitfall 2, liveness alone is insufficient — the predicted abort-during-prefill failure mode is silent KV-cache corruption on a live, running scheduler, not necessarily a crash. Extend beyond `gpu_phase1_check.sh`'s pure liveness check: poll `alive()`/`on_gpu()` around the stress run (crash/zombie detection), AND cross-reference the stress tool's own per-request completion log against `parity_check.py`'s diff output for anomalous/swapped outputs on requests that were *not* aborted but ran concurrently with an aborted one. Document findings in `docs/benchmarks/parity-report.md`'s bisection section (D-05) and `UPSTREAM.md`/`STATE.md` per D-09.

---

### Backend ids-capture hook (shared-fix in `vendor/mini-sglang/python/minisgl/scheduler/scheduler.py`)

**Analog:** the file's own existing natural-finish double-free guard, which is the pattern to extend carefully, not copy verbatim:

```python
# NOTE: overlap scheduling may make the request freed twice, skip second free
if finished and req not in self.finished_reqs:
    self.decode_manager.remove_req(req)
    self._free_req_resources(req)
    new_finished_reqs.add(req)
```
(`scheduler.py:158-162`, read this session per RESEARCH.md Pitfall 2)

**Ids-capture insertion point:** `_process_last_data`'s `reply` list construction (`scheduler.py:144-167`) — add one line logging `(uid, next_token, finished)` for every `DetokenizeMsg` already being built there, applying identically regardless of which frontend is attached. This is additive only (a `logging`/file-append call), not a change to control flow, keeping it a minimal, UPSTREAM.md-recordable shared fix per D-09's "small, localized" branch criteria.

**If D-09's fix-scope triage instead classifies the abort-handling bug itself as fixable:** the guard to extend is the same `if finished and req not in self.finished_reqs:` conditional — the fix must ALSO check `self.finished_reqs` (or equivalent) from the `AbortBackendMsg` branch (`scheduler.py:190-195`) before calling `_free_req_resources` (`scheduler.py:200-202`), since that branch currently calls it unconditionally.

---

### Rust-side ids-capture debug dump (new code in `rsg-server`)

**Analog:** `crates/rsg-wire/src/lib.rs:146-158` (existing `TokenizerMsg` enum, already decoded) + `crates/rsg-server/src/transport.rs:40-48` (`DetokSource::recv_detok`, `Transport` trait)

```rust
// crates/rsg-wire/src/lib.rs:149-154
DetokenizeMsg {
    uid: i64,
    next_token: i64,
    finished: bool,
},
```
```rust
// crates/rsg-server/src/transport.rs:40-43
pub trait DetokSource: Send {
    /// Wait up to `timeout_ms` for one frame; `Ok(None)` on timeout.
    fn recv_detok(&self, timeout_ms: i64) -> anyhow::Result<Option<Vec<u8>>>;
}
```
The Rust frontend already decodes every `TokenizerMsg::DetokenizeMsg { uid, next_token, finished }` off this trait. The new debug-dump path is a thin consumer added alongside the existing FSM decode step: for each decoded `DetokenizeMsg`, append `(uid, next_token, finished)` to a per-run side file/log keyed by uid — no new wire types, no Tier conflict (this is wholly new code, not a vendored-file edit).

---

### `python/tests/test_parity_check.py` (unit test)

**Analog:** `python/tests/test_baseline_profile.py` — follow its structure for CLI-argument-parsing tests, JSON-schema round-trip tests, and `--server-cmd` override usage for Mac-mock-based `discover` smoke tests (same technique `baseline_profile.py`'s own tests use to avoid needing a real server).

### `python/tests/test_<health-watcher>.py` (bash-sourcing test)

**Analog:** `python/tests/test_gpu_check_script.py`

**Header convention** (lines 1-7):
```python
"""Mac-runnable tests for scripts/gpu_phase1_check.sh's GPU-orphan helpers
(G-01-7-WR07 / WR-07, G-01-7-WR08 / WR-08).

Each test sources the script under bash with stub `nvidia-smi` and `setsid`
executables placed first on PATH, so the real preflight, build and GPU steps
never run (the source guard returns before the "# --- Preflight" section).
"""
```

**Stub-executable pattern** (lines 32-60): `_write_stub(tmp_path, name, body)` writes an executable `#!/bin/bash` stub into a `tmp_path/bin` directory placed first on `PATH`; `_write_setsid_stub` writes a Python-shebang stub emulating `setsid`'s exec-in-place behavior with `STUB_SETSID_DELAY`/`STUB_SETSID_MODE` env knobs. Reuse this exact stubbing technique to unit-test the D-12 watcher's `alive()`/`on_gpu()` helpers on the Mac with a stub `nvidia-smi` and stub `ps`.

---

### `docs/benchmarks/parity-report.{md,json}`

**Analog:** `docs/benchmarks/baseline-profile.{md,json}`

**JSON meta block** (from `docs/benchmarks/baseline-profile.json`, structural precedent per RESEARCH.md Pattern 2):
```json
{
  "schema_version": 1,
  "generated_by": "scripts/baseline_profile.py",
  "meta": {
    "created_utc": "2026-10-06T03:17:14Z",
    "git_commit": "d4272b3f...",
    "upstream_sha": "9a91cfafe754aa85daee49998176275667eb58f2",
    "model": "Qwen/Qwen3-0.6B",
    "gpu": "NVIDIA GeForce RTX 3050"
  }
}
```
For `parity-report.json`: same `meta` fields, but `model` becomes `models: ["Qwen/Qwen3-0.6B", "meta-llama/Llama-3.2-1B-Instruct"]` (D-02/D-03), plus a top-level `prompts` array with per-prompt `{prompt_id, category, python_token_ids, rust_token_ids, python_text, rust_text, match: bool}` (D-06), and a `concurrent_load` block for PAR-02's single fixed-concurrency measurement (D-10).

**Markdown narration convention** (lines 1-8 of `baseline-profile.md`):
```markdown
# Python Frontend Baseline Profile

BENCH-01's measured baseline ..., read directly from
`docs/benchmarks/baseline-profile.json` .... Every number below is copied
from that file; none are estimated. Where the JSON carries a `null` for a
metric, this report says so explicitly rather than filling in a number.
```
Copy this framing verbatim for `parity-report.md`, substituting PAR-01/PAR-02 and D-07. Bisection findings (D-05) and the abort-during-prefill reproduction outcome (D-08/D-09) belong in the narrative `.md`, tied back to specific `.json` fields exactly as this convention requires.

## Shared Patterns

### Discover/Run/Validate CLI shape
**Source:** `scripts/baseline_profile.py` lines 44-128
**Apply to:** `scripts/parity_check.py` — same three-verb structure, same `--work-dir`/`--server-cmd`/`--out` flag naming conventions for Mac-testability-first design.

### Validated-sidecar schema/writer
**Source:** `python/rsglang/profiling/sidecar.py` lines 1-91 (`SCHEMA_VERSION`, `_META_KEYS`, `_git_commit`/`_git_dirty`, `SidecarError`)
**Apply to:** new `python/rsglang/parity/sidecar.py` for `parity-report.json`.

### PASS/FAIL GPU bash wrapper with orphan-safety trap
**Source:** `scripts/gpu_phase1_check.sh` lines 1-120 (`record()`, `cleanup()`/`trap`, `start_session()`, `alive()`/`on_gpu()`/`gpu_pids()`)
**Apply to:** `scripts/gpu_phase6_parity.sh` and the D-12 process-health watcher script — both must reuse these exact helper functions rather than reinventing crash/liveness detection.

### `docs/benchmarks/{name}.{md,json}` sidecar convention
**Source:** `docs/benchmarks/baseline-profile.{md,json}`
**Apply to:** `docs/benchmarks/parity-report.{md,json}` — machine JSON with `schema_version`/`meta`/data arrays, hand-written `.md` that narrates only what the JSON carries, explicitly calling out any `null`.

### Bash-sourcing Mac unit tests via stub executables on PATH
**Source:** `python/tests/test_gpu_check_script.py` lines 1-60
**Apply to:** Mac-testable coverage for the D-12 watcher's bash helper functions, following the existing "Mac helper tests source this file" convention already established for `gpu_phase1_check.sh`.

## No Analog Found

| File | Role | Data Flow | Reason |
|------|------|-----------|--------|
| Phase 5's 128-agent stress tool, pointed at real backend (D-11) | test/utility | event-driven | Does not exist in this worktree yet (Phase 5 unplanned as of this research); per D-11 it is reused unmodified once it lands — no new pattern to map, only new CLI args (`--backend-addr`/`--detok-addr`) pointed at real `ipc://` paths, per `crates/rsg-server/src/transport.rs`'s existing `Endpoint`/`Role` types already read above. |

## Metadata

**Analog search scope:** `scripts/`, `python/rsglang/profiling/`, `python/tests/`, `crates/rsg-wire/src/`, `crates/rsg-server/src/`, `docs/benchmarks/`, `vendor/mini-sglang/python/minisgl/scheduler/`
**Files scanned:** 9 (all git-tracked; verified via `git ls-files`)
**Pattern extraction date:** 2026-10-06
