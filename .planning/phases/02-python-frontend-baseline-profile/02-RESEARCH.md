# Phase 2: Python Frontend Baseline Profile - Research

**Researched:** 2026-10-05
**Domain:** Python process profiling (CPU/GIL sampling, memory allocation tracking, GC instrumentation) of a multi-process asyncio + multiprocessing serving system, plus cold-start/PSS measurement
**Confidence:** MEDIUM — the profiling *libraries* are well-documented (py-spy, tracemalloc, gc, psutil, hyperfine), but the specific mechanism for injecting instrumentation into the vendored process tree **without touching vendored code** rests on a documented-elsewhere pattern (coverage.py's `.pth` subprocess hook) that has not been tried against this exact venv/multiprocessing combination yet.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**Profiling method**
- **D-01:** Primary CPU time / GIL contention profiler is **py-spy**, run against the frontend processes (and, per D-05, also the scheduler process).
- **D-02:** Memory profiling (allocation + resident growth) uses **tracemalloc** plus periodic RSS sampling, not `memory_profiler` or scalene's built-in tracking.
- **D-03:** GC pause measurement uses a **`gc.callbacks` hook** (count, duration, timestamp for P99-spike correlation), not `PYTHONDEVMODE`/`gc.set_debug` log parsing.
- **D-04:** Serialization and IPC cost is measured via **py-spy sampling across all frontend processes** (tokenizer/detokenizer/api_server hops in `mp.py`), not hand-added wrapper timers.

**Workload driver for the 3 scenarios**
- **D-05:** Overall driver is a **small throwaway Python script**, reusing `minisgl.benchmark.client` / `bench_simple.py` helpers where they fit — not a third-party tool, not an in-place extension of `bench_simple.py`.
- **D-06:** Scenario 1 (128 concurrent agents, dynamic cancellations) uses a **minimal asyncio/aiohttp script** with a configurable early-cancel fraction.
- **D-07:** Scenario 2 (32-token saturation, RPS) **adapts `bench_simple.py`'s client helpers**, with `MAX_INPUT` pinned near 32 tokens.
- **D-08:** Scenario 3 (cold start + RAM) uses **`hyperfine` + `/v1/models` polling**, with PSS sampled across the whole process tree.

**Radix cache time attribution**
- **D-09:** Measure the scheduler's radix-cache time share via **py-spy sampling against the scheduler process**, bucketing sampled stack frames under `scheduler/cache.py` (`match_req`, `cache_req`) and `kvcache/radix_cache.py` (`match_prefix`, `insert_prefix`, `evict`, `_tree_walk`) as "radix time". Reversible: purely observational, no vendored code touched.
- **D-10:** The radix share's denominator is **radix-attributed sampled time / total scheduler-process sampled time** (not end-to-end request latency).
- **D-11:** Radix-cache sampling runs across **all 3 benchmark scenarios** (piggybacking on the same py-spy pass that already samples the scheduler process for GC/memory/GIL in each scenario run), not just scenario 2.

**Report format & location**
- **D-12:** The profiling report lives at **`docs/benchmarks/baseline-profile.md`**, not a `.planning/phases/...` artifact.
- **D-13:** The report pairs the markdown with a machine-readable sidecar, **`docs/benchmarks/baseline-profile.json`**.
- **D-14:** The **profiling script itself writes `baseline-profile.json` directly**; the markdown narrative is hand-written afterward, reading from the JSON.

### Claude's Discretion
- Exact py-spy invocation flags (sampling rate, `--native` or not), the precise `gc.callbacks` wiring, and the internal script/module layout under wherever the Phase 2 scripts live are left to the planner/executor.

### Deferred Ideas (OUT OF SCOPE)
None — discussion stayed within phase scope.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| BENCH-01 | Profile the Python frontend on the GPU machine and quantify its host-side overhead in the three scenarios — GC pauses (count, duration, correlation with P99 spikes), memory allocation and resident growth, GIL contention between tokenize/detokenize/HTTP handling, serialization and IPC cost — as input for benchmark design and attribution; also record the scheduler's time share spent in the radix cache for v2 evaluation | See Architecture Patterns (process topology + zero-touch instrumentation), Code Examples (gc.callbacks wiring, tracemalloc periodic snapshot, py-spy role identification, speedscope bucketing), Common Pitfalls (GIL-contention framing, PID-role ambiguity, PSS tree summation), Validation Architecture |
</phase_requirements>

## Summary

This phase profiles the frozen Python frontend of a 3-process serving topology (with the default `--num-tokenizer 0` and `--tp-size 1` used in Phase 1's GPU check) rather than a generic Python server: one `python -m minisgl` process that execs in place of the launcher and runs the FastAPI/uvicorn api_server, plus exactly two `multiprocessing.Process` children spawned by that same process before `uvicorn.run()` is called — one scheduler (TP rank 0) and one combined tokenizer+detokenizer worker (`tokenize_worker`, since `share_tokenizer=True` when `num_tokenizer=0`). This was confirmed by reading `vendor/mini-sglang/python/minisgl/server/launch.py`, `api_server.py`, and `python/rsglang/launch.py` directly — it is not a guess. The three constraints that shape every tool choice are: (1) no vendored file may be edited (observational only), (2) none of the processes set an OS-visible process title (`setproctitle`/`prctl` — confirmed absent by grep), so PIDs cannot be told apart by `ps`, and (3) the profiling script must be re-runnable unattended on the GPU machine and must itself emit `docs/benchmarks/baseline-profile.json`.

Two non-obvious findings drive the recommended design. First, because `run_api_server()` calls `start_backend()` (which blocks until every worker's ready-ack is received) **before** `uvicorn.run(...)`, the first successful `GET /v1/models` is a reliable synchronization point: by the time it returns 200, every child PID already exists and the process tree is stable — a profiling script can launch the server itself, capture the top PID from its own `Popen`/`os.execv` call, poll `/v1/models`, and only then enumerate `psutil.Process(pid).children()` with no race. Second, telling the scheduler child apart from the tokenizer/detokenizer child (needed for D-09, which targets the scheduler specifically) cannot rely on PID ordering (multiprocessing gives no ordering guarantee visible from outside) — the reliable, non-invasive method is a single `py-spy dump --pid <child>` per child immediately after boot, grepping the dumped stack for `_run_scheduler` (unique to `server/launch.py:16`) vs `tokenize_worker` (unique to `tokenizer/server.py:31`).

For in-process instrumentation (`gc.callbacks`, `tracemalloc`) without touching vendored code, the right mechanism — used by `coverage.py` for exactly this problem — is a `.pth` file dropped into the project's own venv `site-packages`, env-var-gated, executed by the `site` module on every interpreter start including `multiprocessing`-spawned children. This is documented and battle-tested for coverage.py's own use case, but has not been verified against this project's exact venv + spawn combination; the Assumptions Log and Open Questions below flag it for a cheap pre-flight check before the planner locks in the wiring.

**Primary recommendation:** Build one Python driver script (plus thin bash wrapper mirroring `scripts/gpu_phase1_check.sh`'s `--help`/flag conventions) that (a) launches the server itself to get a race-free top PID, (b) role-identifies children via one-shot `py-spy dump`, (c) installs `gc.callbacks` + periodic `tracemalloc` snapshotting via a `.pth`-file hook gated by an env var, (d) runs each of the 3 scenario drivers while `py-spy record -f speedscope` samples the scheduler (and, for D-04, the frontend processes) in parallel, (e) buckets speedscope frames into "radix" vs "other" by name match, and (f) writes `docs/benchmarks/baseline-profile.json` directly, with `docs/benchmarks/baseline-profile.md` written by hand afterward from that JSON.

## Architectural Responsibility Map

This project's processes do not map cleanly onto the generic Browser/SSR/API/CDN/DB tiers — it is a systems/serving project with a Python frontend process group and a Python/CUDA backend process, per `.claude/CLAUDE.md`'s own Constraints section. The table below adapts the concept to this project's actual tiers (Frontend process group / Backend process / new Tooling layer this phase introduces).

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| HTTP request handling (FastAPI/uvicorn) | Frontend process (api_server, top-level exec'd PID) | — | `api_server.py` runs in the same process that `rsglang.launch --frontend python` execs into; confirmed by reading `python/rsglang/launch.py:85-92` |
| Tokenization + detokenization | Frontend process (combined `tokenize_worker` child) | — | `tokenize_worker` (`tokenizer/server.py:31`) handles both `TokenizeManager.tokenize` and `DetokenizeManager.detokenize` in one process when `num_tokenizer=0` (default) |
| GC pauses, memory allocation/RSS growth | Frontend process group (api_server + tokenize_worker) + Backend process (scheduler) | — | BENCH-01 asks for frontend GC/memory, but the scheduler is also a long-lived Python process worth sampling for comparison; D-09/D-11 explicitly also sample the scheduler |
| Radix-cache time share | Backend process (scheduler, TP rank 0) | — | `CacheManager`/`RadixPrefixCache` live only inside the scheduler process (`scheduler/cache.py`, `kvcache/radix_cache.py`), confirmed by reading both files |
| Profiling orchestration, PID discovery, JSON sidecar emission | New Tooling layer (this phase's script, outside both processes) | — | Purely observational; must not modify either tier |
| Cold-start / PSS measurement | Cross-cutting (whole process tree: api_server + scheduler + tokenize_worker) | — | D-08 requires summing PSS across every process in the tree, not one tier |

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard | Conf. |
|---------|---------|---------|--------------|-------|
| `py-spy` | **0.4.2** (PyPI latest, confirmed via `pip index versions py-spy`) [VERIFIED: PyPI registry] | Sampling CPU/GIL profiler, attaches to a running PID without modifying it | D-01/D-04/D-09 lock this in; it is the standard non-invasive Python sampling profiler and is what `--native` needs for native-extension frames | HIGH (choice, locked by CONTEXT.md) / MEDIUM (exact flags, websearch-sourced) |
| `tracemalloc` | stdlib (built into Python ≥3.10, no install) | Memory allocation tracking, snapshot diff | D-02 locks this in; it is the stdlib tool for this, needs no new dependency | HIGH |
| `gc` (`gc.callbacks`) | stdlib | GC pause count/duration/timestamp hook | D-03 locks this in | HIGH |
| `psutil` | **7.2.2** (PyPI latest) [VERIFIED: PyPI registry]; **not currently pinned in `requirements-mac.txt`** (grep confirmed) | RSS/PSS reads per-PID, `Process.children()` for process-tree discovery | D-08 ("PSS sampled across the whole process tree") needs this; `api_server.py`'s own `shell()` function already uses `psutil.Process().children(recursive=True)` as in-repo precedent (`server/api_server.py:404-408`) | MEDIUM — package itself is well-known, but the package-legitimacy check in this session could not confirm download counts (see audit below) |
| `hyperfine` | **1.20.0** per existing `.claude/CLAUDE.md` stack decision; not a PyPI package (Rust/cargo or brew) | Repeated cold-start timing | D-08 locks this in; already an approved project dependency from Phase 0 stack research, not re-audited this session | HIGH |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `json`, `time`, `os`, `multiprocessing` (stdlib) | stdlib | JSON sidecar emission, timestamps, PID handling | Always — this phase's script is pure stdlib + the 2 above for orchestration |
| `pytest` | **9.1.1** (already project-pinned per STATE.md Phase 1 decision) | Mac-side unit tests of the parsing/bucketing logic | For the JSON-sidecar-schema tests and speedscope-bucketing tests (see Validation Architecture) |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `py-spy` | `cProfile`/`austin` | `cProfile` requires code changes (deterministic profiling, not sampling-attach) — violates "observational only"; `austin` is a close py-spy alternative but CONTEXT.md D-01 already locked py-spy, so not explored further |
| `tracemalloc` + manual RSS sampling | `memory_profiler`, `scalene` | CONTEXT.md D-02 explicitly rules these out |
| `gc.callbacks` | `PYTHONDEVMODE`/`gc.set_debug` log parsing | CONTEXT.md D-03 explicitly rules this out — log parsing is fragile and doesn't give exact per-collection duration without self-timestamping anyway |
| `.pth`-file injection (this research's recommendation for wiring `gc.callbacks`/`tracemalloc` into every process without editing vendored code) | `PYTHONSTARTUP` env var | `PYTHONSTARTUP` only runs in interactive mode, not for `python -m minisgl` or `multiprocessing`-spawned workers — does not work here |

**Installation (GPU machine, into the project's own venv — never system Python, per existing Phase 1 convention):**
```bash
pip install py-spy==0.4.2 psutil==7.2.2
# hyperfine is a separate binary, not pip-installed: cargo install hyperfine, or apt/brew
```

**Version verification:** `py-spy` 0.4.2 and `psutil` 7.2.2 confirmed current via `pip index versions py-spy` / `pip index versions psutil` run in this session [VERIFIED: PyPI registry]. Neither package is currently present in `requirements-mac.txt` (grep found no match) — both need the project's human package-gate approval before being pinned, per the Phase 1 precedent ("Human package gate approved torch 2.9.1, numpy 2.5.3, ... " in STATE.md).

## Package Legitimacy Audit

| Package | Registry | Age | Downloads | Source Repo | Verdict | Disposition |
|---------|----------|-----|-----------|-------------|---------|-------------|
| py-spy | PyPI | long-established (`benfred/py-spy`, widely known Rust-implemented Python profiler) | unknown — the legitimacy tool's download-count signal did not resolve in this sandboxed run | github.com/benfred/py-spy | SUS (`unknown-downloads`) | Flagged — planner must add `checkpoint:human-verify` before install |
| psutil | PyPI | long-established (`giampaolo/psutil`, one of the most widely used Python system-info libraries) | unknown — same signal gap | github.com/giampaolo/psutil | SUS (`unknown-downloads`) | Flagged — planner must add `checkpoint:human-verify` before install |

**Packages removed due to [SLOP] verdict:** none.
**Packages flagged as suspicious [SUS]:** py-spy, psutil — both flagged solely on an `unknown-downloads` signal (the legitimacy tool could not retrieve download-count telemetry in this sandboxed session, not a finding that the download count is actually low). Both have long-lived, well-known GitHub source repos matching their PyPI listing. This is very likely a tooling/network-sandbox limitation rather than a genuine legitimacy concern, but per protocol the SUS verdict is reported as-is and the planner must still gate both installs behind `checkpoint:human-verify` — this doubles as the existing Phase 1 "human package gate" these two new packages need anyway.

*Both packages were already known to the researcher by name before verification (training-data familiarity); the package name itself is therefore `[ASSUMED]` until the human package gate confirms it against the GPU box's actual `pip install` output, even though the PyPI registry lookup in this session is `[VERIFIED: PyPI registry]` for version/existence.*

## Architecture Patterns

### System Architecture Diagram

```
 Mac dev box (no GPU)                          GPU machine (Linux, CUDA)
 ───────────────────                           ──────────────────────────
 Mac-side pytest unit tests                     Phase 2 profiling script (new, this phase)
 (stub py-spy/hyperfine/psutil                      │
  outputs, test bucketing &                         │ 1. launches server itself:
  JSON-schema code only)                            │    Popen/execv "python -m rsglang.launch
                                                     │     --frontend python ..."
                                                     ▼
                                          ┌─────────────────────────┐
                                          │ api_server process      │  <- top PID, captured
                                          │ (FastAPI + uvicorn)      │     directly by the script
                                          │ run_api_server():        │
                                          │   start_backend() BLOCKS │
                                          │   until both acks land   │
                                          │   -> THEN uvicorn.run()  │
                                          └────────────┬─────────────┘
                                   GET /v1/models 200  │  (sync point: tree is stable)
                                                        │
                      psutil.Process(top_pid).children()│
                              ┌─────────────────────────┼─────────────────────────┐
                              ▼                                                   ▼
                  ┌───────────────────────┐                         ┌─────────────────────────┐
                  │ scheduler (TP rank 0)  │                         │ tokenize_worker          │
                  │ _run_scheduler()       │  <- py-spy dump once,   │ (tokenize_worker())      │
                  │ scheduler/cache.py,    │     grep for            │ does BOTH tokenize AND   │
                  │ kvcache/radix_cache.py │     "_run_scheduler"    │ detokenize (num_tokenizer│
                  │  <- D-09/D-11 target   │     to role-identify    │  =0 default: share_token)│
                  └───────────┬────────────┘                         └────────────┬─────────────┘
                              │ py-spy record -f speedscope                        │ py-spy record
                              │ --pid <scheduler_pid> (D-09)                        │ --pid <tokenize_pid> (D-04)
                              ▼                                                     ▼
                  bucket frames: match_req, cache_req,              bucket frames: tokenize/detokenize/
                  match_prefix, insert_prefix, evict,                IPC hops (ZmqPush/PullQueue.put/get)
                  _tree_walk  -> "radix time" / total

                  In-process instrumentation (D-02/D-03), injected into ALL THREE processes above
                  via one env-var-gated .pth file in the venv's site-packages (no vendored file touched):
                     gc.callbacks.append(cb)   -> per-collection start/stop timestamps
                     tracemalloc.start() + periodic asyncio/thread snapshot -> alloc + RSS curve

                  Workload drivers (D-05..D-08), run against the same server from the Mac-reused-on-GPU
                  throwaway script, reusing minisgl.benchmark.client helpers:
                     Scenario 1: asyncio/aiohttp, 128 concurrent, early-cancel fraction
                     Scenario 2: adapts benchmark_one_batch()/benchmark_one() from bench_simple.py, MAX_INPUT≈32
                     Scenario 3: hyperfine wrapping a curl-retry-until-200 command + psutil PSS sum of the tree

                  -> docs/benchmarks/baseline-profile.json   (written directly by the script, D-14)
                  -> docs/benchmarks/baseline-profile.md     (written by hand afterward, reading the JSON, D-12/13)
```

### Recommended Project Structure
```
scripts/
├── gpu_phase2_profile.sh     # thin bash wrapper, mirrors gpu_phase1_check.sh's --help/--offline shape (Discretion)
└── baseline_profile.py       # the actual driver: launch, role-ID, instrument, run 3 scenarios, emit JSON
python/
└── tests/
    └── test_baseline_profile.py   # Mac-side: stub py-spy/hyperfine/psutil outputs, test bucketing + schema
docs/
└── benchmarks/
    ├── baseline-profile.md       # D-12, hand-written narrative
    └── baseline-profile.json     # D-13/D-14, written directly by baseline_profile.py
```
This mirrors the existing split already established in Phase 1: `scripts/gpu_phase1_check.sh` (GPU-only) + `python/tests/test_gpu_check_script.py` (Mac-side, stubs `nvidia-smi`/`setsid` on `PATH` and sources the bash script's helper functions) — confirmed by reading both files this session.

### Pattern 1: Race-free PID discovery via the server's own ready signal
**What:** Launch the server from inside the profiling script (not as a pre-existing external process), capture the top-level PID directly from the `Popen`/`subprocess` call, poll `GET /v1/models` until 200, then enumerate `psutil.Process(top_pid).children()`.
**When to use:** Any time the script needs definite PIDs for py-spy targets or PSS summation.
**Why it is race-free:** `run_api_server()` (`vendor/mini-sglang/python/minisgl/server/api_server.py:446-450`) calls `start_backend()` — which blocks inside `start_subprocess()` (`vendor/mini-sglang/python/minisgl/server/launch.py:105-111`) until every scheduler/tokenizer/detokenizer ack is received — **before** calling `uvicorn.run(app, ...)`. The HTTP port only opens after every child process exists and is ready.
```python
# Source: derived from reading vendor/mini-sglang/python/minisgl/server/api_server.py:446-450
# and vendor/mini-sglang/python/minisgl/server/launch.py:105-111 this session.
import subprocess, time, urllib.request, psutil

proc = subprocess.Popen(
    ["python", "-m", "rsglang.launch", "--frontend", "python", "--model", MODEL, "--port", str(PORT)],
)
top_pid = proc.pid  # exec_python_frontend() os.execv's in place (python/rsglang/launch.py:85-92),
                     # so this PID is also api_server's PID after exec -- no re-fork.
deadline = time.monotonic() + READY_TIMEOUT
while True:
    try:
        urllib.request.urlopen(f"http://127.0.0.1:{PORT}/v1/models", timeout=1)
        break
    except Exception:
        if time.monotonic() > deadline:
            raise TimeoutError("server did not become ready")
        time.sleep(0.5)

children = psutil.Process(top_pid).children()  # exactly 2 with defaults: scheduler + tokenize_worker
```

### Pattern 2: Role identification via a one-shot py-spy dump
**What:** For each child PID, run `py-spy dump --pid <pid>` once and grep the dumped call stack for a role-unique function name.
**When to use:** D-09 needs the scheduler PID specifically; this is the only reliable non-invasive way to distinguish it from the tokenize/detokenize worker, since neither process sets an OS-visible title (confirmed absent by grepping the vendored tree for `setproctitle`/`prctl` this session — no hits).
```bash
# Source: function names confirmed by reading vendor/mini-sglang/python/minisgl/server/launch.py:16
# (_run_scheduler) and vendor/mini-sglang/python/minisgl/tokenizer/server.py:31 (tokenize_worker).
py-spy dump --pid "$CHILD_PID" | grep -q '_run_scheduler' && echo "$CHILD_PID is the scheduler"
py-spy dump --pid "$CHILD_PID" | grep -q 'tokenize_worker' && echo "$CHILD_PID is the tokenizer/detokenizer worker"
```
Note: `py-spy dump`/`--pid` attach to an existing process usually requires root or `CAP_SYS_PTRACE` on Linux [CITED: github.com/benfred/py-spy README, via WebFetch] — budget for `sudo` on the GPU machine, or a documented ptrace-capability grant.

### Pattern 3: Zero-touch in-process instrumentation via an env-var-gated `.pth` hook
**What:** Drop a `.pth` file into the project's own venv `site-packages` whose single line is `import rsglang_profile_hook` (or an inline `import ...; ...()` line), where that tiny module checks an env var (e.g. `RSGLANG_PROFILE_SIDECAR`) and, only if set, calls `tracemalloc.start()` and registers a `gc.callbacks` entry. `.pth` files are executed by the `site` module on every interpreter startup that uses that `site-packages` directory — including every `multiprocessing`-spawned child, since each spawned worker is a fresh Python interpreter using the same installation.
**When to use:** For D-02/D-03, to get `gc.callbacks` and `tracemalloc` running inside api_server, the scheduler, and the tokenize/detokenize worker **without editing any vendored file** — satisfying the phase's "observational only" and D-09's "no vendored code is touched" constraints.
**Why this is the right mechanism (not a guess):** This is exactly the mechanism `coverage.py` documents and uses for its own `COVERAGE_PROCESS_START` subprocess-measurement feature — ".pth file in a site-packages directory" + "As long as the environment variable is visible in your subprocess, it will work" [CITED: coverage.readthedocs.io/en/7.12.0/subprocess.html, cross-checked via WebSearch]. Env vars are reliably inherited by `multiprocessing` spawn children (unlike raw `sys.path`/`PYTHONPATH`, whose propagation into spawned children is contested — see Assumptions Log A1).
```python
# Illustrative pattern, not copied from any source — the gc.callbacks arg shape is
# confirmed from Python's own documented behavior [CITED, cross-checked via WebSearch]:
# callback(phase, info); phase in {"start","stop"}; info has "generation" always,
# plus "collected"/"uncollectable" only when phase == "stop". No duration/timestamp
# is provided by gc itself -- the hook must self-timestamp.
import gc, os, time, json

_pauses = []

def _gc_cb(phase, info):
    now = time.monotonic()
    if phase == "start":
        _gc_cb._t0 = now
    elif phase == "stop":
        _pauses.append({
            "pid": os.getpid(),
            "generation": info["generation"],
            "collected": info["collected"],
            "duration_s": now - _gc_cb._t0,
            "timestamp": now,
        })

if os.environ.get("RSGLANG_PROFILE_SIDECAR"):
    gc.callbacks.append(_gc_cb)
```

### Anti-Patterns to Avoid
- **Guessing child-process role from PID order:** `multiprocessing` gives no documented ordering guarantee visible from a separate observer process; use Pattern 2 instead.
- **Treating `/proc/<pid>/smaps_rollup` as a tree-wide PSS total:** it is pre-summed **per process**, not across a process tree — you must read it once per PID and add the `Pss` fields yourself [CITED: kernel ABI doc `procfs-smaps_rollup`, cross-checked via WebSearch].
- **Timing server readiness with bare `hyperfine <launch-command>`:** hyperfine only times process *exit*, not an async readiness condition inside a long-running server — the timed command must itself be a retry-until-200 wrapper (e.g. a small shell loop around `curl`) that exits once ready, per D-08.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| CPU/GIL sampling of a running process | A custom `sys.setprofile`/`signal.setitimer`-based sampler | `py-spy` | Locked by D-01; also, hand-rolled in-process sampling can't observe a process whose GIL is itself the problem, and can't see native-extension frames without `--native` support that's already implemented |
| Per-collection GC duration | Parsing `gc.set_debug(gc.DEBUG_STATS)` log output | `gc.callbacks` with self-timestamping | Locked by D-03; log parsing has no stable machine-readable schema across Python point releases |
| Aggregate process-tree PSS | A custom `/proc` walker that double-counts shared pages | `psutil.Process(pid).memory_full_info().pss` (which itself is backed by `/proc/pid/smaps` parsing) per PID, summed by the caller | PSS accounting (shared-page apportionment) is exactly the kind of easy-to-get-subtly-wrong arithmetic a battle-tested library should own |
| Injecting instrumentation into subprocess trees you cannot edit | A monkeypatch shipped inside the vendored tree, or an `LD_PRELOAD`-style hack | The `.pth`-file + env-var-gate pattern (Pattern 3) | It is the documented, tested mechanism another major Python tool (`coverage.py`) already uses for precisely this "measure a subprocess tree you don't control the entry point of" problem |

**Key insight:** every "custom" piece this phase would otherwise need to hand-roll (process discovery, PSS summation, subprocess-tree instrumentation injection) already has a narrow, well-tested escape hatch in the standard library or in a tool other major Python projects use for the identical problem — the research task was finding the right escape hatch, not inventing a new one.

## Common Pitfalls

### Pitfall 1: "GIL contention between tokenize/detokenize/HTTP handling" does not mean thread contention
**What goes wrong:** BENCH-01's text reads as if tokenization, detokenization, and HTTP handling are three threads contending for one GIL. Reading the code shows otherwise: `tokenize_worker` (`tokenizer/server.py:31`) runs tokenize *and* detokenize sequentially in a single-threaded `while True` loop inside **one** process (when `num_tokenizer=0`, the default — confirmed `share_tokenizer` property in `server/args.py:21-23`), and HTTP handling (`api_server.py`, uvicorn/asyncio) runs in a **separate** OS process with its own, separate GIL.
**Why it happens:** The requirement text was written before the exact process topology was confirmed by reading the code.
**How to avoid:** Measure what is actually observable: (a) `py-spy --gil` per-process shows the fraction of on-CPU samples holding that process's own GIL (useful for seeing how much of tokenize_worker's wall-clock time is Python bytecode vs IPC-wait/native-kernel time) [CITED: py-spy README, cross-checked via WebSearch]; (b) there is no cross-process GIL contention to measure, only per-process GIL-held fraction plus OS-level CPU scheduling across the 3 processes. Document this distinction explicitly in `baseline-profile.md` rather than reporting a "contention" number that doesn't exist. Flagged as Open Question 1 below for the planner to decide exact wording/metric.
**Warning signs:** A report section titled "GIL contention" with a single percentage and no process-topology caveat.

### Pitfall 2: Default process topology is only 2 children, not N
**What goes wrong:** Assuming the standard "frontend = api_server + tokenizer-pool + detokenizer" 3-worker picture from `.claude/CLAUDE.md`'s general architecture notes, and building PID-discovery/role-ID logic for an arbitrary number of children.
**Why it happens:** `.claude/CLAUDE.md` describes the general `--num-tokenizer` knob (used later, in Phase 7's BENCH-07), which can spin up additional dedicated tokenizer processes. But Phase 1's GPU check and this phase's baseline both use the **default** `--num-tokenizer 0` and (implicitly) `--tp-size 1`, which collapses to exactly 2 `multiprocessing.Process` children: one scheduler (TP rank 0) and one combined tokenize+detokenize worker — confirmed by reading `server/launch.py:71-103`.
**How to avoid:** Write the role-ID/discovery code generically (iterate `psutil.Process(pid).children()`, don't hardcode "exactly 2"), but don't over-engineer for a tokenizer *pool* that this phase's default config never creates.
**Warning signs:** Code that assumes `num_tokenizer_workers` > 1 by default, or that hardcodes array indices into the children list instead of role-identifying each one.

### Pitfall 3: `.pth`-file propagation into `multiprocessing`-spawned children is the one load-bearing unverified assumption
**What goes wrong:** Assuming Pattern 3 (the `.pth`-file hook) "just works" for spawned children without a pre-flight check, then discovering mid-GPU-session that `gc.callbacks`/`tracemalloc` are silently not active in the scheduler or tokenize_worker process.
**Why it happens:** `.pth` files are processed by the `site` module during normal interpreter startup, and `multiprocessing`'s `spawn` start method launches genuinely fresh interpreters (not forks) for each worker — so in principle `site` processing (and thus the `.pth` file) should run for them too, since they use the same Python installation / venv `site-packages`. But env-var/`sys.path` propagation into `multiprocessing` spawn children has documented edges that are *not* the same as normal subprocess env inheritance [CITED: discuss.python.org "Initialization of sys.path in interpreters spawned with multiprocessing", cross-checked via WebSearch] — the search in this session surfaced conflicting-enough detail that this needs a direct test, not an assumption.
**How to avoid:** Before wiring Pattern 3 into the real profiling script, run a 10-line throwaway script locally (even on the Mac, no GPU needed) that does `multiprocessing.Process(target=worker).start()` with a `.pth` hook installed and confirms the child's `gc.callbacks` fired. This is cheap and fully Mac-testable.
**Warning signs:** `baseline-profile.json`'s GC-pause array has entries only for the top-level PID, never for the scheduler/tokenizer child PIDs.

### Pitfall 4: py-spy needs elevated privileges on the GPU box
**What goes wrong:** The profiling script runs fine as a normal user locally but fails with a permission error attaching to a sibling PID on the GPU machine.
**Why it happens:** Attaching `py-spy` to an *existing* PID on Linux "will usually require root"; only *launching a new process under py-spy* avoids this [CITED: github.com/benfred/py-spy README, cross-checked via WebSearch]. This phase's design (Pattern 1/2) always attaches to already-running PIDs (the scheduler, the tokenize_worker), never launches them under py-spy directly.
**How to avoid:** Document in the GPU runbook that the profiling script needs `sudo` or an equivalent `CAP_SYS_PTRACE` grant on the GPU machine; this is an Environment Availability item, not a code problem.
**Warning signs:** `py-spy dump`/`record` exits with a permission-denied error instead of a stack trace.

## Code Examples

### Periodic non-blocking tracemalloc snapshot inside an asyncio server
```python
# Illustrative pattern; the asyncio.sleep-loop approach is a cross-checked common idiom
# [CITED, via WebSearch], not copied from a single named source.
import asyncio, tracemalloc, time

async def snapshot_loop(interval_s: float, sidecar: list):
    tracemalloc.start()
    while True:
        await asyncio.sleep(interval_s)
        current, peak = tracemalloc.get_traced_memory()
        sidecar.append({"t": time.monotonic(), "current_bytes": current, "peak_bytes": peak})
```

### Bucketing a py-spy speedscope profile into "radix time" vs "other"
```python
# Schema confirmed by WebFetch against https://www.speedscope.app/file-format-schema.json
# this session [CITED, LOW per the classify-confidence seam for a single webfetch source --
# treat the exact field names as needing a smoke-test against a real py-spy speedscope
# output before relying on them in the real script].
import json

RADIX_FRAME_NAMES = {
    "match_req", "cache_req",            # scheduler/cache.py (CacheManager)
    "match_prefix", "insert_prefix", "evict", "_tree_walk",  # kvcache/radix_cache.py (RadixPrefixCache)
}

def radix_share(speedscope_path: str) -> float:
    doc = json.loads(open(speedscope_path).read())
    frames = doc["shared"]["frames"]  # list of {"name": ..., "file": ..., ...}
    profile = doc["profiles"][0]      # sampled-type profile
    samples, weights = profile["samples"], profile["weights"]
    radix_weight = total_weight = 0.0
    for stack, weight in zip(samples, weights):
        total_weight += weight
        if any(frames[idx]["name"] in RADIX_FRAME_NAMES for idx in stack):
            radix_weight += weight
    return radix_weight / total_weight if total_weight else 0.0
```

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | A `.pth`-file hook in the venv's `site-packages`, gated by an env var, will reliably execute inside `multiprocessing.Process(spawn)`-started children (scheduler, tokenize_worker) the same way it does for coverage.py's subprocess use case | Architecture Patterns (Pattern 3), Common Pitfalls (Pitfall 3) | If it does not propagate, `gc.callbacks`/`tracemalloc` silently never activate in the scheduler/tokenizer processes, and D-02/D-03's per-process numbers for those roles would be missing or zero without an obvious error — caught only by the pre-flight test recommended in Pitfall 3 |
| A2 | `psutil.Process(pid).memory_full_info().pss` on the GPU machine's installed `psutil` version reads `/proc/pid/smaps` (not `smaps_rollup`) under the hood | Standard Stack, Don't Hand-Roll | If the installed psutil version instead uses `smaps_rollup` (version-dependent), the numbers are still correct, just faster — low risk either way, flagged only for completeness |
| A3 | The speedscope JSON field names (`shared.frames[].name`, `profiles[].samples`, `profiles[].weights`) are exactly as fetched from `speedscope.app/file-format-schema.json` in this session | Code Examples | If the real `py-spy record -f speedscope` output differs in a field name or nesting, the bucketing script in Code Examples would throw a `KeyError` rather than silently producing a wrong number — low risk, but should be smoke-tested against one real py-spy run before relying on it for D-09/D-10's numbers |
| A4 | Both `py-spy` and `psutil` are the legitimate, well-known PyPI packages (not slopsquats) despite the automated legitimacy check returning `SUS` (`unknown-downloads`) in this sandboxed session | Package Legitimacy Audit | Near-zero in practice (both are long-established, widely-known tools with matching GitHub repos), but the human package gate (already required by project convention for any new dependency) is the actual mitigation, not this research session's belief |

## Open Questions

1. **How should "GIL contention" be reported given the actual 3-process topology?**
   - What we know: tokenize and detokenize share one process and one GIL (when `num_tokenizer=0`); HTTP handling is a separate process with its own GIL. There is no cross-process GIL contention to measure in the traditional sense.
   - What's unclear: whether BENCH-01's wording should be satisfied by reporting per-process `py-spy --gil`-held percentage for each of the 3 processes (closest available proxy), or whether the planner wants a different framing.
   - Recommendation: report per-process GIL-held percentage (api_server, tokenize_worker, scheduler) plus an explicit note in `baseline-profile.md` that cross-process "contention" in the literal sense doesn't apply to this topology — this satisfies the spirit of BENCH-01 without inventing a metric that doesn't exist.

2. **Does the `.pth`-file hook need a fallback if Pitfall 3's pre-flight check fails?**
   - What we know: the coverage.py precedent strongly suggests it will work; a cheap Mac-testable pre-flight check is recommended before committing.
   - What's unclear: if it doesn't propagate to spawned children, there's no immediately obvious equally non-invasive fallback (editing vendored code is out of scope; `PYTHONSTARTUP` doesn't apply to `-m` or spawned workers).
   - Recommendation: the planner should budget the pre-flight check as an early task in the plan, before committing further tasks to the `.pth`-file design — if it fails, this becomes a checkpoint for the user (does editing vendored code become acceptable just for this phase's `gc.callbacks` hook, given D-09 already treats "no vendored code touched" as specific to the radix measurement, not necessarily every measurement?).

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| `py-spy` | D-01/D-04/D-09 | ✗ (not on Mac project venv or PATH; must install on GPU box) | — (0.4.2 latest on PyPI) | None — no fallback; required for the phase's core measurement |
| `psutil` | D-08, Pattern 1/2 | ✗ in project venv (system Python on this Mac has 5.9.0, but `requirements-mac.txt` has no pin) | — (7.2.2 latest on PyPI) | None — `psutil` is the standard escape hatch already; no simpler fallback makes sense |
| `hyperfine` | D-08 | ✗ (not on PATH on this Mac) | — (1.20.0 per existing `.claude/CLAUDE.md` stack decision) | A plain shell loop timing `date +%s%N` around the curl-retry command, if hyperfine is unavailable on the GPU box too — lower-quality statistics (no warmup/outlier handling) |
| `tracemalloc`, `gc` | D-02/D-03 | ✓ (stdlib, Python ≥3.10) | bundled with interpreter | — |
| `CAP_SYS_PTRACE` / root, for `py-spy --pid` attach to an existing process | Pattern 2, all py-spy sampling | unknown on the GPU machine (not verified this session — no GPU access) | — | None documented; must be arranged on the GPU machine before this phase's script can run there |
| GPU machine (Linux) | the actual profiling run (this phase's Success Criteria 1-3) | unknown — not probed this session (no GPU access from this Mac) | — | Mac-side work is limited to unit-testing the script's parsing/bucketing logic with stubs; the real run is GPU-only, matching the dev-environment constraint already documented in this phase's scope |

**Missing dependencies with no fallback:** `py-spy` (core measurement tool), `CAP_SYS_PTRACE`/root on the GPU box for py-spy's PID-attach mode.
**Missing dependencies with fallback:** `hyperfine` (shell-loop timing fallback, lower quality), `psutil` (no simpler fallback exists, but installing it is low-risk/low-effort).

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | pytest 9.1.1 (already project-pinned) |
| Config file | `pyproject.toml` `[tool.pytest.ini_options]` — `testpaths = ["python/tests"]`, `markers = ["slow: spawns processes or builds binaries"]` |
| Quick run command | `.venv/bin/python -m pytest python/tests -q -m "not slow"` |
| Full suite command | `.venv/bin/python -m pytest python/tests -q` (per `scripts/check_all.sh` step 2) |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| BENCH-01 | `baseline-profile.json` schema is well-formed (GC pauses, RSS curve, GIL %, radix share %, per scenario) | unit | `pytest python/tests/test_baseline_profile.py::test_json_sidecar_schema -q` | ❌ Wave 0 |
| BENCH-01 | speedscope-bucketing function correctly attributes samples hitting `match_req`/`cache_req`/`match_prefix`/`insert_prefix`/`evict`/`_tree_walk` as "radix time" | unit | `pytest python/tests/test_baseline_profile.py::test_radix_frame_bucketing -q` | ❌ Wave 0 |
| BENCH-01 | role-identification grep logic correctly distinguishes a stub "scheduler" dump from a stub "tokenize_worker" dump | unit | `pytest python/tests/test_baseline_profile.py::test_role_identification -q` | ❌ Wave 0 |
| BENCH-01 | `.pth`-hook propagates `gc.callbacks` into a `multiprocessing.Process(spawn)` child (the Pitfall 3 pre-flight check) | integration (marked `slow`) | `pytest python/tests/test_baseline_profile.py::test_pth_hook_propagates_to_spawn_child -q -m slow` | ❌ Wave 0 |
| BENCH-01 | the bash wrapper's `--help` works anywhere (mirrors `gpu_phase1_check.sh`'s existing pattern) | smoke | `scripts/gpu_phase2_profile.sh --help` | ❌ Wave 0 |

### Sampling Rate
- **Per task commit:** `pytest python/tests/test_baseline_profile.py -q -m "not slow"`
- **Per wave merge:** `pytest python/tests -q` (full suite, mirrors `scripts/check_all.sh` step 2)
- **Phase gate:** the real GPU-machine run of `scripts/gpu_phase2_profile.sh` is the actual Success Criteria verification (criteria 1-4 in the phase description) — it cannot be substituted by Mac-side unit tests, same split already established in Phase 1.

### Wave 0 Gaps
- [ ] `python/tests/test_baseline_profile.py` — new file, covers BENCH-01's parsing/bucketing/role-ID/`.pth`-propagation logic
- [ ] `scripts/baseline_profile.py` and `scripts/gpu_phase2_profile.sh` — new files, no prior version exists
- [ ] `docs/benchmarks/` directory — does not exist yet (only `docs/mini-sglang-reading-guide.md` and `docs/agents/` currently exist under `docs/`)

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-------------------|
| V2 Authentication | No | This phase adds no network-facing auth surface; the profiled server already binds only to `127.0.0.1` per existing `ServerArgs.server_host` default (`server/args.py:16-17`, read this session) |
| V3 Session Management | No | Not applicable — no sessions introduced |
| V4 Access Control | No | Not applicable |
| V5 Input Validation | Yes (narrow) | The `baseline-profile.json` sidecar is read back by future phases (Phase 7, v2 radix decision) — validate its schema with a JSON-schema or dataclass check on write (see Validation Architecture Wave 0 gap) so a malformed sidecar fails loudly in this phase rather than silently misleading Phase 7 |
| V6 Cryptography | No | Not applicable — no secrets or crypto introduced |

### Known Threat Patterns for this stack

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Running `py-spy` with root/`CAP_SYS_PTRACE` on a shared GPU machine | Elevation of Privilege (local only — this is a benign local-dev concern, not a network attack surface) | Document the privilege requirement explicitly in the runbook; scope `sudo`/capability grants to the profiling script's invocation only, not a standing root shell |
| A `.pth`-file hook left installed and unconditionally active after this phase ends | Tampering (future code silently gains an always-on side effect) | Gate it behind an env var (as designed in Pattern 3) and document removing or disabling it as a phase cleanup step once the baseline run is captured |

## Sources

### Primary (HIGH confidence — read directly this session)
- `vendor/mini-sglang/python/minisgl/server/launch.py` (process spawn topology, ack ordering)
- `vendor/mini-sglang/python/minisgl/server/api_server.py` (start_backend-before-uvicorn.run ordering; existing `psutil.Process().children(recursive=True)` precedent at line ~404-408)
- `vendor/mini-sglang/python/minisgl/server/args.py` (`share_tokenizer` property, default `num_tokenizer=0`, default `server_host`)
- `vendor/mini-sglang/python/minisgl/scheduler/cache.py` (`CacheManager.match_req`, `cache_req`)
- `vendor/mini-sglang/python/minisgl/kvcache/radix_cache.py` (`RadixPrefixCache.match_prefix`, `insert_prefix`, `evict`, `_tree_walk`)
- `vendor/mini-sglang/python/minisgl/kernel/radix.py` (native kernel entry point `fast_compare_key`, `load_aot("radix", cpp_files=["radix.cpp"])`)
- `vendor/mini-sglang/python/minisgl/tokenizer/server.py` (`tokenize_worker`, confirms shared tokenize+detokenize in one process by default)
- `vendor/mini-sglang/python/minisgl/utils/mp.py` (ZMQ push/pull queue classes — the IPC hops D-04 samples around)
- `vendor/mini-sglang/benchmark/online/bench_simple.py` and `bench_qwen.py`, and `vendor/mini-sglang/python/minisgl/benchmark/client.py` (`benchmark_one`, `benchmark_one_batch`, `benchmark_trace`, `generate_prompt`, `get_model_name`, `process_benchmark_results`, `read_qwen_trace`, `scale_traces` — exact signatures confirmed)
- `python/rsglang/launch.py` (confirms `exec_python_frontend` uses `os.execv`, so python-mode's top PID never changes across the exec)
- `scripts/gpu_phase1_check.sh` and `python/tests/test_gpu_check_script.py` (existing Mac/GPU split convention for this exact kind of script)
- `scripts/check_all.sh`, `pyproject.toml` (`[tool.pytest.ini_options]`) (test framework/commands)
- `requirements-mac.txt` (grepped: no `psutil`/`py-spy`/`hyperfine` pin exists yet)
- `grep -rn "setproctitle|proctitle|prctl" vendor/mini-sglang/python/minisgl/` (no hits — confirms no OS-visible process title is set, this session)

### Secondary (MEDIUM confidence — WebSearch, cross-checked across multiple results)
- py-spy CLI flags (`--native`, `--rate`, `--subprocesses`, `dump`/`record`/`top` subcommands, `--gil`), confirmed by WebSearch and by WebFetch of `github.com/benfred/py-spy`'s README
- py-spy record output formats (`raw`, `speedscope`, `flamegraph`, `chrometrace`)
- `tracemalloc` periodic-snapshot async pattern
- `gc.callbacks` exact callback signature (`phase`, `info` with `generation`/`collected`/`uncollectable`)
- `hyperfine --warmup`/`--prepare` semantics
- `/proc/<pid>/smaps_rollup` kernel ABI (per-process rollup, not tree-wide)
- `psutil.Process.memory_full_info().pss` semantics on Linux
- `coverage.py`'s `.pth`-file + `COVERAGE_PROCESS_START` subprocess-measurement mechanism (`coverage.readthedocs.io/en/7.12.0/subprocess.html`)

### Tertiary (LOW confidence — single-source WebFetch, flagged for validation)
- speedscope JSON file-format schema field names (`shared.frames[].name`, `profiles[].samples`, `profiles[].weights`, `unit`), fetched from `speedscope.app/file-format-schema.json` — smoke-test against a real `py-spy record -f speedscope` output before relying on it (see Assumption A3)
- `multiprocessing` spawn-child `sys.path`/env propagation nuance from `discuss.python.org` thread — motivated Pitfall 3's recommended pre-flight check rather than being treated as settled

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all 5 tools are locked by CONTEXT.md decisions or are stdlib; versions verified against PyPI this session
- Architecture (process topology, PID discovery, role-ID): HIGH — confirmed by directly reading the vendored and in-repo source this session, not inferred
- Architecture (`.pth`-file instrumentation injection): MEDIUM — mechanism is documented and proven for an analogous tool (coverage.py), but not yet tested against this exact project
- Pitfalls: MEDIUM-HIGH — grounded in source reading (topology, GIL framing) plus cross-checked WebSearch (py-spy permissions, PSS semantics)

**Research date:** 2026-10-05
**Valid until:** 30 days (stable domain: stdlib + long-established tools; the one fast-moving risk is Assumption A1, which should be resolved by a pre-flight test early in this phase's execution, not by a research refresh)
