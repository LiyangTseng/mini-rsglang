---
phase: 01-vendored-base-wire-codec
verified: 2026-10-05T09:30:00Z
status: passed
score: 105/105 must-haves verified
covered_files:
  - .planning/WINDOWS.md
  - .planning/phases/01-vendored-base-wire-codec/01-01-PLAN.md
  - .planning/phases/01-vendored-base-wire-codec/01-01-SUMMARY.md
  - .planning/phases/01-vendored-base-wire-codec/01-02-PLAN.md
  - .planning/phases/01-vendored-base-wire-codec/01-02-SUMMARY.md
  - .planning/phases/01-vendored-base-wire-codec/01-03-PLAN.md
  - .planning/phases/01-vendored-base-wire-codec/01-03-SUMMARY.md
  - .planning/phases/01-vendored-base-wire-codec/01-04-PLAN.md
  - .planning/phases/01-vendored-base-wire-codec/01-04-SUMMARY.md
  - .planning/phases/01-vendored-base-wire-codec/01-05-PLAN.md
  - .planning/phases/01-vendored-base-wire-codec/01-05-SUMMARY.md
  - .planning/phases/01-vendored-base-wire-codec/01-06-PLAN.md
  - .planning/phases/01-vendored-base-wire-codec/01-06-SUMMARY.md
  - .planning/phases/01-vendored-base-wire-codec/01-07-PLAN.md
  - .planning/phases/01-vendored-base-wire-codec/01-07-SUMMARY.md
  - .planning/phases/01-vendored-base-wire-codec/01-08-PLAN.md
  - .planning/phases/01-vendored-base-wire-codec/01-08-SUMMARY.md
  - .planning/phases/01-vendored-base-wire-codec/01-09-PLAN.md
  - .planning/phases/01-vendored-base-wire-codec/01-09-SUMMARY.md
  - .planning/phases/01-vendored-base-wire-codec/01-10-PLAN.md
  - .planning/phases/01-vendored-base-wire-codec/01-10-SUMMARY.md
  - .planning/phases/01-vendored-base-wire-codec/01-11-PLAN.md
  - .planning/phases/01-vendored-base-wire-codec/01-11-SUMMARY.md
  - .planning/phases/01-vendored-base-wire-codec/01-12-PLAN.md
  - .planning/phases/01-vendored-base-wire-codec/01-12-SUMMARY.md
  - .planning/phases/01-vendored-base-wire-codec/01-13-PLAN.md
  - .planning/phases/01-vendored-base-wire-codec/01-13-SUMMARY.md
  - .planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md
  - .planning/phases/01-vendored-base-wire-codec/01-REVIEW.md
  - .planning/phases/01-vendored-base-wire-codec/01-SECURITY.md
  - .planning/phases/01-vendored-base-wire-codec/01-UAT.md
  - .planning/phases/01-vendored-base-wire-codec/01-VALIDATION.md
  - .planning/quick/261004-vqo-fix-cr-01-critical-finding-2026-10-05-in/261004-vqo-PLAN.md
  - .planning/quick/261004-vqo-fix-cr-01-critical-finding-2026-10-05-in/261004-vqo-SUMMARY.md
  - Cargo.toml
  - UPSTREAM.md
  - crates/rsg-server/Cargo.toml
  - crates/rsg-server/src/handshake.rs
  - crates/rsg-server/src/main.rs
  - crates/rsg-server/src/transport.rs
  - crates/rsg-server/tests/cli.rs
  - crates/rsg-wire/Cargo.toml
  - crates/rsg-wire/src/lib.rs
  - crates/rsg-wire/tests/common/mod.rs
  - crates/rsg-wire/tests/dump.rs
  - crates/rsg-wire/tests/fixtures.rs
  - fixtures/wire/manifest.json
  - pyproject.toml
  - python/rsglang/__init__.py
  - python/rsglang/backend.py
  - python/rsglang/handshake.py
  - python/rsglang/launch.py
  - python/rsglang/sockets.py
  - python/rsglang/testing/__init__.py
  - python/rsglang/testing/fake_scheduler.py
  - python/tests/test_check_upstream.py
  - python/tests/test_gpu_check_script.py
  - python/tests/test_handshake.py
  - python/tests/test_launch_args.py
  - python/tests/test_launch_rust_e2e.py
  - python/tests/test_parent_watchdog.py
  - python/tests/test_topology.py
  - python/tests/test_wire_decode.py
  - requirements-mac.in
  - requirements-mac.txt
  - rust-toolchain.toml
  - scripts/bootstrap_mac_env.sh
  - scripts/check_all.sh
  - scripts/check_upstream.py
  - scripts/check_wire_decode.sh
  - scripts/gen_wire_fixtures.py
  - scripts/gpu_phase1_check.sh
  - vendor/UPSTREAM_SHA
