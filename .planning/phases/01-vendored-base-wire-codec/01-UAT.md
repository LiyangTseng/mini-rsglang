---
status: testing
phase: 01-vendored-base-wire-codec
source: [01-VERIFICATION.md]
started: 2026-10-04T04:55:00Z
updated: 2026-10-04T04:55:00Z
---

## Current Test

number: 1
name: Run `bash scripts/gpu_phase1_check.sh` on the Linux GPU box
expected: |
  ALL PASS: step 1 release build, step 2 python-mode chat completion, step 3 real handshake values
  (upstream_sha=9a91cfa..., max_running_req=256, num_pages>1, max_seq_len in 1..40960, page_size 1 or 64,
  eos_token_id=151645 for Qwen3-0.6B), step 4 no orphan after kill -9 of the launcher, step 5 check_upstream.py.
  Setup first: `uv venv --python=3.12 && uv pip install -e vendor/mini-sglang && uv pip install torch-c-dlpack-ext && uv pip install -e .` (build-essential present).
awaiting: user response

## Tests

### 1. Run `bash scripts/gpu_phase1_check.sh` on the Linux GPU box
expected: ALL PASS on steps 1-5 (covers the GPU halves of ROADMAP criteria 2 and 3; WINDOWS.md entry 1)
result: [pending]

### 2. Decide the disposition of code-review finding CR-01 (group SIGINT / Ctrl-C makes the rust-mode launcher exit 1 with a failure report)
expected: Either fix now (re-check stop_requested right after each ready_queue.get and before scanning children, plus an e2e test that SIGINTs the launcher's process group and asserts exit 0), or mark it deferred in 01-REVIEW-DISPOSITION.md with a target phase
result: [pending]

### 3. Decide the disposition of WR-02 (parent watchdog records getppid() only after the scheduler child has booted)
expected: Either pass the launcher pid explicitly (plus PR_SET_PDEATHSIG on Linux), or accept/defer it in 01-REVIEW-DISPOSITION.md
result: [pending]

### 4. Review the judgment-tier prohibition from 01-03: rust mode runs the byte-identical upstream Scheduler and the handshake is not produced by patching vendored code
expected: Agree with the verifier's non-authoritative verdict that it holds (see the Prohibitions table in 01-VERIFICATION.md)
result: [pending]

### 5. Confirm the 01-01 process truth: no Python package was installed before you approved the PyPI names and pins
expected: Confirmed. The session record shows the 01-01 package gate was approved ("approve (use ... uv ...)") before the lock was installed into the project .venv
result: [pending]

## Summary

total: 5
passed: 0
issues: 0
pending: 5
skipped: 0
blocked: 0

## Gaps
