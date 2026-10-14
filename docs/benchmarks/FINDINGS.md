# Findings: does a drop-in Rust frontend rewrite actually help?

**TL;DR.** Holding the GPU backend fixed and identical for both frontends:
- **Cold start and host RAM:** a clear, large Rust win. Frontend RSS is
  123 MB against 1.48–3.03 GB for Python, and end-to-end cold start is
  7–11% faster.
- **Tail latency under load:** tied with Python. S1 P99 TTFT is 302 ms
  against 314 ms for tuned Python, and the CI of the difference includes
  zero.
- **Raw throughput:** tied.

Getting the tail to parity took finding one ordinary bug in the Rust
frontend: every request deep-cloned the whole Hugging Face tokenizer before
it could be submitted. See [The P99 tail bug](#the-p99-tail-bug).

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

All numbers come from one GPU box (NVIDIA RTX 3050 8 GB, WSL2) running
Qwen3-0.6B, with 5 rounds per arm and Welch's 95% CI. Three arms run
against the same backend: Rust, python-default (`--num-tokenizer 0`) and
python-best (`--num-tokenizer 2`, the best setting from a sweep).

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

Full tables: [`tokenizer-clone-fix-after.md`](tokenizer-clone-fix-after.md).

**S1 (128 agents, cancellations)**

| | python-default | python-best | **Rust** |
|---|---|---|---|
| P50 TTFT | 53.7 ms | 53.5 ms | **50.9 ms** |
| P99 TTFT | 315.6 ms | 314.4 ms | **301.7 ms**, CI [275, 329] |
| RPS | 13.87 | 13.87 | 13.89 |
| Frontend RSS max | 1.50 GB | 3.04 GB | **138 MB** |

Rust against python-best:
- **P99 TTFT** −4.0%, CI [−15.7%, +7.7%]: tied.
- **P50** −4.9%, CI [−6.4%, −3.5%]: significantly lower.
- **RPS** +0.15%, CI includes zero: tied.

**S2 (open-loop ramp), P99 TTFT in ms**

| Rate | python-default | python-best | **Rust** |
|---|---|---|---|
| 5 | 38.1 | 38.4 | **53.3** |
| 10 | 42.4 | 40.9 | **57.8** |
| 20 | 45.4 | 46.3 | **58.3** |
| 40 | 51.2 | 50.6 | **59.2** |
| 60 | 90.6 | 73.5 | **63.4** |
| 80 | 98.5 | 110.3 | **59.1** |

Rust's P99 is flat across the whole ramp and lowest at rates 60 and 80.
Peak RPS is tied (72.70 vs 72.71). At low rates (5–40), Rust's P99 is
~10–15 ms above Python's and the CIs separate; this is not yet explained
([#23](https://github.com/LiyangTseng/mini-rsglang/issues/23)).

**S3 (cold start)**

| | python-default | python-best | **Rust** |
|---|---|---|---|
| End-to-end cold start | 8.87 s | 9.28 s | **8.22 s** |
| Frontend RSS | 1.48 GB | 3.03 GB | **123 MB** |

Rust's cold start is 7.3% faster than python-default and 11.4% faster than
python-best, and both CIs exclude zero. One python-best trial failed
(hyperfine's conclusion command exited non-zero), so that arm has 4 of 5
samples.

**`standard_throughput`** (backend-bound, measured in the earlier run in
[`frontend-benchmarks.md`](frontend-benchmarks.md)): throughput and tail
latency are tied, with every CI including zero, as expected when the GPU
is the bottleneck.

## The P99 tail bug

Before the fix, Rust's S1 P99 TTFT was ~4.9 s, about 15× Python's. Issues
[#14](https://github.com/LiyangTseng/mini-rsglang/issues/14)–[#18](https://github.com/LiyangTseng/mini-rsglang/issues/18)
hold the commands and raw outputs for each step below.

### Feedback loop

`scripts/ttft_tail_loop.sh` runs a 10 s, 128-agent S1 round on the mock
backend (`rsg-mock-stack`, no GPU). It goes RED when TTFT P99/P50 exceeds
10. Before the fix it was RED in every round (P99/P50 44–56), so the tail
**reproduces without the real scheduler and without CUDA**.

Minimising the scenario showed that cancellations, `max_tokens` and warmup
make no difference. What matters is the first wave: 128 requests arriving
at once. The first wave waited 3–5 s for its first token, and later
requests waited ~75 ms. Spreading the same arrivals over 5 s turned the
loop GREEN.

### Instrumentation

`rsg-server` records one Prometheus histogram per stage of the request
path, `rsg_stage_<stage>_seconds`, with fixed buckets:
- `spawn_delay`
- `encode`
- `decoder_queue` and `decoder_build`
- `submit_enqueue`
- `writer_queue` and `zmq_send`
- `backend_wait`

Fixed buckets matter here. The exporter's default summaries only keep a
rolling window, so they hid the first-wave burst.

### Hypotheses

| Hypothesis | Verdict | Deciding data |
|---|---|---|
| **H1: per-request tokenizer clone** in `HfCodec::decoder()` | **Confirmed** | `decoder_build` P99 4751 ms tracks TTFT P99, while `decoder_queue` P99 is 4.9 ms. A causal one-variable experiment (share one `Arc<Tokenizer>`) turns the loop GREEN |
| H2: inline encode starves tokio workers | Rejected | `spawn_delay` P99 ≤ 0.67 ms. The encode tail disappears once the clones are gone |
| H3: ZMQ send blocks at the high-water mark | Rejected | Reproduces on mock. `submit_enqueue` P99 0.10 ms, `writer_queue` ≤ 8.5 ms, `zmq_send` ≤ 1.8 ms |
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
it. The `spawn_blocking` hop around the build (code review WR-01) kept
tokio workers free, but it could not make the clones themselves cheap.

### Fix

`Detokenizer` holds an `Arc<tokenizers::Tokenizer>`. `HfCodec` builds one
shared copy at load, and `decoder()` hands out `Arc::clone`, so each build
is a refcount bump. Each request still owns its own decode state. The
`spawn_blocking` hop and the build-before-submit ordering are unchanged
(whether to drop the hop is
[#21](https://github.com/LiyangTseng/mini-rsglang/issues/21)), and
`vendor/` and the wire contract are untouched.

Regression tests are in `crates/rsg-server/src/hf_codec.rs`:
- 1,000 decoder builds must take under 1 s. This test took 24 s before the fix.
- Two decoders that share a tokenizer must keep independent state.

The mock loop is GREEN 5/5 (P99 60–121 ms, frontend RSS ~130 MiB).

### Before and after on the GPU

The pre-fix `rsg-server` (built from `fc491b1`) was run in the same session
as the Result numbers above. Full tables:
[`tokenizer-clone-fix-before.md`](tokenizer-clone-fix-before.md).

| Rust | Before fix | After fix |
|---|---|---|
| S1 P99 TTFT | 4867.7 ms, CI [4492, 5243] | **301.7 ms**, CI [275, 329] |
| S1 P50 TTFT | 69.2 ms | **50.9 ms** |
| S1 RPS | 13.44 | 13.89 |
| S1 frontend RSS max | 8.18 GB | **138 MB** |
| S2 P99 TTFT at rate=60 | 671.7 ms | **63.4 ms** |

The clones were also costing about 3% of S1 throughput.

### Lesson

The tail was first blamed on ZMQ backpressure to the frozen scheduler,
from reading the code and matching the tail's shape to a micro-benchmark.
That was wrong, and it was never measured: nobody had checked where the
time went in the real request path. The tokenizer clone had even been
seen, and filed as fixed, because WR-01 had moved it off the async
workers. That stopped the clone from starving other requests, but each
clone still delayed its own request. Measuring every stage, and
reproducing the tail without the backend, found the real cause directly.

## Reproducing

```bash
COMMON=(--backend-kind real --model-arg Qwen/Qwen3-0.6B --port 31031
        --python .venv/bin/python --runs 5 --python-best-num-tokenizer 2)
PY_T="{python} -m rsglang.launch --frontend python --model {model} --port {port} --num-tokenizer {num_tokenizer}"
RS_T="{python} -m rsglang.launch --frontend rust --model {model} --port {port}"
OUT=docs/benchmarks/tokenizer-clone-fix-after
target/release/rsg-bench s1 "${COMMON[@]}" --python-cmd "$PY_T" --rust-cmd "$RS_T" --out $OUT/s1_cancel.manifest.json
target/release/rsg-bench s2 --mode open "${COMMON[@]}" --python-cmd "$PY_T" --rust-cmd "$RS_T" --out $OUT/s2_saturation.manifest.json
target/release/rsg-bench s3 "${COMMON[@]}" --python-cmd "$PY_T" --rust-cmd "$RS_T" --out $OUT/s3_coldstart.manifest.json
# Before: s1 and s2 again with --arms rust, "--rust-bin <pre-fix rsg-server>"
# appended to RS_T, and output under docs/benchmarks/tokenizer-clone-fix-before/.
target/release/rsg-bench report --manifests-dir $OUT --out-json $OUT.json --out-md $OUT.md
```

The directories were renamed after the runs, so each manifest's recorded
`--out` argument still shows the old `phase8-*` path.

## Future work

- **Decide whether to drop the `spawn_blocking` hop** around the decoder
  build, now that the build is cheap
  ([#21](https://github.com/LiyangTseng/mini-rsglang/issues/21)).
- **In-process, zero-copy IPC.** This would remove the ZMQ hop entirely,
  the way SGLang's own migration
  ([sgl-project/sglang#23206](https://github.com/sgl-project/sglang/issues/23206))
  did. It is not a latency fix for this project, since the per-stage
  measurements show the hop is not on the slow path. It also requires
  patching the vendored Python scheduler, which is out of scope by design
  (see `CLAUDE.md`'s "fair comparison" constraint).
- **Rust radix-tree KV cache.** SGLang's own Rust radix-tree integration
  (`sgl-project/sglang#20415`) splits the tree behind a
  `UnifiedTreeCoreInterface`: Rust owns match/insert/evict/LRU, Python
  keeps the KV pool/allocator, and the tree returns a deferred-action list
  for Python to execute. The same upstream-patching constraint that rules
  out in-process IPC rules this out today; it's recorded here as the
  reference pattern to follow if that ever changes.
