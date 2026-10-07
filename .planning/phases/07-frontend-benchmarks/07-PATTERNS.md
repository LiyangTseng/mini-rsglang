# Phase 7: Frontend Benchmarks - Pattern Map

**Mapped:** 2026-10-06
**Files analyzed:** 13 (new crate + extended module)
**Analogs found:** 11 / 13

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|---|---|---|---|---|
| `crates/rsg-bench/Cargo.toml` | config | — | `crates/rsg-server/Cargo.toml` | exact |
| `crates/rsg-bench/src/main.rs` | controller (CLI entry) | request-response | `crates/rsg-server/src/main.rs` | exact |
| `crates/rsg-bench/src/orchestrator.rs` | service | batch (alternating trials) | `python/rsglang/profiling/scenarios.py` (overall session driver) + `docs/benchmarks/baseline-profile.json` schema | role-match |
| `crates/rsg-bench/src/loadgen/closed_loop.rs` | service | streaming | `python/rsglang/profiling/scenarios.py::run_s2` (`AsyncOpenAI` gather pattern) | role-match |
| `crates/rsg-bench/src/loadgen/open_loop.rs` | service | streaming | `python/rsglang/profiling/scenarios.py::run_s2` | role-match |
| `crates/rsg-bench/src/cancel.rs` | utility | event-driven | *(no direct analog; new client-cancel logic)* | none |
| `crates/rsg-bench/src/sse.rs` | utility | streaming/transform | *(no direct analog — hand-rolled `data:` parser, CLAUDE.md explicit)* | none |
| `crates/rsg-bench/src/metrics.rs` | utility | transform | *(no prior hdrhistogram use in repo; docs.rs API only)* | none |
| `crates/rsg-bench/src/memory.rs` | utility | file-I/O (`/proc` read) | `python/rsglang/profiling/procs.py::tree_memory` | exact (cross-language port) |
| `crates/rsg-bench/src/teardown.rs` | utility | event-driven (signals) | `python/rsglang/profiling/procs.py::teardown` | exact (cross-language port) |
| `crates/rsg-bench/src/scenarios/s3_coldstart.rs` | service | batch (shell-out) | `python/rsglang/profiling/scenarios.py::hyperfine_argv`/`parse_hyperfine_json` | exact (cross-language port) |
| `crates/rsg-bench/tests/common/mod.rs` | test | process spawn | `crates/rsg-server/tests/common/mod.rs` | exact |
| `python/rsglang/profiling/hook.py` (extend, not new) | utility (instrumentation) | event-driven | itself — `install()` lines ~240-280 | exact (same file, add mode toggle) |

## Pattern Assignments

### `crates/rsg-bench/Cargo.toml` (config)

**Analog:** `crates/rsg-server/Cargo.toml`

Full existing file to copy workspace-dependency style from:
```toml
[package]
name = "rsg-server"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[dependencies]
anyhow.workspace = true
clap.workspace = true
rsg-wire.workspace = true
rustc-hash.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
tokio.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
zmq.workspace = true

[dev-dependencies]
proptest.workspace = true
```
`rsg-bench` needs new workspace-level deps added to root `Cargo.toml`'s `[workspace.dependencies]` first (`hdrhistogram = "7.6.0"`, `reqwest = "0.13.5"`, `sysinfo = "0.39.6"`, `nix = "0.31.3"`, `tokio-util`, `futures`, `tokio-stream` as needed), following the existing flat key = "version" style at `Cargo.toml:11-24`, then referenced via `.workspace = true` in the new crate's `Cargo.toml`, exactly like `rsg-server`'s block above. `rsg-bench` is a **new workspace member** — add it to `members = ["crates/*"]` is already a glob, so no root change needed beyond the new dependency pins.

---

### `crates/rsg-bench/src/main.rs` (controller, request-response)

**Analog:** `crates/rsg-server/src/main.rs`

