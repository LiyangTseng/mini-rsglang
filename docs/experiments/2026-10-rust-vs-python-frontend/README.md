# Does a drop-in Rust frontend beat the Python frontend on the same backend?

**Status:** superseded for S1, S2 and S3 by
[`2026-10-p99-ttft-tokenizer-clone/runs/2026-10-12-after-fix`](../2026-10-p99-ttft-tokenizer-clone/runs/2026-10-12-after-fix/report.md).
Its `standard_throughput` result is still current.

**Conclusion:** At the time, Rust won clearly on cold start and host RAM,
tied on throughput, and lost badly on tail latency (S1 P99 TTFT 4687 ms
against 431 ms). The tail turned out to be a Rust frontend bug, a
per-request tokenizer clone, so this run's Rust latency and memory numbers
are not representative. The current numbers are in
[docs/RESULTS.md](../../RESULTS.md).

## Question

Holding the GPU backend fixed and identical, does rewriting only the
frontend in Rust reduce host overhead in the three host-overhead-bound
scenarios (S1 cancellations, S2 saturation, S3 cold start), and does it
cost anything on a backend-bound workload (`standard_throughput`)?

## Hypotheses and verdicts

| Hypothesis | Verdict | Deciding data |
|---|---|---|
| Rust reduces frontend RAM and cold start | Confirmed | S3: frontend RSS 490 MB vs 1.48–3.03 GB; end-to-end ready 9.13 s vs 9.67–10.74 s |
| Rust improves tail latency under load | Rejected in this run | S1 P99 TTFT 4687.5 ms vs 430.8 ms (+988%, CI [913%, 1063%]) |
| The tail comes from ZMQ backpressure to the frozen scheduler | Rejected later | The tail reproduces on the mock backend; per-stage measurements put it in the tokenizer clone. See [the p99 experiment](../2026-10-p99-ttft-tokenizer-clone/README.md) |
| Throughput is backend-bound, so tied | Confirmed | `standard_throughput`: every CI includes zero |

## Setup

- **Git SHA:** `eac5910` (`meta.git_dirty` is true in every manifest).
- **Hardware:** NVIDIA GeForce RTX 3050 8 GB, WSL2.
- **Model:** Qwen/Qwen3-0.6B.
- **Arms:** Rust, python-default (`--num-tokenizer 0`) and python-best,
  chosen by a `--num-tokenizer` sweep (0, 1, 2, 4). 5 rounds per arm,
  Welch's 95% CI.
- **Commands:**

```bash
scripts/gpu_phase7_bench.sh   # Linux + CUDA GPU required
```

The script runs preflight, the `--num-tokenizer` sweep, S1, S2 (open loop),
S3, `standard_throughput`, the combined report and `check_upstream.py`. This
run was written under an older layout and moved here later, so each
manifest's recorded `session.harness_argv` still shows the old `--out`
path, and the report was renamed to `report.json` / `report.md`.

## Runs

| Run | What it measures | Notes |
|---|---|---|
| [`runs/2026-10-09-gpu`](runs/2026-10-09-gpu/report.md) | `--num-tokenizer` sweep, S1, S2, S3, `standard_throughput` | S1, S2 and S3 superseded by the p99 experiment's after-fix run; `standard_throughput` still cited by [docs/RESULTS.md](../../RESULTS.md) |

## Results

| Scenario | Metric | python-best | Rust |
|---|---|---|---|
| S1 (cancellation, 128 agents) | P99 TTFT | 430.8 ms | 4687.5 ms |
| S2 (saturation, rate=60) | P99 TTFT | 126.1 ms | 1016.2 ms |
| S3 | Frontend RSS | 3.03 GB | 490 MB |
| S3 | End-to-end cold start | 10.74 s | 9.13 s |

S1 RPS was ~3% lower for Rust (CI excludes zero); S2 peak RPS differed by
~0.3%. `standard_throughput` throughput and tail latency were tied.

## Links

- Successor: [Why is the Rust frontend's P99 TTFT ~15× Python's under load?](../2026-10-p99-ttft-tokenizer-clone/README.md)
- Requirements BENCH-02..BENCH-08 in `.planning/REQUIREMENTS.md`.
