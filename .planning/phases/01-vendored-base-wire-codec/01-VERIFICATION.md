---
phase: 01-vendored-base-wire-codec
verified: 2026-10-04T08:00:00Z
status: human_needed
score: 58/62 must-haves verified
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
covered_digest: "v2:sha256:818d57c143fbeaa5a6873fe1d7221af4a6633db6ec6f3455f05ec31512de6222"
behavior_unverified: 3
overrides_applied: 0
re_verification:
  previous_status: human_needed
  previous_score: 45/49
  gaps_closed:
    - "G-01-2 / CR-01: group SIGINT to the rust-mode launcher exits 0 with no failure report (plan 01-07)"
    - "G-01-3 / WR-02: scheduler watchdog cannot miss an early-killed launcher (plan 01-08, Mac-verifiable part)"
    - "01-01 package-approval ordering: human-attested in 01-UAT.md test 5 (pass)"
    - "01-03 judgment-tier prohibition: human-confirmed in 01-UAT.md test 4 (pass)"
  gaps_remaining: []
  regressions: []
behavior_unverified_items:
  - truth: "ROADMAP SC2 (GPU half): on the GPU machine --frontend python serves a chat completion through the unmodified Python frontend, and --frontend rust starts the same real backend plus rsg-server"
    test: "On the Linux GPU box run `bash scripts/gpu_phase1_check.sh` (steps 1-3)"
    expected: "Step 2 PASS (non-empty chat completion content from python mode); step 3 PASS (rust mode reaches 'handshake received')"
    why_human: "Needs CUDA and the real upstream Scheduler; on the Mac only a FakeScheduler on upstream's real ZMQ queues was exercised"
  - truth: "ROADMAP SC3 (GPU half): the real backend reports max_seq_len, eos_token_id, page_size, max_running_req at readiness and rsg-server logs them; the Python frontend keeps working against the same backend code"
    test: "Same run, step 3 output line and step 5"
    expected: "'handshake received' line with upstream_sha=9a91cfa..., max_running_req=256, num_pages>1, max_seq_len in 1..40960, page_size 1 or 64, eos_token_id=151645 for Qwen3-0.6B; step 5 check_upstream.py PASS"
    why_human: "extract_handshake reads scheduler.engine.max_seq_len / engine.num_pages / cache_manager.page_size / eos_token_id; attribute names were checked against upstream source, but the real values only exist after a CUDA engine init"
  - truth: "01-08: on Linux start_parent_watchdog arms prctl(PR_SET_PDEATHSIG, SIGKILL) first (raises OSError if prctl fails), then does the getppid re-check"
    test: "On a Linux box run `.venv/bin/python -m pytest python/tests/test_parent_watchdog.py -k pdeathsig` and `bash scripts/gpu_phase1_check.sh` steps 4 and 4b"
    expected: "test_linux_arms_pdeathsig_sigkill passes (PR_GET_PDEATHSIG == 9); step 4b PASS (no rsg-server or scheduler left, and not listed by nvidia-smi, after kill -9 of the launcher right after 'spawned scheduler rank=0')"
    why_human: "The prctl branch is guarded by sys.platform.startswith('linux'); on this macOS machine that branch never executes and its unit test is skipped. The portable half (immediate getppid re-check plus polling thread) is proven by test and by mutation (below)"