**Imports + CLI pattern** (lines 1-44):
```rust
use clap::Parser;
use tracing_subscriber::EnvFilter;

/// Exit codes, documented per-code like rsg-server's EXIT_* consts.
const EXIT_OK: i32 = 0;
const EXIT_STARTUP: i32 = 1;

#[derive(Parser, Debug)]
#[command(name = "rsg-server", about = "mini-rsglang Rust frontend")]
struct Cli {
    #[arg(long, value_name = "ADDR")]
    backend_addr: String,
    #[arg(long, value_enum)]
    backend_role: Role,
    // ... etc
}
```
Copy this shape exactly for `rsg-bench`'s CLI: a `Cli` struct with `#[derive(Parser)]`, named exit-code consts documented by doc-comment, and `tracing_subscriber::EnvFilter` init. `rsg-bench`'s top-level flag should be `--mode closed|open` (D-01) as a `clap::ValueEnum`, mirroring `Role`'s enum pattern used by `--backend-role`/`--detok-role` in the same file and in `mock-scheduler.rs`.

**Subcommand dispatch:** Since `rsg-bench` has 3 scenario runners plus an orchestrator wrapping all three (D-08), use `clap::Subcommand` (e.g. `rsg-bench s1|s2|s3|all`) rather than one flat flag set — not present in `rsg-server` (single-purpose binary) but a natural extension of the same `clap::Parser` derive convention; `mock-scheduler.rs`'s flat `Cli` (lines 34-61) is the right template for each subcommand's own argument struct.

---

### `crates/rsg-bench/src/orchestrator.rs` (service, batch/alternating trials)

**Analog:** `docs/benchmarks/baseline-profile.json` (manifest schema) + `scripts/baseline_profile.py` (the "script writes its own JSON sidecar" convention, D-13/D-14 inherited from Phase 2)

**Manifest shape to reuse verbatim and extend** (`docs/benchmarks/baseline-profile.json:1-24`):
```json
{
  "schema_version": 1,
  "generated_by": "scripts/baseline_profile.py",
  "meta": {
    "created_utc": "2026-10-06T03:17:14Z",
    "platform": "linux",
    "python": "3.12.3",
    "git_commit": "d4272b3f42f59ca1e7dd5a2f73ec4550dc1549ff",
    "git_dirty": false,
    "upstream_sha": "9a91cfafe754aa85daee49998176275667eb58f2",
    "model": "Qwen/Qwen3-0.6B",
    "gpu": "NVIDIA GeForce RTX 3050"
  }
}
```
`rsg-bench`'s orchestrator serializes an equivalent Rust struct with `serde_json` (`generated_by = "crates/rsg-bench"`), extended per D-07 with: `rustc` version, every CLI flag passed to both frontend and harness, RNG seed(s), run count N, and raw per-run hdrhistogram files. Keep `schema_version` as a top-level int for forward compatibility, same as the Python sidecar.

**Alternation logic (D-05/D-06/D-08):** no existing Rust analog; implement as a plain `Vec<Trial>` loop alternating `Frontend::Python`/`Frontend::Rust` for `n` iterations (default 5 pairs = 10 trials), calling into whichever scenario runner (`s1_cancel::run_trial`, `s2_saturation::run_trial`, `s3_coldstart::run_trial`) was selected by the CLI subcommand — one shared function signature across all three, per D-08's explicit "don't reimplement per scenario" instruction.

---

### `crates/rsg-bench/src/loadgen/{closed_loop,open_loop}.rs` (service, streaming)

**Analog:** `python/rsglang/profiling/scenarios.py::run_s2` (lines 190-240, read this session)

**Core async-gather / per-request wrapper pattern** (lines 196-230):
```python
async def run_s2(base_url, *, requests=512, max_input=32, output_tokens=32, seed=42):
    random.seed(seed)
    async with AsyncOpenAI(base_url=f"{base_url}/v1", api_key="dummy") as client:
        model = await get_model_name(client)
        ...
        async def _wrap(prompt: str) -> RequestRecord:
            t_send = time.perf_counter()
            try:
                result = await benchmark_one(client, prompt, output_tokens, model, pbar=False)
                tics = result.tics
                t_first = tics[1] if len(tics) > 1 else None
                t_end = tics[-1]
                outcome = "completed" if t_first is not None else "failed"
            except Exception as exc:
                t_first = None
                t_end = time.perf_counter()
                outcome = "failed"
                error = str(exc)
            return RequestRecord(t_send=t_send, t_first=t_first, t_end=t_end, outcome=outcome, error=error)
        return list(await asyncio.gather(*(_wrap(p) for p in prompts)))
```
Port the **shape** (record `t_send` before the call, catch errors as a recorded `outcome="failed"` rather than propagating, record `t_first`/`t_end` for TTFT/E2E) into Rust using `reqwest`'s streaming body + `tokio::spawn`/`futures::future::join_all` for closed-loop (fixed concurrency `requests::gather`-equivalent), and a Poisson-arrival spawn loop for open-loop. Each per-request future records into the shared `hdrhistogram::Histogram` (see `metrics.rs`) instead of returning a Python dataclass.

