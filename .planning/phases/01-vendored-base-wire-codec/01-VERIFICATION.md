---
phase: 01-vendored-base-wire-codec
verified: 2026-10-04T23:00:00Z
status: human_needed
score: 95/99 must-haves verified
covered_files:
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
  - .planning/phases/01-vendored-base-wire-codec/01-UAT.md
  - .planning/phases/01-vendored-base-wire-codec/01-VALIDATION.md
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
covered_digest: "v2:sha256:d7ccc6a436baca5f28e2e7aadd4e72537e1db3f68b36582a531031eb72ab319e"
behavior_unverified: 3
overrides_applied: 0
re_verification:
  previous_status: human_needed
  previous_score: 89/93
  gaps_closed:
    - "G-01-8 (abbreviated --shell/--shell-m with no rsg-server binary built yet masked the shell-mode rejection with a misleading 'binary not found' error) — plan 01-13"
    - "G-01-9 (an AttributeError from a missing prctl libc symbol aborted the scheduler instead of degrading to the polling watchdog) — plan 01-13"
  gaps_remaining: []
  regressions: []
behavior_unverified_items:
  - truth: "ROADMAP SC2 (GPU half): on the GPU machine --frontend python serves a chat completion through the unmodified Python frontend, and --frontend rust starts the same real backend plus rsg-server"
    test: "On the Linux GPU box run `bash scripts/gpu_phase1_check.sh` (steps 1-3)"
    expected: "Step 2 PASS (non-empty chat completion content from python mode); step 3 PASS (rust mode reaches 'handshake received' and PDEATHSIG armed)"
    why_human: "Needs CUDA and the real upstream Scheduler; on the Mac only a FakeScheduler on upstream's real ZMQ queues was exercised"
  - truth: "ROADMAP SC3 (GPU half): the real backend reports max_seq_len, eos_token_id, page_size, max_running_req at readiness and rsg-server logs them; the Python frontend keeps working against the same backend code"
    test: "Same run, step 3 output line and step 5"
    expected: "'handshake received' line with upstream_sha=9a91cfa..., max_running_req=256, num_pages>1, max_seq_len in 1..40960, page_size 1 or 64, eos_token_id=151645 for Qwen3-0.6B; step 5 check_upstream.py PASS"
    why_human: "extract_handshake reads scheduler.engine.max_seq_len / engine.num_pages / cache_manager.page_size / eos_token_id; attribute names checked against upstream source, but real values only exist after a CUDA engine init"
  - truth: "On Linux, when prctl(PR_SET_PDEATHSIG, SIGKILL) succeeds, PR_GET_PDEATHSIG actually reports SIGKILL (the success path of 01-08/01-10's watchdog, not just the degrade-and-log failure path)"
    test: "On a Linux box run `.venv/bin/python -m pytest python/tests/test_parent_watchdog.py -k pdeathsig_sigkill` and, on the GPU box, confirm `scripts/gpu_phase1_check.sh` step 3 PASSes"
    expected: "test_linux_arms_pdeathsig_sigkill passes (PR_GET_PDEATHSIG == 9); GPU step 3 PASS with no degrade line in rust-mode.log"
    why_human: "The success branch is guarded by sys.platform.startswith('linux'); on this macOS machine it never executes and its unit test is skipped. The failure/degrade branch (01-10, broadened by 01-13 to AttributeError) and the step-3 check for it (01-12) are both proven on the Mac with stubs/fixtures"
