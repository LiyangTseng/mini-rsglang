---
phase: 01-vendored-base-wire-codec
verified: 2026-10-04T09:30:00Z
status: human_needed
score: 89/93 must-haves verified
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
  - .planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md
  - .planning/phases/01-vendored-base-wire-codec/01-REVIEW.md
  - .planning/phases/01-vendored-base-wire-codec/01-UAT.md
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
covered_digest: "v2:sha256:13f24cba5a8a6954a61c1bc3b235d86de3f12785dd1ea26610919bf94fa826c7"
behavior_unverified: 3
overrides_applied: 0
re_verification:
  previous_status: human_needed
  previous_score: 58/62
  gaps_closed:
    - "G-01-7-WR07 (false PASS of the GPU-orphan check on an nvidia-smi error or long listing) — plan 01-09"
    - "G-01-7-WR08 (start_session's fixed 0.5s wait could false-FAIL a healthy run and leak a session) — plan 01-09"
    - "G-01-7-WR06 (a failing prctl aborted the scheduler with no error envelope) — plan 01-10"
    - "G-01-7-WR09 (the watchdog exit test could pass vacuously on any exit 1) — plan 01-10"
    - "G-01-7-WR04 (a handshake line missing eos_token_id was silently accepted as null) — plan 01-11"
    - "G-01-7-WR01 (abbreviated --shell/--shell-m bypassed the shell-mode guard) — plan 01-11"
    - "G-01-7-WR06-CHECK (a GPU step-3 PASS no longer proved PDEATHSIG armed after the WR-06 degrade) — plan 01-12"
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
    test: "On a Linux box run `.venv/bin/python -m pytest python/tests/test_parent_watchdog.py -k pdeathsig_sigkill` and, on the GPU box, confirm `scripts/gpu_phase1_check.sh` step 3 PASSes (which now also fails if the log contains 'PDEATHSIG unavailable', per 01-12)"
    expected: "test_linux_arms_pdeathsig_sigkill passes (PR_GET_PDEATHSIG == 9); GPU step 3 PASS with no degrade line in rust-mode.log"
    why_human: "The success branch is guarded by sys.platform.startswith('linux'); on this macOS machine it never executes and its unit test is skipped. The failure/degrade branch (01-10) and the step-3 check for it (01-12) are both proven on the Mac with stubs/fixtures"
human_verification:
  - test: "On the Linux GPU box run `bash scripts/gpu_phase1_check.sh` (after `uv venv --python=3.12 && uv pip install -e vendor/mini-sglang && uv pip install -e .`, build-essential present)."
    expected: "ALL PASS across steps 1, 2, 3 (incl. the new PDEATHSIG-armed check), 4, 4b and 5. Handshake line: sha 9a91cfa…, max_running_req=256, num_pages>1, max_seq_len in 1..40960, page_size 1 or 64, eos 151645. Step 3/4/4b can no longer false-PASS (WR-07) or false-FAIL (WR-08) per 01-09, and step 3 fails if PDEATHSIG degraded (01-12)."
    why_human: "ROADMAP criteria 2 and 3 and the Linux-only PR_SET_PDEATHSIG success path need CUDA, nvidia-smi and Linux. UAT test 1 is `blocked` in 01-UAT.md (no Linux GPU available yet)."
  - test: "Run the Linux-only unit test: `pytest python/tests/test_parent_watchdog.py::test_linux_arms_pdeathsig_sigkill` on any Linux machine (no GPU needed)."
    expected: "1 passed (PR_GET_PDEATHSIG reports SIGKILL = 9)."
    why_human: "Skipped on macOS by design. UAT test 6 is `blocked` in 01-UAT.md for the same reason."
  - test: "Triage the newly-surfaced WR-01 finding in the latest 01-REVIEW.md (reusing the WR-01 id, so 01-REVIEW-DISPOSITION.md's gate reset its disposition from `fixed` back to `open`, per that file's documented id-reuse rule): in `python/rsglang/launch.py`, `run_rust_mode` resolves `rust_bin` (and can return 2 with 'rsg-server binary not found') BEFORE `_run_rust_mode`'s authoritative `run_shell` check runs, so `--shell-m` with no binary built yet reports the wrong root cause. Decide fixed-now (reorder: parse_args/run_shell check before resolve_rust_bin) or deferred, and record it in 01-REVIEW-DISPOSITION.md."
    expected: "WR-01 (current instance) is not left permanently `open` without a decision; the eventual choice is recorded with a target or a rationale."
    why_human: "Policy call. This is a NEW, narrower finding than the original WR-01 (which IS fixed and tested: the abbreviation-catching logic itself is correct and `--shell`/`--shell-m` always exit 2, never silently run with shell-mode limits). It is a misleading-diagnostic issue only, not a correctness regression, so it does not defeat any Phase 1 must-have — but it is untriaged and should not be silently dropped."
  - test: "Record fixed/deferred for the carried-forward info-level findings IN-01, IN-02, IN-03 (new, from the latest incremental review) alongside the still-open IN-04..IN-13 and the still-deferred WR-03/WR-05/WR-10."
    expected: "Each is marked fixed or deferred with a target phase, or explicitly accepted as non-blocking."
    why_human: "Policy call; none defeats a Phase 1 must-have (see Anti-Patterns)."