**Error-handling pattern:** never let one request's failure abort the whole batch — catch at the per-request future boundary and record `outcome = "failed"`, exactly like the Python `except Exception as exc` block above.

---

### `crates/rsg-bench/src/sse.rs` (utility, streaming/transform)

**No analog in this repo.** CLAUDE.md explicitly forbids `eventsource-stream` (unmaintained) and warns `/generate`'s framing is `data: <text>\n` (single newline, not spec SSE). Write a ~20-line hand-rolled splitter over `reqwest::Response::bytes_stream()`, buffering partial lines across chunk boundaries. No existing Rust line-buffering code in the workspace to copy from; this is genuinely new.

---

### `crates/rsg-bench/src/metrics.rs` (utility, transform)

**No analog in this repo** (first use of `hdrhistogram` in the workspace). Use the verified docs.rs API directly:
```rust
use hdrhistogram::Histogram;

let mut ttft_us: Histogram<u64> = Histogram::new_with_bounds(1, 60_000_000, 3)?; // 1us..60s, 3 sigfigs
ttft_us.record(measured_ttft_micros)?;
let p99 = ttft_us.value_at_percentile(99.0);
```
Wrap three histograms (TTFT, ITL, E2E) behind one `Metrics` struct shared across loadgen futures via `Arc<Mutex<Metrics>>` or per-task-local histograms merged at the end (`Histogram::add`).

---

### `crates/rsg-bench/src/memory.rs` (utility, file-I/O)

**Analog:** `python/rsglang/profiling/procs.py::tree_memory` (lines 261-293, read this session — exact excerpt already in RESEARCH.md Pattern 3)

```python
def tree_memory(top_pid: int) -> dict:
    pids_to_check = [top_pid]
    top = psutil.Process(top_pid)
    pids_to_check.extend(c.pid for c in top.children(recursive=True))
    is_linux = sys.platform.startswith("linux")
    pss_ok = True
    rss_total = pss_total = 0
    for pid in pids_to_check:
        proc = psutil.Process(pid)
        rss_total += proc.memory_info().rss
        if is_linux and pss_ok:
            try:
                pss_total += proc.memory_full_info().pss
            except psutil.AccessDenied:
                pss_ok = False
    pss_bytes = pss_total if (is_linux and pss_ok) else None
    return {"rss_bytes": rss_total, "pss_bytes": pss_bytes}
```
**Port exactly**, three-way gate intact: `sysinfo::System` walks the process tree and sums `Process::memory()` for RSS (always); a small Linux-only helper opens `/proc/<pid>/smaps_rollup`, parses the `Pss:` line, sums per-pid, and the aggregate is `Option<u64>` — `None` on non-Linux or on any read/permission error, never `Some(0)`. This is the #1 pitfall flagged in RESEARCH.md (`sysinfo` alone cannot produce PSS).

---

### `crates/rsg-bench/src/teardown.rs` (utility, event-driven signals)

**Analog:** `python/rsglang/profiling/procs.py::teardown` (lines ~220-255, read this session)

```python
try:
    os.killpg(pgid, signal.SIGINT)
except (ProcessLookupError, PermissionError):
    pass
# ... grace period wait ...
try:
    os.killpg(pgid, signal.SIGKILL)
except (ProcessLookupError, PermissionError):
    pass
```
Rust port (confirmed signatures via docs.rs, `nix` 0.31.3 — first use of `nix` in this workspace):
```rust
use nix::sys::signal::{killpg, Signal};
use nix::unistd::Pid;
let _ = killpg(Pid::from_raw(pgid), Signal::SIGINT); // graceful first
// ... grace period ...
let _ = killpg(Pid::from_raw(pgid), Signal::SIGKILL); // ESRCH/EPERM both mean "already gone"
```
Both signal attempts are intentionally ignore-on-error (`let _ =` / `except ... pass`) because "process already gone" is a success state, not a failure, exactly matching the Python pattern's two bare `except` clauses.

---

