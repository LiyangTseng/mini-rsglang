# Findings: does a drop-in Rust frontend rewrite actually help?

**TL;DR (v2, Phase 8).** Holding the GPU backend fixed and identical for
both frontends: cold start and host RAM are a clear, large Rust win; raw
throughput is statistically tied; and, since Phase 8, tail latency under
load is tied with Python too (S1 P99 TTFT 302 ms vs 314 ms for tuned
Python; the CI of the difference includes zero).

Phase 7 measured Rust's S1 P99 TTFT at **4687 ms**, ~10× Python. The v1 write-up
below blamed the ZMQ transport to the frozen scheduler. **That was wrong.**
Phase 8 instrumented every stage of the request path, rejected the ZMQ
hypothesis with data, and found an ordinary bug in the Rust frontend.
Every request deep-cloned the whole Hugging Face tokenizer (~15–30 MiB of
small allocations) before it could be submitted. Under a burst of 128
concurrent requests, those clones each took seconds. Sharing one
`Arc<Tokenizer>` removed the tail: P99 went from 4868 ms to 302 ms on the
same box. See [v2: the actual root cause (Phase 8)](#v2-the-actual-root-cause-phase-8).

The v1 sections are kept as written, labelled as superseded, because the
wrong turn is part of the record.

## Situation

mini-sglang's Python frontend (HTTP ingress, tokenizer, detokenizer) talks
to its GPU scheduler over ZMQ PUSH/PULL sockets using a fixed MessagePack
wire format. The premise this project tests: if the GPU backend is left
completely untouched, does rewriting *only* the frontend in Rust reduce
host-overhead latency in the scenarios where host overhead — not GPU compute
— dominates request time?

## Task

Build a Rust frontend that is byte-compatible with the existing wire
protocol (so the same, unmodified Python scheduler process can't tell which
frontend it's talking to), verify it produces identical output to the
Python frontend on the same backend (differential testing, not just unit
tests), and build a benchmark harness that isolates and measures exactly
three scenarios engineered to be host-overhead-bound rather than GPU-bound:

1. **S1** — 128 concurrent agents, dynamic requests with mid-stream
   cancellations. Measures P50/P90/P99 time-to-first-token (TTFT).
2. **S2** — 32-token short-prompt saturation. Measures the maximum
   sustainable requests/second (RPS) and the TTFT saturation curve as load
   ramps.
3. **S3** — cold start and host RAM (process-tree RSS/PSS), separating
   frontend-only readiness from end-to-end readiness (which includes GPU
   weight loading, identical for both frontends).

## Action

**Real GPU results (NVIDIA RTX 3050, `scripts/gpu_phase7_bench.sh`, 5 rounds
per arm, Welch's 95% CI). Full tables: `docs/benchmarks/frontend-benchmarks.md`.**

Along the way, four real, reproducible bugs were found and fixed while
getting this benchmark to actually run on real GPU hardware (not just
pass in CI with a mock backend):

- `sysinfo` on Linux surfaces each userland OS thread of a multi-threaded
  process (tokio's worker threads) as its own pid-like entry. Two places in
  the harness that walked the process tree by raw pid/pgid treated every
  thread as a distinct process, inflating both RSS totals (summed once per
  thread instead of once per process) and process-group membership counts.
- The harness's log-watching loop truncated a `String` at a raw byte
  offset to find a readiness marker; the server's log contains multi-byte
  UTF-8 characters (tqdm's progress-bar glyphs during CUDA graph capture),
  and the cut reliably landed mid-character for one frontend's specific log
  timing, panicking `String::drain`.
- Two missing/misconfigured dependencies on the GPU box itself (`ninja` not
  on `$PATH`, the system CUDA toolkit several major versions behind what
  PyTorch/flashinfer were built against) silently prevented the real
  backend from ever reaching a ready state, for every trial, for hours,
  before being root-caused.
- `standard_throughput` (BENCH-06, upstream's own `bench_simple.py`
  workload — 64 concurrent requests, prompts up to 8192 tokens,
  deliberately backend/GPU-bound rather than host-overhead-bound) failed
  5/5 rust trials, every round, with a "Connection error" from the Python
  load driver. Root cause: `rsg-server`'s own backend-inactivity watchdog
  (60s default, reset per token) force-failed requests the backend was
  still legitimately working on — on this 8GB GPU, Python's own measured
  p99 TTFT in the *identical* workload is ~100 **seconds**, and Python's
  frontend has no equivalent watchdog. `rsglang.launch --frontend rust`
  never forwarded `--backend-timeout-ms` to `rsg-server` at all (no way to
  raise it for a workload that needs to); fixed by adding the passthrough,
  mirroring the existing `--abort-timing` one exactly.

## Result

These are the Phase 7 numbers, measured before the Phase 8 fix. The current
numbers are in [v2](#v2-the-actual-root-cause-phase-8).

**Cold start & host RAM (S3): Rust wins, clearly.** Frontend RSS: **490 MB**
(Rust) vs. 1.48 GB (Python, default `--num-tokenizer`) vs. 3.03 GB (Python,
sweep-tuned) — a 67–84% reduction, statistically significant
(p < 0.05, CIs exclude zero). End-to-end cold start: 9.13s (Rust) vs. 9.67s
/ 10.74s (Python) — 5.6–15.0% faster, significant against the tuned
Python config.

**Raw throughput: close to tied, with one small but real exception.** S1's
RPS delta was small (~3%) but *did* reach statistical significance (CIs
exclude zero) — Rust processed marginally fewer requests/sec under
cancellation load. S2's peak RPS delta was smaller still (~0.3%) and also
technically significant given the large sample size, but not practically
meaningful. Once the backend-timeout mismatch above was fixed,
`standard_throughput`'s throughput and tail-latency deltas are both
genuinely tied (every CI includes zero) between Rust and Python —
confirming this is a parity result, not a frontend-dependent one, exactly
as expected for a backend-bound workload.

**Tail latency under load (S1/S2): dramatically worse for Rust — the
headline negative result.**

| Scenario | Metric | Python (best) | Rust | Delta |
|---|---|---|---|---|
| S1 (cancellation, 128 agents) | P99 TTFT | 430.8ms | **4687.5ms** | **+988%**, CI [913%, 1063%] |
| S2 (saturation, rate=60) | P99 TTFT | 126.1ms | **1016.2ms** | ~8x |
| S2 (saturation, rate=80) | P99 TTFT | 159.3ms | 373.5ms | ~2.3x |

S1's P99 TTFT delta is enormous and tightly bounded — not noise. In S2, the
degradation is sharpest at exactly one load level (rate=60); Python stays
smooth across the whole ramp.

### Root cause investigation (v1 hypothesis, superseded)

> **Superseded by Phase 8.** The ZMQ explanation below was tested in Phase 8
> and rejected: the tail reproduces on a mock backend, and the submit, writer
> queue and ZMQ send stages all have small tails. The actual cause is a
> per-request tokenizer clone. See [v2](#v2-the-actual-root-cause-phase-8).

Reading `rsg-server`'s request-lifecycle code (`engine.rs`, `dispatch.rs`,
`writer.rs`) found a well-engineered implementation — Drop-based
cancellation, a `biased` `select!` prioritizing aborts, lock-free per-uid
routing, an already-fixed prior bug (a tokenizer-decoder clone that could
starve sibling requests' own polls). No obvious "the Rust code blocked the
runtime" bug surfaced.

What did surface: every submit and every abort for every in-flight request
funnels through **one** dedicated OS thread (`tx-zmq`) and **one** bounded
queue, which batches whatever's pending and sends it over the ZMQ PUSH
socket to the Python scheduler. If the scheduler falls behind draining its
PULL socket under load, the PUSH send blocks — and that block propagates
backward through the bounded queue into every subsequent request's own
`submit().await`.

A standalone micro-benchmark (`crates/zmq-vs-channel-spike/`, not part of
the production harness) tested this directly: the same ZMQ transport,
socket options, and real wire-format frame sizes `rsg-server` actually
uses, against an equivalent in-process bounded channel, under matched
backpressure. Under a cancel-storm load pattern, the two transports failed
in qualitatively different ways — the in-process channel degraded
uniformly and predictably (bounded by the drain rate), while ZMQ stayed
near-instant at p50/p90/p99 but hit **1.48–1.65 second** maximums: rare,
catastrophic stalls rather than uniform slowdown. That "usually instant,
occasionally catastrophic" shape is consistent with — not proof of — the
real benchmark's measured pattern (tied RPS, a P99 blown out by outliers).
Full write-up: `crates/zmq-vs-channel-spike/README.md`.

### Independent corroboration (v1, superseded)

> The SGLang reference is accurate, but it does not explain this project's
> tail. Phase 8 showed this project's ZMQ hop is not on the slow path.

This isn't a theory invented to excuse a bad number. SGLang's own
production Rust-migration effort
([sgl-project/sglang#23206](https://github.com/sgl-project/sglang/issues/23206))
replaced ZMQ with bounded in-process channels, per-core SPSC returns, and
pinned-memory zero-copy tensor handoff — explicitly to keep the Rust↔Python
boundary off the hot path. Their own risk register separately flags
UTF-8-boundary handling under streaming as a known hazard class requiring
dedicated boundary tests — independent confirmation that the second bug
found above is a recognized failure mode in this exact domain, not
something specific to this project's inexperience.

The difference: SGLang's team can patch the Python scheduler they own.
This project's explicit constraint — the vendored backend stays frozen and
unmodified, so the Rust and Python frontends are compared fairly against
the literal same process — rules that fix out. The measured regression is
likely the honest cost of that constraint, not a defect to be optimized
away within it.

## v2: the actual root cause (Phase 8)

Plan: [`docs/plans/phase-8-p99-root-cause.md`](../plans/phase-8-p99-root-cause.md).
Issues #14–#18 hold the commands and raw outputs for each step.

### Feedback loop (#14)

`scripts/p8_ttft_loop.sh` runs a 10 s, 128-agent S1 round on the mock
backend (`rsg-mock-stack`, no GPU). It goes RED when TTFT P99/P50 exceeds
10. Before the fix it was RED in every round (P99/P50 44–56), so the tail
**reproduces without the real scheduler and without CUDA**.

Minimising the scenario showed that cancellations, `max_tokens` and warmup
make no difference. What matters is the first wave: 128 requests arriving
at once. The first wave waited 3–5 s for its first token, and later
requests waited ~75 ms. Spreading the same arrivals over 5 s turned the
loop GREEN.

### Instrumentation (#15)

`rsg-server` now records one Prometheus histogram per stage of the request
path, `rsg_stage_<stage>_seconds`, with fixed buckets:
- `spawn_delay`
- `encode`
- `decoder_queue` and `decoder_build`
- `submit_enqueue`
- `writer_queue` and `zmq_send`
- `backend_wait`

Fixed buckets matter here. The exporter's default summaries only keep a
rolling window, so they hid the first-wave burst.

### Hypothesis verdicts (#16)

| Hypothesis | Verdict | Deciding data |
|---|---|---|
| **H1: per-request tokenizer clone** in `HfCodec::decoder()` | **Confirmed** | `decoder_build` P99 4751 ms tracks TTFT P99, while `decoder_queue` P99 is 4.9 ms. A causal one-variable experiment (share one `Arc<Tokenizer>`) turns the loop GREEN |
| H2: inline encode starves tokio workers | Rejected | `spawn_delay` P99 ≤ 0.67 ms. The encode tail disappears once the clones are gone |
| H3: ZMQ send blocks at the high-water mark (v1) | Rejected | Reproduces on mock. `submit_enqueue` P99 0.10 ms, `writer_queue` ≤ 8.5 ms, `zmq_send` ≤ 1.8 ms |
| H4: HTTP ingress/egress | Rejected | A temporary probe put the whole delay between handler entry and `Accepted`, before submit |

**Why a clone costs seconds.** `tokenizers::Tokenizer::clone()` copies the
~150k-entry vocabulary and merges, which is 15–30 MiB of small allocations.
One clone alone takes ~18 ms. In a throwaway harness:
- 128 clones started at once on a cold heap took **3.6 s P50 / 4.6 s max each**.
- Throughput stayed at ~50–66 clones/s whatever the thread count.
- A warm heap was ~30× faster, so first-touch heap growth (page faults)
  dominates.
- A single malloc arena was slower still, so allocator contention adds to it.

The frontend's RSS grew from 244 MB to 6.3 GB in one 10 s round.

**Why the clone delays the first token.** The decoder must exist before the
request is registered and submitted. Otherwise a fast backend can overflow
the request's fixed-size token channel before anyone reads it. So each
first-wave request waited for its own clone before the scheduler even saw
it. The `spawn_blocking` hop around the build (code review WR-01) did its
job of keeping tokio workers free, but it could not make the clones
themselves cheap.

### Fix (#17)

`Detokenizer` now holds an `Arc<tokenizers::Tokenizer>`. `HfCodec` builds
one shared copy at load, and `decoder()` hands out `Arc::clone`, so each
build is a refcount bump. Each request still owns its own decode state.

Nothing else changed:
- The `spawn_blocking` hop and the build-before-submit ordering are kept.
  Whether to drop the hop is follow-up #21.
- `vendor/` and the wire contract are untouched.

Regression tests are in `crates/rsg-server/src/hf_codec.rs`:
- 1,000 decoder builds must take under 1 s. This test took 24 s before the fix.
- Two decoders that share a tokenizer must keep independent state.

The mock loop is GREEN 5/5 (P99 60–121 ms, frontend RSS ~130 MiB).

### New numbers (#18)

Same GPU box (RTX 3050 8 GB, WSL2) and the Phase 7 methodology: 5 rounds
per arm, Welch's 95% CI, Qwen3-0.6B.
- **After:** all three arms on the fixed build, in
  [`phase8-benchmarks.md`](phase8-benchmarks.md).
- **Before:** the Rust arm only, on the pre-fix `rsg-server` (built from
  `fc491b1`) in the same session, in [`phase8-before-fix.md`](phase8-before-fix.md).

```bash
COMMON=(--backend-kind real --model-arg Qwen/Qwen3-0.6B --port 31031
        --python .venv/bin/python --runs 5 --python-best-num-tokenizer 2)
PY_T="{python} -m rsglang.launch --frontend python --model {model} --port {port} --num-tokenizer {num_tokenizer}"
RS_T="{python} -m rsglang.launch --frontend rust --model {model} --port {port}"
OUT=docs/benchmarks/phase8-benchmarks
target/release/rsg-bench s1 "${COMMON[@]}" --python-cmd "$PY_T" --rust-cmd "$RS_T" --out $OUT/s1_cancel.manifest.json
target/release/rsg-bench s2 --mode open "${COMMON[@]}" --python-cmd "$PY_T" --rust-cmd "$RS_T" --out $OUT/s2_saturation.manifest.json
target/release/rsg-bench s3 "${COMMON[@]}" --python-cmd "$PY_T" --rust-cmd "$RS_T" --out $OUT/s3_coldstart.manifest.json
# Before: s1 and s2 again with --arms rust, "--rust-bin <pre-fix rsg-server>"
# appended to RS_T, and output under docs/benchmarks/phase8-before-fix/.
target/release/rsg-bench report --manifests-dir $OUT --out-json $OUT.json --out-md $OUT.md
```

**S1 (128 agents, cancellations)**

| | Rust before fix | **Rust after fix** | python-default | python-best |
|---|---|---|---|---|
| P50 TTFT | 69.2 ms | **50.9 ms** | 53.7 ms | 53.5 ms |
| P99 TTFT | 4867.7 ms, CI [4492, 5243] | **301.7 ms**, CI [275, 329] | 315.6 ms | 314.4 ms |
| RPS | 13.44 | 13.89 | 13.87 | 13.87 |
| Frontend RSS max | 8.18 GB | **138 MB** | 1.50 GB | 3.04 GB |

How Rust (after) compares with python-best:
- **P99 TTFT** −4.0%, CI [−15.7%, +7.7%]: tied.
- **P50** −4.9%, CI [−6.4%, −3.5%]: significantly lower.
- **RPS** +0.15%, CI includes zero: tied. Phase 7 had Rust at ~3% fewer RPS;
  the clones were also costing throughput.

The pre-fix run reproduces Phase 7's 4687 ms. Python's P99 in this session
(314 ms) is lower than in Phase 7 (431 ms). Rust's 302 ms is below both.

**S2 (open-loop ramp), P99 TTFT in ms**

| Rate | Rust before fix | **Rust after fix** | python-default | python-best |
|---|---|---|---|---|
| 5 | 75.2 | **53.3** | 38.1 | 38.4 |
| 10 | 78.3 | **57.8** | 42.4 | 40.9 |
| 20 | 80.9 | **58.3** | 45.4 | 46.3 |
| 40 | 227.7 | **59.2** | 51.2 | 50.6 |
| 60 | 671.7 | **63.4** | 90.6 | 73.5 |
| 80 | 282.3 | **59.1** | 98.5 | 110.3 |

The rate=60 spike is gone. Rust is now flat across the whole ramp and lowest
at rates 60 and 80. Peak RPS is tied (72.70 vs 72.71). At low rates (5–40),
Rust's P99 is still ~10–15 ms above Python's. That gap is small and was not
investigated in Phase 8.

**S3 (cold start)**

| | python-default | python-best | **Rust** |
|---|---|---|---|
| End-to-end cold start | 8.87 s | 9.28 s | **8.22 s** |
| Frontend RSS | 1.48 GB | 3.03 GB | **123 MB** |

Rust's frontend RSS is down from 490 MB in Phase 7. This most likely comes
from the same fix, since a per-request clone grows the heap even under
light load. It was not separately measured. One python-best S3 trial failed
(hyperfine's conclusion command exited non-zero), so that arm has 4 of 5
samples. Absolute cold-start times differ from Phase 7 for every arm, so
compare within a session.

### What v1 got wrong, and why

v1 read the code, found no obvious blocking bug, and matched the tail's
shape to a ZMQ micro-benchmark. It never measured where the time went in
the real request path. v1 did see the tokenizer clone, but filed it as
"already fixed", because WR-01 had moved it off the async workers. That fix
stopped the clone from starving other requests, but the clone still delayed
its own request. Phase 8 measured each stage first and reproduced the tail
without the backend, which ruled ZMQ out directly.

## Future work

- **Decide whether to drop the `spawn_blocking` hop** around the decoder
  build, now that the build is cheap (#21).
- **In-process, zero-copy IPC.** This would remove the ZMQ hop entirely, the
  way SGLang's own migration did. It is no longer motivated by this
  project's tail latency, since Phase 8 showed the hop is not on the slow
  path. It still requires patching the vendored Python scheduler, which is
  out of scope by design (see `CLAUDE.md`'s "fair comparison" constraint).
- **Rust radix-tree KV cache.** SGLang's own Rust radix-tree integration
  (`sgl-project/sglang#20415`) splits the tree behind a
  `UnifiedTreeCoreInterface`: Rust owns match/insert/evict/LRU, Python
  keeps the KV pool/allocator, and the tree returns a deferred-action list
  for Python to execute. The same upstream-patching constraint that rules
  out in-process IPC rules this out today; it's recorded here as the
  reference pattern to follow if that ever changes.