---

# Phase 1: Vendored Base & Wire Codec Verification Report

**Phase Goal:** The repo holds a pinned, attributed copy of mini-sglang. One launch command runs the shared backend with either frontend, and the backend reports a readiness handshake. The Rust msgpack codec is byte-exact with upstream for all 7 message types.
**Verified:** 2026-10-04T09:30:00Z
**Status:** human_needed
**Re-verification:** Yes. After gap closure plans 01-09 (G-01-7-WR07, WR08), 01-10 (G-01-7-WR06, WR09), 01-11 (G-01-7-WR04, WR01), and 01-12 (G-01-7-WR06-CHECK) — the last 4 of the `--gaps-only` chain that began with 01-07/01-08.

## Goal Achievement

All seven gaps from UAT test 7's triage (WR-01, WR-04, WR-06, WR-07, WR-08, WR-09, plus the follow-up WR-06-CHECK) are closed in code and proven by tests that were observed failing before the fix (RED) and passing after (GREEN), several with explicit mutation checks. I re-ran the full Mac gate myself and independently re-derived each claim from the current source rather than trusting the SUMMARYs' narration. The full gate is green: `cargo test --workspace` (27 Rust tests across rsg-wire and rsg-server), 84 pytest tests passed (37 skipped — GPU/Linux-only), fixture freshness, WIRE-02 decode (37 passed), and `check_upstream.py --offline` (vendored tree still pristine against 9a91cfa).

One new, narrower finding surfaced by the latest incremental code review reused the `WR-01` id in `01-REVIEW-DISPOSITION.md`, which per that file's own documented rule reset its disposition from `fixed` back to `open`. I verified this is NOT a regression of the original WR-01 fix (confirmed: the abbreviation-catching `run_shell` check is present, tested, and `--shell`/`--shell-m` always exit 2 — the file-level `grep` and the passing `test_rust_mode_rejects_abbreviated_shell_mode_without_spawning` tests both still hold). It is a new, distinct, minor diagnostic-ordering issue (a missing `rsg-server` binary masks the shell-mode rejection message with a misleading "binary not found" error, though the exit code is still 2 either way). This does not defeat any Phase 1 must-have, but it is untriaged — surfaced as a human-verification item below, not silently dropped.

### Re-verification of the four remaining gap-closure plans

**01-09 (G-01-7-WR07 / WR-08): CLOSED.**
- `gpu_pids()` no longer discards nvidia-smi's stderr (`scripts/gpu_phase1_check.sh:108`). `on_gpu()` (line 110+) captures its output once into a local and returns 0/1/2 (listed/not-listed/nvidia-smi-failed) instead of piping into `grep -qx`, which eliminates the old SIGPIPE-driven false-absent under `pipefail` on a long listing. `wait_no_orphans()` (line 121+) fails loudly on code 2. `step4`/`step4_early` now call only `wait_no_orphans 30 ...` / `wait_no_orphans 120 ...` — I grepped and confirmed no remaining piped `gpu_pids | grep` form anywhere in the file.
- `start_session()` (line 78+) replaced the fixed `sleep 0.5` with a 50-round, 0.1s poll of `ps -o pgid=`, and on timeout it `kill -9`s and `wait`s the single pid it started (never a process group) before returning 1.
- Tests: `.venv/bin/python -m pytest python/tests/test_gpu_check_script.py -q` → 14 passed (I ran this myself), including all 11 of 01-09's on_gpu/wait_no_orphans/start_session cases plus 01-12's 3 pdeathsig_degraded cases.
- `01-REVIEW-DISPOSITION.md` records WR-07 and WR-08 as `fixed` (confirmed by direct read).

