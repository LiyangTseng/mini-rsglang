# Phase 7: Frontend Benchmarks - Research

**Researched:** 2026-10-06
**Domain:** Rust benchmark-harness engineering (load generation, process-tree memory sampling, A/B statistics, cross-process GC attribution) against a mixed Rust/Python serving stack
**Confidence:** MEDIUM-HIGH (crate versions and most code patterns verified directly against this repo and crates.io; the GPU-measured headline numbers themselves are necessarily unverifiable from a Mac session)

## Summary

Phase 7 does not need new technology choices — `.claude/CLAUDE.md` already pins the exact
stack (hdrhistogram, reqwest, sysinfo, nix, hyperfine, clap, tokio) and every one of those
versions was re-verified against crates.io today and is still each crate's current
`max_stable_version`. What this research adds is the **one level deeper** the task asked for:
reading the actual vendored/first-party files the phase's 16 locked decisions depend on, and
resolving concretely (as recommendations, since these stay "Claude's Discretion" in
07-CONTEXT.md) the crate layout, sampling interval, and report-file split.

Four findings materially change how the planner should scope tasks, beyond confirming
CLAUDE.md's table:

1. **`sysinfo` has no PSS API at all** — only `Process::memory()` (RSS). Phase 2's Python
   profiler gets PSS from `psutil.Process.memory_full_info().pss` (which itself reads
   `/proc/<pid>/smaps_rollup` under the hood), gated to Linux-only with a `None` fallback
   elsewhere. The Rust harness must replicate this exact gating by hand-parsing
   `/proc/<pid>/smaps_rollup`'s `Pss:` line — `sysinfo` alone cannot produce the number
   CLAUDE.md's table implies it can.
2. **Phase 2's `gc.callbacks`+`tracemalloc` hook (`python/rsglang/profiling/hook.py`) is
   monolithic** — `install()` always starts both together. D-15 requires GC-only
   instrumentation during timed runs; today's `install()` cannot do that split. The planner
   must add a mode toggle (or a second, simpler shim) before D-14/D-15 can be satisfied as
   written.
