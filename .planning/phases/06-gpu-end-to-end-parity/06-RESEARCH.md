# Phase 6: GPU End-to-End Parity - Research

**Researched:** 2026-10-06
**Domain:** GPU-backed output-parity verification between two frontends sharing one vendored Python/CUDA backend; abort-path crash/corruption reproduction
**Confidence:** HIGH (code read directly this session for every structural claim) / MEDIUM-LOW only where noted (gated-model access, exact repro trigger window)

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

- **D-01:** The ~100 PAR-01 prompts are a **curated, category-covering corpus** (short/long, multi-turn chat, code, CJK/emoji edge cases), reusing Phase 4's tokenizer test corpus where it overlaps — not a random sample from a public chat dataset, not a generic fixed Q&A set.
- **D-02:** The Llama-3.x comparison target is **Llama-3.2-1B-Instruct** — smallest gated checkpoint, not 3B or 8B — chosen to minimize GPU iteration cost during parity debugging.
- **D-03:** The **same curated prompt corpus (D-01) is reused for both Qwen3-0.6B and Llama-3.2-1B-Instruct**, rather than maintaining two separate sets.
- **D-04:** **Zero tolerance** — any single-token mismatch on any of the 100 prompts fails the gate. Greedy decoding + temperature 0 + one request at a time removes GPU batching, so the run should be fully deterministic end to end; any divergence is a real bug, not acceptable noise.
- **D-05:** On a failure, the immediate next step is to **bisect to the first diverging token** for that prompt and trace whether the root cause sits in tokenization/chat-template (Phase 4), FSM/API (Phase 5), or the backend itself — before deciding whether it's fixable in-phase or a hard blocker.
- **D-06:** The diff compares **token-id sequences first** (isolates whether the backend itself is nondeterministic, since ids are the shared ground truth for both frontends), with **detokenized text as a secondary check** specifically for Rust-vs-Python detokenization discrepancies against a real model.
- **D-07:** Results are recorded as **`docs/benchmarks/parity-report.{md,json}`**, following Phase 2's `baseline-profile.{md,json}` precedent — narrative findings and any bisection results in `.md`, per-prompt token-id/text diff data in `.json` for reproducibility.
- **D-08:** The response to the suspected abort-during-prefill double-free is **not predetermined before the stress test runs**. First, empirically establish (a) whether it reproduces and under what trigger conditions, and (b) its failure mode — full scheduler crash/restart vs. an isolated corrupted request.
- **D-09:** Only once that reproduction data exists, triage by **scope of the fix**, not severity alone: a small, localized fix is attempted as a **shared backend fix** per `PROJECT.md`'s allowed-shared-fixes rule, recorded in `UPSTREAM.md`. A deep/structural CUDA-memory issue is **documented and routed around** by locking the project-wide `--abort-timing` default (Phase 5 D-01) to `deferred`, recorded in `STATE.md`/`UPSTREAM.md` — no deep CUDA-memory debugging. **Reversibility: costly** — this sets the project-wide default Phase 7's A/B benchmarks depend on applying equally to both frontends.
- **D-10:** Criterion 3's concurrent-load match-rate measurement (PAR-02, informational only) uses **one fixed concurrency level** (matching the 128-agent scenario) and the same curated prompt set (D-01), reported once — not a multi-level concurrency curve.
- **D-11:** Criterion 4's 128-request cancellation stress test against the real backend **reuses Phase 5's throwaway stress-test tool as-is** (05-CONTEXT.md D-04), pointed at the real backend instead of `mock-scheduler` through the same `Transport` abstraction — no changes to the tool itself.
- **D-12:** Detection of whether the abort-during-prefill bug reproduced during that stress run is **not built into the stress tool**. Instead, a separate, thin process-health watcher follows the existing `scripts/gpu_phase1_check.sh` convention (`ps`/`nvidia-smi` checks around the run) to catch scheduler crash/restart/zombie states.

### Claude's Discretion

- Exact composition of the curated 100-prompt corpus (how many prompts per category: short/long/multi-turn/code/CJK/emoji) — D-01 fixes the sourcing method, not the exact per-category breakdown.
- The exact fixed concurrency level and sample size for D-10's single-point concurrent-load measurement.
- The precise fix-scope threshold for D-09's "small/localized vs. deep/structural" triage — decided case-by-case once the actual bug (if it reproduces) is read, not fixed in advance.
- Internal module/script layout for `docs/benchmarks/parity-report` generation and D-12's process-health watcher script (new script vs. extending `scripts/gpu_phase1_check.sh`'s pattern).

### Deferred Ideas (OUT OF SCOPE)

None — discussion stayed within phase scope. No reviewed-but-not-folded todos either.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| PAR-01 | On the GPU machine, with greedy decoding (temperature 0) sent one request at a time, the Rust and Python frontends produce identical output on at least 100 prompts | §Architecture Patterns (parity-diff driver design), §Common Pitfalls (Pitfall 1: HTTP response carries no token ids), §Code Examples, §Don't Hand-Roll |
| PAR-02 | Under concurrent load, the output match rate is reported (not a hard gate, because GPU batch composition affects results) | §Architecture Patterns (Pattern 3), §Validation Architecture |
</phase_requirements>

## Summary