**01-10 (G-01-7-WR06 / WR-09): CLOSED.**
- `start_parent_watchdog`'s Linux branch (`python/rsglang/backend.py:79-94`) wraps the CDLL load, `libc.prctl.argtypes` (5 entries: c_int + 4×c_ulong, confirmed by direct read), and the `prctl` call in one `try`/`except OSError`, printing `rsglang: PDEATHSIG unavailable (...); using the polling watchdog only` and falling through to the getppid re-check and polling thread, rather than raising.
- `run_scheduler` (line 112+) now calls `start_parent_watchdog(launcher_pid)` as the first statement inside its error-envelope `try` — I read the exact code and confirmed the ordering and comment.
- Tests: `.venv/bin/python -m pytest python/tests/test_parent_watchdog.py -q -rs` → 19 combined with test_gpu_check_script.py earlier, individually 5 passed + 1 skipped (Linux-only), matching the plan's stated expectation.
- `01-REVIEW-DISPOSITION.md` records WR-06 and WR-09 as `fixed` (confirmed).

**01-11 (G-01-7-WR04 / WR-01): CLOSED for the stated scope; a narrower, new issue was found by the next review round (see above).**
- `crates/rsg-server/src/handshake.rs:28` carries `#[serde(deserialize_with = "Option::deserialize")]` on `eos_token_id: Option<u64>`, which makes the key required while an explicit `null` still parses to `None` (confirmed by direct read and by `cargo test -p rsg-server` passing, including `missing_eos_key_is_malformed` and the CLI-level `missing_eos_key_exits_2`).
- `python/rsglang/launch.py:123-126`: `server_args, run_shell = parse_args(rest); if run_shell: ... return 2` — confirmed present, before any side effect inside `_run_rust_mode` (socket unlink, setpgid, signal handlers, spawns all come after). The literal `"--shell-mode" in rest` pre-check (line 101) is also still present, as the plan intended.
- `01-REVIEW-DISPOSITION.md`'s table currently shows WR-04 `fixed` and WR-01 `open` — the WR-04 disposition is intact; WR-01's frontmatter/table flipped back to `open` because the latest incremental review (`01-REVIEW.md`, reviewed 2026-10-04T00:00:00Z) found a different bug in the same code area and reused the `WR-01` id, which per the disposition file's own stated rule ("when a finding id is REUSED by a different finding, the earlier decision cannot keep a row ... it is dropped") discards the prior `fixed` decision. I verified this is accurate: the original defect (abbreviations not caught at all) is fixed and tested; the new defect (ordering: `resolve_rust_bin` runs before the authoritative `run_shell` check in `run_rust_mode`, so a missing binary masks the rejection message) is real, confirmed by reading `python/rsglang/launch.py:100-113` directly. Both exit code 2 either way — no silent shell-mode execution in either case.

**01-12 (G-01-7-WR06-CHECK): CLOSED.**
- `pdeathsig_degraded() { grep -qF 'PDEATHSIG unavailable' "$1" 2>/dev/null && return 0; return 1; }` is defined above the source guard (`scripts/gpu_phase1_check.sh:119`, confirmed by direct read).
- `step3` (line 245+) calls it right after the handshake line and pids are read, and fails with "scheduler fell back to the polling watchdog: PDEATHSIG unavailable (see $log)" when it returns 0 (confirmed, line 258).
- `--help`'s step 3 line now reads "...and the scheduler armed PDEATHSIG (no 'PDEATHSIG unavailable' in the log)" (confirmed by running `--help` myself).
- Tests: the 3 new `test_pdeathsig_degraded_*` cases are part of the 14-passed run above.

### Observable Truths: ROADMAP Success Criteria (the contract)

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| SC1 | Repo contains mini-sglang @ 9a91cfa with MIT LICENSE + copyright; UPSTREAM.md names the commit and lists every modified vendored file | ✓ VERIFIED | `check_upstream.py --offline` (this run): "tree 02d3e4ad… matches pristine 9a91cfa, 0 listed modifications". `git status --porcelain vendor/` empty. |
| SC2 | On GPU, `--frontend python` serves a chat completion via the unmodified frontend; `--frontend rust` starts the same backend plus the Rust skeleton | ⚠️ PRESENT_BEHAVIOR_UNVERIFIED | Mac half verified (e2e tests pass, 14 gpu-check-script tests pass against stubs). GPU half needs `scripts/gpu_phase1_check.sh` steps 2-3 on Linux, still `blocked` per 01-UAT.md test 1. |
| SC3 | Backend reports max_seq_len, eos_token_id, page_size, max_running_req at readiness; Rust logs them; Python frontend unchanged against same backend code | ⚠️ PRESENT_BEHAVIOR_UNVERIFIED | `extract_handshake` attribute names checked against upstream source (prior verification); rsg-server logs all values and now requires `eos_token_id` strictly (01-11). Real values need a CUDA engine init. |
| SC4 | For each of the 7 upstream message types, Rust codec bytes equal golden fixtures from upstream's Python encoder, checked on the Mac | ✓ VERIFIED | `cargo test --workspace` (this run): rsg-wire 10 lib + 1 dump + 6 fixture tests pass. `gen_wire_fixtures.py --check` passed (34 cases, byte-identical). |
| SC5 | Every message the Rust codec emits decodes through upstream's real Python decoder (cls(**kwargs)) | ✓ VERIFIED | `check_wire_decode.sh` (this run): Rust dump test passed, then 37 pytest tests passed. |

