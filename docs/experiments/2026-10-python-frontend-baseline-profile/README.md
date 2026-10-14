# Where does the Python frontend spend its time and memory?

**Status:** concluded

**Conclusion:** Measured before any Rust code ran, this profile set the
metrics the frontend comparison captures: per-process GC pauses alongside
P99 TTFT, the tokenizer's GIL-held share across `--num-tokenizer` settings,
and process-tree PSS at ready separately from after serving. It also
separated frontend costs a Rust rewrite can change from shared backend
costs it cannot.

## Question

For the frozen Python frontend on the real backend, which processes and
code paths carry the time and memory in each of the three
host-overhead-bound scenarios, and how does GC line up with tail latency?
The answer decides what the Rust-vs-Python comparison must measure.

## Hypotheses and verdicts

This is a measurement, not a hypothesis test. The findings that shaped the
comparison:

| Finding | Deciding data |
|---|---|
| GC overlap must be reported per process, not pooled | S1: frontend GC overlaps spikes and non-spikes alike (100% / 96.4%); scheduler GC is spike-specific (100% / 5.7%) |
| The shared tokenizer worker is far from GIL saturation on this workload | S2: 27.20% GIL-held, 29.96% CPU-active |
| Report PSS at ready and after serving separately | S3: tree PSS at ready 3118.5–3122.9 MB, 4260–4655 MB after serving |

## Setup

- **Git SHA:** `d4272b3` (`meta.git_dirty` is false).
- **Hardware:** NVIDIA GeForce RTX 3050 8 GB, WSL2.
- **Model:** Qwen/Qwen3-0.6B.
- **Arms:** the Python frontend only, `--num-tokenizer 0`, with py-spy
  (100 Hz), a `gc.callbacks` hook and `tracemalloc` active. Absolute
  latencies are higher than an uninstrumented run; this run is for
  attribution, not headline speed.
- **Commands:**

```bash
scripts/gpu_phase2_profile.sh   # Linux + CUDA GPU required
```

The run's own report lists the full reproduce steps. This run was written
under an older layout and moved here later, so its report still names the
old path of its JSON sidecar. The sidecar records `meta.created_utc`
2026-10-06T03:17Z; the run directory's date is its commit date.

## Runs

| Run | What it measures | Notes |
|---|---|---|
| [`runs/2026-10-05-gpu`](runs/2026-10-05-gpu/baseline-profile.md) | S1, S2, S3 on the Python frontend, instrumented | `baseline-profile.json` is the sidecar; the `.md` is written from it and checked by `python/tests/test_baseline_profile_report.py` |

## Results

See the run's [report](runs/2026-10-05-gpu/baseline-profile.md), in
particular "Inputs to Phase 7 benchmark design" and "Known blind spots".

## Links

- Requirement BENCH-01 in `.planning/REQUIREMENTS.md`.
- Used by: [Does a drop-in Rust frontend beat the Python frontend on the same backend?](../2026-10-rust-vs-python-frontend/README.md)
