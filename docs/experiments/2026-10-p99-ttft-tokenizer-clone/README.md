# Why is the Rust frontend's P99 TTFT ~15× Python's under load?

**Status:** concluded

**Conclusion:** Every request deep-cloned the whole Hugging Face tokenizer
before it could be submitted, and a burst of 128 concurrent clones took
seconds each. Sharing one `Arc<Tokenizer>` brought S1 P99 TTFT from 4868 ms
to 302 ms, tied with Python (314 ms).

## Question

The [2026-10-09 frontend comparison](../2026-10-rust-vs-python-frontend/README.md)
measured Rust's S1 P99 TTFT at 4687 ms against 431 ms for python-best. Where
does that time go, and can it be fixed inside the Rust frontend without
touching the frozen backend?

## Hypotheses and verdicts

| Hypothesis | Verdict | Deciding data |
|---|---|---|
| **H1: per-request tokenizer clone** in `HfCodec::decoder()` | **Confirmed** | `decoder_build` P99 4751 ms tracks TTFT P99, while `decoder_queue` P99 is 4.9 ms. A causal one-variable experiment (share one `Arc<Tokenizer>`) turns the loop GREEN |
| H2: inline encode starves tokio workers | Rejected | `spawn_delay` P99 ≤ 0.67 ms. The encode tail disappears once the clones are gone |
| H3: ZMQ send blocks at the high-water mark | Rejected | Reproduces on mock. `submit_enqueue` P99 0.10 ms, `writer_queue` ≤ 8.5 ms, `zmq_send` ≤ 1.8 ms |
| H4: HTTP ingress/egress | Rejected | A temporary probe put the whole delay between handler entry and `Accepted`, before submit |

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

### Why a clone costs seconds

`tokenizers::Tokenizer::clone()` copies the ~150k-entry vocabulary and
merges, which is 15–30 MiB of small allocations. One clone alone takes
~18 ms. In a throwaway harness:
- 128 clones started at once on a cold heap took **3.6 s P50 / 4.6 s max each**.
- Throughput stayed at ~50–66 clones/s whatever the thread count.
- A warm heap was ~30× faster, so first-touch heap growth (page faults)
  dominates.
- A single malloc arena was slower still, so allocator contention adds to it.

The frontend's RSS grew from 244 MB to 6.3 GB in one 10 s round.

### Why the clone delays the first token

The decoder must exist before the request is registered and submitted.
Otherwise a fast backend can overflow the request's fixed-size token
channel before anyone reads it. So each first-wave request waited for its
own clone before the scheduler even saw it. The `spawn_blocking` hop around
the build (code review WR-01) kept tokio workers free, but it could not
make the clones themselves cheap.

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

## Setup

- **Git SHA:** both runs were driven from `d8c7103` (recorded in every
  manifest). The after-fix arms use the `rsg-server` built from that
  commit. The before-fix Rust arm uses an `rsg-server` built from
  `fc491b1`, the last commit before the fix, passed with `--rust-bin`.
- **Hardware:** NVIDIA GeForce RTX 3050 8 GB, WSL2.
- **Model:** Qwen/Qwen3-0.6B.
- **Arms:** after-fix runs Rust, python-default (`--num-tokenizer 0`) and
  python-best (`--num-tokenizer 2`). before-fix runs the Rust arm only, in
  the same session, so the before/after comparison stays within one
  session. 5 rounds per arm, Welch's 95% CI.
- **Commands:**