human_verification:
  - test: "On the Linux GPU box run `bash scripts/gpu_phase1_check.sh` (after `uv venv --python=3.12 && uv pip install -e vendor/mini-sglang && uv pip install -e .`, build-essential present). UAT test 1 is currently `blocked` because no Linux GPU is available; run it on any GPU machine (a colleague or a rented box) from a pushed branch"
    expected: "ALL PASS: step 1 release build, step 2 python-mode chat completion, step 3 real handshake values, step 4 no orphan after kill -9 of the launcher, step 4b no orphan after an early kill -9 during scheduler boot, step 5 check_upstream.py"
    why_human: "ROADMAP criteria 2 and 3 and the Linux-only PR_SET_PDEATHSIG branch need CUDA, nvidia-smi and Linux"
  - test: "Run the Linux-only unit test: `pytest python/tests/test_parent_watchdog.py::test_linux_arms_pdeathsig_sigkill` on any Linux machine (no GPU needed)"
    expected: "1 passed (PR_GET_PDEATHSIG reports SIGKILL = 9)"
    why_human: "Skipped on macOS by design"
  - test: "Triage the five open warnings from the incremental review (WR-06 prctl failure is fatal and posts no error envelope; WR-07 gpu_pids/SIGPIPE can false-PASS the GPU orphan check; WR-08 0.5 s start_session race; WR-09 weak assertion in test_exits_at_once_...; WR-10 setpgid under wrappers) and record fixed/deferred in 01-REVIEW-DISPOSITION.md. Also decide WR-01 and WR-04 from the earlier review"
    expected: "Each is marked fixed or deferred with a target phase. WR-07 and WR-08 matter before the GPU run, since they can make the GPU script report a wrong verdict"
    why_human: "Policy call; none defeats a Phase 1 must-have (see Anti-Patterns)"
---

# Phase 1: Vendored Base & Wire Codec Verification Report

**Phase Goal:** The repo holds a pinned, attributed copy of mini-sglang. One launch command runs the shared backend with either frontend, and the backend reports a readiness handshake. The Rust msgpack codec is byte-exact with upstream for all 7 message types.
**Verified:** 2026-10-04T08:00:00Z
**Status:** human_needed
**Re-verification:** Yes. After gap closure plans 01-07 (G-01-2 / CR-01) and 01-08 (G-01-3 / WR-02).

## Goal Achievement

Everything that can run on the Mac holds, and both UAT gaps are closed in code and tests. I re-ran the phase gate and then mutation-checked the two new regression suites, so the green results are not just the tests agreeing with themselves. What remains is the GPU/Linux run that was always a human step, plus triage of five advisory review warnings. No must-have is FAILED.

### Re-verification of the two UAT gaps

**G-01-2 / CR-01 (group SIGINT exits 0): CLOSED.**
- Code: `python/rsglang/launch.py` re-checks `stop_requested` after every `ready_queue.get` in both loops (ready-wait lines ~238-243, supervise lines ~278-281). It also re-checks before each child-state-driven `shutdown(1)`: the children scan in both loops, and the handshake `BrokenPipeError` branch. The error-envelope branches run after the post-get check. `shutdown()` still sends its group SIGINT unchanged (WR-05 stays separate, as the plan required).
- Tests: `test_group_sigint_after_ready_exits_0`, `..._while_scheduler_boots_exits_0` and `..._while_scheduler_hangs_exits_0` each call `os.killpg(run.proc.pid, SIGINT)`. They assert exit 0, `rsglang.launch: exit code 0`, none of "exited with code" / "failed" / "lines of rsg-server stderr" / "escalating to SIGKILL", children gone and the five sockets removed.
- Mutation check (mine): I swapped in the pre-fix `launch.py` from commit c69a4f3, with only the `os.getpid()` spawn argument added so it still runs. All 3 group-SIGINT tests failed. I restored the file and confirmed byte equality with `diff`. The tests do discriminate the bug.
- Failure paths are not regressed: the crash-before-ready, ready-timeout, crash-after-ready and rsg-server-death tests still assert non-zero exit and a printed cause. All pass.

