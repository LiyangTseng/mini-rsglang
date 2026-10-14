# Results: does a drop-in Rust frontend rewrite actually help?

This is the only page with the current numbers. Every number cites the run
it comes from. How each was reached, including the investigations, is in
[docs/experiments/](experiments/README.md).

**TL;DR.** Holding the GPU backend fixed and identical for both frontends:
- **Cold start and host RAM:** a clear, large Rust win. Frontend RSS is
  123 MB against 1.48–3.03 GB for Python, and end-to-end cold start is
  7–11% faster.
- **Tail latency under load:** tied with Python. S1 P99 TTFT is 302 ms
  against 314 ms for tuned Python, and the CI of the difference includes
  zero.
- **Raw throughput:** tied.
- **Output:** identical. All 128 parity prompts match byte for byte on both
  models.

Getting the tail to parity took finding one ordinary bug in the Rust
frontend: every request deep-cloned the whole Hugging Face tokenizer before
it could be submitted. See
[Why is the Rust frontend's P99 TTFT ~15× Python's under load?](experiments/2026-10-p99-ttft-tokenizer-clone/README.md)

## Runs cited

| Run | Used for |
|---|---|
| [`2026-10-p99-ttft-tokenizer-clone/runs/2026-10-12-after-fix`](experiments/2026-10-p99-ttft-tokenizer-clone/runs/2026-10-12-after-fix/report.md) | S1, S2, S3 |
| [`2026-10-rust-vs-python-frontend/runs/2026-10-09-gpu`](experiments/2026-10-rust-vs-python-frontend/runs/2026-10-09-gpu/report.md) | `standard_throughput` |
| [`2026-10-rust-python-output-parity/runs/2026-10-07-gpu`](experiments/2026-10-rust-python-output-parity/runs/2026-10-07-gpu/parity-report.md) | Output parity |

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
python-best (`--num-tokenizer 2`, the best setting from a sweep). Arms that
are compared ran in the same session.

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

**S1 (128 agents, cancellations)** — run `2026-10-12-after-fix`

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

**S2 (open-loop ramp), P99 TTFT in ms** — run `2026-10-12-after-fix`

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

**S3 (cold start)** — run `2026-10-12-after-fix`

| | python-default | python-best | **Rust** |
|---|---|---|---|
| End-to-end cold start | 8.87 s | 9.28 s | **8.22 s** |
| Frontend RSS | 1.48 GB | 3.03 GB | **123 MB** |

Rust's cold start is 7.3% faster than python-default and 11.4% faster than
python-best, and both CIs exclude zero. One python-best trial failed
(hyperfine's conclusion command exited non-zero), so that arm has 4 of 5
samples.

**`standard_throughput`** (backend-bound) — run `2026-10-09-gpu`:
throughput and tail latency are tied, with every CI including zero, as
expected when the GPU is the bottleneck. This run predates the tokenizer
fix and was not repeated.

**Output parity** — run `2026-10-07-gpu`: greedy, one request at a time,
128/128 prompts byte-identical in token ids and text for both Qwen3-0.6B
and Llama-3.2-1B-Instruct.

## Reproducing

Every run goes into a new directory; see
[How to add an experiment](experiments/README.md#how-to-add-an-experiment).
For the S1–S3 comparison:

```bash
RUN=docs/experiments/<YYYY-MM>-<question-slug>/runs/<YYYY-MM-DD>-<label>
COMMON=(--backend-kind real --model-arg Qwen/Qwen3-0.6B --port 31031
        --python .venv/bin/python --runs 5 --python-best-num-tokenizer 2)
PY_T="{python} -m rsglang.launch --frontend python --model {model} --port {port} --num-tokenizer {num_tokenizer}"
RS_T="{python} -m rsglang.launch --frontend rust --model {model} --port {port}"
target/release/rsg-bench s1 "${COMMON[@]}" --python-cmd "$PY_T" --rust-cmd "$RS_T" --out $RUN/s1_cancel.manifest.json
target/release/rsg-bench s2 --mode open "${COMMON[@]}" --python-cmd "$PY_T" --rust-cmd "$RS_T" --out $RUN/s2_saturation.manifest.json
target/release/rsg-bench s3 "${COMMON[@]}" --python-cmd "$PY_T" --rust-cmd "$RS_T" --out $RUN/s3_coldstart.manifest.json
target/release/rsg-bench report --manifests-dir $RUN --out-json $RUN/report.json --out-md $RUN/report.md
scripts/plot_benchmarks.py $RUN/report.json docs/experiments/<YYYY-MM>-<question-slug>/plots
```

`scripts/gpu_phase7_bench.sh` runs the full sweep and every scenario into
a dated run directory by default.

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