```bash
COMMON=(--backend-kind real --model-arg Qwen/Qwen3-0.6B --port 31031
        --python .venv/bin/python --runs 5 --python-best-num-tokenizer 2)
PY_T="{python} -m rsglang.launch --frontend python --model {model} --port {port} --num-tokenizer {num_tokenizer}"
RS_T="{python} -m rsglang.launch --frontend rust --model {model} --port {port}"
OUT=docs/experiments/2026-10-p99-ttft-tokenizer-clone/runs/2026-10-12-after-fix
target/release/rsg-bench s1 "${COMMON[@]}" --python-cmd "$PY_T" --rust-cmd "$RS_T" --out $OUT/s1_cancel.manifest.json
target/release/rsg-bench s2 --mode open "${COMMON[@]}" --python-cmd "$PY_T" --rust-cmd "$RS_T" --out $OUT/s2_saturation.manifest.json
target/release/rsg-bench s3 "${COMMON[@]}" --python-cmd "$PY_T" --rust-cmd "$RS_T" --out $OUT/s3_coldstart.manifest.json
target/release/rsg-bench report --manifests-dir $OUT --out-json $OUT/report.json --out-md $OUT/report.md
# before-fix: s1 and s2 again with --arms rust, "--rust-bin <pre-fix rsg-server>"
# appended to RS_T, and OUT=.../runs/2026-10-12-before-fix.
```

The commands above use this directory's paths. The runs were first written
under an older layout and moved here later, so each manifest's recorded
argv (`session.harness_argv`), the exact command as run, still shows the old
`--out` path. The plots in [`plots/`](plots/) are drawn from the after-fix run with
`scripts/plot_benchmarks.py runs/2026-10-12-after-fix/report.json plots`.

## Runs

| Run | What it measures | Notes |
|---|---|---|
| [`runs/2026-10-12-after-fix`](runs/2026-10-12-after-fix/report.md) | S1, S2, S3; Rust, python-default, python-best | Cited by [docs/RESULTS.md](../../RESULTS.md) |
| [`runs/2026-10-12-before-fix`](runs/2026-10-12-before-fix/report.md) | S1, S2; Rust on the pre-fix build | |

## Results

| Rust | before-fix | after-fix |
|---|---|---|
| S1 P99 TTFT | 4867.7 ms, CI [4492, 5243] | **301.7 ms**, CI [275, 329] |
| S1 P50 TTFT | 69.2 ms | **50.9 ms** |
| S1 RPS | 13.44 | 13.89 |
| S1 frontend RSS max | 8.18 GB | **138 MB** |
| S2 P99 TTFT at rate=60 | 671.7 ms | **63.4 ms** |

The clones were also costing about 3% of S1 throughput. Against python-best
in the after-fix run, Rust's S1 P99 is −4.0%, CI [−15.7%, +7.7%]: tied. The
full comparison is in [docs/RESULTS.md](../../RESULTS.md).

### Lesson

The tail was first blamed on ZMQ backpressure to the frozen scheduler,
from reading the code and matching the tail's shape to a micro-benchmark.
That was wrong, and it was never measured: nobody had checked where the
time went in the real request path. The tokenizer clone had even been
seen, and filed as fixed, because WR-01 had moved it off the async
workers. That stopped the clone from starving other requests, but each
clone still delayed its own request. Measuring every stage, and
reproducing the tail without the backend, found the real cause directly.

## Links

- Plan: [`docs/plans/p99-ttft-root-cause.md`](../../plans/p99-ttft-root-cause.md)
- Issues: [#14](https://github.com/LiyangTseng/mini-rsglang/issues/14)
  (feedback loop), [#15](https://github.com/LiyangTseng/mini-rsglang/issues/15)
  (instrumentation), [#16](https://github.com/LiyangTseng/mini-rsglang/issues/16)
  (verdicts), [#17](https://github.com/LiyangTseng/mini-rsglang/issues/17)
  (fix), [#18](https://github.com/LiyangTseng/mini-rsglang/issues/18) (GPU
  before/after). Each holds its commands and raw outputs.
- Follow-ups: [#21](https://github.com/LiyangTseng/mini-rsglang/issues/21),
  [#22](https://github.com/LiyangTseng/mini-rsglang/issues/22),
  [#23](https://github.com/LiyangTseng/mini-rsglang/issues/23).
- PR: [#19](https://github.com/LiyangTseng/mini-rsglang/pull/19).