### `crates/rsg-bench/src/scenarios/s3_coldstart.rs` (service, batch/shell-out)

**Analog:** `python/rsglang/profiling/scenarios.py::hyperfine_argv` / `parse_hyperfine_json` (lines ~260-300, read this session)

```python
def hyperfine_argv(*, hyperfine, runs, warmup, export_json, once_cmd, stop_cmd):
    return [
        hyperfine, "--runs", str(runs), "--warmup", str(warmup),
        "--export-json", str(export_json),
        "--conclude", shlex.join(list(stop_cmd)),
        shlex.join(list(once_cmd)),
    ]

def parse_hyperfine_json(path):
    doc = json.loads(Path(path).read_text(encoding="utf-8"))
    results = doc.get("results")
    if not results:
        raise ValueError(f"{path}: hyperfine export has no 'results'")
    result = results[0]
    times = list(result["times"])
    return {
        "mean_s": result["mean"], "stddev_s": result["stddev"], "median_s": result["median"],
        "min_s": result["min"], "max_s": result["max"], "times_s": times, "runs": len(times),
    }
```
**Port verbatim**, not redesign: build the same `std::process::Command` argv shape (`--runs`, `--warmup`, `--export-json`, `--conclude <shell-escaped stop_cmd>` + `<shell-escaped once_cmd>` — use `shell-escape`-equivalent crate or manual quoting, mirroring `shlex.join`'s guarantee), then parse the same `results[0]` fields defensively (explicit error on missing `"results"` key, per the V5 input-validation note in RESEARCH.md's Security Domain section — never a bare `["results"][0]` without a guard).

---

### `crates/rsg-bench/tests/common/mod.rs` (test, process spawn)

**Analog:** `crates/rsg-server/tests/common/mod.rs` (lines 1-70+, read this session)

```rust
static COUNTER: AtomicUsize = AtomicUsize::new(0);

pub struct MockScheduler {
    pub backend_addr: String,
    pub detok_addr: String,
    child: Child,
    stdin: Option<ChildStdin>,
    stdout_lines: Arc<Mutex<Vec<String>>>,
    stderr_lines: Arc<Mutex<Vec<String>>>,
}

impl MockScheduler {
    pub fn spawn(extra_args: &[&str]) -> MockScheduler {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let pid = std::process::id();
        let backend_addr = format!("ipc:///tmp/rsgm-{pid}-{n}-0");
        let detok_addr = format!("ipc:///tmp/rsgm-{pid}-{n}-1");

        let mut child = Command::new(env!("CARGO_BIN_EXE_mock-scheduler"))
            .args(["--backend-addr", &backend_addr, "--backend-role", "bind",
                   "--detok-addr", &detok_addr, "--detok-role", "connect"])
            .args(extra_args)
            .env("RUST_LOG", "info")
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
            .spawn()
            .expect("spawn mock-scheduler");
        // ... stdout/stderr line-collector threads ...
    }
}
```
`rsg-bench`'s test harness should **reuse this helper directly** (via a `dev-dependencies` path reference, or a small wrapper module) rather than copy-pasting it — RESEARCH.md's Wave-0 gap list flags this exact choice as a planner decision. If direct reuse across crates proves awkward (the struct lives in `rsg-server`'s own `tests/` dir, not its `src/`, so it's not importable as a library item), copy the file into `crates/rsg-bench/tests/common/mod.rs` with the same `COUNTER`/`spawn(extra_args)` shape and unique-ipc-address convention (`ipc:///tmp/rsgm-{pid}-{n}-0`), additionally spawning `rsg-server` itself for the full Mac dev-pass (D-03) pointed at the same addresses, per RESEARCH.md Pattern 5.

---

### `python/rsglang/profiling/hook.py` (extend, not new)

**Analog:** itself, `install()` (lines ~248-280, read this session)

```python
gc.callbacks.append(_gc_callback)

if not tracemalloc.is_tracing():
    tracemalloc.start(1)
```
**Required change (D-15 / Pitfall 2):** gate the `tracemalloc.start(1)` call behind a new env var, e.g.:
```python
PROFILE_MODE_ENV = "RSGLANG_PROFILE_MODE"  # "full" (default, Phase 2 behavior) | "gc_only"

mode = os.environ.get(PROFILE_MODE_ENV, "full")
gc.callbacks.append(_gc_callback)  # always: near-zero overhead, needed in both modes
if mode == "full" and not tracemalloc.is_tracing():
    tracemalloc.start(1)
```
Keep `write_shim()`/`hook_env()`/the sitecustomize chain-load mechanism completely unchanged — only `install()`'s body gains the mode check, preserving backward compatibility with every existing Phase 2 caller (which doesn't set `RSGLANG_PROFILE_MODE` and therefore still gets `"full"`).