3. **The Python frontend cannot run on the Mac at all** (CUDA-only backend, confirmed by
   PROJECT.md's Out-of-Scope table). D-03's "Mac-side development/verification pass" can
   therefore only exercise the harness's own mechanics against `rsg-server` +
   `mock-scheduler` — it cannot be a Python-vs-Rust A/B rehearsal. Scope D-03's acceptance
   criteria accordingly (prove loop modes/cancellation/hdrhistogram/manifest code paths
   work, not that the alternation produces a *comparison*).
4. **`mock-scheduler`'s CLI surface today is narrower than the task brief assumed.** As of
   this session it is exactly `--backend-addr/--backend-role/--detok-addr/--detok-role
   /--prefill-delay-ms/--decode-delay-ms/--max-seq-len`. The `--misbehave-uids`/
   `--behavior`/`--batch-size`/`--observe-file` flags referenced by the phase brief and by
   03-CONTEXT.md's own "Integration Points" section exist only in Phase 3's **not-yet-merged**
   `03-06-PLAN.md` (Phase 3 is being executed in a parallel worktree). This is not a blocker —
   D-02 already says Phase 7 does not need them — but the planner must not write a task that
   assumes they exist without re-checking at execution time.

**Primary recommendation:** add a new workspace member `crates/rsg-bench` (binary crate,
clap-driven like `rsg-server`/`mock-scheduler`) that owns the load generator, the A/B
orchestrator, the hdrhistogram recording, the process-tree memory sampler, and the manifest
writer; extend `python/rsglang/profiling/hook.py` with a GC-only mode; keep the single
combined `docs/benchmarks/`-style md+json report pair, following Phase 2's exact convention.

## Architectural Responsibility Map

This phase is a benchmark-harness tool, not a web app, so the standard Browser/SSR/API/CDN/DB
tiers don't apply cleanly. The table below adapts them to this project's actual process
topology (client harness, frontend-under-test, shared backend, OS/process layer).

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Load generation, open/closed-loop driving, mid-stream cancellation (BENCH-02/03) | Harness (new `rsg-bench`, client role) | — | Must run outside both frontends to time them fairly; D-02 locks cancellation logic here, not in `mock-scheduler` |
| TTFT/ITL/E2E percentile recording (BENCH-02/07) | Harness | — | hdrhistogram lives in the measuring process; the frontend-under-test is a black box over HTTP |
| HTTP surface under test (`/v1/chat/completions`, `/generate`) | Frontend-under-test (Python `api_server` or Rust `rsg-server`, Phase 5's API) | — | The thing being measured; harness never imports its code, only calls its HTTP API |
| Tokenize/detokenize, GC pauses inside the frontend process | Frontend-under-test | Harness (collects, does not instrument the Rust side — Rust has no GC) | D-16's GC-pause table is per-process; Rust frontend contributes zero GC rows by construction |
| Shared scheduler (GPU weights, batching, radix cache) | Backend (Python scheduler, shared code per BASE-02/03) | — | Identical cost paid by both frontends; Phase 7 reports it as a floor, not something either frontend changes (per `baseline-profile.md`'s own framing) |
| Process spawn/teardown, whole-tree RSS/PSS sampling (BENCH-05/08) | OS/process layer (Harness, via `nix` + `sysinfo` + `/proc` parsing) | Backend (scheduler process is part of the sampled tree) | Memory ownership spans the whole process tree, not one tier; mirrors Python's `psutil`-based `tree_memory()` |
| Third-party cross-checks (`vllm bench serve`, `sglang.benchmark.serving`) | External tooling (independent process, Scenario-2-only, D-04) | — | No shared code with the custom harness; hits the same HTTP endpoint as an unrelated client |
| A/B alternation, run manifest, md+json report (BENCH-07) | Harness orchestrator | — | D-08 locks this as one shared layer wrapping all three scenario runners |

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| BENCH-02 | A Rust load generator: open-loop, supports mid-stream cancellation, records TTFT, P99 and RPS | `reqwest` streaming + stream-drop cancellation pattern; `hdrhistogram` API confirmed via docs.rs; D-01's `--mode closed\|open` flag shape |
| BENCH-03 | Scenario 1: 128 concurrent agents with random cancellations, P99 TTFT, Python vs Rust frontend | Mac dev pass scoping (Python cannot run on Mac — see Pitfall 3); `baseline-profile.md`'s existing s1 numbers as the comparison's backdrop |
| BENCH-04 | Scenario 2: 32-token short-prompt saturation, RPS-vs-latency curve, Python vs Rust frontend | `vllm bench serve` flags confirmed live (docs.vllm.ai); `bench_simple.py`/`scenarios.py:run_s2` reuse shape |
| BENCH-05 | Scenario 3: frontend cold-start time and frontend memory; end-to-end startup reported separately | `hyperfine_argv`/`parse_hyperfine_json` (Phase 2, verbatim reusable shape); PSS-is-not-RSS finding |
| BENCH-06 | Standard inference throughput shows no regression versus the Python frontend | `bench_simple.py` + `minisgl.benchmark.client` API read directly; `scenarios.py:run_s2`'s lazy-import reuse pattern (D-12) |
| BENCH-07 | Python frontend reported at default and best `--num-tokenizer`; A/B alternation; CIs; reproducible manifest | `--num-tokenizer`/`--tokenizer-count` flag confirmed in `server/args.py`; `baseline-profile.json`'s `meta` block as the manifest schema template |
| BENCH-08 | Every scenario report shows frontend memory usage and Python GC pause counts alongside TTFT/P99/RPS | `hook.py`'s monolithic `gc.callbacks`+`tracemalloc` install (Pitfall 2); `RssSampler`'s per-role sampling pattern as the symmetry target |

</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard | Conf. |
|---------|---------|---------|---------------|-------|
| `hdrhistogram` | **7.6.0** | TTFT/ITL/E2E percentile recording | De-facto Rust histogram crate; `Histogram::new_with_bounds`/`record`/`value_at_percentile` confirmed on docs.rs | `[VERIFIED: crates.io max_stable_version 7.6.0, queried 2026-10-06; package-legitimacy check OK — downloads 1.67M/wk, repo github.com/HdrHistogram/HdrHistogram_rust]` |
| `reqwest` | **0.13.5** | Streaming HTTP client for the load generator | The standard async client; pairs naturally with `tokio`/`hyper` already in the workspace | `[VERIFIED: crates.io max_stable_version 0.13.5, queried 2026-10-06; package-legitimacy check OK — downloads 16.0M/wk]` |
| `sysinfo` | **0.39.6** | Process enumeration and RSS sampling | Cross-platform process-tree walk (works on both Mac dev and the Linux GPU box) — **RSS only, see Pitfall 1 for PSS** | `[VERIFIED: crates.io max_stable_version 0.39.6, queried 2026-10-06; package-legitimacy check OK — downloads 4.0M/wk]` |
| `nix` | **0.31.3** | `setpgid`/`killpg` for clean teardown of the mock-scheduler/backend process tree | Thin libc bindings; `nix::unistd::setpgid(pid, pgid)` and `nix::sys::signal::killpg` confirmed on docs.rs | `[VERIFIED: crates.io max_stable_version 0.31.3, queried 2026-10-06; package-legitimacy check OK — downloads 15.1M/wk]` |
| `clap` (workspace dep, already pinned) | 4.6.7 | CLI for the new `rsg-bench` binary | Same derive-macro convention `rsg-server`/`mock-scheduler` already use | `[VERIFIED: crates/Cargo.toml:14, read this session]` |
| `tokio`, `serde`/`serde_json`, `tracing`/`tracing-subscriber` (workspace deps) | 1.53.1 / 1.0.229+1.0.151 / 0.1.44+0.3.23 | Async runtime, manifest/report JSON, structured logs | Already workspace-wide deps; no new choice needed | `[VERIFIED: Cargo.toml:14-24, read this session]` |

### Supporting
| Library | Version | Purpose | When to Use | Conf. |
|---------|---------|---------|-------------|-------|
| `hyperfine` (external binary, not a Cargo dep) | **1.20.0** | Scenario 3 cold-start timing | Installed via `cargo install hyperfine --version 1.20.0 --locked`, exactly as `docs/benchmarks/baseline-profile.md`'s own "Reproduce" section already does; **not installed on this Mac dev machine as of this session** — see Environment Availability | `[VERIFIED: crates.io max_stable_version 1.20.0, queried 2026-10-06; docs/benchmarks/baseline-profile.md:361-363, read this session]` |
| `tokio-util` | 0.7.19 | `CancellationToken` for coordinated shutdown of the A/B orchestrator's N trials | If the orchestrator needs to abort an in-progress trial cleanly (e.g. Ctrl-C mid-run) | `[VERIFIED: crates.io max_stable_version 0.7.19, queried 2026-10-06]` |
| `futures` / `tokio-stream` | 0.3.34 / 0.1.19 | Stream combinators for the closed/open-loop driver | Turning a Poisson-arrival generator or a fixed-concurrency pool into a `Stream` of request futures | `[VERIFIED: crates.io, queried 2026-10-06]` |

### Alternatives Considered
No alternatives were re-opened for this research — CONTEXT.md's decisions (D-01 through D-16)
already lock the architecture shape, and CLAUDE.md's own "Alternatives Considered" section
(oha/wrk/vegeta can't parse SSE timing; `tmq` is edge-triggered and fragile; `eventsource-stream`
is unmaintained) already covers this phase's load-generator choice. Re-litigating it would
contradict the "research THESE, not alternatives" instruction for locked decisions.

**Installation:**
```bash
# Cargo.toml additions (workspace member crates/rsg-bench)
cargo install hyperfine --version 1.20.0 --locked   # external binary, scenario 3 only
```

**Version verification:** All versions above were checked against crates.io's
`max_stable_version` field on 2026-10-06 (network access confirmed reachable from this Mac
session with a `User-Agent` header — the bare request otherwise 403s). None are stale
relative to CLAUDE.md's existing pins.

## Package Legitimacy Audit

| Package | Registry | Age | Downloads | Source Repo | Verdict | Disposition |
|---------|----------|-----|-----------|-------------|---------|-------------|
| hdrhistogram | crates.io | 11 yrs (2015-07-07) | 1.67M/wk | github.com/HdrHistogram/HdrHistogram_rust | OK | Approved |
| reqwest | crates.io | 10 yrs (2016-10-16) | 16.0M/wk | github.com/seanmonstar/reqwest | OK | Approved |
| sysinfo | crates.io | 11 yrs (2015-07-25) | 4.0M/wk | github.com/GuillaumeGomez/sysinfo | OK | Approved |
| nix | crates.io | 12 yrs (2014-11-11) | 15.1M/wk | github.com/nix-rust/nix | OK | Approved |
| hyperfine | crates.io | 8 yrs (2018-01-13) | 10.5K/wk | github.com/sharkdp/hyperfine | OK | Approved |

**Packages removed due to [SLOP] verdict:** none.
**Packages flagged as suspicious [SUS]:** none.

All five packages were already named in CLAUDE.md's existing Recommended Stack table; this
session's `package-legitimacy check` run against the crates ecosystem (plus the crates.io
registry query above) confirms all five independently, so they earn
`[VERIFIED: crates.io registry + package-legitimacy check OK]` rather than `[ASSUMED]`.

## Architecture Patterns

### System Architecture Diagram

```
┌─────────────────────────────── Harness process (new crates/rsg-bench) ───────────────────────────────┐
│                                                                                                          │
│  CLI (clap) ──▶ Orchestrator (D-08)                                                                     │
│                    │  alternates P,R,P,R,... (D-05/D-06, default N=5)                                   │
│                    ├──▶ Scenario-1 runner (closed-loop, 128 agents, seeded RNG cancels)                  │
│                    ├──▶ Scenario-2 runner (closed|open-loop, saturation sweep incl. --num-tokenizer)     │
│                    ├──▶ Scenario-3 runner (wraps `hyperfine`, D-08's shared "single-trial runner" slot)  │
│                    │                                                                                     │
│                    ├──▶ hdrhistogram recorder  (TTFT/ITL/E2E per trial, shared by all 3 scenario runners)│
│                    ├──▶ Memory sampler (sysinfo RSS + /proc/<pid>/smaps_rollup PSS, 1s interval, D-14)   │
│                    ├──▶ Process-tree teardown (nix setpgid/killpg, mirrors Python's os.killpg)           │
│                    └──▶ Manifest + report writer (JSON meta block + docs/benchmarks/*.md, D-07/D-13)     │
│                                                                                                           │
└──────────────────────────────────────┬───────────────────────────────────────────────────────────────┘
                                        │ HTTP (reqwest streaming client)
                                        ▼
                     ┌─────────────────────────────────────┐
                     │   Frontend-under-test (black box)    │
                     │   Python api_server+tokenizer   OR   │
                     │   Rust rsg-server (Phase 5's API)    │
                     └──────────────────┬────────────────────┘
                                        │ ZMQ ipc:// (msgpack)
                                        ▼
                     ┌─────────────────────────────────────┐
                     │  Shared backend: real scheduler (GPU)│
                     │  OR mock-scheduler (Mac dev pass,     │
                     │  D-03 — harness-mechanics only)       │
                     └─────────────────────────────────────┘

Parallel, out-of-band (D-14): gc.callbacks hook inside every Python process (api_server,
tokenizer, scheduler) → per-process GC event log, read back by the report writer.
Third-party cross-check (D-04, Scenario 2 only): vllm bench serve / sglang.benchmark.serving
hit the same HTTP endpoint independently — no code path through the harness above.
```

### Recommended Project Structure
```
crates/rsg-bench/              # new workspace member (binary crate, like rsg-server)
├── Cargo.toml
├── src/
│   ├── main.rs                 # clap CLI, dispatches to orchestrator
│   ├── orchestrator.rs         # D-08: alternates P,R,P,R..., drives N trials, writes manifest
│   ├── loadgen/
│   │   ├── mod.rs
│   │   ├── closed_loop.rs      # fixed concurrency C
│   │   └── open_loop.rs        # Poisson arrivals
│   ├── cancel.rs                # D-02: seeded RNG think-time + abort-after-N-tokens, drives stream drop
│   ├── sse.rs                   # hand-written `data: ` line parser (CLAUDE.md: eventsource-stream unmaintained)
│   ├── metrics.rs               # hdrhistogram wrapper: TTFT/ITL/E2E recorders
│   ├── memory.rs                # sysinfo RSS + manual smaps_rollup PSS (Pitfall 1), per-role sampler
│   ├── teardown.rs              # nix setpgid/killpg process-group kill
│   ├── scenarios/
│   │   ├── s1_cancel.rs
│   │   ├── s2_saturation.rs     # includes D-09's --num-tokenizer sweep pre-pass
│   │   └── s3_coldstart.rs      # wraps hyperfine (external binary), D-05's "hyperfine-based runner"
│   └── report.rs                # D-13: docs/benchmarks/ md+json writer
└── tests/
    └── ...                      # process-level tests, modeled on rsg-server's tests/common/

python/rsglang/profiling/
└── hook.py                      # EXTEND (not new): add a GC-only install mode for D-15
```

### Pattern 1: A/B Orchestrator Shared Across Scenarios (D-05/D-06/D-08)
**What:** One orchestrator runs N alternating trials (P,R,P,R,...), each trial a call into a
scenario-specific single-trial runner, and writes one manifest JSON per scenario per session.
**When to use:** All three scenarios (D-08 explicitly forbids reimplementing alternation per
scenario).
**Example (manifest shape to reuse, read from the actual Phase 2 sidecar):**
```json
// Source: docs/benchmarks/baseline-profile.json:1-24, read this session
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
D-07 adds rustc version, every CLI flag passed to both frontend and harness, and the RNG
seed(s) on top of this exact shape — same depth, same "script writes the JSON directly" rule
(D-14 of Phase 2, reused by this phase's D-13).

### Pattern 2: Scenario-3 Runner Wraps `hyperfine`, Does Not Reimplement It (D-08)
**What:** Shell out to `hyperfine` with `--conclude` doing teardown, export JSON, parse it back.
**When to use:** Scenario 3 only.
**Example (verbatim existing pattern to port to Rust, not redesign):**
```python
# Source: python/rsglang/profiling/scenarios.py:260-296, read this session
def hyperfine_argv(*, hyperfine, runs, warmup, export_json, once_cmd, stop_cmd):
    # hyperfine runs --conclude/the timed command through its own shell, so
    # these must be built with shlex.join, never by string concatenation.
    return [
        hyperfine, "--runs", str(runs), "--warmup", str(warmup),
        "--export-json", str(export_json),
        "--conclude", shlex.join(list(stop_cmd)),
        shlex.join(list(once_cmd)),
    ]

def parse_hyperfine_json(path):
    doc = json.loads(Path(path).read_text(encoding="utf-8"))
    result = doc["results"][0]
    return {
        "mean_s": result["mean"], "stddev_s": result["stddev"],
        "median_s": result["median"], "min_s": result["min"], "max_s": result["max"],
        "times_s": list(result["times"]), "runs": len(result["times"]),
    }
```
The Rust scenario-3 runner should shell out identically (`std::process::Command` building the
same `--conclude <stop_cmd> <once_cmd>` shape, shell-escaped the same way) and parse the same
`results[0]` fields — not invent a new hyperfine invocation shape.

### Pattern 3: Whole-Tree Memory Sampling with PSS Gated to Linux (D-14, Pitfall 1)
**What:** Walk the process tree from the top PID, sum RSS always; sum PSS only on Linux, set
PSS to "unavailable" (not zero, not a crash) everywhere else or on a permission error.
**Example (verbatim existing pattern the Rust side must mirror for symmetry):**
```python
# Source: python/rsglang/profiling/procs.py:261-293, read this session
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
`sysinfo`'s `Process::memory()` replaces `psutil`'s `memory_info().rss` directly. There is no
`sysinfo` equivalent of `memory_full_info().pss` — the Rust harness must open
`/proc/<pid>/smaps_rollup`, find the `Pss:` line (format: `Pss:          1234 kB`), sum it
per-pid, and set the aggregate to `None`/absent when not on Linux or on a read/permission
error — exactly the same three-way gate as above.

### Pattern 4: Process-Tree Teardown via Process Group (D-14, mirrors existing Python pattern)
**What:** Signal the whole process group, not just the top PID, so GPU child processes never
leak.
**Example (existing Python pattern; Rust side has no prior art yet — first use of `nix` in this
workspace):**
```python
# Source: python/rsglang/profiling/procs.py:238-241, read this session
try:
    os.killpg(pgid, signal.SIGKILL)
except (ProcessLookupError, PermissionError):
    pass
```
```rust
// Rust equivalent, confirmed signatures via docs.rs (nix 0.31.3):
use nix::sys::signal::{killpg, Signal};
use nix::unistd::Pid;
let _ = killpg(Pid::from_raw(pgid), Signal::SIGKILL); // ESRCH/EPERM both mean "already gone"
```

### Pattern 5: Reuse `mock-scheduler`'s Existing Spawn Convention for the Mac Dev Pass (D-03)
**What:** Spawn `mock-scheduler` with unique `ipc://` addresses, exactly like every existing
Phase 3 test does.
**Example (verbatim existing helper, not a new convention):**
```rust
// Source: crates/rsg-server/tests/common/mod.rs:33-56, read this session
let backend_addr = format!("ipc:///tmp/rsgm-{pid}-{n}-0");
let detok_addr = format!("ipc:///tmp/rsgm-{pid}-{n}-1");
Command::new(env!("CARGO_BIN_EXE_mock-scheduler"))
    .args(["--backend-addr", &backend_addr, "--backend-role", "bind",
           "--detok-addr", &detok_addr, "--detok-role", "connect"])
    .args(extra_args)
    .spawn()
```
Phase 7's Mac dev pass should spawn the full `rsg-server` (once Phase 5 lands) pointed at these
same addresses, not reimplement the spawn/handshake-read loop.

### Anti-Patterns to Avoid
- **Trusting `sysinfo` for PSS:** it only has RSS (`Process::memory()`); see Pitfall 1.
- **Reusing `hook.py`'s `install()` unmodified for the "lightweight" GC-only pass:** it always
  starts `tracemalloc` too; see Pitfall 2.
- **Treating D-03's Mac dev pass as a Python-vs-Rust comparison:** Python cannot run on Mac at
  all (PROJECT.md Out of Scope); see Pitfall 3.
- **Assuming `mock-scheduler` already has `--misbehave-uids`/`--behavior`/`--batch-size`:** it
  doesn't, as of this session; see Pitfall 4. (Not a blocker for Phase 7 per D-02, but don't
  write a task that depends on them existing.)
- **`eventsource-stream` for parsing the SSE-ish `data: ` lines:** unmaintained since 2022
  (already flagged in CLAUDE.md); hand-write the trivial line splitter instead.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Percentile/latency statistics | A manual sort-and-index array (what the frozen Python `process_benchmark_results` does — fine for the frozen baseline, not for new Rust code) | `hdrhistogram::Histogram` | Correct merging across repeated A/B trials (D-05/D-06), bounded memory regardless of sample count, standard percentile semantics |
| Process-group signal delivery | A manual PID-tree walk + per-PID `kill()` loop | `nix::unistd::setpgid` at spawn + `nix::sys::signal::killpg` at teardown | One syscall reaches every descendant atomically; matches the existing Python `os.killpg` pattern exactly, so Mac-dev and GPU-run teardown behave identically |
| SSE/chunked streaming parse for `/v1/chat/completions` | A regex-based or `eventsource-stream`-based parser | A ~20-line hand-written `data: ` line splitter over `reqwest::Response::bytes_stream()` | `/generate`'s framing is a single `\n`, not spec SSE (CLAUDE.md's own finding); a generic SSE crate would mis-parse it, and `eventsource-stream` is unmaintained anyway |
| Whole-tree PSS on Linux | A second copy of `/proc/<pid>/smaps` full-field parsing | Read only `/proc/<pid>/smaps_rollup`'s `Pss:` line | `smaps_rollup` exists precisely to avoid the O(mappings) cost of full `smaps`; `psutil` already uses the rollup file internally, so matching it keeps the two languages' numbers comparable |