human_verification:
  - test: "On the Linux GPU box run `bash scripts/gpu_phase1_check.sh` (after `uv venv --python=3.12 && uv pip install -e vendor/mini-sglang && uv pip install -e .`, build-essential present)."
    expected: "ALL PASS across steps 1, 2, 3 (incl. the PDEATHSIG-armed check), 4, 4b and 5. Handshake line: sha 9a91cfa…, max_running_req=256, num_pages>1, max_seq_len in 1..40960, page_size 1 or 64, eos 151645."
    why_human: "ROADMAP criteria 2 and 3 and the Linux-only PR_SET_PDEATHSIG success path need CUDA, nvidia-smi and Linux. 01-UAT.md test 1 is `blocked` (no Linux GPU available yet)."
  - test: "Run the Linux-only unit test: `pytest python/tests/test_parent_watchdog.py::test_linux_arms_pdeathsig_sigkill` on any Linux machine (no GPU needed)."
    expected: "1 passed (PR_GET_PDEATHSIG reports SIGKILL = 9)."
    why_human: "Skipped on macOS by design. 01-UAT.md test 6 is `blocked` for the same reason."
  - test: "Triage CR-01 (critical, 2026-10-05 incremental review, currently `open`): `_run_rust_mode` in python/rsglang/launch.py has no top-level try/except/finally around the rsg-server + TP-rank spawn loop (lines ~169-176) and the handshake write (ValueError from encode_handshake_line is uncaught beyond BrokenPipeError). An unanticipated exception there leaves already-spawned rsg-server and scheduler-rank processes running, unkilled — `run_rust_mode`'s `finally` only unlinks socket files. Decide fixed-now or deferred-with-target-phase, and record it in 01-REVIEW-DISPOSITION.md."
    expected: "CR-01 is not left permanently `open` without a decision. Given it directly contradicts the project's stated D-12 goal ('benchmarks don't leak GPU processes') and the only prior Critical finding in this phase (the original CR-01, group-SIGINT) was fixed immediately rather than deferred, this is flagged for priority attention rather than routine triage."
    why_human: "Policy call on severity/timing. It does not break Phase 1's happy-path must-haves (all passing tests exercise the anticipated-failure paths, which already call shutdown()) — it is a gap in the unanticipated-exception safety net, exactly as the review describes. Untriaged; must not be silently dropped."
  - test: "Triage WR-01 (new instance, 2026-10-05 review): `rust_tail` deque appended to by `_pump_rsg_stderr` on a background thread and read via `list(rust_tail)` in `shutdown()` with no lock (python/rsglang/launch.py:94-100, 158-161, 210-217) can raise `RuntimeError: deque mutated during iteration` if the pump thread is still writing when shutdown's grace period expires — crashing shutdown() before its own SIGKILL escalation."
    expected: "Recorded fixed or deferred with a target phase."
    why_human: "Policy call; low-probability race, but it can defeat the SIGKILL escalation path CR-01 and 01-07/01-08's cleanup guarantees depend on."
  - test: "Triage IN-01 (new instance) and IN-02 (new instance), both from the 2026-10-05 review: IN-01 is `extract_handshake`'s dict literal hand-duplicating `HANDSHAKE_KEYS` instead of building from the constant (backend.py:42-55 vs handshake.py:16-24); IN-02 is dead/racy code after the self-directed SIGKILL in launch.py's shutdown() (lines 219-233)."
    expected: "Each recorded fixed or deferred, or explicitly accepted as non-blocking, in 01-REVIEW-DISPOSITION.md."
    why_human: "Policy call; neither defeats a Phase 1 must-have on its own."
  - test: "Refresh 01-UAT.md to reflect that G-01-8 and G-01-9 (tests 8 and 9) are closed by 01-13, and that the WR-01/IN-01 ids it references are now superseded by new 2026-10-05 findings under the same ids (see 01-REVIEW-DISPOSITION.md's ID-reuse notice)."
    expected: "01-UAT.md's own record no longer reads `result: issue` for tests whose underlying gaps are closed."
    why_human: "Documentation-staleness note, not a code gap; a human/maintainer decision on whether to amend the historical UAT record."
---

# Phase 1: Vendored Base & Wire Codec Verification Report

**Phase Goal:** The repo holds a pinned, attributed copy of mini-sglang. One launch command runs the shared backend with either frontend, and the backend reports a readiness handshake. The Rust msgpack codec is byte-exact with upstream for all 7 message types.
**Verified:** 2026-10-04T23:00:00Z
**Status:** human_needed
**Re-verification:** Yes. This is the 13th and final plan's gap-closure re-verification (closes G-01-8/WR-01 and G-01-9/IN-01 from 01-UAT.md tests 8/9), superseding the prior `01-VERIFICATION.md` dated 2026-10-04T09:30:00Z.

## Goal Achievement

I independently re-ran the full Mac gate myself rather than trusting SUMMARY narration, re-read every line of code the plan claims to have changed, and ran the specific new tests individually (not just as part of the full suite) to confirm each is a real behavioral proof, not a renamed no-op.