Phase 6 is almost entirely an application of patterns this project already established in Phases 1 and 2, pointed at the real GPU backend instead of the mock. There is no new external library to evaluate: the stack is fixed (existing `rsg-server`/`rsg-wire` crates, upstream's vendored Python, `openai`/`transformers`/`aiohttp` already declared in `vendor/mini-sglang/pyproject.toml`), and the work is reading vendored code, writing one new comparison/report tool, and running two existing conventions (human-run GPU bash script with PASS/FAIL-per-step output; `docs/benchmarks/{name}.{md,json}` sidecar pair) against new inputs.

Two findings from reading the vendored scheduler source directly change how the planner should scope this phase's tooling:

1. **The public HTTP API never carries token ids.** `UserReply` (`message/frontend.py:24-27`) — the only thing either frontend's HTTP layer ever sees from the backend's detokenized output — has fields `uid`, `incremental_output: str`, `finished: bool`. No `token_ids` field exists anywhere on that path. D-06's "token-id sequences first" comparison therefore cannot be built by pointing an `openai`-SDK client at `:1919/v1`; it needs a side-channel that taps the raw `DetokenizeMsg.next_token` stream each frontend already receives over ZMQ *before* detokenization. Rust already has this value in hand (`rsg-wire`'s `TokenizerMsg::DetokenizeMsg { next_token: i64, .. }`, `crates/rsg-wire/src/lib.rs:150-154`); getting an equivalent on the Python side means either reusing Python's own `tokenizer/detokenize.py` `DecodeStatus.decoded_ids` field via a debug hook, or a one-line, applies-to-both-frontends log line in `scheduler/scheduler.py`'s send path. Either path needs explicit planning — it is not a byproduct of hitting the existing endpoints.
2. **The suspected abort-during-prefill bug has a concrete, readable mechanism, and it is not a crash by construction.** `TableManager.free()` (`scheduler/table.py:20-21`) and `CacheManager.cache_req()` (`scheduler/cache.py:55-75`) have no double-free guard — freeing the same `req.table_idx`/cache handle twice just appends a duplicate slot into `free_slots`, which can later be handed to two live requests simultaneously (silent KV-cache corruption on an *unrelated* request), not an exception. The scheduler's own comment at `scheduler.py:158` ("overlap scheduling may make the request freed twice, skip second free") shows this exact risk is already known and partially guarded in the *natural*-finish path (`scheduler.py:159`, `if finished and req not in self.finished_reqs`) — but that guard is keyed on `self.finished_reqs`, which `AbortBackendMsg` handling (`scheduler.py:190-195`) never populates. The abort path calls `_free_req_resources` (`scheduler.py:200-202`) unconditionally whenever `prefill_manager.abort_req`/`decode_manager.abort_req` returns a req, with no cross-check against a request whose forward pass was already dispatched to the GPU and is waiting to be finalized by `_process_last_data` in a later iteration. This predicts the failure mode D-08 asks to establish empirically leans toward **silent, isolated corruption of a concurrent request's output**, with a full scheduler crash only if `CacheManager.check_integrity()` (`scheduler/cache.py`, raises `RuntimeError` on `free_pages + cache_pages != num_pages`) happens to run while the backend goes idle — and `run_when_idle()` (`scheduler.py` class method) is the only caller of that check.