**Key insight:** every "don't hand-roll" item above has a working reference implementation
already in this repo (Python side, Phase 1-2) or in the CLAUDE.md-pinned crate's own docs — this
phase is porting proven patterns to Rust, not inventing new ones.

## Common Pitfalls

### Pitfall 1: `sysinfo` Cannot Produce PSS — Only RSS
**What goes wrong:** A task that says "sample RSS/PSS with `sysinfo`" (as CLAUDE.md's prose
implies) will silently only get RSS, because `sysinfo::Process` has no PSS field or method.
**Why it happens:** PSS requires reading `/proc/<pid>/smaps_rollup` (or `/proc/<pid>/smaps`) and
summing the `Pss:` field across mappings — a Linux-specific kernel interface `sysinfo`'s
cross-platform API does not wrap.
**How to avoid:** Use `sysinfo` for process discovery (walking the tree by parent PID) and RSS;
add a small Linux-only helper that opens `/proc/<pid>/smaps_rollup`, finds the `Pss:` line, and
parses the kB value. Report PSS as absent (not zero) on macOS, exactly like
`python/rsglang/profiling/procs.py:292` does (`pss_bytes = ... if (is_linux and pss_ok) else None`).
**Warning signs:** A report where Mac-measured "PSS" exactly equals RSS — that means the
Linux-only code path silently never engaged, or was never written.