covered_digest: "v2:sha256:d57191231bf525605a558dbd5352f46b748b05d1961b29c81575f2f3ea59d16a"
behavior_unverified: 0
overrides_applied: 4
overrides:
  - must_have: "ROADMAP SC2 (GPU half): on the GPU machine --frontend python serves a chat completion through the unmodified Python frontend, and --frontend rust starts the same real backend plus rsg-server"
    reason: "No Linux+CUDA GPU box is available on this Mac dev machine. The Mac half (e2e tests against a FakeScheduler on upstream's real ZMQ queues, 14 gpu-check-script tests against stubs) passes. The user has arranged for a GitHub collaborator to run scripts/gpu_phase1_check.sh on their own Linux+GPU machine; this is tracked as an open hardware-handoff window (WINDOWS.md #1), not a code gap. External hardware constraint makes the must-have literally impossible to run on this machine."
    accepted_by: "LiyangTseng"
    accepted_at: "2026-10-05T09:00:00Z"
  - must_have: "ROADMAP SC3 (GPU half): the real backend reports max_seq_len, eos_token_id, page_size, max_running_req at readiness and rsg-server logs them; the Python frontend keeps working against the same backend code"
    reason: "Same hardware constraint as SC2's GPU half. extract_handshake's attribute names are checked against upstream source (prior verification pass) and rsg-server logs all four values on the Mac against FakeScheduler constants; the real CUDA-engine values can only be observed on the Linux GPU box the collaborator will run. Tracked via WINDOWS.md #1."
    accepted_by: "LiyangTseng"
    accepted_at: "2026-10-05T09:00:00Z"
  - must_have: "scripts/gpu_phase1_check.sh automates the GPU-only checks for ROADMAP criteria 2 and 3 and is signed off by a human at the end of the phase"
    reason: "The human sign-off itself needs the Linux+GPU box (01-UAT.md test 1, blocked_by: physical-device). Deferred to the GitHub collaborator per the user's explicit 2026-10-05 decision; tracked as WINDOWS.md #1, not a code gap on this Mac."
    accepted_by: "LiyangTseng"
    accepted_at: "2026-10-05T09:00:00Z"
  - must_have: "On Linux, when prctl(PR_SET_PDEATHSIG, SIGKILL) succeeds, PR_GET_PDEATHSIG actually reports SIGKILL (the success path of 01-08/01-10's watchdog, not just the degrade-and-log failure path)"
    reason: "test_linux_arms_pdeathsig_sigkill is guarded by sys.platform.startswith('linux') and is skipped on this macOS machine by design (01-UAT.md test 6, blocked_by: physical-device). The failure/degrade branch (01-10, broadened by 01-13 to AttributeError) and the GPU-script check for it (01-12) are both proven on the Mac. The success branch needs any Linux machine; deferred to the collaborator per the same 2026-10-05 decision, tracked via WINDOWS.md #1."
    accepted_by: "LiyangTseng"
    accepted_at: "2026-10-05T09:00:00Z"
re_verification:
  previous_status: human_needed
  previous_score: 95/99
  gaps_closed:
    - "CR-01 (critical, highest-priority human-verification item in the prior pass): _run_rust_mode had no top-level exception/finally guard around the spawn loop or the handshake write. Fixed via quick task 261004-vqo: an except BaseException guard now prints the cause, unlinks sockets, flushes streams and SIGKILLs the process group before re-raising. Independently re-verified in this pass: direct code read confirms the guard; test_unexpected_error_leaves_no_orphans[spawn_loop] and [handshake_encode] both run in isolation and pass."
    - "WR-01 (new instance, deque race) triage: resolved via 01-UAT.md test 11. The user explicitly passed without fixing or deferring; disposition recorded as open (an acknowledged, accepted low-probability risk, not a silently-dropped finding)."
    - "IN-01 (new instance, handshake-key duplication) and IN-02 (new instance, dead/racy post-SIGKILL code) triage: resolved via 01-UAT.md test 12. Both accepted as non-blocking and recorded as deferred in 01-REVIEW-DISPOSITION.md (open count 13 -> 11)."
    - "01-UAT.md stale-record item: refreshed. Tests 8 and 9 now carry explicit resolution notes citing 01-13 and this verification pass; tests 10-12 record the new 2026-10-05 review findings' triage; the summary block reads 10 passed, 0 issues, 0 pending, 2 blocked."
    - "GPU/Linux hardware human-verification items: per the user's explicit 2026-10-05 decision, these are no longer treated as blocking. They are documented as overrides above (hardware constraint external to the codebase) and tracked via WINDOWS.md #1 as a collaborator handoff, not a verification gap."
  gaps_remaining: []
  regressions: []