**G-01-3 / WR-02 (launcher killed early leaves no orphan): CLOSED for everything checkable on a Mac; the Linux prctl branch is human-verification.**
- Code: `launch.py` passes `os.getpid()` as the 4th `mp.Process` argument. `backend.run_scheduler(args, ready_queue, upstream_sha, launcher_pid)` calls `start_parent_watchdog(launcher_pid)` as its first statement. The watchdog arms PDEATHSIG on Linux, then does an immediate `os.getppid() != launcher_pid` check followed by `os._exit(1)`, then starts a polling thread that compares against `launcher_pid`. No getppid value is read late any more.
- Tests: `test_launcher_sigkill_during_scheduler_boot_leaves_no_orphans` widens the boot window with a test-only `sitecustomize`, kills the launcher pid right after "spawned scheduler rank=0", and asserts that both children disappear and no sockets remain. `test_parent_watchdog.py` has the wrong-parent-exits-1 test, the right-parent-stays-alive test and a Linux-only PDEATHSIG test (skipped here).
- Mutation check (mine): I changed the watchdog to read `os.getppid()` at call time again, which is the old bug. The early-kill e2e test failed with "scheduler pid=26687 orphaned", and the wrong-parent unit test failed with a timeout. I restored the file and confirmed byte equality. The window is real and the tests catch it. I also killed the stray scheduler this mutation left behind.
- Not provable here: the `prctl(PR_SET_PDEATHSIG)` branch. It is `sys.platform.startswith("linux")`-gated, and its test is skipped on macOS. This is the third behavior-unverified item, and the Linux GPU script has the matching step 4b.

### Observable Truths: ROADMAP Success Criteria (the contract)

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| SC1 | Repo contains mini-sglang @ 9a91cfa with MIT LICENSE + copyright; UPSTREAM.md names the commit and lists every modified vendored file | ✓ VERIFIED | `check_upstream.py --offline` in this run: "tree 02d3e4ad… matches pristine 9a91cfa, 0 listed modifications". `git status` shows no changes under `vendor/`. The prior online check cloned GitHub and compared 121 paths. |
| SC2 | On GPU, `--frontend python` serves a chat completion via the unmodified frontend; `--frontend rust` starts the same backend plus the Rust skeleton | ⚠️ PRESENT_BEHAVIOR_UNVERIFIED | Mac half verified: the rust-mode tracer e2e passed in this run. Python mode `execv`s `python -m minisgl` (7 tests in test_launch_args.py). GPU half needs `scripts/gpu_phase1_check.sh` steps 2-3. |
| SC3 | Backend reports max_seq_len, eos_token_id, page_size, max_running_req at readiness; Rust logs them; Python frontend unchanged against same backend code | ⚠️ PRESENT_BEHAVIOR_UNVERIFIED | `extract_handshake` reads the engine and cache_manager attributes (names checked against upstream source in the first verification); rsg-server logs all values. The vendored tree is pristine, so both modes run the same backend code. Real values need a CUDA engine init. |
| SC4 | For each of the 7 upstream message types, Rust codec bytes equal golden fixtures from upstream's Python encoder, checked on the Mac | ✓ VERIFIED | In this run's `cargo test --workspace`, rsg-wire passed 10 lib tests and 6 fixture tests. `gen_wire_fixtures.py --check` passed (fixtures regenerate byte-identically through the vendored `serialize_type`). |
| SC5 | Every message the Rust codec emits decodes through upstream's real Python decoder (cls(**kwargs)) | ✓ VERIFIED | `check_wire_decode.sh`: Rust dump test passed, then 37 pytest tests passed, including the extra-key negative control. |

### Observable Truths: PLAN must_haves

Plans 01-01 to 01-06 are unchanged since the first verification. I re-ran the gate that exercises them, so their statuses stand, with three updates marked below. The full per-truth evidence for them is in the first report; summary here:

| Plan | Truths | Status | Evidence |
|------|--------|--------|----------|
| 01-01 (vendoring, env) | 8 | ✓ 8 VERIFIED | The package-approval-ordering truth moved from ? UNCERTAIN to VERIFIED: the user confirmed it in 01-UAT.md test 5 (pass). Tree hash, LICENSE, UPSTREAM.md, .venv imports unchanged. |
| 01-02 (rsg-server) | 7 | ✓ 7 VERIFIED | `cargo test --workspace`: rsg-server 11 + 9 tests pass |
| 01-03 (launcher, e2e) | 7 | ✓ 7 VERIFIED | e2e and topology tests pass in the 65-passed pytest run |
| 01-04 (codec, fixtures) | 9 | ✓ 9 VERIFIED | fixtures suite; `--check` fresh |
| 01-05 (failure contract, GPU script) | 7 | ✓ 6 VERIFIED, ? 1 UNCERTAIN | "SIGKILL of the launcher leaves no orphan" is now verified without the old caveat (see 01-08). The truth that the GPU script is signed off by a human is still pending: UAT test 1 is `blocked` (no Linux GPU). |
| 01-06 (check_upstream, WIRE-02) | 11 | ✓ 11 VERIFIED | check_upstream tests, decode tests |
| 01-07 (CR-01, G-01-2) | 6 | ✓ 6 VERIFIED | Details below |
| 01-08 (WR-02, G-01-3) | 7 | ✓ 6 VERIFIED, ⚠️ 1 PRESENT_BEHAVIOR_UNVERIFIED | Details below |