### Pitfall 2: `hook.py`'s GC+Tracemalloc Install Is Not Separable Today
**What goes wrong:** D-15 requires `gc.callbacks` active but `tracemalloc` and `py-spy` off
during timed runs. `python/rsglang/profiling/hook.py:262-266`'s `install()` unconditionally does
both `gc.callbacks.append(_gc_callback)` and `tracemalloc.start(1)` — there is no existing flag
to split them.
**Why it happens:** Phase 2 only ever needed the combined, maximally-instrumented mode (its own
BENCH-01 explicitly wanted GC + memory + CPU attribution together); nothing in Phase 2 needed a
GC-only mode.
**How to avoid:** Add a new env var (e.g. `RSGLANG_PROFILE_MODE=gc_only|full`, defaulting to
`full` so Phase 2's own callers are unaffected) that `install()` checks before calling
`tracemalloc.start(1)`. This is a small, backward-compatible extension to an existing module, not
a new module — keep the existing sitecustomize-shim/`PYTHONPATH` injection mechanism
(`write_shim`/`hook_env`) unchanged.
**Warning signs:** A "lightweight" timed-run report that still shows the ~3.5x instrumentation
overhead `baseline-profile.md:247-254` measured for the full py-spy+tracemalloc+gc.callbacks
combination — that means `tracemalloc` is still running during what was supposed to be the
uninstrumented pass.

### Pitfall 3: The Python Frontend Cannot Run on the Mac — At All
**What goes wrong:** Planning D-03's "Mac-side development/verification pass" as if it proves
anything about the *comparison* between frontends, rather than just the harness's own code
paths.
**Why it happens:** PROJECT.md's Out-of-Scope table states "Running the backend on a Mac" is
excluded ("Upstream backend supports Linux/CUDA only"), and the Python frontend's own launch path
(`python -m minisgl`, execed unchanged by `rsglang.launch --frontend python`) starts the real
scheduler/engine in the same process group — there's no way to run just the Python frontend's
HTTP layer against a mock backend on a GPU-free machine.
**How to avoid:** Scope D-03's Mac dev pass as "prove `rsg-bench`'s loop modes, cancellation
injection, hdrhistogram recording, and manifest writing work correctly against `rsg-server` +
`mock-scheduler`" — a single-frontend mechanics test, not an A/B rehearsal. If the orchestrator's
alternation logic itself needs exercising pre-GPU, run it with two `rsg-server`+`mock-scheduler`
instances (R vs R) rather than inventing a Python stand-in.
**Warning signs:** A Mac-only CI/dev-test asserting a Python-vs-Rust delta — that assertion is
unachievable without a GPU machine.

### Pitfall 4: `mock-scheduler`'s Misbehavior Flags Are Planned, Not Yet Merged
**What goes wrong:** Assuming `mock-scheduler --misbehave-uids 3,7 --behavior late-abort-token
--batch-size 4` works today, because it's described that way in 03-CONTEXT.md's "Integration
Points" section and the phase brief that spawned this research.
**Why it happens:** Those flags are **designed** in `03-CONTEXT.md` D-09 and **planned** in
`03-06-PLAN.md` (read this session: lines 21, 31, 80, 101, 141, 148 all reference them), but the
actual `crates/rsg-server/src/bin/mock-scheduler.rs` in this worktree (read this session, lines
37-61) only has 7 flags: `--backend-addr`, `--backend-role`, `--detok-addr`, `--detok-role`,
`--prefill-delay-ms`, `--decode-delay-ms`, `--max-seq-len`. Phase 3 is being executed in a
parallel worktree as of this research session, so its state here is a snapshot, not final.
**How to avoid:** Per D-02, Phase 7 doesn't need the misbehavior flags at all (cancellation logic
lives in the harness driver, not in `mock-scheduler`). The planner should write tasks against the
7 flags that demonstrably exist today, and treat any dependency on `--misbehave-uids`/
`--behavior`/`--batch-size`/`--observe-file` as something to re-verify against `mock-scheduler
--help` at execution time, not something to assume from this or any other CONTEXT.md.
**Warning signs:** A task's verification step invoking `mock-scheduler --misbehave-uids ...` and
getting a clap "unexpected argument" error.