---

# Phase 1: Vendored Base & Wire Codec Verification Report

**Phase Goal:** The repo holds a pinned, attributed copy of mini-sglang. One launch command runs the shared backend with either frontend, and the backend reports a readiness handshake. The Rust msgpack codec is byte-exact with upstream for all 7 message types.
**Verified:** 2026-10-05T09:30:00Z
**Status:** passed
**Re-verification:** Yes. This pass re-verifies after (a) the quick task that fixed CR-01, (b) UAT re-triage of WR-01/IN-01/IN-02 (new instances), and (c) the user's explicit decision to treat the two hardware-blocked UAT items as a tracked, non-blocking collaborator handoff. Supersedes the prior `01-VERIFICATION.md` dated 2026-10-04T23:00:00Z (status `human_needed`, score 95/99).

## Goal Achievement

I did not trust the SUMMARY/UAT narration. I independently re-ran the full Mac gate, read `python/rsglang/launch.py` in full, and ran the new CR-01 regression test in isolation (not just as part of the full suite) to confirm it is a real behavioral proof.

- `bash scripts/check_all.sh --offline` (this run, full output captured): exit 0. Cargo workspace: 10+1+6 (rsg-wire) + 10+12 (rsg-server CLI) = all pass. Pytest: **90 passed, 37 skipped** (up from 88 in the prior pass — the 2 new cases are `test_unexpected_error_leaves_no_orphans[spawn_loop]` and `[handshake_encode]`). Fixture freshness OK (34 cases). WIRE-02 decode: 37 passed. `check_upstream.py --offline`: tree `02d3e4ad…` still matches pristine `9a91cfa`, 0 listed modifications. `git status --porcelain vendor/` empty.
- `python/rsglang/launch.py` (direct full read, this run): the `try:` block now opens on the line directly after `rust = subprocess.Popen(...)` (line 158) and covers everything through the end of the supervise loop (line 300), including the nested `children`/`report_errors`/`shutdown` defs, the ready-wait loop, and the handshake write. An `except BaseException:` (line 301) logs "unexpected error in the launcher; escalating to SIGKILL for the process group", writes `traceback.format_exc()` to stderr, unlinks run sockets, flushes stdout/stderr (all inside an inner `try`), and an unconditional `finally: os.killpg(os.getpgrp(), signal.SIGKILL)` — mirroring `shutdown()`'s own escalation tail exactly — followed by a bare `raise`. This is precisely the fix the prior pass's #1 priority human-verification item asked for.
- Ran the new regression test in isolation: `.venv/bin/python -m pytest python/tests/test_launch_rust_e2e.py -k unexpected_error -v` → **2 passed** (`[spawn_loop]`, `[handshake_encode]`). Each case asserts the launcher exits non-zero within 30 s, a `rsglang.launch:` line contains "unexpected error in the launcher", `set(run.pids()) == {"rsg-server", "scheduler"}`, and both pids are gone within 15 s. This is a state-transition/cleanup invariant (an unanticipated exception must still kill every already-spawned process) — exactly the kind of truth that requires a passing behavioral test, not just symbol presence, and this test provides it.
- `git diff -w --numstat de6ba69^..c39851f -- python/rsglang/launch.py` (this run): `23  0` — confirmed additions-only, matching the SUMMARY's claim and the plan's acceptance criteria. No anticipated path was touched.
- `grep -n -E "TBD|FIXME|XXX|TODO|HACK|PLACEHOLDER" python/rsglang/launch.py python/tests/test_launch_rust_e2e.py .planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md` (this run): no matches. No new debt markers.
- Read `.planning/phases/01-vendored-base-wire-codec/01-UAT.md` end to end (this run): 12 tests, summary block `passed: 10, issues: 0, pending: 0, blocked: 2`. Tests 10-12 (new) record: CR-01 fixed via the quick task (test 10), WR-01 (new instance) explicitly passed-without-decision and left `open` (test 11), IN-01/IN-02 (new instances) accepted as non-blocking and recorded `deferred` (test 12). Tests 8 and 9 (previously stale, flagged by the prior verification pass) now carry resolution notes citing this re-verification. The 2 `blocked` tests (1: GPU box, 6: Linux-only watchdog unit test) are both `blocked_by: physical-device` with the human's own stated reason ("手上沒有linux gpu...").
- Read `.planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md` end to end (this run): frontmatter `open: 11, total: 24`. I counted the table rows myself: 11 `open` (WR-01 + IN-04..IN-13's 10 info rows, minus the 4 now-fixed/deferred — recounted below), 7 `fixed` (WR-06/07/08/09/02/04, CR-01), 6 `deferred` (IN-01/02/03, WR-03/05/10) = 24. Matches. The ID-reuse notice at the top still accurately documents the 4 dropped prior decisions; nothing was silently lost.
- WINDOWS.md (this run): `open_count: 1`, entry 1 names `scripts/gpu_phase1_check.sh`, "GPU end-of-phase check... needs the Linux GPU box; human sign-off pending" — the project's own tracked ledger for exactly this hardware constraint, consistent with the explicit user decision for this verification pass (see Overrides below).

**Explicit user decision applied in this pass (see `overrides:` in frontmatter):** the two UAT items blocked on physical hardware (the Linux+CUDA GPU check, and the Linux-only `test_linux_arms_pdeathsig_sigkill` unit test) are treated as a legitimate, already-tracked (WINDOWS.md #1) hardware handoff to a GitHub collaborator — not a reason to withhold a `passed` verdict given everything verifiable on this Mac genuinely passes. I applied this as four explicit overrides (the two ROADMAP SC halves, the GPU-script human-sign-off truth, and the Linux PDEATHSIG-success truth) rather than silently calling the phase "passed" without accounting for them. They remain visible — see "Hardware Handoff" below — so nobody loses track of the pending collaborator verification; they just no longer gate this phase's status.

### Observable Truths: ROADMAP Success Criteria (the contract)

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| SC1 | Repo contains mini-sglang @ 9a91cfa with MIT LICENSE + copyright; UPSTREAM.md names the commit and lists every modified vendored file | ✓ VERIFIED | `check_upstream.py --offline` (this run): tree `02d3e4ad…` matches pristine 9a91cfa, 0 listed modifications. `git status --porcelain vendor/` empty. Unchanged since prior pass. |
| SC2 | On GPU, `--frontend python` serves a chat completion via the unmodified frontend; `--frontend rust` starts the same backend plus the Rust skeleton | ✓ PASSED (override) | Mac half verified (e2e tests pass, 14 gpu-check-script tests pass against stubs). GPU half: Override — hardware handoff to collaborator, tracked WINDOWS.md #1. See frontmatter overrides. |
| SC3 | Backend reports max_seq_len, eos_token_id, page_size, max_running_req at readiness; Rust logs them; Python frontend unchanged against same backend code | ✓ PASSED (override) | `extract_handshake` attribute names checked against upstream source; rsg-server logs all values on the Mac (FakeScheduler constants). GPU half: Override — same hardware handoff. |
| SC4 | For each of the 7 upstream message types, Rust codec bytes equal golden fixtures from upstream's Python encoder, checked on the Mac | ✓ VERIFIED | `cargo test --workspace` (this run): rsg-wire 10 lib + 1 dump + 6 fixture tests pass. `gen_wire_fixtures.py --check` passed (34 cases, byte-identical). Unchanged since prior pass. |
| SC5 | Every message the Rust codec emits decodes through upstream's real Python decoder (cls(**kwargs)) | ✓ VERIFIED | `check_wire_decode.sh` (this run): Rust dump test passed, then 37 pytest tests passed. Unchanged since prior pass. |

### Observable Truths: PLAN must_haves

Plans 01-01 through 01-12 are unchanged in source since the prior verification pass (confirmed: `git log` shows only `python/rsglang/launch.py`, `python/tests/test_launch_rust_e2e.py`, and `.planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md` changed since then, all via the quick task). I carried forward their per-truth statuses with a regression check: the full gate run above (90 passed vs 88 before, no regressions), independently re-run by me.

| Plan | Truths | Status | Evidence |
|------|--------|--------|----------|
| 01-01 (vendoring, env) | 8 | ✓ 8 VERIFIED | Unchanged; tree hash, LICENSE, UPSTREAM.md all intact. |
| 01-02 (rsg-server) | 7 | ✓ 7 VERIFIED | `cargo test --workspace` green (this run). |
| 01-03 (launcher, e2e) | 7 | ✓ 7 VERIFIED | e2e and topology tests pass in this run's pytest (90 passed). |
| 01-04 (codec, fixtures) | 9 | ✓ 9 VERIFIED | fixtures suite; `--check` fresh (this run). |
| 01-05 (failure contract, GPU script) | 8 | ✓ 7 VERIFIED, ✓ 1 PASSED (override) | 7 behavioral truths pass; the human-sign-off truth is overridden (hardware handoff, see frontmatter). |
| 01-06 (check_upstream, WIRE-02) | 11 | ✓ 11 VERIFIED | check_upstream and decode tests pass (this run). |
| 01-07 (CR-01 original, G-01-2) | 6 | ✓ 6 VERIFIED | Group-SIGINT tests still pass; unaffected by the quick task. |
| 01-08 (WR-02, G-01-3) | 7 | ✓ 6 VERIFIED, ✓ 1 PASSED (override) | Portable early-kill/watchdog-pid truths verified. Linux-only "PDEATHSIG armed" success-path truth is overridden (hardware handoff). |
| 01-09 (WR-07, WR-08 gap closure) | 8 | ✓ 8 VERIFIED | Unaffected; re-confirmed in this run's 90-pass gate. |
| 01-10 (WR-06, WR-09 gap closure) | 8 | ✓ 8 VERIFIED | Unaffected. |
| 01-11 (WR-04, WR-01-original gap closure) | 6 | ✓ 6 VERIFIED | Unaffected directly; the WR-01 id it recorded `fixed` is superseded by a newer, unrelated finding under the same id — not a regression of 01-11's own work (see ID-reuse notice). |
| 01-12 (WR-06-CHECK gap closure) | 4 | ✓ 4 VERIFIED | Unaffected. |
| 01-13 (G-01-8, G-01-9 gap closure) | 6 | ✓ 6 VERIFIED | The one "superseded-not-failed" truth (ledger id reuse) is counted verified, consistent with the prior pass's treatment — the underlying fix is intact and proven by passing tests. |
| 261004-vqo (quick task: CR-01 fix) | 6 | ✓ 6 VERIFIED | See below — the critical finding flagged by the prior verification pass is now closed with behavioral proof. |

**261004-vqo truths (quick task, CR-01 fix)**

| Truth | Status | Evidence |
|-------|--------|----------|
| With --tp-size 2, rank 1's unanticipated start() OSError (after rank 0 is up) makes the launcher exit non-zero within 30 s with rsg-server and scheduler rank 0 both gone (D-12) | ✓ VERIFIED | Ran `pytest test_launch_rust_e2e.py -k unexpected_error -v` myself: `[spawn_loop]` passed. |
| An unanticipated ValueError from encode_handshake_line after rank 0 is ready makes the launcher exit non-zero within 30 s with rsg-server and the scheduler both gone (D-12) | ✓ VERIFIED | Same run: `[handshake_encode]` passed. |
| Before the group SIGKILL, the launcher prints "unexpected error in the launcher" and the exception's traceback | ✓ VERIFIED | Confirmed by direct code read (launch.py:301-321) and the passing tests' own assertion on that exact string. |
| Both regression cases fail on the unmodified launch.py (the launcher hangs in multiprocessing's exit-time join of rank 0, children alive) | ✓ VERIFIED | SUMMARY records the RED output (30-36 s hangs, both children alive, "launcher still running" failure) before the fix commit; the fix commit (`c39851f`) immediately follows the RED test commit (`de6ba69`) in git history, consistent with a real TDD cycle. |
| Anticipated paths unchanged: every existing test in test_launch_rust_e2e.py and the whole python/tests suite still pass; launch.py's whitespace-insensitive diff is additions only | ✓ VERIFIED | Full gate (90 passed, 37 skipped, this run); `git diff -w --numstat de6ba69^..c39851f -- python/rsglang/launch.py` → `23  0` (this run, confirmed). |
| 01-REVIEW-DISPOSITION.md records CR-01 as fixed; open count drops from 14 to 13 at that point in history | ✓ VERIFIED | `git show f3122be -- .planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md` shows exactly the claimed 3-line change; current file (after the later UAT re-triage) shows CR-01 `fixed` and `open: 11` (further reduced by the subsequent WR-01/IN-01/IN-02 triage, consistent with the ledger's documented process). |

**Score:** 105/105 verified (5 roadmap + 94 plan truths carried forward/re-confirmed + 6 from the quick task). 4 of the carried-forward truths (SC2, SC3, 01-05's human-sign-off truth, 01-08's Linux PDEATHSIG-success truth) are counted via explicit override (hardware handoff to a collaborator — see frontmatter `overrides:`), not via a passing behavioral test on this Mac. 0 behavior-unverified. 0 uncertain.

**Interpretation note (SC4, "7 message types"):** unchanged from prior verifications — read as the 6 scheduler-boundary messages plus `SamplingParams`, with `Tensor` also covered.

### Prohibitions

| Plan | Prohibition | Tier | Disposition |
|------|-------------|------|-------------|
| 01-01 | Vendored LICENSE / copyright never removed or altered | test | ✓ VERIFIED. `LICENSE_MISSING` check and its test still pass. |
| 01-06 | Tier A frozen frontend never modified | test | ✓ VERIFIED. `check_upstream.py --offline` clean (0 modifications), this run. |
| 01-03 | Rust mode runs the byte-identical upstream Scheduler; the handshake is not produced by patching vendored code | judgment | Resolved by a human: 01-UAT.md test 4 recorded `pass`. Unaffected by the quick task (which touched only the launcher/test Python and a disposition file). |

### Required Artifacts

| Artifact | Status | Details |
|----------|--------|---------|
| vendor/mini-sglang/ (+LICENSE), UPSTREAM.md, vendor/UPSTREAM_SHA | ✓ VERIFIED | pristine tree, SHA single-sourced (this run) |
| Cargo.toml, rust-toolchain.toml, crates/rsg-server/*, crates/rsg-wire/* | ✓ VERIFIED | cargo gate green (29 tests across workspace, this run) |
| fixtures/wire/*.msgpack + manifest.json | ✓ VERIFIED | 34 cases, fresh (this run) |
| python/rsglang/launch.py | ✓ VERIFIED | Full read (this run): CR-01 guard present exactly as described — `try:` after the `Popen`, `except BaseException:` with log/traceback/unlink/flush then `finally: killpg SIGKILL`, then `raise`. Substantive and wired. |
| python/tests/test_launch_rust_e2e.py | ✓ VERIFIED | `test_unexpected_error_leaves_no_orphans[spawn_loop]` and `[handshake_encode]` present and passing in isolation (confirmed this run) |
| python/rsglang/backend.py | ✓ VERIFIED | `except (OSError, AttributeError)` confirmed (unchanged since 01-13; carried forward) |
| crates/rsg-server/src/handshake.rs | ✓ VERIFIED | `deserialize_with = "Option::deserialize"` on eos_token_id, confirmed (unchanged) |
| scripts/gpu_phase1_check.sh | ✓ VERIFIED (exists, substantive, Mac-tested) | not executed end-to-end on GPU (no GPU); tracked as a collaborator handoff (override, WINDOWS.md #1) |
| .planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md | ✓ VERIFIED (exists, substantive) | `open: 11, total: 24`, counted by hand and matches: 7 `fixed` (incl. CR-01), 6 `deferred` (incl. IN-01/IN-02 new instances), 11 `open` (incl. WR-01 new instance, explicitly left open by user choice) |
| .planning/phases/01-vendored-base-wire-codec/01-UAT.md | ✓ VERIFIED (exists, substantive) | 12 tests, 10 passed / 0 issues / 0 pending / 2 blocked (physical-device); tests 10-12 record the new 2026-10-05 findings' triage |

### Key Link Verification

| From | To | Via | Status |
|------|----|-----|--------|
| python/rsglang/launch.py `_run_rust_mode` `except BaseException` guard | every process in the launcher's own process group | `os.killpg(os.getpgrp(), signal.SIGKILL)`, opened only after `os.setpgid(0, 0)` and the rsg-server `Popen`, so the group is always the launcher's own (confirmed by direct read and the passing awk-ordering check from the plan) | ✓ WIRED |
| python/tests/test_launch_rust_e2e.py sitecustomize hooks | the launcher subprocess only | PYTHONPATH points at a tmp_path hook dir; each hook is gated on `"rsglang.launch" in sys.orig_argv` | ✓ WIRED (confirmed by reading the test source) |
| python/rsglang/launch.py `run_rust_mode` | upstream `parse_args` / `run_shell` | `server_args, run_shell = parse_args(rest)` then `if run_shell: return 2`, before `resolve_rust_bin` | ✓ WIRED (unchanged since 01-13) |

### Data-Flow Trace (Level 4)

| Artifact | Data | Source | Real data | Status |
|----------|------|--------|-----------|--------|
| rsg-server "handshake received" log | max_seq_len, eos, page_size, max_running_req, num_pages, sha | stdin ← launcher ← ready_queue ← `extract_handshake(scheduler, …)` | Mac: FakeScheduler constants; GPU: real engine attributes | ✓ FLOWING on Mac; GPU half overridden (hardware handoff) |
| Golden fixtures | msgpack bytes | vendored upstream `serialize_type` | yes (`--check` regenerates, this run) | ✓ FLOWING |
| launcher stderr on unanticipated exception | "unexpected error in the launcher..." + traceback | `_write_stderr(traceback.format_exc())` inside the new guard, called before the SIGKILL | yes (captured by the passing regression test's assertion) | ✓ FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Phase gate, single full run | `bash scripts/check_all.sh --offline` | exit 0. Cargo green (29 tests). Pytest: 90 passed, 37 skipped (+2 vs prior pass). Fixtures fresh. Decode: 37 passed. check_upstream: OK | ✓ PASS |
| CR-01 regression test, isolated | `.venv/bin/python -m pytest python/tests/test_launch_rust_e2e.py -k unexpected_error -v` | 2 passed (`spawn_loop`, `handshake_encode`) | ✓ PASS |
| Additions-only diff check | `git diff -w --numstat de6ba69^..c39851f -- python/rsglang/launch.py` | `23  0` | ✓ PASS |
| Debt-marker scan | `grep -n -E "TBD\|FIXME\|XXX\|TODO\|HACK\|PLACEHOLDER"` on launch.py, test_launch_rust_e2e.py, 01-REVIEW-DISPOSITION.md | no matches | ✓ PASS |
| Restoration check | `git status --porcelain vendor/` | clean | ✓ PASS |
| GPU / Linux checks | `scripts/gpu_phase1_check.sh`, PDEATHSIG success-path unit test | not run (macOS, no CUDA) | Overridden — hardware handoff (see frontmatter) |

### Probe Execution

No `scripts/*/tests/probe-*.sh` exists and no plan declares one. Step 7c: N/A.

### Requirements Coverage

| Requirement | Source Plan(s) | Description | Status | Evidence |
|-------------|----------------|-------------|--------|----------|
| BASE-01 | 01-01, 01-06 | vendored @ 9a91cfa with LICENSE; UPSTREAM.md records commit and modified files | ✓ SATISFIED | SC1 |
| BASE-02 | 01-02, 01-03, 01-05, 01-07, 01-08, 01-09, 01-10, 01-11, 01-12, 01-13, 261004-vqo | one launch command for `--frontend python` / `--frontend rust`; shell-mode rejection, watchdog hardening, and now the CR-01 process-leak guard | ✓ SATISFIED on Mac; GPU hardware half overridden (collaborator handoff) | launcher, tests, 10 closed gaps (incl. CR-01 via the quick task); GPU steps 2-3, 4, 4b tracked via WINDOWS.md #1 |
| BASE-03 | 01-02, 01-03, 01-05, 01-09, 01-10, 01-11, 01-12 | backend readiness handshake (4 values, strictly requiring eos_token_id); both frontends use same backend code | ✓ SATISFIED on Mac; GPU half overridden | e2e handshake test; GPU step 3 tracked via WINDOWS.md #1 |
| WIRE-01 | 01-04, 01-06 | byte-identical codec for all 7 types via golden fixtures | ✓ SATISFIED | SC4 |
| WIRE-02 | 01-06 | every Rust message decodes through the real Python decoder | ✓ SATISFIED | SC5 |

No orphaned requirements: REQUIREMENTS.md maps exactly BASE-01/02/03 and WIRE-01/02 to Phase 1 (and marks all 5 `[x]` complete); every ID is claimed by at least one plan's `requirements:` frontmatter, including the quick task (`[BASE-02]`), checked across all 13 plans plus the quick task.

### Anti-Patterns Found

No TBD/FIXME/XXX/TODO/HACK/PLACEHOLDER debt markers in any file touched since the prior verification pass (confirmed by grep, this run; the only historical hit anywhere in scope remains `mktemp ...XXXXXX`, a template, not a marker).

The 2026-10-05 incremental code review's 4 findings are now all triaged (none left silently untriaged):

| Finding | File | Severity | Disposition | Defeats a must-have? |
|---------|------|----------|-------------|----------------------|
| CR-01 (new instance, same id): missing exception guard around the rust-mode spawn loop and handshake write | python/rsglang/launch.py | 🛑 Critical → now fixed | **Fixed** via quick task 261004-vqo; verified behaviorally in this pass (`test_unexpected_error_leaves_no_orphans`, both cases pass) | No longer applicable — closed. |
| WR-01 (new instance, same id): `rust_tail` deque appended-to from the stderr-pump thread, read via `list()` in `shutdown()` with no lock | python/rsglang/launch.py:94-100, 158-161, 210-217 | ⚠️ Warning | **Open** — the user explicitly reviewed this (01-UAT.md test 11) and chose "pass" without fixing or deferring it; the disposition ledger correctly records it as `open` (an acknowledged, accepted risk), not silently dropped | No — happy-path tests all pass. A low-probability race that could, under load, defeat the SIGKILL-escalation path CR-01's fix and 01-07/01-08's cleanup guarantees rely on. Revisit before relying on `shutdown()`'s SIGKILL escalation under load (e.g. before Phase 2's benchmark harness work). |
| IN-01 (new instance): `extract_handshake`'s dict literal hand-duplicates `HANDSHAKE_KEYS` | python/rsglang/backend.py:42-55 | ℹ️ Info | **Deferred** to Phase 6 (01-UAT.md test 12) | No — a runtime check in `encode_handshake_line` still catches an actual mismatch today. |
| IN-02 (new instance): dead/racy code after the self-directed SIGKILL in `shutdown()` | python/rsglang/launch.py:219-233 | ℹ️ Info | **Deferred** to Phase 6 (01-UAT.md test 12) | No — harmless if skipped, cosmetic. |
| Carried-forward: WR-02/04/06/07/08/09 `fixed`; WR-03/05/10, IN-03 `deferred`; IN-04..IN-13 `open` | various | ⚠️/ℹ️ | Unchanged from the prior verification pass | No bearing on this phase's must-haves. |

`01-REVIEW-DISPOSITION.md` frontmatter now shows `open: 11, total: 24`. I counted the table rows myself: 11 `open` (WR-01 + IN-04 through IN-13, 10 info rows), 7 `fixed` (CR-01, WR-02/04/06/07/08/09), 6 `deferred` (IN-01/02/03, WR-03/05/10) — matches. The ID-reuse notice at the top still accurately documents the 4 decisions it dropped when the 2026-10-05 review reused their ids; none of the underlying fixes/deferrals were undone.

### Hardware Handoff (Tracked, Not Blocking — per explicit user decision)

These two checks still need to run on real Linux/CUDA hardware. They are not part of this verification's blocking gate (see the `overrides:` in frontmatter and the user's explicit 2026-10-05 decision), but they are not forgotten: WINDOWS.md entry #1 tracks the handoff, and both are described here for whoever runs them (a GitHub collaborator, per the user's plan).

1. **GPU end-of-phase check (ROADMAP SC2/SC3 GPU halves).** On a Linux+CUDA box, after `uv venv --python=3.12 && uv pip install -e vendor/mini-sglang && uv pip install -e .`, run `bash scripts/gpu_phase1_check.sh`. Expected: ALL PASS across steps 1, 2, 3 (including the PDEATHSIG-armed check), 4, 4b and 5. Handshake line should show `sha 9a91cfa…`, `max_running_req=256`, `num_pages>1`, `max_seq_len` in `1..40960`, `page_size` 1 or 64, `eos 151645` (Qwen3-0.6B). Tracked: WINDOWS.md #1; UAT: 01-UAT.md test 1 (`blocked`).
2. **Linux-only watchdog success-path unit test.** On any Linux machine (no GPU needed): `.venv/bin/python -m pytest python/tests/test_parent_watchdog.py::test_linux_arms_pdeathsig_sigkill -q`. Expected: 1 passed (`PR_GET_PDEATHSIG` reports `SIGKILL` = 9). Skipped on macOS by design. UAT: 01-UAT.md test 6 (`blocked`).

### Human Verification Required

None. All items that required a human policy decision in the prior verification pass (CR-01 triage, WR-01/IN-01/IN-02 triage, the stale 01-UAT.md record) have been resolved and are recorded in 01-UAT.md tests 8-12 and 01-REVIEW-DISPOSITION.md. The two hardware-hardware-dependent checks are tracked above as a non-blocking collaborator handoff per the explicit user decision for this pass, not as an open human-verification gate.

### Gaps Summary

No gaps. Compared with the prior verification (95/99, `human_needed`):

- **CR-01 (critical) is closed.** The guard the prior pass flagged as its top-priority human-verification item now exists, is wired correctly (confirmed by direct code read), and is proven by a passing two-case regression test that I ran myself in isolation and that reproduced the predicted hang on the unmodified code first (RED evidence in the quick-task SUMMARY, corroborated by git history: the RED test commit `de6ba69` immediately precedes the GREEN fix commit `c39851f`).
- **WR-01/IN-01/IN-02 (new instances) are triaged.** None are silently dropped: WR-01 is an explicit, recorded "accept as-is" decision by the user (disposition stays `open` by the ledger's own convention for "acknowledged, not fixed"); IN-01/IN-02 are explicitly deferred to Phase 6.
- **The stale 01-UAT.md record is refreshed.** Tests 8/9 carry resolution notes; tests 10-12 record the new triage.
- **The two hardware-blocked UAT items (GPU box, Linux-only watchdog test) remain genuinely blocked on physical hardware this Mac does not have.** Per the user's explicit 2026-10-05 decision, these are treated as a tracked, non-blocking collaborator handoff (WINDOWS.md #1) rather than a reason to withhold `passed` — applied here as four explicit overrides on the affected truths, not as a silent pass. The pending hardware verification itself is not erased: it is documented above under "Hardware Handoff" for the collaborator who will run it.

---

_Verified: 2026-10-05T09:30:00Z_
_Verifier: Claude (gsd-verifier)_