## Shared Patterns

### CLI argument parsing (`clap::Parser` derive)
**Source:** `crates/rsg-server/src/main.rs` lines 24-44, `crates/rsg-server/src/bin/mock-scheduler.rs` lines 34-61
**Apply to:** `rsg-bench`'s `main.rs` and every scenario subcommand struct.
```rust
#[derive(Parser, Debug)]
#[command(name = "...", about = "...")]
struct Cli {
    #[arg(long, value_name = "ADDR")]
    backend_addr: String,
    #[arg(long, value_enum)]
    backend_role: Role,
    #[arg(long, default_value_t = 0)]
    prefill_delay_ms: u64,
}
```

### Documented exit codes as named consts
**Source:** `crates/rsg-server/src/main.rs` lines 15-21, `crates/rsg-server/src/bin/mock-scheduler.rs` lines 19-25
**Apply to:** `rsg-bench`'s `main.rs` — give each failure mode (bad manifest write, hyperfine missing, subprocess spawn failure) a named, doc-commented `const EXIT_*: i32`.

### Process-group spawn/teardown symmetry
**Source:** `python/rsglang/profiling/procs.py::teardown`, `crates/rsg-server/tests/common/mod.rs::MockScheduler::spawn`
**Apply to:** `rsg-bench`'s scenario runners that spawn `rsg-server`/`mock-scheduler`/the Python frontend subprocess tree — always a dedicated process group at spawn, `SIGINT` then `SIGKILL` at teardown with ignored "already gone" errors, matching both the Python harness side and the Rust test-harness side so Mac-dev and GPU-run teardown behave identically.

### Defensive external-JSON parsing
**Source:** `python/rsglang/profiling/scenarios.py::parse_hyperfine_json`
**Apply to:** `rsg-bench`'s hyperfine-JSON parser and any `vllm`/`sglang` cross-check output parser — explicit schema check with a clear error, never a bare indexing chain into untrusted JSON.

### Manifest/report "script writes its own JSON sidecar" convention
**Source:** `docs/benchmarks/baseline-profile.json`, `scripts/baseline_profile.py` (D-13/D-14 origin)
**Apply to:** `rsg-bench`'s `report.rs` — the orchestrator itself serializes and writes both the `docs/benchmarks/*.json` manifest and the paired `.md` narrative directly, not via a separate post-processing script.

## No Analog Found

| File | Role | Data Flow | Reason |
|---|---|---|---|
| `crates/rsg-bench/src/cancel.rs` | utility | event-driven | No existing client-side cancellation-timing code anywhere in the repo (Phase 5's throwaway stress test is explicitly out of scope per D-02/D-04 in 05-CONTEXT.md); build fresh using a seeded RNG (`rand` crate, not yet a workspace dep) for think-time/abort-after-N-tokens, driving a `reqwest` stream drop |
| `crates/rsg-bench/src/sse.rs` | utility | streaming/transform | No SSE/chunked-line parser exists in Rust yet; CLAUDE.md explicitly rules out the obvious crate (`eventsource-stream`, unmaintained) — write the ~20-line splitter fresh, as RESEARCH.md's "Don't Hand-Roll" table already scopes it |
| `crates/rsg-bench/src/metrics.rs` | utility | transform | First use of `hdrhistogram` in this workspace; no prior percentile-recording code to copy from (the frozen Python baseline's own percentile logic is a manual sort-and-index array, explicitly *not* the pattern to copy per RESEARCH.md's Don't-Hand-Roll table) |

## Metadata

**Analog search scope:** `crates/rsg-server/` (all source + tests), `python/rsglang/profiling/` (`hook.py`, `procs.py`, `scenarios.py`), `docs/benchmarks/baseline-profile.json`, `scripts/baseline_profile.py`, workspace root `Cargo.toml`
**Files scanned:** 9 read directly this session (all confirmed git-tracked via `git ls-files`)
**Pattern extraction date:** 2026-10-06