### Pitfall 5: Client-Side Stream-Drop Cancellation Is Not Formally Documented the Same Way Server-Side Is
**What goes wrong:** Assuming `reqwest`'s client-side cancel-on-drop is as well-documented and
immediate as hyper's server-side "drop the body stream when the client disconnects" behavior
that CLAUDE.md's Pattern A already relies on and already flags `(MEDIUM: verify with a disconnect
test)`.
**Why it happens:** The only concrete signal found this session was a `reqwest` WASM-target bug
report (GitHub PR #1782, "Fix premature abort for streaming bodies") about keeping an abort
handle alive for the duration of stream consumption — evidence the drop-cancels-request
mechanism exists and has had edge cases, not a guarantee of exact timing on native targets.
**How to avoid:** Treat client-side cancellation timing as needing the same explicit disconnect
test CLAUDE.md already calls for on the server side — write one as part of D-03's Mac dev pass,
using `mock-scheduler`'s existing fixed-delay flags to create a predictable window to cancel
within.
**Warning signs:** Scenario 1's measured abort-to-quiescence time (D-05's own requirement) is
inconsistent between runs in ways thermal/scheduler noise doesn't explain.

## Code Examples

### hdrhistogram basic usage (confirmed via docs.rs, not yet present in this repo)
```rust
// Source: https://docs.rs/hdrhistogram/7.6.0/hdrhistogram/struct.Histogram.html
use hdrhistogram::Histogram;