### Observable Truths: PLAN must_haves

Plans 01-01 to 01-08 are unchanged in source since the last verification (confirmed: `git status --porcelain` shows no changes to their files beyond what 01-09..01-12 touched, and the full gate still passes). I carried forward their per-truth statuses with a regression check (the full gate run above), except where 01-10 explicitly supersedes a prior behavior (noted below).

| Plan | Truths | Status | Evidence |
|------|--------|--------|----------|
| 01-01 (vendoring, env) | 8 | ✓ 8 VERIFIED | Unchanged; tree hash, LICENSE, UPSTREAM.md all intact. |
| 01-02 (rsg-server) | 7 | ✓ 7 VERIFIED | `cargo test --workspace` green. |
| 01-03 (launcher, e2e) | 7 | ✓ 7 VERIFIED | e2e and topology tests pass in this run's pytest. |
| 01-04 (codec, fixtures) | 9 | ✓ 9 VERIFIED | fixtures suite; `--check` fresh. |
| 01-05 (failure contract, GPU script) | 7 | ✓ 6 VERIFIED, ? 1 UNCERTAIN | The GPU-script human sign-off truth is still pending (01-UAT.md test 1 `blocked`); the underlying script logic is now hardened further by 01-09/01-12. |
| 01-06 (check_upstream, WIRE-02) | 11 | ✓ 11 VERIFIED | check_upstream and decode tests pass. |
| 01-07 (CR-01, G-01-2) | 6 | ✓ 6 VERIFIED | Group-SIGINT tests still pass; unaffected by 01-09..01-12. |
| 01-08 (WR-02, G-01-3) | 7 | ✓ 6 VERIFIED, ⚠️ 1 PRESENT_BEHAVIOR_UNVERIFIED | The portable (macOS+Linux) early-kill/watchdog-pid truths are verified. The Linux-only "PDEATHSIG armed" truth is refined by 01-10 (a prctl failure now degrades-and-logs instead of raising) but the underlying success-path claim (prctl succeeding actually arms SIGKILL) is still only provable on Linux; status unchanged from before. |
| 01-09 (WR-07, WR-08 gap closure) | 8 | ✓ 8 VERIFIED | Details below. |
| 01-10 (WR-06, WR-09 gap closure) | 8 | ✓ 8 VERIFIED | Details below. |
| 01-11 (WR-04, WR-01 gap closure) | 6 | ✓ 6 VERIFIED | Details below. The code and tests fully satisfy this plan's own stated must-haves; the newly-discovered ordering issue is outside this plan's must-have scope (see Anti-Patterns). |
| 01-12 (WR-06-CHECK gap closure) | 4 | ✓ 4 VERIFIED | Details below. |

**01-09 truths**

| Truth | Status | Evidence |
|-------|--------|----------|
| nvidia-smi failure during the orphan check is a step failure | ✓ VERIFIED | `on_gpu` returns 2 on failure; `wait_no_orphans` fails loudly; `test_wait_no_orphans_fails_when_nvidia_smi_fails` passes |
| A pid nvidia-smi still lists is never reported absent, regardless of listing length | ✓ VERIFIED | `on_gpu` captures output once, greps the captured text; `test_on_gpu_detects_pid_in_long_listing` passes |
| Steps 4/4b judge orphans only through `wait_no_orphans`; no piped `gpu_pids`-into-grep remains | ✓ VERIFIED | `grep gpu_pids scripts/gpu_phase1_check.sh` shows only the definition and one capture-then-check use, no pipe-into-grep |
| `start_session` polls every 0.1s up to 5s | ✓ VERIFIED | Code read: `for round in $(seq 1 50); do ... sleep 0.1; done` |
| On timeout, prints the message, kills+reaps the pid only (never a group), returns 1 | ✓ VERIFIED | Code read confirms `kill -9 "$BG_PID"` then `wait "$BG_PID"`, with a comment explaining why no group signal |
| An already-exited process is left to the caller's check (`start_session` returns 0) | ✓ VERIFIED | Code read: empty pgid → `return 0`; `test_start_session_leaves_an_exited_process_to_the_caller` passes |
| Source guard: nothing below it runs when sourced; `bash -n`/`--help` unaffected | ✓ VERIFIED | `bash -n scripts/gpu_phase1_check.sh` exits 0; `--help` prints Usage; guard present at line 162 |
| `01-REVIEW-DISPOSITION.md` records WR-07/WR-08 fixed | ✓ VERIFIED | Direct read: both rows `fixed` |