- `bash scripts/check_all.sh --offline` (this run): exit 0. Cargo workspace: 10+1+6 (rsg-wire) + 10+12 (rsg-server, included) all pass. Pytest: **88 passed, 37 skipped** (up from 84 passed before 01-13 — the 4 new cases: 3 shell-mode spellings + 1 prctl-missing). Fixture freshness OK (34 cases). WIRE-02 decode: 37 passed. `check_upstream.py --offline`: tree `02d3e4ad…` still matches pristine `9a91cfa`, 0 listed modifications.
- `python/rsglang/launch.py:103-124` (direct read): `run_rust_mode` now does the literal `"--shell-mode" in rest` pre-check first, then `from minisgl.server.args import parse_args; server_args, run_shell = parse_args(rest)`, then `if run_shell: ...; return 2` — all **before** `rust_bin = resolve_rust_bin(ns.rust_bin)`. `_run_rust_mode`'s signature is now `(ns, server_args: ServerArgs, rust_bin, suffix)` and no longer calls `parse_args` itself. Confirmed by `grep -c "server_args, run_shell = parse_args(rest)"` = 1, and the awk ordering check (parse line 109 < resolve line 113) exits 0.
- `python/rsglang/backend.py` (direct read): the prctl try/except clause now reads `except (OSError, AttributeError) as exc:`, confirmed exactly once in the file; the "PDEATHSIG unavailable" log string is still present exactly once, byte-identical to what 01-12's `pdeathsig_degraded` greps for.
- Ran the new tests in isolation (not just inside the full-suite count):
  - `pytest python/tests/test_launch_args.py -k test_rust_mode_reports_shell_mode_before_missing_binary -v` → 3 passed (`--shell`, `--shell-m`, `--shell-mode`), each asserting the rejection message is present, "rsg-server binary not found" is absent, and no `subprocess.Popen`/`multiprocessing.Process`/`os.setpgid`/`signal.signal` call occurred.
  - `pytest python/tests/test_parent_watchdog.py::test_prctl_failure_degrades_to_polling -v` → 3 passed (`prctl-fails`, `cdll-fails`, `prctl-missing`); the new `prctl-missing` case fakes `ctypes.CDLL` returning an object with no `prctl` attribute, forces `sys.platform='linux'`, and asserts the child exits 0, prints 'armed'/'polling'/'survived', and has "PDEATHSIG unavailable" (naming "prctl") in stderr with no Traceback.
  - `pytest python/tests/test_launch_rust_e2e.py -q` → 10 passed (full e2e suite unaffected by the `_run_rust_mode` signature change).

**Both G-01-8 and G-01-9 are closed with real behavioral evidence, not just code presence** — these are state-transition/failure-path invariants (the AttributeError-degrades-to-polling case is exactly the kind of invariant Step 3 of this process requires a passing test for, not just symbol presence), and in both cases a named test exercising the exact failure mode passes.