let mut ttft_us: Histogram<u64> = Histogram::new_with_bounds(1, 60_000_000, 3)?; // 1us..60s, 3 sigfigs
ttft_us.record(measured_ttft_micros)?;
let p99 = ttft_us.value_at_percentile(99.0);
```

### nix process-group kill (confirmed via docs.rs, first use of `nix` in this workspace)
```rust
// Source: https://docs.rs/nix/0.31.3/nix/unistd/fn.setpgid.html,
//         https://docs.rs/nix/0.31.3/nix/sys/signal/index.html
use nix::sys::signal::{killpg, Signal};
use nix::unistd::{setpgid, Pid};

// at spawn, before exec (mirrors the launcher's pattern already described in STATE.md):
setpgid(Pid::from_raw(0), Pid::from_raw(0))?; // new process group, leader = self

// at teardown:
let _ = killpg(Pid::from_raw(pgid), Signal::SIGKILL); // ESRCH/EPERM both mean "already gone"
```

### `minisgl.benchmark.client` reuse shape for BENCH-06 (verbatim existing Phase 2 pattern, D-12)
```python
# Source: python/rsglang/profiling/scenarios.py:203-240, read this session
from minisgl.benchmark.client import benchmark_one, generate_prompt, get_model_name
from openai import AsyncOpenAI

async with AsyncOpenAI(base_url=f"{base_url}/v1", api_key="dummy") as client:
    model = await get_model_name(client)
    tokenizer = AutoTokenizer.from_pretrained(model)
    prompts = [generate_prompt(tokenizer, n) for n in lengths]
    results = await asyncio.gather(*(
        benchmark_one(client, p, output_tokens, model, pbar=False) for p in prompts
    ))