**01-10 truths**

| Truth | Status | Evidence |
|-------|--------|----------|
| A failing prctl does not kill the scheduler; logs and degrades to polling | ✓ VERIFIED | Code read: `try`/`except OSError` around CDLL+argtypes+prctl, prints "PDEATHSIG unavailable", falls through |
| Same degradation when `ctypes.CDLL` itself raises `OSError` | ✓ VERIFIED | Same `except OSError` catches both; `test_prctl_failure_degrades_to_polling[cdll-fails]` passes |
| `libc.prctl.argtypes` declared as 5 args before the call | ✓ VERIFIED | Code read: `[c_int, c_ulong, c_ulong, c_ulong, c_ulong]` |
| Watchdog startup failures reach the launcher as `{kind: error}` with traceback | ✓ VERIFIED | `start_parent_watchdog(launcher_pid)` is the first statement inside `run_scheduler`'s existing error-envelope `try`; `test_watchdog_startup_failure_reaches_launcher_as_error_envelope` passes |
| `test_exits_at_once_when_parent_is_not_the_launcher` proves the watchdog caused the exit | ✓ VERIFIED | Hardened with "calling"/no-"returned"/no-Traceback assertions; test passes |
| Mutations (misspelled import; raising watchdog) make the hardened test fail | ✓ VERIFIED (per SUMMARY's recorded mutation runs; not independently re-run by me, consistent with re-verification's regression-check tier for gap-closure evidence already backed by RED/GREEN commit pairs) | `01-10-SUMMARY.md` records both mutation runs failing as expected, with pytest summary lines |
| Every e2e test, including 01-07/01-08's, still passes | ✓ VERIFIED | Full gate (84 pytest passed) includes these files |
| `01-REVIEW-DISPOSITION.md` records WR-06/WR-09 fixed | ✓ VERIFIED | Direct read: both rows `fixed` |

**01-11 truths**

| Truth | Status | Evidence |
|-------|--------|----------|
| rsg-server rejects a handshake line missing `eos_token_id` as Malformed, exit 2 | ✓ VERIFIED | `cargo test -p rsg-server` passes `missing_eos_key_is_malformed` and `missing_eos_key_exits_2` |
| An explicit `"eos_token_id":null` still parses to `None`; every other handshake test still passes | ✓ VERIFIED | `cargo test -p rsg-server` green (no regressions) |
| rust mode rejects `--shell`/`--shell-m` via the authoritative `run_shell` flag | ✓ VERIFIED | Code read: `server_args, run_shell = parse_args(rest); if run_shell: ... return 2`; `test_rust_mode_rejects_abbreviated_shell_mode_without_spawning` passes |
| That rejection happens before socket cleanup, setpgid, signal handlers, and any spawn | ✓ VERIFIED | Code read: the check is the first statement after `parse_args` inside `_run_rust_mode`, ahead of every later side effect in that function |
| The literal pre-check and python-mode passthrough are unchanged | ✓ VERIFIED | `"--shell-mode" in rest` still present at line 101; python-mode tests pass |
| `01-REVIEW-DISPOSITION.md` records WR-01/WR-04 as fixed | ⚠️ Superseded by a later review cycle for WR-01 only (not a failure of this plan) | WR-04's row is still `fixed`. WR-01's row is `open` because the next incremental review round found a distinct issue and reused the id, which the disposition file's own stated rule resets to `open` on id reuse. This plan's own acceptance criteria (checked at execution time) did pass; the state has since moved on, as the file's process explicitly allows. See Anti-Patterns and Human Verification. |

**01-12 truths**

| Truth | Status | Evidence |
|-------|--------|----------|
| Step 3 fails when the log contains "PDEATHSIG unavailable" | ✓ VERIFIED | Code read: `if pdeathsig_degraded "$log"; then ...; return 1; fi` at line 258 |
| `pdeathsig_degraded <log>` helper above the source guard, testable on the Mac | ✓ VERIFIED | Defined at line 119; 3 new tests pass |
| 01-10's runtime degrade behavior is unchanged | ✓ VERIFIED | `python/rsglang/backend.py` untouched by this plan (confirmed: 01-12's `files_modified` lists only the script and its test file) |
| `--help`'s step 3 line mentions PDEATHSIG | ✓ VERIFIED | Ran `--help` myself: "...and the scheduler armed PDEATHSIG (no 'PDEATHSIG unavailable' in the log)" |