**01-07 truths**

| Truth | Status | Evidence |
|-------|--------|----------|
| Group SIGINT after "handshake sent" exits 0 with a clean report | ✓ VERIFIED | `test_group_sigint_after_ready_exits_0` passed; fails against the pre-fix launcher (mutation) |
| Group SIGINT while the scheduler boots exits 0 (children-scan branch) | ✓ VERIFIED | `..._while_scheduler_boots_exits_0` passed; fails on mutation |
| Group SIGINT while the scheduler hangs pre-ready exits 0 (error-envelope branch) | ✓ VERIFIED | `..._while_scheduler_hangs_exits_0` passed; fails on mutation. `hang_entered` marker exists in fake_scheduler.py and is used by the test. |
| stop_requested re-checked after every get and before each child-state shutdown(1) (both loops, BrokenPipe branch) | ✓ VERIFIED | Read in launch.py (see above) |
| Real failures still reported; pid-only SIGTERM tracer still exits 0 | ✓ VERIFIED | The 4 failure tests and the tracer passed in the gate and in my 12-test re-run |
| 01-REVIEW-DISPOSITION.md records CR-01 fixed | ✓ VERIFIED | Row `CR-01 | critical | fixed` present |

**01-08 truths**

| Truth | Status | Evidence |
|-------|--------|----------|
| kill -9 of the launcher during scheduler boot leaves no scheduler | ✓ VERIFIED | `test_launcher_sigkill_during_scheduler_boot_leaves_no_orphans` passed; fails on mutation (orphaned) |
| Launcher passes `os.getpid()` to every rank; watchdog compares against it | ✓ VERIFIED | launch.py `args=(rank_args, ready_queue, upstream_sha, os.getpid())`; backend.py `_watch` compares `os.getppid() != launcher_pid` |
| `start_parent_watchdog(launcher_pid)` exits 1 at once on the wrong parent, stays alive on the right one | ✓ VERIFIED | Both unit tests passed; the wrong-parent test fails on mutation |
| Linux: PDEATHSIG armed first, OSError on prctl failure, then getppid re-check; polling stays | ⚠️ PRESENT_BEHAVIOR_UNVERIFIED | Code present and ordered correctly (backend.py). Branch is not executable on macOS and its test is skipped. Human item. |
| Post-handshake kill -9 test and all other e2e tests, including the group-SIGINT tests, still pass | ✓ VERIFIED | `test_launcher_sigkill_leaves_no_orphans` passed. e2e + watchdog files: 12 passed, 1 skipped. |
| `gpu_phase1_check.sh` has early-kill step 4b | ✓ VERIFIED (exists, substantive) | Lines 239-281: waits for 'spawned scheduler rank=0', refuses if "backend ready" already appeared, `kill -9` of the launcher, checks ps and nvidia-smi for both pids. Not executed (no GPU). |
| Disposition records WR-02 fixed | ✓ VERIFIED | Row `WR-02 | warning | fixed` present |

**Score:** 58/62 verified (5 roadmap + 57 plan truths). 3 are present but behavior-unverified (SC2, SC3, and the Linux PDEATHSIG branch). 1 is UNCERTAIN: the pending human sign-off of the GPU script (UAT test 1).