```
BENCH-06's "standard inference" workload (D-12) should call these same three functions the same
way — not redefine prompt generation or client construction.

## State of the Art

No deprecations or stale pins were found. Every crate version CLAUDE.md pinned for this phase's
stack (`hdrhistogram` 7.6.0, `reqwest` 0.13.5, `sysinfo` 0.39.6, `nix` 0.31.3, `hyperfine` 1.20.0,
plus the already-adopted `tokio-util`/`futures`/`tokio-stream`) is still each crate's current
`max_stable_version` on crates.io as of 2026-10-06 — re-querying the registry found no newer
stable release for any of them. `vllm bench serve`'s documented flags (`--backend`, `--endpoint`,
`--percentile-metrics`, `--metric-percentiles`, `--save-result`) also match CLAUDE.md's existing
description, confirmed live against `docs.vllm.ai/en/v0.22.1/cli/bench/serve/`.

**Not deprecated, but newly load-bearing this session:** `python/rsglang/profiling/hook.py` and
`procs.py` (Phase 2 artifacts) are not mentioned in CLAUDE.md at all, but they are the only
working reference implementations in this repo for the exact GC-hook and PSS-sampling mechanics
D-14/D-15 require — the planner should treat them as the canonical pattern to port, not CLAUDE.md's
higher-level prose description of "sysinfo... PSS... gc.callbacks."

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `reqwest`'s client-side cancel-on-drop for a streaming body behaves like hyper's documented server-side drop detection (immediate-ish, no lingering request) | Architecture Patterns / Pitfall 5 | Scenario 1's abort-to-quiescence timing could be slower or less deterministic than the harness assumes; D-03's Mac dev pass must include an explicit disconnect test before this is relied on for real measurements |
| A2 | Adding a `RSGLANG_PROFILE_MODE=gc_only` toggle to `hook.py` is the right fix for Pitfall 2, rather than writing a second, separate shim module | Pitfall 2 / Don't Hand-Roll | A second shim would duplicate the `write_shim`/`hook_env`/sitecustomize-chain-load machinery; this is a design recommendation, not a locked decision — Claude's Discretion in 07-CONTEXT.md explicitly leaves "sampling interval and sysinfo invocation details" open, and this extends that same discretion to the hook's mode split |
| A3 | `sglang.benchmark.serving --backend sglang-oai-chat`'s current CLI flags match CLAUDE.md's existing description (not independently re-fetched this session — only `vllm bench serve` was) | Standard Stack / D-04 cross-check | If sglang's benchmark module's flags have changed, the Scenario-2 cross-check task's exact invocation would need adjustment; low risk since `pip install sglang` for benchmarking is a GPU-machine-only concern this session couldn't reach anyway |
| A4 | `mock-scheduler`'s flag surface will have gained Phase 3's planned `--misbehave-uids`/`--behavior`/`--batch-size`/`--observe-file` flags by the time Phase 7 executes, since Phase 3 is being actively worked in a parallel worktree | Pitfall 4 | Low risk for Phase 7 specifically (D-02 says it doesn't need them), but any task text referencing them should re-verify via `mock-scheduler --help` rather than trusting this snapshot |

**If this table is empty:** N/A — see rows above.

## Open Questions

1. **Exact `crates/rsg-bench` internal module split**
   - What we know: CONTEXT.md leaves this to Claude's Discretion; D-01/D-08 fix the
     architecture *shape* (one configurable driver, one shared orchestrator) but not file layout.
   - What's unclear: whether scenario runners should be separate files (as recommended above) or
     one larger `scenarios.rs` with mode-dispatch, matching this repo's existing preference for
     small, single-purpose files (`rsg-server`'s `handshake.rs`/`transport.rs` split) vs. a
     monolith.
   - Recommendation: follow the `src/bin/*.rs` + `src/*.rs` module split shown above; it mirrors
     `rsg-server`'s existing file granularity.

2. **Where the GC-only mode toggle lives (Pitfall 2 / A2)**
   - What we know: `hook.py`'s `install()` is monolithic today; D-15 needs a split.
   - What's unclear: whether the planner wants an env-var toggle (minimal diff to existing code)
     or a parallel, simpler shim module (zero risk of touching Phase 2's own BENCH-01 behavior,
     at the cost of duplicated shim-injection plumbing).
   - Recommendation: env-var toggle (A2), but this needs an explicit planner decision since it
     touches a file from a different, already-complete phase.

3. **Report file split: one combined file vs. per-scenario files (D-13's open item)**
   - What we know: `docs/benchmarks/baseline-profile.{md,json}` is a single combined file
     covering all three BENCH-01 scenarios plus cross-scenario sections (topology, radix share).
   - What's unclear: whether Phase 7's three scenarios plus the BENCH-06 regression check are
     better served by one `phase7-benchmarks.{md,json}` (consistent with Phase 2's own file) or
     four smaller files.
   - Recommendation: one combined file, matching Phase 2's own precedent directly — but this stays
     Claude's Discretion per CONTEXT.md and should be confirmed with the user if the planner wants
     certainty before writing tasks.

4. **GC-pause reporting shape when the frontend under test is Rust**
   - What we know: BENCH-08 wants "Python GC pause counts" alongside TTFT/P99/RPS per scenario
     report (D-16's table shape). `rsg-server` is Rust and has no GC.
   - What's unclear: whether the report template should show an explicit "N/A — no GC" row for
     the Rust frontend process, or just omit the frontend-process GC row entirely when reporting
     the Rust side (the scheduler-process GC row still applies to both, since the scheduler is
     always Python).
   - Recommendation: show the row with an explicit "N/A (Rust frontend has no garbage collector)"
     value rather than omitting it — this directly supports BENCH-08's "frontend memory usage and
     Python GC pause counts... so P99 spikes can be compared against GC pauses" framing by making
     the asymmetry itself legible, not a gap in the table.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| rustc/cargo | Build `rsg-bench` | ✓ | 1.99.0 | — |
| `uv` | Python env for `hook.py` changes, bootstrap | ✓ | 0.9.2 | — |
| Project `.venv` | Running `python/tests`, `scripts/*.py` | ✗ (not bootstrapped in this worktree) | — | `scripts/bootstrap_mac_env.sh` (Phase 1, already exists) |
| `hyperfine` | Scenario 3 cold-start timing (D-08) | ✗ (not installed) | — | `cargo install hyperfine --version 1.20.0 --locked` |
| GPU / `nvidia-smi` | Headline numbers for all 3 scenarios (the actual measured comparison) | ✗ on this Mac | — | D-03's Mac dev pass proves harness mechanics only; headline numbers wait on a GPU machine + Phase 6 landing (already accounted for in 07-CONTEXT.md, not a new blocker) |
| `vllm` / `sglang` (pip) | Scenario-2 third-party cross-check (D-04) | ✗ on this Mac | — | GPU-machine-only pip install; not exercisable from Mac at all, not just missing today |
| Docker | Not required by this phase | ✓ | — | n/a |

**Missing dependencies with no fallback:** GPU access for the actual headline comparison numbers
— expected and already scoped out of this phase's Mac-verifiable slice per 07-CONTEXT.md's
dependency note; not a planning blocker.

**Missing dependencies with fallback:** `hyperfine` (install via `cargo install`), project
`.venv` (re-run the existing bootstrap script).

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | `cargo test` (built-in; `cargo-nextest` is CLAUDE.md's recommendation but **not actually wired into `scripts/check_all.sh` yet** — confirmed by reading the script) + `pytest` 9.1.1 for `python/tests` |
| Config file | none dedicated; `scripts/check_all.sh` is the phase-gate script (read this session) |
| Quick run command | `cargo test -p rsg-bench` (once the crate exists); `.venv/bin/python -m pytest python/tests -k profiling -q` |
| Full suite command | `scripts/check_all.sh` |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| BENCH-02 | hdrhistogram percentile correctness against a known synthetic distribution | unit | `cargo test -p rsg-bench --test metrics -- --nocapture` | ❌ Wave 0 |
| BENCH-02 | closed-loop driver against `mock-scheduler`+`rsg-server`: sent/completed/cancelled counts match | integration | `cargo test -p rsg-bench --test loadgen_cancel` | ❌ Wave 0 |
| BENCH-03 | Scenario-1 report JSON schema includes P99 TTFT field | integration | `cargo test -p rsg-bench --test s1_report_schema` | ❌ Wave 0 |
| BENCH-04 | RPS-vs-latency curve builder produces monotonically-labeled points | unit | `cargo test -p rsg-bench --test s2_curve` | ❌ Wave 0 |
| BENCH-05 | `hyperfine` JSON parser round-trips Phase 2's existing export shape | unit | `cargo test -p rsg-bench --test hyperfine_parse` | ❌ Wave 0 |
| BENCH-05 | PSS reports `None`/absent on non-Linux, a parsed value on Linux | unit (runs on Mac — proves the gate, not a real Linux PSS number) | `cargo test -p rsg-bench --test memory_pss_gate` | ❌ Wave 0 |
| BENCH-06 | `minisgl.benchmark.client` import path used by `run_s2`-style reuse stays stable | smoke (python) | `.venv/bin/python -m pytest python/tests -k bench_simple_reuse -q` | ❌ Wave 0 |
| BENCH-07 | Orchestrator given N=5 produces exactly `[P,R,P,R,P,R,P,R,P,R]` | unit | `cargo test -p rsg-bench --test orchestrator_alternation` | ❌ Wave 0 |
| BENCH-07 | Manifest JSON matches `baseline-profile.json`'s `meta` block shape plus D-07's additions | unit | `cargo test -p rsg-bench --test manifest_schema` | ❌ Wave 0 |
| BENCH-08 | `hook.py`'s new `gc_only` mode installs `gc.callbacks` without starting `tracemalloc` | unit (python) | `.venv/bin/python -m pytest python/tests -k hook_gc_only -q` | ❌ Wave 0 |

### Sampling Rate
- **Per task commit:** the relevant `cargo test -p rsg-bench --test <name>` or targeted `pytest -k`
- **Per wave merge:** `scripts/check_all.sh`
- **Phase gate:** Full suite green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `crates/rsg-bench/` — new crate, new `tests/` dir; no test infrastructure exists yet for this capability
- [ ] `python/tests/test_hook_gc_only.py` (or similar) — covers BENCH-08's `hook.py` mode-toggle extension
- [ ] A tiny known-distribution fixture for the hdrhistogram unit test (e.g. a fixed array of latencies with a hand-computed P99)
- [ ] `crates/rsg-bench/tests/common/` — likely reuses/extends `rsg-server`'s existing `tests/common/mod.rs` `MockScheduler::spawn` helper rather than duplicating it; whether that helper should move to a shared `dev-dependencies` crate instead of being copy-pasted is a Wave-0-level decision for the planner

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | No | Internal dev benchmarking tool hitting a local server with `api_key="dummy"` (same as upstream's own `bench_simple.py`); no real auth surface introduced |
| V3 Session Management | No | Stateless HTTP requests only; no session state held by the harness |
| V4 Access Control | No | Single-tenant local process, no multi-user concerns |
| V5 Input Validation | Yes | CLI argument parsing via `clap` (already validates types/enums, matching `rsg-server`/`mock-scheduler`'s existing convention); external tool output (`hyperfine --export-json`, `vllm`/`sglang` JSON) must be parsed defensively — mirror `parse_hyperfine_json`'s explicit `ValueError` on a missing `"results"` key rather than trusting external JSON blindly |
| V6 Cryptography | No | No secrets, no crypto surface in this phase |

### Known Threat Patterns for This Stack

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Leaked GPU child processes after a crashed/killed benchmark run (self-inflicted resource exhaustion, not an external attacker) | Denial of Service | `setpgid` at spawn + `killpg(SIGKILL)` at teardown after a grace period, mirroring `procs.py`'s existing `_kill_group`/`kill_process_tree` pattern exactly |
| Malformed/truncated external-tool JSON (`hyperfine`, `vllm`, `sglang`) crashing the report writer mid-run | Tampering (of tool output, not necessarily malicious) | Explicit schema checks with a clear error (as `parse_hyperfine_json` already does), never a bare `json.loads(...)["results"][0]` without a guard |

## Sources

### Primary (HIGH confidence)
- `crates.io` registry API, queried 2026-10-06 (`User-Agent` header required; bare requests 403) — `max_stable_version` for `hdrhistogram` 7.6.0, `reqwest` 0.13.5, `sysinfo` 0.39.6, `nix` 0.31.3, `hyperfine` 1.20.0, `tokio-util` 0.7.19, `tokio-stream` 0.1.19, `futures` 0.3.34, `async-stream` 0.3.6, `serde_bytes` 0.11.19, `bytemuck` 1.25.2, `metrics` 0.24.6, `metrics-exporter-prometheus` 0.18.3
- `gsd-tools query package-legitimacy check --ecosystem crates` — OK verdict for all 5 newly-added crates, run this session
- This repo, read directly this session: `docs/benchmarks/baseline-profile.{md,json}`, `scripts/baseline_profile.py`, `python/rsglang/profiling/{hook,procs,scenarios,session}.py`, `crates/rsg-server/src/bin/mock-scheduler.rs`, `crates/rsg-server/src/main.rs`, `crates/rsg-server/tests/common/mod.rs`, `crates/rsg-server/tests/mock_scheduler_process.rs`, `Cargo.toml` (workspace), `crates/rsg-server/Cargo.toml`, `scripts/check_all.sh`, `python/rsglang/launch.py`, `vendor/mini-sglang/python/minisgl/server/args.py`, `vendor/mini-sglang/python/minisgl/benchmark/client.py`, `vendor/mini-sglang/benchmark/online/bench_simple.py`
- `.planning/phases/{02,03,05}-*/\*-CONTEXT.md`, `.planning/phases/03-zmq-transport-mock-scheduler/03-06-PLAN.md`, `.planning/REQUIREMENTS.md`, `.planning/STATE.md`, `.planning/config.json` — read directly this session

### Secondary (MEDIUM confidence)
- `docs.vllm.ai/en/v0.22.1/cli/bench/serve/` (WebFetch, this session) — `--backend`, `--endpoint`, `--num-prompts`, `--percentile-metrics`, `--metric-percentiles`, `--save-result` flag names and defaults confirmed live; explicitly no mid-stream cancellation flag documented
- docs.rs, `hdrhistogram` 7.6.0 / `nix` 0.31.3 (WebFetch, this session) — `Histogram::new_with_bounds`/`record`/`value_at_percentile`; `nix::unistd::setpgid`; `nix::sys::signal::killpg` existence and module path
- docs.rs, `sysinfo` 0.39.6 `Process` struct (WebFetch, this session) — confirms no PSS method exists, only `memory()` (RSS) and `virtual_memory()`

### Tertiary (LOW confidence)
- WebSearch on `reqwest` client-side stream-drop cancellation semantics — only concrete evidence found was a WASM-target bug-fix PR (#1782) about abort-handle lifetime; native-target timing guarantees were not independently confirmed this session (see Assumption A1 / Pitfall 5)
- `sglang.benchmark.serving --backend sglang-oai-chat`'s current flags — relied on CLAUDE.md's existing citation, not independently re-fetched this session (see Assumption A3)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — every version independently re-verified against crates.io's live registry this session, not carried over from CLAUDE.md unchecked
- Architecture: HIGH for patterns with a working reference implementation already in this repo (A/B manifest shape, hyperfine wrapping, PSS gating, process teardown); MEDIUM for the new `rsg-bench` crate's internal file layout (Claude's Discretion, no prior art for *this* crate specifically)
- Pitfalls: HIGH — all five are grounded in a specific file+line read this session (`sysinfo` docs, `hook.py`, PROJECT.md's Out-of-Scope table, `mock-scheduler.rs` vs. `03-06-PLAN.md`, the reqwest WASM PR), not inferred from training data alone

**Research date:** 2026-10-06
**Valid until:** 30 days (crate versions/CLI flags move slowly; the `mock-scheduler` flag-surface finding (Pitfall 4) is explicitly time-sensitive — re-verify against `mock-scheduler --help` at execution time regardless of this date, since Phase 3 is being executed concurrently in a parallel worktree)
