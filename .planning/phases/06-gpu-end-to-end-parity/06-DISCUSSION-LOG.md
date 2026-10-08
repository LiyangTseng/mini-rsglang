# Phase 6: GPU End-to-End Parity - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-10-06
**Phase:** 06-gpu-end-to-end-parity
**Areas discussed:** Parity corpus & models, Diff/mismatch protocol, Abort-during-prefill bug response, Concurrent-load & stress scope

---

## Parity corpus & models

| Option | Description | Selected |
|--------|-------------|----------|
| Curated, category-covering set | Hand-assemble ~100 prompts spanning short/long, multi-turn chat, code, CJK/emoji; reuse Phase 4's tokenizer corpus | ✓ |
| Sampled from a public chat dataset | Random sample from ShareGPT-style corpus | |
| Fixed generic prompt set | Uniform short Q&A prompts | |

**User's choice:** Curated, category-covering set.

| Option | Description | Selected |
|--------|-------------|----------|
| Llama-3.2-1B/3B-Instruct | Smallest gated checkpoints, fast iteration | ✓ |
| Llama-3.1-8B-Instruct | More "real-sized", slower | |
| Not decided — flag as open blocker | Defer, note gated-repo access blocker | |

**User's choice:** Llama-3.2-1B/3B-Instruct family, then narrowed:

| Option | Description | Selected |
|--------|-------------|----------|
| 1B-Instruct | Fastest iteration, lowest GPU cost | ✓ |
| 3B-Instruct | Closer to real deployment size | |
| Whichever Phase 4 already picked | Defer to Phase 4's checkpoint choice | |

**User's choice:** 1B-Instruct.

| Option | Description | Selected |
|--------|-------------|----------|
| Same prompt set for both models | One corpus, apples-to-apples comparison | ✓ |
| Separate sets per model | Llama gets its own BOS/space-cleanup-focused set | |

**User's choice:** Same prompt set for both models.

**Notes:** Llama size choice was explicitly about minimizing GPU iteration cost during debugging, not representativeness.

---

## Diff / mismatch protocol

| Option | Description | Selected |
|--------|-------------|----------|
| Zero tolerance — any mismatch fails | Greedy+temp0+single-request should be fully deterministic | ✓ |
| Investigate before failing | Categorize mismatches (bug vs. FP non-associativity) before failing | |
| Small tolerance threshold | Allow up to N/100 mismatches | |

**User's choice:** Zero tolerance — any mismatch fails.

| Option | Description | Selected |
|--------|-------------|----------|
| Bisect to find the divergence point | Trace first diverging token, attribute to Phase 4/5/backend | ✓ |
| Report and stop — treat as a phase blocker | Hand off to follow-up session without inline debugging | |

**User's choice:** Bisect to find the divergence point.

| Option | Description | Selected |
|--------|-------------|----------|
| Token ids first, text as secondary check | Isolates backend determinism vs. detokenization bugs | ✓ |
| Text only | Simpler but conflates backend vs. detokenization issues | |

**User's choice:** Token ids first, text as secondary check.

| Option | Description | Selected |
|--------|-------------|----------|
| Same pattern: docs/benchmarks/parity-report.{md,json} | Follows Phase 2's baseline-profile.{md,json} precedent | ✓ |
| Something else | — | |

**User's choice:** Same pattern — `docs/benchmarks/parity-report.{md,json}`.

---

## Abort-during-prefill bug response

**Initial question rejected by user** — the user asked for clarification instead of selecting an option: "你建議怎麼評估這個問題?" (What do you recommend for evaluating this problem?).

**Claude's recommendation (given in response):** Don't pre-commit to fix-vs-document before the stress test runs. First, use criterion 4's real-backend stress test to determine (a) whether the bug reproduces and under what trigger conditions, and (b) its failure mode (full scheduler crash vs. isolated corrupted request). Second, triage the response by the *scope* of the fix rather than severity alone: a small, localized fix (e.g. an ordering/refcount bug) is worth attempting as a shared backend fix per PROJECT.md's allowed-shared-fixes rule; a deep/structural CUDA-memory issue gets documented and routed around via the `--abort-timing deferred` default, since deep CUDA debugging is disproportionate to this project's scope as a learning/proof project.

**User's response:** 同意 (Agreed).

**Notes:** This reframed the original three-option question (document-only / attempt-fix / report-only-decide-later) into a two-step evaluation process (reproduce-first, then triage-by-fix-scope) rather than picking one of the three up front. Captured as D-08/D-09 in CONTEXT.md.

---

## Concurrent-load & stress scope

| Option | Description | Selected |
|--------|-------------|----------|
| One fixed concurrency level, small sample | Minimal GPU time, single informational measurement | ✓ |
| A few concurrency levels (curve) | More GPU time, richer signal for Phase 7 | |

**User's choice:** One fixed concurrency level, small sample.

| Option | Description | Selected |
|--------|-------------|----------|
| Reuse Phase 5's tool as-is | Swap mock-scheduler for real backend, no other changes | ✓ |
| Reuse it, but add real-backend-specific instrumentation | Add crash/health capture inside the tool | |

**User's choice:** Reuse Phase 5's tool as-is.

| Option | Description | Selected |
|--------|-------------|----------|
| External process-health watch (reuse GPU script convention) | Thin wrapper following scripts/gpu_phase1_check.sh's ps/nvidia-smi pattern | ✓ |
| Just read backend logs/exit code after the run | Manual post-hoc check, less rigorous | |

**User's choice:** External process-health watch (reuse GPU script convention).

**Notes:** This resolves the apparent tension between "reuse the stress tool unmodified" and criterion 4's requirement to record whether the bug reproduced — detection lives in a separate script layer, not in the stress tool itself.

---

## Claude's Discretion

- Exact per-category breakdown of the curated 100-prompt corpus
- Exact fixed concurrency level and sample size for the single-point concurrent-load measurement
- The precise fix-scope threshold for "small/localized vs. deep/structural" — decided case-by-case once an actual bug is read
- Internal script/module layout for the parity-report generator and the process-health watcher

## Deferred Ideas

None — discussion stayed within phase scope.