**Interpretation note (SC4, "7 message types"):** unchanged from the first verification. "7" is read as the 6 scheduler-boundary messages plus `SamplingParams`, with `Tensor` also covered. `TokenizeMsg`, `AbortMsg`, `UserReply` and `BatchFrontendMsg` are internal to the Python frontend that rsg-server replaces and never cross the wire from Rust.

### Prohibitions

| Plan | Prohibition | Tier | Disposition |
|------|-------------|------|-------------|
| 01-01 | Vendored LICENSE / copyright never removed or altered | test | ✓ VERIFIED. Enforced by the `LICENSE_MISSING` check and its test. |
| 01-06 | Tier A frozen frontend never modified | test | ✓ VERIFIED. Tier A tests pass; the offline check is clean. |
| 01-03 | Rust mode runs the byte-identical upstream Scheduler; the handshake is not produced by patching vendored code | judgment | Resolved by a human: UAT test 4 recorded `pass`. My own verdict agrees (default factory `minisgl.scheduler:Scheduler`, handshake read in `python/rsglang/backend.py`, vendored tree pristine). |

### Required Artifacts

| Artifact | Status | Details |
|----------|--------|---------|
| vendor/mini-sglang/ (+LICENSE), UPSTREAM.md, vendor/UPSTREAM_SHA | ✓ VERIFIED | pristine tree, SHA single-sourced |
| Cargo.toml, rust-toolchain.toml, crates/rsg-server/*, crates/rsg-wire/* | ✓ VERIFIED | cargo gate green |
| fixtures/wire/*.msgpack + manifest.json | ✓ VERIFIED | 34 cases, fresh |
| python/rsglang/{launch,backend,handshake,sockets}.py, testing/fake_scheduler.py | ✓ VERIFIED | substantive and wired; launch → backend.run_scheduler(…, os.getpid()) → start_parent_watchdog(launcher_pid) |
| python/tests/test_launch_rust_e2e.py, test_parent_watchdog.py | ✓ VERIFIED | new tests present, run, and fail under mutation |
| scripts/{gen_wire_fixtures,check_upstream}.py, check_wire_decode.sh, check_all.sh | ✓ VERIFIED | gate green |
| scripts/gpu_phase1_check.sh (incl. step 4b) | ✓ VERIFIED (exists, substantive) | not executed (no GPU) |

### Key Link Verification

| From | To | Via | Status |
|------|----|-----|--------|
| launch.py | backend.run_scheduler | `mp.Process(target=backend.run_scheduler, args=(rank_args, ready_queue, upstream_sha, os.getpid()))` | ✓ WIRED |
| backend.run_scheduler | start_parent_watchdog | first statement, `start_parent_watchdog(launcher_pid)` | ✓ WIRED |
| launch.py `_request_stop` | `shutdown(0)` | `stop_requested` re-checked after each get and before each child-driven `shutdown(1)` | ✓ WIRED |
| launch.py | rsg-server | `subprocess.Popen` + `encode_handshake_line` on stdin | ✓ WIRED |
| backend.py | upstream Scheduler | `DEFAULT_SCHEDULER_FACTORY = "minisgl.scheduler:Scheduler"` | ✓ WIRED |
| rsg-server | vendor/UPSTREAM_SHA | `include_str!` | ✓ WIRED |
| gen_wire_fixtures.py | upstream `serialize_type` | `msgpack.packb(serialize_type(obj), use_bin_type=True)` | ✓ WIRED |
| check_wire_decode.sh | tests/dump.rs → test_wire_decode.py | dump dir → pytest | ✓ WIRED |
| gpu_phase1_check.sh step 4b | `rsglang.launch --frontend rust` | `start_session`, polls log for 'spawned scheduler rank=0', `kill -9` | ✓ WIRED |

### Data-Flow Trace (Level 4)

| Artifact | Data | Source | Real data | Status |
|----------|------|--------|-----------|--------|
| rsg-server "handshake received" log | max_seq_len, eos, page_size, max_running_req, num_pages, sha | stdin ← launcher ← ready_queue ← `extract_handshake(scheduler, …)` | Mac: FakeScheduler constants; GPU: real engine attributes | ✓ FLOWING on Mac; GPU pending |
| Golden fixtures | msgpack bytes | vendored upstream `serialize_type` | yes (`--check` regenerates) | ✓ FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Phase gate, single full run | `bash scripts/check_all.sh --offline` | exit 0. Cargo: 11+9+10+1+6 passed. Pytest: 65 passed, 37 skipped (dump-dependent plus the Linux-only test; the dump-dependent ones run in step 4). Fixtures fresh. Decode: 37 passed. check_upstream: tree matches 9a91cfa | ✓ PASS |
| Gap-closure suites, re-run after mutation restore | `pytest python/tests/test_launch_rust_e2e.py python/tests/test_parent_watchdog.py` | 12 passed, 1 skipped (Linux PDEATHSIG) in 50 s | ✓ PASS |
| Mutation: pre-fix CR-01 launcher | the 3 group-SIGINT tests | 3 failed | ✓ tests discriminate |
| Mutation: old late-getppid watchdog | early-kill e2e + wrong-parent unit test | both failed (orphaned; timeout) | ✓ tests discriminate |
| Restoration check | `diff` of launch.py and backend.py against saved copies; `git status` for vendor, python, scripts, crates | identical; clean | ✓ no stray edits |
| GPU / Linux checks | `scripts/gpu_phase1_check.sh`, PDEATHSIG unit test | not run (macOS, no CUDA) | ? SKIP → human |

### Probe Execution

No `scripts/*/tests/probe-*.sh` exists and no plan declares one. Step 7c: N/A.

### Requirements Coverage

| Requirement | Source Plan(s) | Description | Status | Evidence |
|-------------|----------------|-------------|--------|----------|
| BASE-01 | 01-01, 01-06 | vendored @ 9a91cfa with LICENSE; UPSTREAM.md records commit and modified files | ✓ SATISFIED | SC1 |
| BASE-02 | 01-02, 01-03, 01-05, 01-07, 01-08 | one launch command for `--frontend python` / `--frontend rust` | ✓ SATISFIED on Mac / ? NEEDS HUMAN on GPU | launcher, tests, closed G-01-2 and G-01-3; GPU steps 2-3, 4, 4b |
| BASE-03 | 01-02, 01-03, 01-05 | backend readiness handshake (4 values); both frontends use same backend code | ✓ SATISFIED on Mac / ? NEEDS HUMAN on GPU | e2e handshake test; GPU step 3 |
| WIRE-01 | 01-04, 01-06 | byte-identical codec for all 7 types via golden fixtures | ✓ SATISFIED | SC4 |
| WIRE-02 | 01-06 | every Rust message decodes through the real Python decoder | ✓ SATISFIED | SC5 |

No orphaned requirements: REQUIREMENTS.md maps exactly BASE-01/02/03 and WIRE-01/02 to Phase 1 (WIRE-03 and others belong to later phases), and every ID is claimed by a plan. REQUIREMENTS.md already ticks BASE-02/03 as Complete although their GPU halves are unrun. That is a tracking optimism, not a code gap.

### Anti-Patterns Found

No TBD/FIXME/XXX debt markers in `python/rsglang`, `python/tests` or `scripts/gpu_phase1_check.sh` (the only grep hit is the `mktemp …XXXXXX` template). No TODO/HACK, `todo!()` or `unimplemented!()` in phase sources.

The incremental review (01-REVIEW.md: 0 critical, 5 warning, 4 info) is advisory. None of it makes a must-have false:

| Finding | File | Severity here | Defeats a must-have? |
|---------|------|---------------|----------------------|
| WR-06 prctl failure is fatal and posts no error envelope | backend.py:77-80, 100 | ⚠️ Warning | No. The 01-08 truth literally says the watchdog "raises OSError if prctl fails". It is a robustness choice only a seccomp-restricted Linux container would hit. |
| WR-07 `gpu_pids` errors swallowed and SIGPIPE under pipefail can false-PASS the GPU orphan check | gpu_phase1_check.sh | ⚠️ Warning | No on the Mac. It can make step 4/4b report a wrong verdict on the GPU box, so fix it before the GPU run. |
| WR-08 `start_session` fixed 0.5 s sleep can fail a healthy run | gpu_phase1_check.sh:85-92 | ⚠️ Warning | No. A false FAIL, not a false PASS. |
| WR-09 wrong-parent unit test asserts only exit 1 | test_parent_watchdog.py:27-39 | ⚠️ Warning | No. My mutation shows it does fail on the old behavior; an import failure would pass it vacuously. |
| WR-10 / earlier WR-03 `setpgid(0,0)` detaches the launcher from the terminal foreground group under wrappers | launch.py | ⚠️ Warning | No. Tests and the GPU script start the launcher as its own group leader. A terminal Ctrl-C under `uv run` may not reach it. |
| Earlier WR-01 (`--shell` abbreviation), WR-04 (missing eos key accepted), WR-05 (shutdown re-SIGINTs the group) | launch.py, handshake.rs | ⚠️ Warning | No in Phase 1 (unchanged from the first report) |
| IN-10..IN-13 and IN-01..IN-09 | various | ℹ️ Info | No. IN-13 (sitecustomize might not widen the window) is answered by my mutation run: the early-kill test fails on the old code, so the window is real. |
| Residual window in CR-01 fix | launch.py | ℹ️ Info | A signal landing between the last `stop_requested` read and `shutdown(1)` is a microsecond window; the plan accepted it. |
| Known 1-in-~110 unexplained e2e escalation | deferred-items.md | ℹ️ Info | Not seen in this run (two full e2e passes plus the gate). |

01-REVIEW-DISPOSITION.md shows CR-01 and WR-02 as `fixed` and 22 other rows `open`. Nothing there blocks the phase.

### Human Verification Required

#### 1. GPU end-of-phase check (ROADMAP SC2 and SC3, plus the Linux PDEATHSIG branch)
**Test:** On a Linux GPU box run `bash scripts/gpu_phase1_check.sh` after the documented setup. You asked in UAT test 1 what to do without a Linux GPU. The planned answer is to commit and push the branch, then have someone with a GPU (or a rented box) run it. Per this report the Mac work is finished, so that handoff is a reasonable next step.
**Expected:** ALL PASS across steps 1, 2, 3, 4, 4b and 5. Handshake line: sha 9a91cfa…, max_running_req=256, num_pages>1, max_seq_len in 1..40960, page_size 1 or 64, eos 151645.
**Why human:** Needs CUDA, nvidia-smi and Linux.

#### 2. Linux-only watchdog unit test
**Test:** `pytest python/tests/test_parent_watchdog.py::test_linux_arms_pdeathsig_sigkill` on any Linux machine (no GPU needed).
**Expected:** 1 passed.
**Why human:** Skipped on macOS by design.

#### 3. Review-warning triage
**Test:** Record fixed or deferred for WR-06..WR-10 (and WR-01, WR-04) in 01-REVIEW-DISPOSITION.md. Do WR-07 and WR-08 first, because they change how trustworthy the GPU script's verdict is.
**Expected:** No `open` warning without a decision.
**Why human:** Policy call; none defeats a Phase 1 must-have.

### Gaps Summary

No blocking gaps. Compared with the first verification:
- CR-01 (group SIGINT exit 1) is fixed and covered by 3 e2e tests that fail on the pre-fix launcher.
- WR-02 (early-killed launcher leaves a scheduler orphan) is fixed on the portable path and covered by an e2e test and unit tests that fail on the old watchdog. The Linux PDEATHSIG layer is present but only a Linux run can prove it.
- Human items 4 and 5 from the first report (judgment prohibition, package-approval ordering) are now human-confirmed in UAT.
- The remaining items are the GPU/Linux run and warning triage. Neither is a code gap on the Mac.

01-UAT.md still says `diagnosed` with tests 2 and 3 as `issue`. It should be updated to resolved, and test 1 stays `blocked` until a Linux GPU run happens.

---

_Verified: 2026-10-04T08:00:00Z_
_Verifier: Claude (gsd-verifier)_
