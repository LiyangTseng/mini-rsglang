---
status: testing
phase: 07-frontend-benchmarks
source: [07-VERIFICATION.md]
started: 2026-10-07T20:35:00.000Z
updated: 2026-10-07T20:35:00.000Z
---

## Current Test

number: 1
name: Run `scripts/gpu_phase7_bench.sh` on the Linux CUDA GPU box (after Phase 6 lands) to produce the real measured Python-vs-Rust comparison and write `docs/benchmarks/frontend-benchmarks.{json,md}`.
expected: |
  The report shows no `NOT A FRONTEND COMPARISON` banner (backend_kind: real, GPU present), with S1/S2/S3/throughput numbers, GC/memory tables, and the BENCH-06 regression/no-regression sentence with a Welch 95% CI.
awaiting: user response

## Tests

### 1. Run `scripts/gpu_phase7_bench.sh` on the Linux CUDA GPU box
expected: The report shows no `NOT A FRONTEND COMPARISON` banner (backend_kind: real, GPU present), with S1/S2/S3/throughput numbers, GC/memory tables, and the BENCH-06 regression/no-regression sentence with a Welch 95% CI.
result: [pending — requires a Linux CUDA machine with Phase 6 landed; cannot run on this Mac dev machine]

## Summary

total: 1
passed: 0
issues: 0
pending: 1
skipped: 0
blocked: 0

## Gaps
