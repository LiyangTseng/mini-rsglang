# Phase 8: P99 TTFT Root Cause

Branch: `feat/phase-8-p99-root-cause` · Started 2026-10-10

## Why

Phase 7 ended with a split result. The Rust frontend wins on cold start and
memory, but loses badly on tail latency under load: in the cancellation
scenario (S1), P99 TTFT is 4687 ms vs 431 ms for python-best. FINDINGS.md
attributes this to the single ZMQ socket stalling, **by inference only**.
`rsg_writer_queue_depth` was never correlated with a real run.

A code read suggests otherwise:

- The Rust and Python ZMQ send paths are nearly identical: a plain blocking
  `send` with the default HWM on both sides.
- The Rust frontend does extra per-request CPU work the Python one does not:
  - `HfCodec::decoder()` deep-clones the whole `tokenizers::Tokenizer`
    (~150k vocab) for every request (`crates/rsg-server/src/hf_codec.rs:167-175`,
    `engine.rs:410-433`).
  - `encode` runs inline on a tokio worker (`engine.rs:360`).

Phase 8 measures instead of infers: attribute the tail to a lifecycle stage,
fix it inside the Rust frontend, and re-measure.

## Goal and success target

- **Primary target:** S1 Rust P99 TTFT ≤ python-best (431 ms), measured with
  the Phase 7 methodology (5 runs, Welch's 95% CI).
- Anything short of the target is still reported with real numbers. A
  disproved hypothesis backed by data is a valid outcome.

## Constraints

- **Frozen backend:** `vendor/` is not modified. Every fix lives in the Rust
  frontend.
- The wire contract and output parity stay unchanged, and the existing parity
  tests must pass.
- Keep the decoder-before-submit ordering guarantee documented at
  `engine.rs:410-433`.

## Workflow

The whole phase is driven by the `diagnosing-bugs` skill: build the feedback
loop first, then hypothesise. One agent session works through the issues in
order, on this branch.

| # | Issue | GPU |
|---|---|---|
| 1 | [#14](https://github.com/LiyangTseng/mini-rsglang/issues/14) Tight feedback loop (mock-stack S1-lite, GPU short-S1 fallback) | maybe |
| 2 | [#15](https://github.com/LiyangTseng/mini-rsglang/issues/15) Per-stage latency instrumentation | no |
| 3 | [#16](https://github.com/LiyangTseng/mini-rsglang/issues/16) Root cause: confirm or reject hypotheses | maybe |
| 4 | [#17](https://github.com/LiyangTseng/mini-rsglang/issues/17) Fix and regression test | no |
| 5 | [#18](https://github.com/LiyangTseng/mini-rsglang/issues/18) GPU before/after, FINDINGS v2, README | yes |
| 6 | *(conditional)* If the target is still unmet after #18, open a follow-up `diagnosing-bugs` round | yes |

### Feedback loop (#14)

`scripts/p8_ttft_loop.sh` runs S1-lite (128 agents, 10 s, 25% cancellations,
real Qwen3-0.6B tokenizer) against the release Rust frontend on the mock
stack. No GPU is needed and a round takes about 16 s. Every request gets the
mock's fixed 20 ms prefill delay with no queueing, so any tail above that is
added outside the backend model. The script prints TTFT P50/P99 and the
first-wave vs. rest split, and exits 1 (RED) when P99/P50 > `--max-ratio`
(default 10) in any round. Since #15 it also prints each lifecycle stage's
mean/P50/P99 from the server's `rsg_stage_*_seconds` histograms, scraped at
the end of the round.

    scripts/p8_ttft_loop.sh            # one round
    scripts/p8_ttft_loop.sh --runs 5   # reproduction rate

Calibration (2026-10-11): RED in 5/5 rounds, P99/P50 = 44–54. The same load
with arrivals spread over 5 s (`--think-max-s 5`) is GREEN (P99 106 ms).

### Fix-decision gate (after #16)

Once #16 names the root cause, the agent does **not** start #17 on its own.
It posts the fix options on #17, then waits for the maintainer to choose. Each
option lists:

- what changes
- the expected effect, backed by the #16 data
- the risks (e.g. the decoder-before-submit ordering, parity)
- the effort

Fixes have real trade-offs (share vs. pool, which pool implementation), so
the choice and its reasoning are recorded on #17.

After #18, run `code-review` over the branch before marking the PR ready.

### Checkpoint

At the end of 2026-10-11, summarise what is proven so far: root cause,
partial improvement, or rejected hypotheses. That summary decides what can be
claimed at that point.

The checkpoint is **not a stop**. If the data points to a new high-probability
lead, keep investigating.

## Environment

- **Single machine:** the GPU box, a Windows host with WSL2 and an RTX 3050
  8GB, Qwen3-0.6B only. The clone, toolchains and GPU workloads live in WSL.
  The agent runs on the same machine and may sit on the Windows side, but
  every build, test, git and benchmark command runs inside WSL.
- Machine-specific setup notes (paths, how to invoke commands, environment
  activation) live in an untracked `CLAUDE.local.md`, written by a one-off
  setup pass.
- Before running on the GPU:
  1. Check `nvidia-smi`.
  2. Use a non-default port (31000+).
  3. Do not build or run tests while a benchmark is running. Build contention
     on the same CPU distorts tail latency.
- After the run, kill every process you started.

## Documentation policy

- `docs/benchmarks/FINDINGS.md` keeps the Phase 7 root-cause analysis,
  labelled as the **v1 hypothesis**. Phase 8 adds a **v2** section covering the
  instrumentation, the verdict on each hypothesis, the fix, and the new
  numbers.
- `README.md` Results is updated from the new run and links to FINDINGS for
  the v1 → v2 story.

## Git

- Commits 1–2 on this branch are tooling only: retire GSD, then configure the
  agent skills. Commit 1 stays revertable on its own.
- Every later commit references its issue (`#14` … `#18`).
- The draft PR is opened early. It is merged with **a merge commit, not a
  squash**, so individual commits stay revertable.

## Next: Phase 9 (preview, not yet designed)

The planned next phase is the first step of moving the backend into Rust: a
Rust radix-tree prefix cache exposed to the Python scheduler via PyO3. It
would sit behind an opt-in flag (e.g. `--cache rust-radix`), so the
frozen-backend A/B comparison stays intact.

The design will be settled in a separate planning pass: build tooling
(maturin vs setuptools-rust), which interface to match in
`kvcache/radix_cache.py`, and the shared-prefix TTFT benchmark. Phase 9 starts
only after Phase 8, sequentially, because both need the single GPU box.
