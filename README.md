# mini-rsglang

mini-sglang with its Python frontend swapped for an optimized Rust frontend —
built on top of [`sgl-project/mini-sglang`](https://github.com/sgl-project/mini-sglang)
(MIT), pinned at `9a91cfa`.

The Python/CUDA backend (scheduler, engine, KV cache, kernels) is vendored in
unmodified and shared by both frontends. The frontend processes — API server,
tokenizer, detokenizer — are rewritten in Rust: concurrent ingress, a
request-lifecycle FSM, Hugging Face tokenization/detokenization, and the same
ZMQ + MessagePack wire format upstream's own Python frontend speaks to the
scheduler. One repo, one backend, two frontends you can launch with a single
flag and A/B against each other on identical hardware.

```
Rust:    HTTP/SSE ingress → lifecycle FSM → tokenize/detokenize  ─┐
                                                                   ├─► ZMQ/msgpack ─► Python scheduler + GPU kernels (unmodified)
Python:  HTTP ingress → tokenizer/detokenizer managers           ─┘
```

This is a learning-and-proof project: it exists to answer, with real
measurements rather than assumption, *how much does rewriting only the
frontend in Rust actually buy you*, while holding the GPU backend fixed and
identical for both.

## Why

Rewrite-the-frontend-in-Rust is a real, live architectural bet — SGLang's own
team is doing it in production
([sgl-project/sglang#23206](https://github.com/sgl-project/sglang/issues/23206)).
This project reproduces the same bet at a scale one person can build,
benchmark, and fully understand end to end: a byte-compatible Rust
reimplementation of the frontend, a rigorous differential-correctness suite
(same input → same tokens, same streamed output, byte-for-byte), and a
benchmark harness built for exactly three host-overhead-bound scenarios where
a frontend rewrite should matter most — concurrent load with cancellations,
short-prompt RPS saturation, and cold start / host RAM.

**The result is not "Rust wins everything."** See below for the numbers, or
[`docs/benchmarks/FINDINGS.md`](docs/benchmarks/FINDINGS.md) for the full
write-up.

## Results

The GPU backend is completely unmodified and identical for both frontends.
- **Cold start and memory:** Rust wins decisively.
- **Tail latency under load:** tied with Python.
- **Raw throughput:** tied.

Full methodology, statistical significance (Welch's 95% CI), and the history
of how these numbers were reached: **[docs/benchmarks/FINDINGS.md](docs/benchmarks/FINDINGS.md)**.
Full tables: [`phase8-benchmarks.md`](docs/benchmarks/phase8-benchmarks.md).

Three frontends were benchmarked against the *same* backend process:

- **Rust** — this project's frontend.
- **python-default** — Python's out-of-the-box setting (`--num-tokenizer=0`:
  one process does both tokenize and detokenize).
- **python-best** — Python tuned by sweeping `--num-tokenizer` for its
  highest peak RPS (`--num-tokenizer=2`: separate tokenizer processes).
  Included so Rust is measured against Python's *best* case, not just its
  defaults.

### Cold start & memory — Rust wins

<img src="docs/benchmarks/plots/s3_coldstart_memory.svg" alt="Bar charts comparing end-to-end cold start time and frontend memory across Python (default), Python (tuned), and Rust — Rust is lowest on both" width="100%">

| | python-default | python-best | **Rust** |
|---|---|---|---|
| End-to-end cold start | 8.87s | 9.28s | **8.22s** |
| Frontend memory (RSS) | 1.48 GB | 3.03 GB | **123 MB** |

### Tail latency under load — tied

P50 is the typical case. **P99 is the worst 1% of requests**, the users most
likely to notice something is wrong.

**128 concurrent agents, random requests with mid-stream cancellations:**

| | python-default | python-best | **Rust** |
|---|---|---|---|
| P50 time-to-first-token | 53.7 ms | 53.5 ms | **50.9 ms** |
| **P99 time-to-first-token** | 315.6 ms | 314.4 ms | **301.7 ms** |

Rust vs python-best on P99: −4.0%, CI [−15.7%, +7.7%], so statistically tied.

**Steadily increasing request rate, until the server saturates:**

<img src="docs/benchmarks/plots/s2_saturation_p99.svg" alt="P99 time-to-first-token per request rate; Rust stays flat around 53 to 63ms across the ramp, slightly above Python at low rates and below it at 60 and 80 requests/sec" width="100%">

Rust's P99 stays flat across the ramp. It is ~10–15 ms above Python at low
rates and below Python at 60 and 80 req/s. Peak throughput is statistically
tied, which is expected when the GPU backend, not the frontend, is the
bottleneck.

### Future work

- **Decide whether to drop the `spawn_blocking` hop** around the per-request
  decoder build, which is now cheap
  ([#21](https://github.com/LiyangTseng/mini-rsglang/issues/21)).
- **In-process, zero-copy IPC.** This would remove the ZMQ hop entirely.
  Per-stage measurements show the hop is not on the slow path, so this would
  not be a latency fix. It would also require patching the vendored backend,
  so it remains a deliberate scope decision.
- **Rust radix-tree KV cache** — follow SGLang's own
  `UnifiedTreeCoreInterface` pattern
  ([sgl-project/sglang#20415](https://github.com/sgl-project/sglang/issues/20415))
  if the backend-frozen constraint is ever revisited.

## Running it

```bash
# Python frontend (upstream's own launcher, unchanged)
python -m rsglang.launch --frontend python --model Qwen/Qwen3-0.6B --port 1919

# Rust frontend (same backend, same wire protocol, same port)
python -m rsglang.launch --frontend rust --model Qwen/Qwen3-0.6B --port 1919
```

Both speak the same OpenAI-compatible HTTP API on the same port. Requires a
CUDA GPU for real inference; a mock backend (`crates/rsg-bench/src/bin/rsg-mock-stack.rs`)
lets the Rust side be developed and tested without one — the whole repo is
developed on a Mac and only verified end-to-end on Linux/CUDA.

## Reproducing the benchmarks

```bash
scripts/gpu_phase7_bench.sh   # Linux + CUDA GPU required
```

Runs preflight checks, a `--num-tokenizer` sweep, then all three scenarios for
both frontends, and writes `docs/benchmarks/frontend-benchmarks.{json,md}`.
The numbers above come from the same `rsg-bench` scenarios, run with the
commands in [FINDINGS.md § New numbers](docs/benchmarks/FINDINGS.md#new-numbers-18).
`scripts/plot_benchmarks.py REPORT_JSON docs/benchmarks/plots` redraws the
charts above from a report. See
[`docs/benchmarks/FINDINGS.md`](docs/benchmarks/FINDINGS.md) for the
write-up.

## Repository layout

| Path | What |
|---|---|
| `vendor/mini-sglang/` | Upstream, unmodified, pinned at `vendor/UPSTREAM_SHA` |
| `python/rsglang/launch.py` | The single `--frontend python\|rust` entry point |
| `crates/rsg-server/` | The Rust frontend: HTTP/SSE, lifecycle FSM, ZMQ transport |
| `crates/rsg-tokenizer/` | Hugging Face tokenizer/detokenizer, chat templates |
| `crates/rsg-wire/` | msgpack wire types, byte-for-byte matched to upstream |
| `crates/rsg-bench/` | The benchmark harness (this project's other real deliverable) |
| `docs/benchmarks/` | Every measured result, baseline through final findings |
| `.planning/` | Read-only archive of the Phase 1–7 plans, verification reports, and decisions |
| `docs/plans/` | Plans for work after Phase 7 |

## License

The vendored upstream (`vendor/mini-sglang/`) keeps its own MIT license and
copyright notice — see `vendor/mini-sglang/LICENSE`. This project's own code
(everything outside `vendor/`) is not yet under a declared license.