**Primary recommendation:** Build one new, Mac-testable-first tool (`scripts/parity_check.py`, matching `baseline_profile.py`'s `discover`/`run`/`validate` subcommand shape) that drives both frontends sequentially through the existing HTTP API for the text/response-shape half of the comparison, and separately captures ground-truth token ids via a thin instrumentation hook on each frontend (new Rust debug-dump code; a recorded, UPSTREAM.md-logged one-line addition to the unrestricted `scheduler/scheduler.py` for the Python side, not to the frozen Tier A frontend files). Reuse Phase 5's stress tool unmodified for D-11, add a bash process-health watcher following `gpu_phase1_check.sh`'s exact convention for D-12, and write `docs/benchmarks/parity-report.{md,json}` following the `baseline-profile.{md,json}` schema precedent exactly.

## Architectural Responsibility Map

This project's architecture is not a web-tier stack; it is a two-process frontend/backend split over a fixed IPC boundary, plus a verification-tooling layer this phase adds to. Capabilities below are mapped to *this* project's tiers, not the generic browser/server/CDN taxonomy.

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Pointing `--frontend rust`/`--frontend python` at the real GPU backend | Launcher (existing, Phase 1) | — | `rsg-server`'s CLI and the Python `launch.py` already select backend addresses; Phase 6 adds no new wiring here beyond `--model` pointed at a real checkpoint |
| Greedy-decoding token-id ground truth capture | Rust Frontend (new debug dump) + Backend (one shared-fix log line) | — | Neither frontend's HTTP layer carries ids (see Summary finding 1); each side must tap its own pre-detokenization data |
| Text-level response parity (`/v1/chat/completions`, `/generate`) | Verification tooling (new `scripts/parity_check.py`) | Rust/Python Frontend (consumed, not modified) | Mirrors `bench_simple.py`'s `AsyncOpenAI`-against-`:1919/v1` pattern per `.claude/CLAUDE.md`'s Benchmark Harness table |
| Concurrent-load match-rate sampling (PAR-02) | Verification tooling | Rust/Python Frontend | Same driver, one fixed concurrency point, informational only |
| 128-request cancellation stress test against the real backend | Rust Frontend (Phase 5's throwaway tool, reused as-is per D-11) | Backend (scheduler, the thing being stressed) | `Transport` trait abstraction swaps `mock-scheduler` for the real `ipc://` addresses without touching the tool |
| Abort-during-prefill crash/corruption detection | Verification tooling (new bash process-health watcher, D-12) | Backend (scheduler, where the bug lives) | Follows `gpu_phase1_check.sh`'s `ps`/`nvidia-smi` convention; kept separate from the stress tool itself |
| Abort-handling fix, if small/localized (D-09) | Backend (`scheduler/scheduler.py`, `prefill.py`, `decode.py`, `table.py`, `cache.py`) | — | None of these files are Tier A/B/C in `UPSTREAM.md` — they are unrestricted backend code, eligible for a shared fix recorded in `UPSTREAM.md`, same precedent as the readiness handshake |
| `docs/benchmarks/parity-report.{md,json}` generation | Verification tooling | — | Direct structural precedent: `docs/benchmarks/baseline-profile.{md,json}` (Phase 2) |

## Standard Stack

No new external libraries are introduced by this phase. Every dependency needed is already present and approved.

### Core (already in the project, reused as-is)

| Library | Version | Purpose | Why Standard | Provenance |
|---------|---------|---------|---------------|------------|
| `openai` (Python) | unpinned in `requirements-mac.txt`; resolved transitively via `uv pip install -e vendor/mini-sglang` | `AsyncOpenAI` client against `:1919/v1` for the text-parity driver | Already imported by upstream's own `benchmark/online/bench_simple.py` (Tier C, frozen) and Phase 2's `baseline_profile.py` scenario drivers | [VERIFIED: vendor/mini-sglang/pyproject.toml:35 — `"openai",` listed under `[project]` dependencies] |
| `transformers` | `4.57.3` (pinned project-wide) | Loading `AutoTokenizer` for any local re-tokenize/detokenize sanity check during bisection (D-05) | Already the project's pinned tokenizer dependency | [VERIFIED: already pinned per CLAUDE.md §0 Ground Truth and Phase 1 package gate — STATE.md line 83] |
| `aiohttp` | `3.14.4` (pinned, Mac lock) | If the concurrent-load driver (PAR-02) needs raw async HTTP rather than the OpenAI SDK, matching Phase 2's scenario-driver pattern | Already approved in Phase 2's package gate | [VERIFIED: requirements-mac.txt:7] |
| Rust: `rsg-wire`, `rsg-server` (in-repo crates) | workspace-pinned | Decode `DetokenizeMsg`/`TokenizerMsg` directly; reuse `Transport` trait to point at real `ipc://` addresses | Already built in Phase 1/3; this phase adds no new crate | [VERIFIED: crates/rsg-wire/src/lib.rs:150-154; crates/rsg-server/src/transport.rs:1-50] |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| Python stdlib `json`, `difflib`, `dataclasses` | stdlib | Per-prompt token-id/text diff computation and the `parity-report.json` sidecar schema | Always — no reason to add a diff library for a simple id-sequence/string compare |
| `hyperfine` | `1.20.0` (already installed per Phase 2) | Not required for this phase's PASS/FAIL gates, but available if Claude's Discretion wants a repeat-run determinism check (D-04's "should be fully deterministic end to end" claim) | Optional sanity check: run the same prompt N times and hyperfine-time it to confirm zero drift, if D-04's zero-tolerance gate needs an extra confidence pass |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Custom `scripts/parity_check.py` text driver | Extend `vendor/mini-sglang/benchmark/online/bench_simple.py` directly | Rejected: `benchmark/` is Tier C (frozen, "treated like Tier A" per `UPSTREAM.md`) — it can be *run unchanged* for the throughput-reference comparison per CLAUDE.md's Benchmark Harness table, but cannot be edited to add greedy-parity diff logic. A new script outside `vendor/` is required. |
| New Python debug hook in `scheduler/scheduler.py` for token-id capture | Add the hook inside `tokenizer/detokenize.py` instead | Rejected: `python/minisgl/tokenizer/` is listed as Tier A ("frontend-only. Never modifiable, no exception") in `UPSTREAM.md`. `scheduler/scheduler.py` is untiered (unrestricted backend code), so it is the only legal place for a Python-side ids-capture hook. |
| A raw ZMQ "sniffer" process subscribing to the detokenizer socket | N/A | Not viable: the scheduler→frontend detokenizer link is PUSH/PULL (single consumer), not PUB/SUB — `scheduler/config.py:29` (`_1` address) and `args.py`'s `zmq_detokenizer_addr`. A passive sniffer cannot coexist with the real frontend consuming the same socket. |

**Installation:** none — no new packages to install for this phase.

## Package Legitimacy Audit

**Not applicable.** This phase installs zero new external packages. All libraries used (`openai`, `transformers`, `aiohttp`, `hyperfine`) were already vetted and approved in Phase 1's and Phase 2's package-legitimacy gates (STATE.md: "Human package gate approved torch 2.9.1, numpy 2.5.3, msgpack 1.2.3, pyzmq 27.2.0, transformers 4.57.3, pytest 9.1.1..."; `requirements-mac.txt` carries the hash-pinned Mac lock). `openai` ships as a vendored upstream dependency (`vendor/mini-sglang/pyproject.toml:35`), not a project-added one.

**Packages removed due to [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none

## Architecture Patterns

### System Architecture Diagram

```
                     curated 100-prompt corpus (D-01, shared across models)
                                    |
                                    v
                    +----------------------------------+
                    |  scripts/parity_check.py (new)    |
                    |  (text driver: openai SDK client) |
                    +----------------------------------+
                        |                        |
                 run against                run against
            --frontend python           --frontend rust
          (sequentially, D-04:              (same model,
           one request at a time)         same backend process)
                        |                        |
                        v                        v
              +-------------------+     +-------------------+
              | Python frontend   |     | Rust frontend      |
              | (api_server,      |     | (rsg-server, Phase |
              |  tokenizer proc)  |     |  5 FSM/HTTP)       |
              +-------------------+     +-------------------+
                        |                        |
             ZMQ ipc://.../minisgl_1   ZMQ ipc://.../minisgl_1
          (DetokenizeMsg: next_token,    (same addr family, Rust
           uid, finished — captured     side already decodes
           via a shared-fix debug log   next_token directly:
           line in scheduler.py, not    rsg-wire TokenizerMsg)
           in the frozen tokenizer/)
                        |                        |
                        +-----------+------------+
                                    v
                         real backend (one GPU,
                         one scheduler process,
                         shared by both runs)
                                    |
                                    v
                    +----------------------------------+
                    |  diff: token-ids first (D-06),    |
                    |  detokenized text second           |
                    |  -> docs/benchmarks/               |
                    |     parity-report.{md,json} (D-07) |
                    +----------------------------------+

Separately, for criterion 4:

  Phase 5's 128-agent stress tool (unmodified, D-11)
            |
            v   via Transport trait, pointed at real ipc:// addrs
  real backend (same scheduler) <---- scripts/<health-watcher>.sh (new, D-12)
                                        ps/nvidia-smi polling around the run,
                                        following gpu_phase1_check.sh's
                                        start_session/wait_no_orphans pattern
```

### Recommended Project Structure

```
scripts/
├── parity_check.py          # new: discover/run/validate subcommands, mirrors baseline_profile.py's shape
├── gpu_phase6_parity.sh      # new: human-run GPU wrapper, PASS/FAIL per step, mktemp log dir
├── gpu_phase1_check.sh       # existing: template for the wrapper's structure
└── gpu_phase2_profile.sh     # existing: template for --py-spy-sudo-style option handling, preflight pattern

docs/benchmarks/
├── parity-report.md         # new (D-07): narrative + bisection findings
├── parity-report.json       # new (D-07): per-prompt token-id/text diff data
└── baseline-profile.{md,json} # existing: the structural precedent to mirror field-for-field

UPSTREAM.md                  # updated if D-09's shared backend fix branch is taken
STATE.md                     # updated with the abort-timing default lock-in (D-09) regardless of branch
```

### Pattern 1: Discover/Run/Validate subcommand shape (Mac-testable-first, GPU-judged-last)

**What:** Both Phase 1 and Phase 2 built their GPU-only verification as a Python driver with separable subcommands (`baseline_profile.py discover|run|validate`) wrapped by a thin bash script that only adds PASS/FAIL judgment and logging. The Python driver's argument parsing, JSON-writing, and comparison logic are unit-testable against `mock-scheduler` on the Mac; only the final GPU run needs real hardware.

**When to use:** Any phase whose success criteria require a real GPU but whose *logic* (argument handling, diff computation, JSON schema validation) does not.

**Example:**
```python
# Source: scripts/baseline_profile.py's CLI shape (read this session, Phase 2 precedent)
# scripts/baseline_profile.py discover --model ... --out discover.json
# scripts/baseline_profile.py run --model ... --out docs/benchmarks/baseline-profile.json
# scripts/baseline_profile.py validate docs/benchmarks/baseline-profile.json --require-gpu
```
Apply the same three-verb shape to `parity_check.py`: `discover` (connectivity + model-name smoke against both frontends, runnable against `mock-scheduler` on the Mac), `run` (the actual 100+ prompt sweep, GPU-only), `validate` (schema-check the written JSON, `--require-gpu` flag to assert it wasn't faked on the Mac).

### Pattern 2: `docs/benchmarks/{name}.{md,json}` sidecar convention

**What:** A machine-written `.json` (schema_version, `meta` block with git commit/upstream SHA/model/GPU/clock source, then per-scenario data) plus a hand-written `.md` that narrates findings and is *tied to* specific JSON fields, never inventing a number the JSON doesn't carry.

**When to use:** D-07 requires this exact shape for `parity-report.{md,json}`.

**Example:**
```json
// Source: docs/benchmarks/baseline-profile.json (read this session, Phase 2 precedent)
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
`parity-report.json`'s `meta` block should carry the same fields (git commit, upstream SHA, GPU, both models tested) plus a `scenarios` (or `prompts`) array recording, per prompt: input text, Python token ids, Rust token ids, Python text, Rust text, and a `match: bool`. When the JSON carries a `null` for anything (e.g. a prompt that errored), the `.md` must say so explicitly rather than omit it — `baseline-profile.md`'s "Where the JSON carries a `null` for a metric, this report says so explicitly" convention (line 6) is binding here too.

### Pattern 3: Human-run GPU bash wrapper with PASS/FAIL-per-step output

**What:** `set -euo pipefail`, `mktemp -d` log directory, a `record()` helper appending to a `RESULTS` array, one `step N` function per criterion, a trap-based `cleanup()` safety net, and a final summary loop that exits 0 only if every recorded result is `PASS`.

**When to use:** Every GPU-only phase so far (1, 2) uses exactly this shape; Phase 6's own GPU script (and D-12's process-health watcher) should follow it rather than inventing a new convention.

**Example:**
```bash
# Source: scripts/gpu_phase1_check.sh (read this session), lines 60-75
declare -a RESULTS=()
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

### Pattern 4: Deterministic greedy-decode comparison needs sequential, not concurrent, GPU access for the hard gate

**What:** D-04's zero-tolerance gate depends on removing GPU batching effects entirely. Upstream's scheduler batches whatever requests are pending at `schedule_next_batch` time (`scheduler.py`'s `_schedule_next_batch`); running Python's 100 prompts and Rust's 100 prompts *concurrently* against the same backend would reintroduce batch-composition nondeterminism into what's supposed to be the deterministic baseline.

**When to use:** PAR-01's hard-gate run. Run Python's 100-prompt sweep to completion, then Rust's 100-prompt sweep, each one-request-at-a-time (never two in flight). PAR-02's concurrent-load measurement is the *separate*, informational-only criterion where concurrency is deliberately introduced (D-10).

### Anti-Patterns to Avoid

- **Treating `/v1/chat/completions` text output as a stand-in for token-id comparison:** detokenization is lossy in the reverse direction (text → ids is not guaranteed to reproduce the exact ids a greedy decode produced, especially across BPE merge boundaries). A text-only match does not prove PAR-01's token-id claim; do not skip the ids tap because it's easier.
- **Editing `vendor/mini-sglang/python/minisgl/tokenizer/` or `server/api_server.py` to add instrumentation:** both are Tier A ("never modifiable, no exception" per `UPSTREAM.md`). Any Python-side debug hook must live in untiered backend code (`scheduler/scheduler.py` and friends) or in a new script outside `vendor/`.
- **Running the Python and Rust 100-prompt sweeps interleaved or concurrently for the PAR-01 hard gate:** defeats the "one request at a time" determinism D-04 relies on. Run them as two fully sequential sweeps.
- **Letting `_free_req_resources` run twice silently:** if D-09's shared-fix branch is taken, any fix must guard against freeing the same `req.table_idx`/cache handle twice — the existing `self.finished_reqs` guard (`scheduler.py:159`) is scoped to the natural-finish path only and does not cover the abort path (`scheduler.py:190-195`). A fix that re-purposes `self.finished_reqs`-style tracking must extend coverage to aborted-and-freed reqs too, or it will silently reintroduce the same bug under a different trigger.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Text-level parity client against `/v1/chat/completions` | A new raw-HTTP SSE parser from scratch | The already-approved `openai` `AsyncOpenAI` client, same as `bench_simple.py` and Phase 2's scenario drivers | SSE framing for `/v1/chat/completions` is spec-compliant (unlike `/generate`'s single-newline framing per CLAUDE.md's "What NOT to Use" table); the SDK already parses it correctly |
| PASS/FAIL GPU verification harness shape | A new bespoke script structure | `gpu_phase1_check.sh`/`gpu_phase2_profile.sh`'s `record()`/`cleanup()`/`trap`/summary-loop pattern | Already proven across two phases; reinventing it risks losing the orphan-process safety net (`cleanup()` trap) that matters specifically because this phase spawns real GPU processes |
| Process-crash/zombie detection for the abort-during-prefill stress run (D-12) | New crash-detection logic inside the stress tool | `gpu_phase1_check.sh`'s `alive()`/`on_gpu()`/`wait_no_orphans()` helpers, in a new standalone script | D-12 explicitly keeps this out of Phase 5's stress tool; `gpu_phase1_check.sh` already has every needed primitive (process liveness, `nvidia-smi`-listed check, orphan wait) |

**Key insight:** every tool this phase needs a *shape* for already exists in Phase 1 or Phase 2's verification scripts. The only genuinely new logic is the token-id/text diff computation itself and the ids-capture instrumentation — everything else is assembly of existing, already-proven pieces.

## Common Pitfalls

### Pitfall 1: The HTTP response never carries token ids — D-06's primary comparison has no data source without new instrumentation

**What goes wrong:** A naive parity driver built purely against `/v1/chat/completions` or `/generate` can only ever compare *text*, never the "token-id sequences first" ground truth D-06 requires.

**Why it happens:** `UserReply` — the only message either frontend's HTTP layer receives back from its own tokenizer/detokenizer process (`message/frontend.py:24-27`) — is:
```python
@dataclass
class UserReply(BaseFrontendMsg):
    uid: int
    incremental_output: str
    finished: bool
```
There is no `token_ids` field. [VERIFIED: vendor/mini-sglang/python/minisgl/message/frontend.py:24-27]

**How to avoid:** Capture ids before detokenization, independently on each side:
- Rust: trivial — the FSM already decodes `TokenizerMsg::DetokenizeMsg { uid: i64, next_token: i64, finished: bool }` off the wire (`crates/rsg-wire/src/lib.rs:150-154`); add a debug-dump path (new Rust code, no Tier conflict) that records `next_token` per uid to a side file/log for the parity run.
- Python: `tokenizer/detokenize.py`'s `DetokenizeManager.detokenize` already accumulates `DecodeStatus.decoded_ids` per uid (`tokenizer/detokenize.py:55-61`) — but that file is Tier A (frozen, no exception). The legal place for a Python-side ids-capture hook is the **untiered** `scheduler/scheduler.py`, which already has every `DetokenizeMsg` passing through `_process_last_data`'s `reply` list (`scheduler.py:144-167`) before it's even sent to the frontend. A one-line debug log of `(uid, next_token, finished)` there, applying identically regardless of which frontend is attached, is a shared-backend addition recorded in `UPSTREAM.md` — not a frontend change.

**Warning signs:** If the parity driver's design doc/plan only mentions hitting `:1919/v1/chat/completions` and never mentions a debug-log or side-channel for ids, PAR-01's "token-id sequences first" requirement (D-06) will not be satisfiable as planned.

### Pitfall 2: Abort-during-prefill is predicted to be silent corruption, not a crash — a stress test that only checks "did the process die" can false-PASS

**What goes wrong:** D-12's process-health watcher (ps/nvidia-smi liveness) will correctly catch a full scheduler crash, but the code strongly suggests the more likely failure mode is a *live, running* scheduler silently handing the same KV-cache page slot to two different concurrent requests — garbling one unrelated request's output with no process-level signal at all.

**Why it happens:** `TableManager.free()` has no double-free guard:
```python
def free(self, slot: int) -> None:
    self._free_slots.append(slot)
```
[VERIFIED: vendor/mini-sglang/python/minisgl/scheduler/table.py:20-21] — calling `free()` twice with the same `slot` just appends it twice; the next two `allocate()` calls (`self._free_slots.pop()`, table.py:17) will each hand out the *same* physical slot to two different requests.

The call site that can trigger this, `_free_req_resources` (`scheduler.py:200-202`), is invoked unconditionally from the `AbortBackendMsg` branch (`scheduler.py:190-195`, no guard) and separately, guarded only against the *natural*-finish double-free, from `_process_last_data` (`scheduler.py:158-162`):
```python
# NOTE: overlap scheduling may make the request freed twice, skip second free
if finished and req not in self.finished_reqs:
    self.decode_manager.remove_req(req)
    self._free_req_resources(req)
    new_finished_reqs.add(req)
```
[VERIFIED: vendor/mini-sglang/python/minisgl/scheduler/scheduler.py:158-162] — `self.finished_reqs` is never populated by the abort path, so if a request's forward pass was already dispatched to the GPU in iteration N (added to `decode_manager.running_reqs` via `filter_reqs`) and an `AbortBackendMsg` for it arrives and is processed in iteration N+1 *before* `_process_last_data` finalizes iteration N's results in that same N+1 call, both the abort handler and `_process_last_data` can call `_free_req_resources` on the same `req`. The only backstop that would ever turn this into a visible crash is `CacheManager.check_integrity()` (raises `RuntimeError` if `free_pages + cache_pages != num_pages`), and its only caller is `run_when_idle()` (`scheduler.py`'s idle hook) — meaning under continuous load (never idle), the mismatch can persist undetected indefinitely.

**How to avoid:** Design D-12's watcher (and the stress test's own result reporting) to check for more than liveness: compare the stress run's observed per-request outputs/finish reasons for anomalies (e.g. a finished request whose output looks like a different request's, or a request that silently never completes), not just "is the scheduler process still running." If D-09's "reproduce first" step surfaces this, document the empirical failure mode precisely (crash vs. corrupted-but-live) — do not assume crash because it's easier to detect.

**Warning signs:** The stress run exits "clean" (no crash, `nvidia-smi` and `ps` both happy) but a subsequent parity check on an unrelated request from the *same* run shows an unexplained token-id mismatch, or two requests whose outputs look suspiciously swapped/overlapping.

### Pitfall 3: Running Python's and Rust's 100-prompt sweeps concurrently (even accidentally, by not tearing down between runs) breaks the determinism premise

**What goes wrong:** If any part of the harness launches both frontends against the same backend simultaneously for the hard-gate run, GPU batch composition becomes a confound again, and a mismatch found could be a real bug or could just be batching nondeterminism — exactly what D-04's "one request at a time" design is meant to rule out.

**Why it happens:** The backend's `_schedule_next_batch()` greedily batches whatever is pending (`scheduler.py`'s prefill/decode scheduling); it has no concept of "which frontend" a request came from.

**How to avoid:** Fully stop/restart (or at minimum fully drain and confirm zero in-flight requests on) the frontend between the Python sweep and the Rust sweep for the PAR-01 hard gate. PAR-02's concurrent measurement is the one place concurrency is intentional (D-10).

## Code Examples

### Reusing Phase 5's stress tool against the real backend (D-11)

```
# Conceptual flow (Phase 5's stress tool does not exist yet in this worktree as of this
# research session — 05-CONTEXT.md is the authoritative interface contract per that
# phase's own dependency note). Per D-11, no code changes to the tool itself:

<phase-5-stress-tool-binary> \
  --transport zmq \
  --backend-addr ipc:///tmp/minisgl_0<suffix> \
  --detok-addr ipc:///tmp/minisgl_1<suffix> \
  --agents 128 --abort-timing <immediate|deferred>
```
The only change from Phase 5's Mac usage is which `ipc://` addresses are passed — the real backend's addresses (bound by the real scheduler, not `mock-scheduler`), reached through the same `Transport`/`BackendSink`/`DetokSource` trait boundary already defined in `crates/rsg-server/src/transport.rs` (read this session, lines 1-50).

### Socket address family (for wiring the GPU script correctly)

```python
# Source: vendor/mini-sglang/python/minisgl/scheduler/config.py:21-33 (read this session)
_unique_suffix: str = field(default_factory=_get_pid_suffix)
# ipc:///tmp/minisgl_0<suffix>  -> backend (scheduler PULL; scheduler binds)
# ipc:///tmp/minisgl_1<suffix>  -> detokenizer (scheduler PUSH)
# ipc:///tmp/minisgl_2<suffix>  -> TP broadcast
# Source: vendor/mini-sglang/python/minisgl/server/args.py (read this session)
# ipc:///tmp/minisgl_3<suffix>  -> frontend
# ipc:///tmp/minisgl_4<suffix>  -> tokenizer (only if --num-tokenizer > 0)
```

## State of the Art

Not applicable in the usual sense — this phase introduces no new external technology. The relevant "state of the art" is this project's own evolving convention, already current as of Phase 2:

| Prior Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Ad hoc GPU verification per phase | `discover`/`run`/`validate` Python driver + bash PASS/FAIL wrapper, `docs/benchmarks/{name}.{md,json}` sidecar | Phase 2 (2026-10-06) | Phase 6 should follow this convention exactly rather than inventing a new verification shape |

**Deprecated/outdated:** None.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | The abort-during-prefill failure mode will most often manifest as silent KV-page corruption on an unrelated concurrent request rather than a scheduler crash | Summary, Pitfall 2 | This is a code-reading-based prediction (D-08 explicitly requires empirical confirmation), not an observed run. If wrong — e.g. it reliably crashes instead — D-12's watcher design (which should check for more than liveness per Pitfall 2) is still correct as a superset, but the planner should not treat "crash" as impossible; both detection paths are needed regardless. |
| A2 | A one-line debug log added to `scheduler/scheduler.py` is an acceptable, UPSTREAM.md-recordable way to capture ground-truth token ids, because that file is untiered (not Tier A/B/C) | Pitfall 1, Alternatives Considered | If the project owner reads this as touching backend behavior too invasively (even a log line), the fallback is a Rust-only ids capture plus a *separate*, out-of-band raw-ZMQ oracle client (new tool, no vendored-file edits at all) that submits the same tokenized input directly to the scheduler and captures `DetokenizeMsg` itself — more code, but zero vendored-file touches. |
| A3 | Llama-3.2-1B-Instruct is still the smallest Llama-3.x Instruct checkpoint and still gated on Hugging Face as of this research date | User Constraints (D-02, carried from CONTEXT.md, not independently re-verified this session) | STATE.md already records this as an open Phase 4 blocker ("Access to the gated Llama-3.x repo is needed") — if access has since changed, the planner should re-confirm before scheduling GPU time around it. |

**If this table is empty:** N/A — see above.

## Open Questions

1. **Does Phase 4's tokenizer test corpus actually exist by the time Phase 6 executes, and in a format Phase 6 can reuse directly?**
   - What we know: D-01 says to reuse it "where it overlaps." 05-CONTEXT.md notes Phase 4's own context was "in progress but unplanned" (in a different worktree) as of this research session, and this worktree has no `.planning/phases/04-tokenizer-detokenizer-parity/` directory at all yet.
   - What's unclear: the corpus's file location, format, and exact category labels are not yet fixed anywhere this research could read.
   - Recommendation: the planner should design Phase 6's corpus-loading step defensively — build/curate the 100-prompt set as a self-contained artifact for Phase 6 (optionally seeded from Phase 4's fixtures *if* present by merge time), rather than hard-depending on a specific Phase 4 file path that may not exist yet.

2. **Exact trigger window and reproduction rate for the abort-during-prefill bug.**
   - What we know: the mechanism (Pitfall 2) is readable from source; the window is "abort for uid X processed in iteration N+1's message loop, while X's forward pass from iteration N is still pending in `last_data`, to be finalized by `_process_last_data` later in that same N+1 call."
   - What's unclear: how often this window is actually hit under the 128-agent stress test's real timing (vs. theoretically possible but rare), and whether it requires specifically "during prefill" (as currently named) or can also occur for a request transitioning out of decode.
   - Recommendation: D-08 already calls for empirical reproduction before any fix decision — this is exactly right; the planner should not try to resolve this analytically beyond what's in Pitfall 2.

3. **Where exactly does the Python-side ids-capture hook live if `scheduler.py` instrumentation is rejected?**
   - See Assumption A2's fallback (a standalone raw-ZMQ oracle client). Not resolved here because it is a Claude's-Discretion-level implementation choice per CONTEXT.md, not a locked decision.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| GPU machine (Linux, CUDA, `nvidia-smi`) | All of Phase 6 (PAR-01, PAR-02, criteria 1-4) | ✗ (not checked from this Mac session — GPU-only phase, matches Phase 1/2's own "needs GPU machine access" blocker) | — | None — this phase cannot execute without GPU access, same as Phase 1/2 |
| `nvidia-smi`, `curl`, `cargo`, `setsid` on the GPU box | GPU verification script preflight | Unknown (GPU-only) | — | `gpu_phase1_check.sh`'s existing preflight already checks for these; Phase 6's wrapper should do the same |
| Hugging Face access to `meta-llama/Llama-3.2-1B-Instruct` (gated) | Criterion 2's Llama-3.x comparison | Unknown | — | Already an open Phase 4 blocker per STATE.md; if access is unavailable at execution time, criterion 2's Llama comparison is blocked independent of Phase 6's own work |
| `openai`, `transformers`, `aiohttp` Python packages | Parity driver | ✓ on the GPU box once `uv pip install -e vendor/mini-sglang` runs (per Phase 2's `Reproduce` section) | openai: unpinned/transitive; transformers 4.57.3; aiohttp 3.14.4 | — |

**Missing dependencies with no fallback:**
- GPU machine access — this phase is entirely GPU-gated, same as Phases 1 and 2.
- Llama-3.2-1B-Instruct gated-repo access — blocks only criterion 2's Llama comparison, not criteria 1/3/4's Qwen3-0.6B work, so it need not block the whole phase.

**Missing dependencies with fallback:** none identified beyond the above.

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | `pytest` (Python, `cargo test --workspace` (Rust) for any Mac-testable logic in the new driver/script; human-run bash GPU script for the hardware-gated criteria themselves — same split as Phase 1/2 |
| Config file | none new — reuses the existing `python/tests/`, `crates/*/tests/` layout |
| Quick run command | `cargo test --workspace && .venv/bin/python -m pytest python/tests -q` (existing `scripts/check_all.sh` steps 1-2) |
| Full suite command | `bash scripts/check_all.sh` (Mac) then the new `scripts/gpu_phase6_parity.sh` (GPU, human-run) |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| PAR-01 | `parity_check.py`'s argument parsing, JSON schema, and diff-computation logic are correct | unit (Mac) | `pytest python/tests/test_parity_check.py -q` | ❌ Wave 0 |
| PAR-01 | `parity_check.py discover` smoke-tests connectivity against `mock-scheduler` | integration (Mac) | `python scripts/parity_check.py discover --mock ...` | ❌ Wave 0 (tool doesn't exist yet) |
| PAR-01 | 100-prompt greedy hard gate, Qwen3-0.6B and Llama-3.2-1B-Instruct, zero-tolerance | manual-only (GPU) | `bash scripts/gpu_phase6_parity.sh` | ❌ Wave 0 — GPU-gated by nature, matches Phase 1/2 precedent; justification: real-model, real-GPU output cannot be reproduced on the Mac |
| PAR-02 | Concurrent-load match-rate measurement, one fixed concurrency point | manual-only (GPU) | same GPU script, separate step | ❌ Wave 0 |
| (criterion 4) | 128-request cancellation stress test against the real backend; process-health watcher | manual-only (GPU) | same GPU script, reusing Phase 5's tool | ❌ Wave 0 — also blocked on Phase 5's tool existing |

### Sampling Rate
- **Per task commit:** `cargo test --workspace` / `pytest python/tests -q` for any new Mac-testable logic in `parity_check.py` or the health watcher's bash helper functions (sourced and unit-tested the same way `gpu_phase1_check.sh` already is, per its own comment: "Mac helper tests in `python/tests/test_gpu_check_script.py` source this file").
- **Per wave merge:** `bash scripts/check_all.sh` (Mac gate).
- **Phase gate:** `bash scripts/gpu_phase6_parity.sh` green, human-signed-off, on the GPU machine — matches Phase 1/2's "a human runs it once at the end of the phase and signs off" convention exactly.

### Wave 0 Gaps
- [ ] `scripts/parity_check.py` — does not exist yet; needed for PAR-01/PAR-02
- [ ] `scripts/gpu_phase6_parity.sh` — does not exist yet; the human-run GPU wrapper
- [ ] `python/tests/test_parity_check.py` — Mac-side unit coverage for the diff logic, following `python/tests/test_gpu_check_script.py`'s pattern of sourcing bash helpers for testability
- [ ] Phase 5's stress-test tool (D-11 dependency) and Phase 5's HTTP/FSM code generally — does not exist in this worktree as of this research session; Phase 6 cannot execute (only plan) until it lands

*(Not all gaps are closable within Phase 6's own planning — the Phase 5 dependency gap is a cross-phase sequencing fact, not a Phase 6 tooling omission.)*

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | No | No auth surface introduced or exercised; the GPU box runs under human/operator control, not multi-tenant |
| V3 Session Management | No | No sessions |
| V4 Access Control | No | No new access-control surface |
| V5 Input Validation | No (already owned by Phase 5) | Phase 6 only *exercises* Phase 5's existing overlong-prompt 400 and FSM validation against real prompts; it does not add new input-handling code |
| V6 Cryptography | No | No new cryptography |

### Known Threat Patterns for this stack

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Resource corruption via double-free (the abort-during-prefill bug itself) | Tampering (of another request's data, not attacker-controlled) | Not a security boundary issue in the classic sense (no untrusted multi-tenant input in this project's threat model — GPU box is single-operator), but D-09's shared-fix-or-document-and-route-around triage is the correct engineering response; do not treat it as requiring ASVS-level mitigation design beyond what D-08/D-09 already specify |

This phase's "security" surface is effectively nil beyond what Phase 5 already owns — it is a correctness/parity verification phase on infrastructure the operator controls directly, not a new externally-reachable feature.

## Sources

### Primary (HIGH confidence — read directly this session)

- `vendor/mini-sglang/python/minisgl/scheduler/scheduler.py` (lines 134-202) — `_process_last_data`, `_process_one_msg`'s `AbortBackendMsg` branch, `_free_req_resources`
- `vendor/mini-sglang/python/minisgl/scheduler/table.py` (full file, 21 lines) — `TableManager.free`/`allocate`, no double-free guard
- `vendor/mini-sglang/python/minisgl/scheduler/cache.py` (lines 1-135) — `CacheManager.cache_req`, `check_integrity`
- `vendor/mini-sglang/python/minisgl/scheduler/prefill.py` (full file) — `PrefillManager.abort_req`, chunked-req handling
- `vendor/mini-sglang/python/minisgl/scheduler/decode.py` (full file) — `DecodeManager.abort_req`, `running_reqs`
- `vendor/mini-sglang/python/minisgl/message/frontend.py` (lines 1-27) — `UserReply` field list (no token ids)
- `vendor/mini-sglang/python/minisgl/tokenizer/detokenize.py` (lines 1-90) — `DecodeStatus`, `DetokenizeManager.detokenize`
- `vendor/mini-sglang/python/minisgl/server/api_server.py` (lines 1-60, 200-260) — `/generate`, `/v1/chat/completions` route shapes
- `vendor/mini-sglang/python/minisgl/scheduler/config.py` (lines 21-33) — `ipc://` address family `_0`.._2
- `vendor/mini-sglang/python/minisgl/server/args.py` (lines 1-50) — `ServerArgs`, `zmq_frontend_addr`/`zmq_tokenizer_addr` (`_3`/`_4`)
- `vendor/mini-sglang/benchmark/online/bench_simple.py` (full file) — the frozen Tier C throughput-reference benchmark
- `vendor/mini-sglang/pyproject.toml` (line 35) — `openai` already a vendored dependency
- `crates/rsg-wire/src/lib.rs` (lines 140-163) — `TokenizerMsg::DetokenizeMsg`/`BatchTokenizerMsg` field shapes
- `crates/rsg-server/src/transport.rs` (lines 1-80) — `Transport`/`BackendSink`/`DetokSource` traits, `Role`/`Endpoint`
- `scripts/gpu_phase1_check.sh`, `scripts/gpu_phase2_profile.sh` (full files) — the human-run GPU verification convention
- `docs/benchmarks/baseline-profile.md`, `docs/benchmarks/baseline-profile.json` (read in full / head) — the `docs/benchmarks/{name}.{md,json}` sidecar precedent
- `UPSTREAM.md` (full file) — Tier A/B/C frozen-path definitions, modified-files ledger
- `.planning/PROJECT.md`, `.planning/REQUIREMENTS.md`, `.planning/STATE.md`, `.planning/ROADMAP.md` — requirements text, traceability, decisions, blockers
- `.planning/phases/05-request-lifecycle-http-api/05-CONTEXT.md`, `.planning/phases/06-gpu-end-to-end-parity/06-CONTEXT.md` — the authoritative interface contracts this research treats as ground truth per their own "dependency note" sections
- `.claude/CLAUDE.md` (project instructions, provided in context) — stack pins, Benchmark Harness table, What NOT to Use table

### Secondary (MEDIUM confidence)

None used this session beyond what's cited above as primary — no WebSearch/MCP provider calls were needed; this phase's research question was entirely answerable by reading the vendored source and the project's own prior artifacts.

### Tertiary (LOW confidence)

None.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — no new libraries; every dependency traced to an already-pinned, already-approved source
- Architecture: HIGH — patterns are direct extensions of Phase 1/2 conventions, read from their actual scripts/output this session
- Abort-during-prefill bug mechanism: HIGH on the *mechanism* (read directly, line-cited), LOW on the *empirical reproduction rate/trigger frequency* (D-08 explicitly defers this to the stress test itself — correctly, per this research)
- Pitfalls: HIGH — each pitfall traces to specific, quoted, line-numbered source

**Research date:** 2026-10-06
**Valid until:** Tied to the vendored upstream SHA (`9a91cfafe754aa85daee49998176275667eb58f2`) and the current state of Phases 3/4/5 (unplanned/in-progress as of this date) — re-check the Tier A/B/C listing in `UPSTREAM.md` and this phase's dependency on Phase 5's not-yet-existing code if either changes materially before Phase 6 executes.