**Score:** 89/93 verified (3 roadmap + 86 plan truths). 3 are present but behavior-unverified (SC2, SC3, and the Linux PDEATHSIG-armed success path). 1 is UNCERTAIN (the pending human sign-off of the GPU script, 01-UAT.md test 1, `blocked`).

**Interpretation note (SC4, "7 message types"):** unchanged from the first verification — read as the 6 scheduler-boundary messages plus `SamplingParams`, with `Tensor` also covered.

### Prohibitions

| Plan | Prohibition | Tier | Disposition |
|------|-------------|------|-------------|
| 01-01 | Vendored LICENSE / copyright never removed or altered | test | ✓ VERIFIED. `LICENSE_MISSING` check and its test still pass. |
| 01-06 | Tier A frozen frontend never modified | test | ✓ VERIFIED. `check_upstream.py --offline` clean (0 modifications). |
| 01-03 | Rust mode runs the byte-identical upstream Scheduler; the handshake is not produced by patching vendored code | judgment | Resolved by a human: 01-UAT.md test 4 recorded `pass`. My own re-check agrees (default factory `minisgl.scheduler:Scheduler`, vendored tree pristine, handshake extraction lives in `python/rsglang/backend.py` only). |

### Required Artifacts

| Artifact | Status | Details |
|----------|--------|---------|
| vendor/mini-sglang/ (+LICENSE), UPSTREAM.md, vendor/UPSTREAM_SHA | ✓ VERIFIED | pristine tree, SHA single-sourced |
| Cargo.toml, rust-toolchain.toml, crates/rsg-server/*, crates/rsg-wire/* | ✓ VERIFIED | cargo gate green (27 tests) |
| fixtures/wire/*.msgpack + manifest.json | ✓ VERIFIED | 34 cases, fresh |
| python/rsglang/{launch,backend,handshake,sockets}.py, testing/fake_scheduler.py | ✓ VERIFIED | substantive and wired; watchdog degrade-and-log, run_shell guard, eos_token_id-independent of Python change all confirmed |
| crates/rsg-server/src/handshake.rs | ✓ VERIFIED | `deserialize_with = "Option::deserialize"` on eos_token_id, confirmed |
| scripts/gpu_phase1_check.sh (incl. on_gpu, wait_no_orphans, pdeathsig_degraded, polling start_session) | ✓ VERIFIED (exists, substantive, Mac-tested) | not executed end-to-end on GPU (no GPU) |
| python/tests/test_gpu_check_script.py | ✓ VERIFIED | 14 tests, all pass |
| python/tests/test_parent_watchdog.py | ✓ VERIFIED | 5 passed, 1 skipped (Linux-only) |
| python/tests/test_launch_args.py | ✓ VERIFIED | abbreviated shell-mode test present and passing |
| .planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md | ✓ VERIFIED (exists, substantive) | WR-06/07/08/09/04 `fixed`; WR-01 `open` again (id-reuse reset, see above); WR-03/05/10 `deferred`; the rest `open` |

### Key Link Verification

| From | To | Via | Status |
|------|----|-----|--------|
| scripts/gpu_phase1_check.sh step4 / step4_early | wait_no_orphans → on_gpu → gpu_pids | `wait_no_orphans 30/120 "$pid"...` calls, confirmed at lines 288 and 322 | ✓ WIRED |
| scripts/gpu_phase1_check.sh step3 | pdeathsig_degraded | `if pdeathsig_degraded "$log"; then ...; fi`, confirmed at line 258 | ✓ WIRED |
| python/rsglang/backend.py run_scheduler try | start_parent_watchdog(launcher_pid) | first statement inside the try, confirmed at line 121 | ✓ WIRED |
| python/rsglang/launch.py _run_rust_mode | the parsed `run_shell` guard | `server_args, run_shell = parse_args(rest); if run_shell: ... return 2`, confirmed at lines 123-126, before any side effect in that function | ✓ WIRED |
| python/rsglang/handshake.py encode_handshake_line | crates/rsg-server/src/handshake.rs parse_handshake | always writes eos_token_id (Python side unchanged); Rust side requires it via deserialize_with | ✓ WIRED |

### Data-Flow Trace (Level 4)

| Artifact | Data | Source | Real data | Status |
|----------|------|--------|-----------|--------|
| rsg-server "handshake received" log | max_seq_len, eos, page_size, max_running_req, num_pages, sha | stdin ← launcher ← ready_queue ← `extract_handshake(scheduler, …)` | Mac: FakeScheduler constants; GPU: real engine attributes | ✓ FLOWING on Mac; GPU pending |
| Golden fixtures | msgpack bytes | vendored upstream `serialize_type` | yes (`--check` regenerates) | ✓ FLOWING |
| gpu_phase1_check.sh step3's PDEATHSIG check | rust-mode.log text | start_session's stdout/stderr redirect of the launcher (which the scheduler inherits) | yes (grep on the real log file the run produced) | ✓ FLOWING on Mac fixtures; GPU pending |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Phase gate, single full run | `bash scripts/check_all.sh --offline` | exit 0. Cargo: 10+1+6 (rsg-wire) passed; rsg-server tests included via workspace. Pytest: 84 passed, 37 skipped. Fixtures fresh. Decode: 37 passed. check_upstream: tree matches 9a91cfa | ✓ PASS |
| Gap-closure suites | `.venv/bin/python -m pytest python/tests/test_gpu_check_script.py python/tests/test_parent_watchdog.py -q -rs` | 19 passed, 1 skipped (Linux PDEATHSIG) | ✓ PASS |
| `cargo test -p rsg-server` | includes `missing_eos_key_is_malformed`, `missing_eos_key_exits_2` | all pass (22 tests per 01-11-SUMMARY; included in full workspace run above) | ✓ PASS |
| Help text / syntax | `bash -n scripts/gpu_phase1_check.sh`, `--help` | syntax OK; help mentions PDEATHSIG and the 4b step | ✓ PASS |
| Restoration check | `git status --porcelain vendor/` | clean | ✓ no stray edits |
| GPU / Linux checks | `scripts/gpu_phase1_check.sh`, PDEATHSIG success-path unit test | not run (macOS, no CUDA) | ? SKIP → human |

### Probe Execution

No `scripts/*/tests/probe-*.sh` exists and no plan declares one. Step 7c: N/A.

### Requirements Coverage

| Requirement | Source Plan(s) | Description | Status | Evidence |
|-------------|----------------|-------------|--------|----------|
| BASE-01 | 01-01, 01-06 | vendored @ 9a91cfa with LICENSE; UPSTREAM.md records commit and modified files | ✓ SATISFIED | SC1 |
| BASE-02 | 01-02, 01-03, 01-05, 01-07, 01-08, 01-09, 01-10, 01-11, 01-12 | one launch command for `--frontend python` / `--frontend rust`; GPU-orphan and watchdog hardening | ✓ SATISFIED on Mac / ? NEEDS HUMAN on GPU | launcher, tests, 7 closed gaps; GPU steps 2-3, 4, 4b |
| BASE-03 | 01-02, 01-03, 01-05, 01-09, 01-10, 01-11, 01-12 | backend readiness handshake (4 values, now strictly requiring eos_token_id); both frontends use same backend code | ✓ SATISFIED on Mac / ? NEEDS HUMAN on GPU | e2e handshake test; GPU step 3 (now also checks PDEATHSIG) |
| WIRE-01 | 01-04, 01-06 | byte-identical codec for all 7 types via golden fixtures | ✓ SATISFIED | SC4 |
| WIRE-02 | 01-06 | every Rust message decodes through the real Python decoder | ✓ SATISFIED | SC5 |

No orphaned requirements: REQUIREMENTS.md maps exactly BASE-01/02/03 and WIRE-01/02 to Phase 1, and every ID is claimed by a plan.

### Anti-Patterns Found

No TBD/FIXME/XXX/TODO/HACK/PLACEHOLDER debt markers in the files touched by 01-09..01-12 (the only grep hit anywhere in scope is `mktemp ...XXXXXX`, a template, not a marker).

The latest incremental review (`01-REVIEW.md`, 2026-10-04T00:00:00Z: 0 critical, 1 warning, 3 info) is advisory. None of it makes a Phase 1 must-have false:

| Finding | File | Severity here | Defeats a must-have? |
|---------|------|---------------|----------------------|
| WR-01 (new instance, same id): abbreviated `--shell-mode` rejection can be masked by an unrelated "binary not found" error when no `rsg-server` binary is built yet | python/rsglang/launch.py:100-128 | ⚠️ Warning | No. Both paths exit 2; rust mode never silently runs with shell-mode limits either way. It is a misleading-diagnostic-ordering issue, confirmed by direct code reading. Untriaged — surfaced as a human-verification item. |
| IN-01: prctl failure handling catches only `OSError`, not a missing symbol (`AttributeError` from `libc.prctl.argtypes =`) | python/rsglang/backend.py:81-97 | ℹ️ Info | No. Still caught one level up by `run_scheduler`'s `except BaseException`, so it reaches the launcher as an error envelope rather than failing silently — just doesn't hit WR-06's specific "degrade and keep running" path for this one failure mode. |
| IN-02: `gpu_phase1_check.sh`'s safety-net cleanup can signal an unrelated process group if `setsid` never detached | scripts/gpu_phase1_check.sh:68-74, 97-103 | ℹ️ Info | No. Low-likelihood edge case on the cleanup trap, already discussed as low severity in the review itself. |
| IN-03: duplicated pass/report logic in `wait_no_orphans` | scripts/gpu_phase1_check.sh:121-158 | ℹ️ Info | No. A maintainability note, not a correctness gap; both call sites are exercised by tests. |
| Carried-forward: WR-03/WR-05/WR-10 (`setpgid(0,0)` under a wrapper), WR-05 (shutdown re-SIGINTs the group), IN-04..IN-13 | various | ⚠️/ℹ️ | No. Unchanged from the prior verification; still `deferred`/`open` with no bearing on this phase's must-haves. |

`01-REVIEW-DISPOSITION.md` now shows: CR-01, WR-02, WR-04, WR-06, WR-07, WR-08, WR-09 all `fixed` (7); WR-03, WR-05, WR-10 `deferred` (3); WR-01 and 13 info-level findings `open` (14); total 24. Nothing there blocks the phase.

### Human Verification Required

#### 1. GPU end-of-phase check (ROADMAP SC2 and SC3, plus the Linux PDEATHSIG success path)
**Test:** On a Linux GPU box run `bash scripts/gpu_phase1_check.sh` after the documented setup.
**Expected:** ALL PASS across steps 1, 2, 3 (now also checking PDEATHSIG armed via `pdeathsig_degraded`), 4, 4b and 5.
**Why human:** Needs CUDA, nvidia-smi and Linux. 01-UAT.md test 1 is `blocked`.

#### 2. Linux-only watchdog unit test
**Test:** `pytest python/tests/test_parent_watchdog.py::test_linux_arms_pdeathsig_sigkill` on any Linux machine (no GPU needed).
**Expected:** 1 passed.
**Why human:** Skipped on macOS by design. 01-UAT.md test 6 is `blocked`.

#### 3. Triage the new WR-01 (ordering) finding
**Test:** Decide fixed-now or deferred for the `resolve_rust_bin`-before-`run_shell`-check ordering issue in `python/rsglang/launch.py`, and record it in `01-REVIEW-DISPOSITION.md`.
**Expected:** Not left silently `open`.
**Why human:** Policy call; does not defeat a must-have (both paths exit 2).

#### 4. Triage remaining open/info findings
**Test:** Decide fixed/deferred for IN-01, IN-02, IN-03 (new) and the carried-forward IN-04..IN-13.
**Expected:** Each recorded, even if the decision is "accept as-is."
**Why human:** Policy call; none defeats a Phase 1 must-have.

### Gaps Summary

No blocking gaps. Compared with the prior verification (58/62, human_needed):
- All 7 gaps from UAT test 7's triage are closed: WR-07, WR-08 (01-09), WR-06, WR-09 (01-10), WR-04, WR-01-original (01-11), and WR-06-CHECK (01-12). Each is proven by tests observed failing before the fix and passing after, several with mutation checks recorded in their SUMMARYs.
- The full Mac gate is green (84 pytest passed vs. 65 before; cargo workspace green; fixtures fresh; decode and check_upstream clean).
- A fresh incremental code review (run after wave 9) found one new, narrower issue that happens to reuse the `WR-01` id (a diagnostic-ordering bug, not a correctness regression), which per the disposition file's own rule reset that row to `open`. This is now a human-verification item, not a gap against this phase's must-haves.
- The remaining items are unchanged in kind from before: the GPU/Linux run (01-UAT.md tests 1 and 6, both still `blocked`) and review-warning triage. Neither is a code gap on the Mac.

01-UAT.md's test 7 still shows `result: issue` in its own record (it predates this verification's gap-closure chain finishing); the gaps it lists are now all `status: failed`→closed per this report's evidence, but the UAT document itself has not been refreshed to reflect that. This is a documentation-staleness note, not a code gap.

---

_Verified: 2026-10-04T09:30:00Z_
_Verifier: Claude (gsd-verifier)_
