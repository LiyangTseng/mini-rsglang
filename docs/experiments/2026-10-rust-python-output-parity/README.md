# Does the Rust frontend produce the same output as the Python frontend?

**Status:** concluded

**Conclusion:** Yes. With greedy sampling, one request at a time, all 128
prompts produced byte-identical token ids and text on both frontends, for
both Qwen3-0.6B (the hard gate) and Llama-3.2-1B-Instruct. Every endpoint
answers on both. The cancellation stress run reproduced an upstream
abort-during-prefill crash under `--abort-timing immediate`, so the
project-wide default became `deferred`.

## Question

Given the same backend and the same prompts, does serving through the Rust
frontend produce output identical to the Python frontend? This is the
project's core correctness claim.

## Hypotheses and verdicts

| Criterion | Verdict | Deciding data |
|---|---|---|
| 1. Every endpoint works through the real backend | Pass | 8/8 Rust and 5/5 Python endpoints `ok` |
| 2. Greedy parity, one request at a time (PAR-01, hard gate) | Pass | Qwen3-0.6B 128/128; Llama-3.2-1B-Instruct 128/128 (reported, not gated) |
| 3. Match rate under 128-way concurrency (PAR-02, informational) | Same range as each frontend's self-agreement | Rust vs Python 42/128; Python vs its sequential 37/128; Rust vs its sequential 30/128 |
| 4. Cancellation stress | Upstream crash under `immediate`, none under `deferred` | Watcher verdict `unhealthy` under `immediate`; `failure_mode: none` under `deferred` |

An earlier version of the report blamed a 127/128 result on GPU
nondeterminism. The real cause was a bug in the parity harness, fixed in
`ae8feec` before this run; the run's report has the full account.

## Setup

- **Git SHA:** `ae8feec` (`meta.git_dirty` is false).
- **Hardware:** NVIDIA GeForce RTX 3050 8 GB, WSL2.
- **Models:** Qwen/Qwen3-0.6B (gate), meta-llama/Llama-3.2-1B-Instruct.
- **Arms:** Rust and Python frontends on the same backend, corpus
  `fixtures/parity/corpus.json`.
- **Commands:**

```bash
bash scripts/gpu_phase6_parity.sh   # Linux + CUDA GPU required
```

This run was written under an older layout and moved here later, so its
report still names the old path of its JSON sidecar.

## Runs

| Run | What it measures | Notes |
|---|---|---|
| [`runs/2026-10-07-gpu`](runs/2026-10-07-gpu/parity-report.md) | Criteria 1–4 | `parity-report.json` is the sidecar; the `.md` is written from it and checked by `python/tests/test_parity_report.py` |

## Results

See the run's [report](runs/2026-10-07-gpu/parity-report.md), including the
"Abort-timing decision (D-09)" section.

## Links

- Requirements PAR-01 and PAR-02 in `.planning/REQUIREMENTS.md`.
- Upstream issue record: [`UPSTREAM.md`](../../../UPSTREAM.md), "Known upstream issues".