**A new, more serious finding surfaced since the prior verification pass.** A 2026-10-05 incremental code review re-scoped to the 4 files 01-13 touched found 4 new issues and reused the ids `CR-01`, `WR-01`, `IN-01`, `IN-02` for them (per `01-REVIEW-DISPOSITION.md`'s own documented id-reuse rule, which correctly dropped the four prior `fixed`/`deferred` decisions under those ids and preserved them in an "ID-reuse notice" at the top of the file — I verified this notice accurately describes what was dropped and why, and that none of the underlying source-code fixes were undone). The most significant of the four is **CR-01 (critical): `_run_rust_mode` has no top-level exception/finally guard around the process-spawn loop**, so an unanticipated exception (e.g. `mp.Process.start()` failing for one TP rank under memory pressure, or a `ValueError` from `encode_handshake_line` escaping the lone `except BrokenPipeError`) leaves already-spawned `rsg-server` and scheduler-rank processes running, unkilled. I confirmed this by direct code read: lines ~151-176 spawn `rsg-server` and then each TP rank via a bare `for` loop with no enclosing `try`, and `shutdown()`/`children()`/`report_errors()` (the only cleanup helpers) are defined later in the function body, not as a guard around the spawn loop. `run_rust_mode`'s own `finally` (confirmed) only calls `sockets.unlink_run_sockets(suffix)` — it does not kill any process. This directly contradicts the project's own stated D-12 goal ("Kill the Python shim and its TP children cleanly … so benchmarks don't leak GPU processes") and, unlike the three other new findings (all Warning/Info), is a **Critical** severity that I'm not comfortable silently waving through to a non-blocking disposition the way the prior verification pass treated open Warning/Info items — it is reported below as the highest-priority human-verification item, not folded quietly into routine triage. It does **not** defeat any of Phase 1's happy-path must-haves (every automated test that passes exercises an *anticipated* failure path, each of which already calls `shutdown()`), so it does not, on its own, flip the overall status to `gaps_found` — but it is untriaged and is flagged prominently.

### Observable Truths: ROADMAP Success Criteria (the contract)

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| SC1 | Repo contains mini-sglang @ 9a91cfa with MIT LICENSE + copyright; UPSTREAM.md names the commit and lists every modified vendored file | ✓ VERIFIED | `check_upstream.py --offline` (this run): tree `02d3e4ad…` matches pristine 9a91cfa, 0 listed modifications. `git status --porcelain vendor/` empty. |
| SC2 | On GPU, `--frontend python` serves a chat completion via the unmodified frontend; `--frontend rust` starts the same backend plus the Rust skeleton | ⚠️ PRESENT_BEHAVIOR_UNVERIFIED | Mac half verified (e2e tests pass, 14 gpu-check-script tests pass against stubs). GPU half needs `scripts/gpu_phase1_check.sh` steps 2-3 on Linux, still `blocked` per 01-UAT.md test 1. Unchanged by 01-13. |
| SC3 | Backend reports max_seq_len, eos_token_id, page_size, max_running_req at readiness; Rust logs them; Python frontend unchanged against same backend code | ⚠️ PRESENT_BEHAVIOR_UNVERIFIED | `extract_handshake` attribute names checked against upstream source (prior verification); rsg-server logs all values. Real values need a CUDA engine init. Unchanged by 01-13. |
| SC4 | For each of the 7 upstream message types, Rust codec bytes equal golden fixtures from upstream's Python encoder, checked on the Mac | ✓ VERIFIED | `cargo test --workspace` (this run): rsg-wire 10 lib + 1 dump + 6 fixture tests pass. `gen_wire_fixtures.py --check` passed (34 cases, byte-identical). Untouched by 01-13. |
| SC5 | Every message the Rust codec emits decodes through upstream's real Python decoder (cls(**kwargs)) | ✓ VERIFIED | `check_wire_decode.sh` (this run): Rust dump test passed, then 37 pytest tests passed. Untouched by 01-13. |

### Observable Truths: PLAN must_haves

Plans 01-01 to 01-12 are unchanged in source since the last verification pass (confirmed: only `python/rsglang/launch.py`, `python/rsglang/backend.py`, `python/tests/test_launch_args.py`, `python/tests/test_parent_watchdog.py` and `01-REVIEW-DISPOSITION.md` changed, per 01-13's `files_modified` and `git log`). I carried forward their per-truth statuses with a regression check (the full gate run above, 88 passed vs 84 before, no regressions).

| Plan | Truths | Status | Evidence |
|------|--------|--------|----------|
| 01-01 (vendoring, env) | 8 | ✓ 8 VERIFIED | Unchanged; tree hash, LICENSE, UPSTREAM.md all intact. |
| 01-02 (rsg-server) | 7 | ✓ 7 VERIFIED | `cargo test --workspace` green. |
| 01-03 (launcher, e2e) | 7 | ✓ 7 VERIFIED | e2e and topology tests pass in this run's pytest. |
| 01-04 (codec, fixtures) | 9 | ✓ 9 VERIFIED | fixtures suite; `--check` fresh. |
| 01-05 (failure contract, GPU script) | 7 | ✓ 6 VERIFIED, ? 1 UNCERTAIN | The GPU-script human sign-off truth is still pending (01-UAT.md test 1 `blocked`). |
| 01-06 (check_upstream, WIRE-02) | 11 | ✓ 11 VERIFIED | check_upstream and decode tests pass. |
| 01-07 (CR-01 original, G-01-2) | 6 | ✓ 6 VERIFIED | Group-SIGINT tests still pass; unaffected by 01-13. |
| 01-08 (WR-02, G-01-3) | 7 | ✓ 6 VERIFIED, ⚠️ 1 PRESENT_BEHAVIOR_UNVERIFIED | Portable early-kill/watchdog-pid truths verified. Linux-only "PDEATHSIG armed" success-path truth still only provable on Linux. |
| 01-09 (WR-07, WR-08 gap closure) | 8 | ✓ 8 VERIFIED | Unaffected by 01-13; re-confirmed in this run's 88-pass gate. |
| 01-10 (WR-06, WR-09 gap closure) | 8 | ✓ 8 VERIFIED | Unaffected by 01-13. |
| 01-11 (WR-04, WR-01-original gap closure) | 6 | ✓ 6 VERIFIED | Unaffected by 01-13 directly; the WR-01 id it recorded `fixed` is now superseded by a newer, unrelated finding under the same id (see below) — not a regression of 01-11's own work. |
| 01-12 (WR-06-CHECK gap closure) | 4 | ✓ 4 VERIFIED | Unaffected by 01-13. |
| 01-13 (G-01-8, G-01-9 gap closure) | 6 | ✓ 5 VERIFIED, ⚠️ 1 superseded-not-failed | Details below. |

**01-13 truths**

| Truth | Status | Evidence |
|-------|--------|----------|
| With --frontend rust and no resolvable rsg-server binary, each of --shell, --shell-m, --shell-mode exits 2 with the shell-mode rejection and never prints "rsg-server binary not found" | ✓ VERIFIED | Ran `pytest test_launch_args.py -k test_rust_mode_reports_shell_mode_before_missing_binary -v` myself: 3 passed, one per spelling, each asserting both the message's presence and the absence of "rsg-server binary not found", plus no spawn/setpgid/signal calls. |
| run_rust_mode parses upstream args exactly once and checks run_shell before resolve_rust_bin; _run_rust_mode receives the already-parsed server_args and no longer calls parse_args; rust mode still starts end to end | ✓ VERIFIED | Code read confirms the ordering (`grep -c "server_args, run_shell = parse_args(rest)"` = 1; awk ordering check exits 0: parse at line 109, resolve at line 113). Ran `pytest test_launch_rust_e2e.py -q` myself: 10 passed. |
| The literal --shell-mode pre-check, the abbreviated-flag test with a valid --rust-bin, and the python-mode passthrough are unchanged and still pass | ✓ VERIFIED | `grep -c '"--shell-mode" in rest'` = 1; full pytest run (88 passed) includes these tests with no regressions. |
| A missing prctl symbol (AttributeError) logs "PDEATHSIG unavailable", returns normally, the polling thread stays alive, and the process keeps running | ✓ VERIFIED | Ran `pytest test_parent_watchdog.py::test_prctl_failure_degrades_to_polling -v` myself: the new `prctl-missing` case passes (exit 0, 'armed'/'polling'/'survived' in stdout, "PDEATHSIG unavailable" naming "prctl" in stderr, no Traceback). This is a state-transition/degrade invariant and the behavioral test, not just symbol presence, is what grounds the VERIFIED status. |
| The "PDEATHSIG unavailable" log text is byte-identical so 01-12's gpu_phase1_check.sh step 3 still fails on this new failure mode | ✓ VERIFIED | `grep -c "PDEATHSIG unavailable" python/rsglang/backend.py` = 1 (unchanged string); `test_gpu_check_script.py` (unmodified by 01-13) still passes in the full gate, confirming `pdeathsig_degraded`'s grep target is untouched. |
| 01-REVIEW-DISPOSITION.md records WR-01 and IN-01 as fixed and open count drops from 12 to 10 | ⚠️ Superseded by a later review cycle (not a failure of this plan) | At the moment 01-13 completed, WR-01 and IN-01 were correctly marked `fixed` and `open` was 10 (confirmed in 01-13-SUMMARY.md and the commit history: `2241e53 docs(01-13): mark IN-01 fixed`). A subsequent 2026-10-05 incremental review found two *new, unrelated* defects and reused the `WR-01`/`IN-01` ids for them, which — per the disposition file's own documented rule — correctly reset those rows to `open` and dropped the prior `fixed` decisions (preserved in the file's "ID-reuse notice" for the record). The underlying G-01-8/G-01-9 fixes are intact and still proven by the passing tests above; only the ledger's id-to-finding mapping moved on, exactly as the file's process allows. |

**Score:** 95/99 verified (3 roadmap + 92 plan truths, 86 carried forward + 6 from 01-13, 5 of which are newly-VERIFIED here and 1 superseded-not-failed). 3 are present but behavior-unverified (SC2, SC3, Linux PDEATHSIG-armed success path). 1 is UNCERTAIN (the pending human sign-off of the GPU script, 01-UAT.md test 1, `blocked`).

**Interpretation note (SC4, "7 message types"):** unchanged from prior verifications — read as the 6 scheduler-boundary messages plus `SamplingParams`, with `Tensor` also covered.

### Prohibitions

| Plan | Prohibition | Tier | Disposition |
|------|-------------|------|-------------|
| 01-01 | Vendored LICENSE / copyright never removed or altered | test | ✓ VERIFIED. `LICENSE_MISSING` check and its test still pass. |
| 01-06 | Tier A frozen frontend never modified | test | ✓ VERIFIED. `check_upstream.py --offline` clean (0 modifications). |
| 01-03 | Rust mode runs the byte-identical upstream Scheduler; the handshake is not produced by patching vendored code | judgment | Resolved by a human: 01-UAT.md test 4 recorded `pass`. Unaffected by 01-13 (which touched only the launcher/watchdog Python and a disposition file). |

### Required Artifacts

| Artifact | Status | Details |
|----------|--------|---------|
| vendor/mini-sglang/ (+LICENSE), UPSTREAM.md, vendor/UPSTREAM_SHA | ✓ VERIFIED | pristine tree, SHA single-sourced |
| Cargo.toml, rust-toolchain.toml, crates/rsg-server/*, crates/rsg-wire/* | ✓ VERIFIED | cargo gate green (27 tests) |
| fixtures/wire/*.msgpack + manifest.json | ✓ VERIFIED | 34 cases, fresh |
| python/rsglang/launch.py | ✓ VERIFIED | `run_rust_mode`/`_run_rust_mode` reordering confirmed by direct read and grep; substantive and wired |
| python/rsglang/backend.py | ✓ VERIFIED | `except (OSError, AttributeError)` confirmed; docstring updated |
| crates/rsg-server/src/handshake.rs | ✓ VERIFIED | `deserialize_with = "Option::deserialize"` on eos_token_id, confirmed (unchanged by 01-13) |
| scripts/gpu_phase1_check.sh | ✓ VERIFIED (exists, substantive, Mac-tested) | not executed end-to-end on GPU (no GPU); untouched by 01-13 |
| python/tests/test_launch_args.py | ✓ VERIFIED | `test_rust_mode_reports_shell_mode_before_missing_binary` present and passing (3 cases, verified in isolation) |
| python/tests/test_parent_watchdog.py | ✓ VERIFIED | `test_prctl_failure_degrades_to_polling[prctl-missing]` present and passing (verified in isolation) |
| .planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md | ✓ VERIFIED (exists, substantive) | WR-06/07/08/09/04/02 `fixed` (6); WR-03/05/10/IN-03 `deferred` (4); WR-01/IN-01/IN-02/CR-01 (new 2026-10-05 instances) and 10 other info items `open` (14); total 24. ID-reuse notice accurately documents the 4 dropped decisions. |

### Key Link Verification

| From | To | Via | Status |
|------|----|-----|--------|
| python/rsglang/launch.py run_rust_mode | upstream parse_args / run_shell | `server_args, run_shell = parse_args(rest)` then `if run_shell: return 2`, confirmed before `resolve_rust_bin` (lines 109-113) | ✓ WIRED |
| python/rsglang/launch.py run_rust_mode | _run_rust_mode | `_run_rust_mode(ns, server_args, rust_bin, suffix)` — parsed ServerArgs passed, not raw argv; confirmed, 1 call site | ✓ WIRED |
| python/rsglang/backend.py start_parent_watchdog prctl try | the broadened except clause | `except (OSError, AttributeError) as exc:` wraps the CDLL+argtypes+call, confirmed exactly once | ✓ WIRED |
| python/rsglang/backend.py PDEATHSIG-unavailable log | scripts/gpu_phase1_check.sh pdeathsig_degraded (01-12) | fixed string "PDEATHSIG unavailable", confirmed unchanged (count=1) | ✓ WIRED |

### Data-Flow Trace (Level 4)

| Artifact | Data | Source | Real data | Status |
|----------|------|--------|-----------|--------|
| rsg-server "handshake received" log | max_seq_len, eos, page_size, max_running_req, num_pages, sha | stdin ← launcher ← ready_queue ← `extract_handshake(scheduler, …)` | Mac: FakeScheduler constants; GPU: real engine attributes | ✓ FLOWING on Mac; GPU pending (unchanged by 01-13) |
| Golden fixtures | msgpack bytes | vendored upstream `serialize_type` | yes (`--check` regenerates) | ✓ FLOWING |
| launch.py stderr on shell-mode rejection | "--shell-mode is not supported with --frontend rust" | `_log(...)` called from the `if run_shell:` branch, before any spawn | yes (captured via `capsys` in the new test, for all 3 spellings) | ✓ FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Phase gate, single full run | `bash scripts/check_all.sh --offline` | exit 0. Cargo green. Pytest: 88 passed, 37 skipped (+4 vs prior pass). Fixtures fresh. Decode: 37 passed. check_upstream: OK | ✓ PASS |
| New shell-mode-ordering test, isolated | `.venv/bin/python -m pytest python/tests/test_launch_args.py -k test_rust_mode_reports_shell_mode_before_missing_binary -v` | 3 passed | ✓ PASS |
| New prctl-missing test, isolated | `.venv/bin/python -m pytest "python/tests/test_parent_watchdog.py::test_prctl_failure_degrades_to_polling" -v` | 3 passed (prctl-fails, cdll-fails, prctl-missing) | ✓ PASS |
| Full rust-mode e2e suite, isolated | `.venv/bin/python -m pytest python/tests/test_launch_rust_e2e.py -q` | 10 passed | ✓ PASS |
| Acceptance-criteria greps from 01-13-PLAN.md | `grep -c ...` x6 (see commands above) | all match expected counts (1 each) and ordering | ✓ PASS |
| Restoration check | `git status --porcelain vendor/` | clean | ✓ no stray edits |
| GPU / Linux checks | `scripts/gpu_phase1_check.sh`, PDEATHSIG success-path unit test | not run (macOS, no CUDA) | ? SKIP → human |

### Probe Execution

No `scripts/*/tests/probe-*.sh` exists and no plan declares one. Step 7c: N/A.

### Requirements Coverage

| Requirement | Source Plan(s) | Description | Status | Evidence |
|-------------|----------------|-------------|--------|----------|
| BASE-01 | 01-01, 01-06 | vendored @ 9a91cfa with LICENSE; UPSTREAM.md records commit and modified files | ✓ SATISFIED | SC1 |
| BASE-02 | 01-02, 01-03, 01-05, 01-07, 01-08, 01-09, 01-10, 01-11, 01-12, 01-13 | one launch command for `--frontend python` / `--frontend rust`; shell-mode rejection and watchdog hardening | ✓ SATISFIED on Mac / ? NEEDS HUMAN on GPU | launcher, tests, 9 closed gaps (incl. G-01-8/G-01-9); GPU steps 2-3, 4, 4b |
| BASE-03 | 01-02, 01-03, 01-05, 01-09, 01-10, 01-11, 01-12 | backend readiness handshake (4 values, strictly requiring eos_token_id); both frontends use same backend code | ✓ SATISFIED on Mac / ? NEEDS HUMAN on GPU | e2e handshake test; GPU step 3 |
| WIRE-01 | 01-04, 01-06 | byte-identical codec for all 7 types via golden fixtures | ✓ SATISFIED | SC4 |
| WIRE-02 | 01-06 | every Rust message decodes through the real Python decoder | ✓ SATISFIED | SC5 |

No orphaned requirements: REQUIREMENTS.md maps exactly BASE-01/02/03 and WIRE-01/02 to Phase 1 (and marks all 5 complete); every ID is claimed by at least one plan (checked across all 13 plans' `requirements:` frontmatter).

### Anti-Patterns Found

No TBD/FIXME/XXX/TODO/HACK/PLACEHOLDER debt markers in the files touched by 01-13 (the only historical grep hit anywhere in scope remains `mktemp ...XXXXXX`, a template, not a marker).

The 2026-10-05 incremental review (re-scoped to the 4 files 01-13 touched: `backend.py`, `launch.py`, `test_launch_args.py`, `test_parent_watchdog.py`) found 1 critical, 1 warning, 2 info issues, all currently `open`:

| Finding | File | Severity here | Defeats a must-have? |
|---------|------|---------------|----------------------|
| CR-01 (new instance, same id): `_run_rust_mode` has no top-level exception/finally guard around the spawn loop or the handshake write; an unanticipated failure leaks already-spawned GPU-holding processes | python/rsglang/launch.py:124-298 (spawn loop ~151-176; unguarded handshake write ~268-275) | 🛑 Critical | Not for Phase 1's happy-path must-haves (every passing test exercises an anticipated failure path, which already calls `shutdown()`). But it contradicts the project's own D-12 goal and the only precedent Critical finding in this phase was fixed immediately, not deferred — flagged as the top-priority human-verification item below, not quietly absorbed. |
| WR-01 (new instance, same id): `rust_tail` deque appended to by a background thread and read via `list()` in `shutdown()` with no lock — can raise `RuntimeError` mid-teardown on a slow-exit rsg-server | python/rsglang/launch.py:94-100, 158-161, 210-217 | ⚠️ Warning | No, for the happy path. But it can defeat the SIGKILL-escalation path that CR-01's own suggested fix and 01-07/01-08's cleanup guarantees rely on, so it is linked to CR-01's concern, not independent of it. |
| IN-01 (new instance): `extract_handshake`'s dict literal hand-duplicates `HANDSHAKE_KEYS` instead of building from the constant | python/rsglang/backend.py:42-55 vs python/rsglang/handshake.py:16-24 | ℹ️ Info | No. Only a maintainability/future-drift note; a runtime check in `encode_handshake_line` still catches an actual mismatch today. |
| IN-02 (new instance): statements after the self-directed SIGKILL in `shutdown()` are dead/racy code | python/rsglang/launch.py:219-233 | ℹ️ Info | No. Harmless if skipped; a comment/reordering note. |
| Carried-forward: WR-02/04/06/07/08/09 `fixed`; WR-03/05/10, IN-03 `deferred`; IN-04..IN-13 `open` | various | ⚠️/ℹ️ | No. Unchanged from the prior verification; no bearing on this phase's must-haves. |

`01-REVIEW-DISPOSITION.md` frontmatter now shows: `open: 14`, `total: 24`. I counted the table rows myself (14 `open`, 6 `fixed`, 4 `deferred` = 24) and it matches. The file's own "ID-reuse notice" at the top accurately documents that CR-01 (original, group-SIGINT fix), WR-01 (original, abbreviation fix), IN-01 (original, prctl-OSError-only note) and IN-02 (original, GPU-script cleanup note) decisions were dropped from their rows because 2026-10-05's review reused those ids for different findings — none of the underlying fixes/deferrals are undone, only the ledger's id mapping moved on, exactly as the file's documented process intends.

### Human Verification Required

#### 1. Triage CR-01 (critical, highest priority)
**Test:** Decide fixed-now or deferred-with-target-phase for the missing top-level exception/finally guard around `_run_rust_mode`'s spawn loop and handshake write (python/rsglang/launch.py).
**Expected:** Not left silently `open`. Given it contradicts D-12 and the prior Critical finding in this phase was fixed immediately, this is flagged for priority attention.
**Why human:** Policy call on severity/timing; does not break a Phase 1 happy-path must-have on its own.

#### 2. GPU end-of-phase check (ROADMAP SC2 and SC3, plus the Linux PDEATHSIG success path)
**Test:** On a Linux GPU box run `bash scripts/gpu_phase1_check.sh` after the documented setup.
**Expected:** ALL PASS across steps 1, 2, 3 (incl. PDEATHSIG armed), 4, 4b and 5.
**Why human:** Needs CUDA, nvidia-smi and Linux. 01-UAT.md test 1 is `blocked`.

#### 3. Linux-only watchdog unit test
**Test:** `pytest python/tests/test_parent_watchdog.py::test_linux_arms_pdeathsig_sigkill` on any Linux machine (no GPU needed).
**Expected:** 1 passed.
**Why human:** Skipped on macOS by design. 01-UAT.md test 6 is `blocked`.

#### 4. Triage WR-01 (new instance — deque race)
**Test:** Decide fixed/deferred for the unsynchronized `rust_tail` deque access in `shutdown()`.
**Expected:** Recorded, not left silently `open`.
**Why human:** Policy call; low-probability race linked to the CR-01 cleanup concern.

#### 5. Triage IN-01 and IN-02 (new instances)
**Test:** Decide fixed/deferred for the handshake-key duplication (IN-01) and the dead/racy post-SIGKILL code (IN-02).
**Expected:** Each recorded, even if the decision is "accept as-is."
**Why human:** Policy call; neither defeats a Phase 1 must-have.

#### 6. Refresh 01-UAT.md's stale record
**Test:** Amend 01-UAT.md tests 8/9 (currently `result: issue`) to reflect that G-01-8/G-01-9 are closed, and note that the WR-01/IN-01 ids they reference are now superseded by distinct 2026-10-05 findings under the same ids.
**Expected:** The historical UAT record no longer reads as an open issue for work that is in fact closed.
**Why human:** Documentation-staleness note, not a code gap.

### Gaps Summary

No blocking gaps against Phase 1's must-haves. Compared with the prior verification (89/93, human_needed):

- Both gaps this plan (01-13) targeted are closed with real behavioral evidence: G-01-8 (shell-mode rejection now beats a missing-binary error, for every spelling) and G-01-9 (a missing prctl symbol degrades to polling instead of aborting the scheduler). I ran the new tests myself, in isolation, and they pass; the full gate grew from 84 to 88 passed pytest tests with no regressions.
- A fresh incremental code review (re-scoped to the 4 files 01-13 touched) found 4 new issues that happen to reuse the `CR-01`/`WR-01`/`IN-01`/`IN-02` ids. The disposition file's own documented id-reuse rule correctly reset those rows to `open` and preserved the dropped prior decisions in a notice — this is the file's process working as designed, not data loss. The most serious of the four, **CR-01 (critical)** — no top-level exception guard around the rust-mode process-spawn loop — contradicts the project's own D-12 process-cleanliness goal and is flagged as the top-priority item for human triage, unlike the Warning/Info items which previous verification passes have treated as routine, non-blocking policy calls.
- The remaining items are unchanged in kind from the prior pass: the GPU/Linux run (01-UAT.md tests 1 and 6, both still `blocked`) needs physical hardware, and review-finding triage (now including the 4 new 2026-10-05 findings) is a policy call. Neither is a code gap on the Mac.
- 01-UAT.md's tests 8 and 9 still show `result: issue` in their own record; the gaps they describe are now closed per this report's evidence, but the UAT document itself has not been refreshed. This is a documentation-staleness note, not a code gap (see Human Verification item 6).

---

_Verified: 2026-10-04T23:00:00Z_
_Verifier: Claude (gsd-verifier)_
