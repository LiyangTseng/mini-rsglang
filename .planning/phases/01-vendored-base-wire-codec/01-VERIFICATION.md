---
phase: 01-vendored-base-wire-codec
verified: 2026-10-04T04:51:24Z
status: human_needed
score: 45/49 must-haves verified
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
covered_digest: "v2:sha256:6c93343d8df3673ecce367801e74929bd3f232c3c5ce4d98289167ee830a7aca"
behavior_unverified: 2
overrides_applied: 0
behavior_unverified_items:
  - truth: "ROADMAP SC2 (GPU half): on the GPU machine --frontend python serves a chat completion through the unmodified Python frontend, and --frontend rust starts the same real backend plus rsg-server"
    test: "On the Linux GPU box run `bash scripts/gpu_phase1_check.sh` (steps 1-3)"
    expected: "Step 2 PASS (non-empty chat completion content from python mode); step 3 PASS (rust mode reaches 'handshake received')"
    why_human: "Needs CUDA and the real upstream Scheduler; on the Mac only a FakeScheduler on upstream's real ZMQ queues was exercised"
  - truth: "ROADMAP SC3 (GPU half): the real backend reports max_seq_len, eos_token_id, page_size, max_running_req at readiness and rsg-server logs them; the Python frontend keeps working against the same backend code"
    test: "Same run, step 3 output line and step 5"
    expected: "'handshake received' line with upstream_sha=9a91cfa..., max_running_req=256, num_pages>1, max_seq_len in 1..40960, page_size 1 or 64, eos_token_id=151645 for Qwen3-0.6B; step 5 check_upstream.py PASS"
    why_human: "extract_handshake reads scheduler.engine.max_seq_len / engine.num_pages / cache_manager.page_size / eos_token_id; attribute names were checked against upstream source, but the real values only exist after a CUDA engine init"
human_verification:
  - test: "Run `bash scripts/gpu_phase1_check.sh` on the Linux GPU box (after `uv venv --python=3.12 && uv pip install -e vendor/mini-sglang && uv pip install -e .`, build-essential present)"
    expected: "ALL PASS: step 1 release build, step 2 python-mode chat completion, step 3 real handshake values, step 4 no orphan after kill -9 of the launcher, step 5 check_upstream.py"
    why_human: "ROADMAP criteria 2 and 3 are GPU-only; this script is the planned end-of-phase human check (01-05 Task 3, WINDOWS.md entry 1)"
  - test: "Decide the disposition of code-review finding CR-01 (group SIGINT / Ctrl-C makes the rust-mode launcher exit 1 with a failure report)"
    expected: "Either fix now (re-check stop_requested right after each ready_queue.get and before scanning children, plus an e2e test that SIGINTs the launcher's process group and asserts exit 0) or mark it deferred in 01-REVIEW-DISPOSITION.md with a target phase"
    why_human: "Reproduced by the verifier (exit 1, 'rsg-server exited with code 0', failure tail printed; children and sockets were still cleaned up). It violates no Phase 1 must-have, but it will mislabel every interactive or harness-driven stop as a failure from Phase 3 onward"
  - test: "Decide the disposition of WR-02 (parent watchdog records getppid() only after the spawned child has booted and unpickled ServerArgs)"
    expected: "Either pass the launcher pid explicitly (plus PR_SET_PDEATHSIG on Linux) or accept/defer it in 01-REVIEW-DISPOSITION.md"
    why_human: "01-05 truth 'SIGKILL of the launcher alone leaves no orphan' is proven by test only after ready; a kill -9 during the first seconds of scheduler boot can leave a GPU-holding orphan. Narrow window, judgment call on timing"
  - test: "Review the judgment-tier prohibition from 01-03: rust mode runs the byte-identical upstream Scheduler and the handshake is not produced by patching vendored code"
    expected: "Agree with the non-authoritative verifier verdict: holds (see Prohibitions table)"
    why_human: "unverified-prohibition — human review recommended (judgment tier, ADR-550 D4)"
  - test: "Confirm the 01-01 process truth: no Python package was installed before you approved the PyPI names and pins"
    expected: "You recall approving 'approve (use appropriate virtual environemnt such as uv ...)' before the .venv was built"
    why_human: "A past human act; nothing in the codebase can prove ordering"
---

# Phase 1: Vendored Base & Wire Codec Verification Report

**Phase Goal:** The repo holds a pinned, attributed copy of mini-sglang. One launch command runs the shared backend with either frontend, and the backend reports a readiness handshake. The Rust msgpack codec is byte-exact with upstream for all 7 message types.
**Verified:** 2026-10-04T04:51:24Z
**Status:** human_needed
**Re-verification:** No (initial verification)

## Goal Achievement

Everything that can be checked on the Mac holds. I checked it myself instead of trusting the summaries. The remaining items are the GPU run, which was always planned as a human step, two code-review findings that need a decision, and two human-attested items. None of the 15 review findings makes a must-have false. CR-01 is real (I reproduced it), but no Phase 1 truth covers it.

### Observable Truths: ROADMAP Success Criteria (the contract)

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| SC1 | Repo contains mini-sglang @ 9a91cfa with MIT LICENSE + copyright; UPSTREAM.md names the commit and lists every modified vendored file | ✓ VERIFIED | `git rev-parse HEAD:vendor/mini-sglang` = `git write-tree --prefix=vendor/mini-sglang/` = `02d3e4ad…`. I ran `scripts/check_upstream.py` online: it cloned github.com/sgl-project/mini-sglang, checked the root tree of 9a91cfa against KNOWN_TREES, `git archive`d it, and compared 121 paths, with 0 listed modifications. LICENSE reads "Copyright (c) 2026 sgl-project". UPSTREAM.md has the full SHA and an empty `## Modified files` table, which is correct because 0 files differ. |
| SC2 | On GPU, `--frontend python` serves a chat completion via the unmodified frontend; `--frontend rust` starts the same backend plus the Rust skeleton | ⚠️ PRESENT_BEHAVIOR_UNVERIFIED | Mac half verified: `test_rust_mode_handshake_reaches_rsg_server` passed when I ran it on its own (7.9 s). Python mode `execv`s `python -m minisgl <args>` (launch.py:81-88, 7 tests in test_launch_args.py), and the vendored `__main__.py` calls upstream `launch_server`. GPU half not run: `scripts/gpu_phase1_check.sh` steps 2-3. |
| SC3 | Backend reports max_seq_len, eos_token_id, page_size, max_running_req at readiness; Rust logs them; Python frontend unchanged against same backend code | ⚠️ PRESENT_BEHAVIOR_UNVERIFIED | `backend.extract_handshake` reads `scheduler.engine.max_seq_len`, `engine.num_pages`, `cache_manager.page_size` and `eos_token_id`. I checked these names against upstream `scheduler/scheduler.py:47-70` and `engine/engine.py:55-67`. rsg-server main.rs:144-152 logs all values at "handshake received". The vendored tree is pristine, so both modes run identical backend code. Real values need the GPU run (step 3). |
| SC4 | For each of the 7 upstream message types, Rust codec bytes equal golden fixtures from upstream's Python encoder, checked on the Mac | ✓ VERIFIED | `cargo test --workspace` passed: rsg-wire lib has 10 tests and the fixtures suite has 6. `every_fixture_roundtrips_byte_exact` and `hand_built_cases_match_fixtures` pass over 34 committed fixtures. `all_type_tags_covered` asserts a `base_` case for all 8 tags. `gen_wire_fixtures.py --check` passed: it regenerated the fixtures through the vendored `serialize_type` + `msgpack.packb(use_bin_type=True)` and byte-diffed them. |
| SC5 | Every message the Rust codec emits decodes through upstream's real Python decoder (cls(**kwargs)) | ✓ VERIFIED | `scripts/check_wire_decode.sh`: the Rust `dump` test wrote 34 hand-built encodings, then pytest ran 37 tests. Each frame decodes through `BaseBackendMsg.decoder` / `BaseTokenizerMsg.decoder`, re-encodes to identical bytes, and equals the committed fixture. The negative control (extra key → TypeError) passes. |

### Observable Truths: PLAN must_haves (merged, grouped by plan)

| Plan | Truth (abridged) | Status | Evidence |
|------|------------------|--------|----------|
| 01-01 | vendor tree = upstream root tree 02d3e4ad | ✓ VERIFIED | write-tree and online check_upstream (above) |
| 01-01 | whole repo vendored; tests/core/test_scheduler.py, benchmark/online/bench_simple.py exist; .dockerignore symlink | ✓ VERIFIED | `ls` and `ls -la` (symlink → .gitignore) |
| 01-01 | LICENSE is upstream MIT with copyright | ✓ VERIFIED | `head vendor/mini-sglang/LICENSE` |
| 01-01 | UPSTREAM.md names URL + full SHA + parseable empty table | ✓ VERIFIED | file read; check_upstream parses it (a parse error would exit 1) |
| 01-01 | No package installed before human approval | ? UNCERTAIN | Past human act, recorded only in 01-01-SUMMARY. Listed as a human item. |
| 01-01 | .venv (3.12) imports minisgl.{message,core,utils,scheduler,server.args}, rsglang, no CUDA pkgs, minisgl resolves to vendored tree | ✓ VERIFIED | Python 3.12.12. `minisgl.__path__` = vendor/mini-sglang/python/minisgl (namespace pkg), also from cwd=/tmp. flashinfer/sgl_kernel absent. |
| 01-02 | rsg-server takes the 6 CLI flags and binds/connects per role | ✓ VERIFIED | main.rs:28-47, 119-135; transport tests (3) passed |
| 01-02 | reads one JSON line, logs "handshake received" with all 6 values | ✓ VERIFIED | main.rs:140-152; cli tests (9) passed |
| 01-02 | exit 2 on SHA mismatch (both SHAs), malformed JSON, unknown fields, version≠1 | ✓ VERIFIED | handshake.rs (deny_unknown_fields, version and SHA checks); 8 unit tests and the cli tests passed. See WR-04 under Anti-Patterns: a *missing* eos key is accepted. |
| 01-02 | exit 3 on stdin EOF before/after handshake | ✓ VERIFIED | main.rs:159-166, 177-184; cli tests |
| 01-02 | exit 0 on SIGINT/SIGTERM; never sends on backend socket | ✓ VERIFIED | main.rs:90-93, 127-129 (`_transport` never used to send); cli tests |
| 01-02 | vendor/UPSTREAM_SHA single source, compiled in | ✓ VERIFIED | include_str! in handshake.rs:16 and lib.rs:27; `sha_single_source`, `expected_sha_is_vendor_file` pass |
| 01-03 | Mac e2e: launcher + FakeScheduler + rsg-server logs 4096/151645/16/8/1024/SHA | ✓ VERIFIED | Ran the single named e2e test; it passed |
| 01-03 | rsg-server spawned first and "awaiting handshake" before "handshake sent" (D-10) | ✓ VERIFIED | launch.py:143-170; e2e asserts the index ordering |
| 01-03 | Rust binds _1; scheduler connects with upstream ZmqPushQueue; ZMQ_IMMEDIATE probe sees the peer (D-07) | ✓ VERIFIED | sockets.py:39-46; fake_scheduler.py:55-82; e2e asserts the `detok_peer_connected` marker |
| 01-03 | suffix `.rsg=<pid>`; only this run's 5 sockets unlinked at start/exit | ✓ VERIFIED | launch.py:107-112, 125, 211; test_topology decoy test; no `/tmp/minisgl_*` left after my CR-01 repro |
| 01-03 | SIGTERM to launcher exits 0, no children left | ✓ VERIFIED | e2e test asserts `wait()==0`, children dead, sockets gone |
| 01-03 | `--frontend python` execs `python -m minisgl` with upstream args unchanged, launcher flags removed | ✓ VERIFIED | launch.py:81-88, 278-282; test_launch_args (7) passed |
| 01-04 | "7 types" pinned to the 7 cls(**kwargs) classes; standalone base_ case for all 8 tags | ✓ VERIFIED | WIRE_TYPE_TAGS; manifest type_tags; `all_type_tags_covered`. See the interpretation note below. |
| 01-04 | decode→encode and hand-built encode both equal fixture bytes for every case | ✓ VERIFIED | fixtures.rs tests passed |
| 01-04 | fixtures generated by upstream serialize_type on the Mac; `--check` regenerates and byte-diffs | ✓ VERIFIED | gen_wire_fixtures.py pins sys.path to vendored tree (exits 2 otherwise); `--check` passed in check_all |
| 01-04 | UPSTREAM_SHA == manifest == vendor file | ✓ VERIFIED | `sha_single_source` passed; manifest upstream_sha = 9a91cfa… |
| 01-04 | integer and bin8/16/32 width boundaries byte-identical | ✓ VERIFIED | 34 cases include uid 127/128/255/256/65535/65536/2^32, top_k -1/-32/-33/-128/-129, tensor 63/64/16383/16384 |
| 01-04 | ExitMsg = 81a85f…; 1-entry batch and 1-token tensor match | ✓ VERIFIED | base_exit_msg, base_batch_tokenizer_msg, tensor_len_1 fixtures; lib known-answer tests |
| 01-04 | whole-frame byte equality: str keys, float64, bin buffer, 'torch.int32' | ✓ VERIFIED | lib.rs types (f64, serde_bytes, TENSOR_DTYPE_INT32); float fixtures pass |
| 01-04 | `__type__` first, then dataclass field order; batch order kept | ✓ VERIFIED | Byte equality implies key order; `test_batch_decode_keeps_element_order` passed |
| 01-05 | rust_endpoints roles with real parse_args (bind for num_tokenizer 0, connect for 2) | ✓ VERIFIED | test_topology.py passed in the full pytest run (59 passed) |
| 01-05 | extract_handshake uses cache_manager.page_size / engine.max_seq_len; contract bytes; eos None → null | ✓ VERIFIED | backend.py:38-51; test_handshake.py passed |
| 01-05 | scheduler crash before ready → traceback, killpg, non-zero, rsg-server gone | ✓ VERIFIED | `test_scheduler_crash_before_ready` passed |
| 01-05 | never-ready → "backend not ready after <t> s", non-zero, all children gone | ✓ VERIFIED | `test_ready_timeout` passed |
| 01-05 | rsg-server dies after handshake → stderr tail, stop scheduler, non-zero | ✓ VERIFIED | `test_rsg_server_death_triggers_shutdown` passed |
| 01-05 | SIGKILL of the launcher leaves no orphan (stdin EOF + watchdog ≤20 s) | ✓ VERIFIED (post-ready path) | `test_launcher_sigkill_leaves_no_orphans` passed. Caveat WR-02: if the launcher is killed during the child's spawn bootstrap, before `start_parent_watchdog` reads getppid, the scheduler can be orphaned. Listed as a human decision. |
| 01-05 | unlink_run_sockets leaves another suffix's file untouched | ✓ VERIFIED | test_topology decoy test |
| 01-05 | gpu_phase1_check.sh automates GPU checks for SC2/SC3 and is signed off by a human | ? UNCERTAIN | The script is substantive: 5 steps, grep-based field checks, setsid/SIG_DFL handling. Sign-off is pending (human item 1). |
| 01-06 | check_upstream diffs vs pristine; fails on unlisted differing/added/removed | ✓ VERIFIED | test_check_upstream (24) passed; online run OK |
| 01-06 | Tier A/C never; Tier B only with "yes" | ✓ VERIFIED | tier tests passed |
| 01-06 | modified = bytes/symlink-ness/target/exec bit | ✓ VERIFIED | symlink-replacement and exec-bit tests passed |
| 01-06 | empty table parses to 0 entries; missing header is a parse failure; STALE_LISTING | ✓ VERIFIED | parse-error and stale tests passed |
| 01-06 | violations sorted by path | ✓ VERIFIED | `test_violations_print_sorted_by_path` |
| 01-06 | (backstop) never writes under vendor/; per-run temp dir; interrupted/concurrent run leaves vendored tree unchanged | ✓ VERIFIED | `test_check_never_writes_under_vendor` passed. I also observed the behavior directly: two concurrent online runs plus a third, then three runs killed with SIGTERM at 0.3/0.7/1.2 s. `git status --porcelain --ignored vendor/` was unchanged and the tree was still 02d3e4ad. (The killed runs left their `check_upstream-*` temp dirs in $TMPDIR. I removed them; see Info.) |
| 01-06 | every Rust-emitted case decodes via upstream decoder and re-encodes identically | ✓ VERIFIED | check_wire_decode.sh: 37 passed |
| 01-06 | boundary cases, empty ExitMsg and 1-entry batches round-trip; batch order kept | ✓ VERIFIED | parametrized over all 34 cases |
| 01-06 | upstream decoder rejects extra key (negative control) | ✓ VERIFIED | `test_upstream_decoder_rejects_extra_key` |
| 01-06 | check_wire_decode.sh one command; check_all.sh runs all 5 steps | ✓ VERIFIED | I ran `bash scripts/check_all.sh` (online): exit 0, "check_all: OK" |

**Score:** 45/49 truths verified (5 roadmap + 44 plan). 2 are present but behavior-unverified (SC2 and SC3 GPU halves). 2 are UNCERTAIN and attested by a human (01-01 approval ordering, 01-05 GPU sign-off).

**Interpretation note (SC4, "7 message types"):** upstream `message/*.py` defines 10 concrete classes. The phase pins "7" to the 6 scheduler-boundary messages plus `SamplingParams` (RESEARCH A1), and the fixtures also cover `Tensor`. `TokenizeMsg`, `AbortMsg`, `UserReply` and `BatchFrontendMsg` are internal to the Python frontend that rsg-server replaces, and Rust never puts them on a wire, so I accept this reading. If "7" was meant as the 7 non-batch classes across all modules (UserMsg, AbortBackendMsg, ExitMsg, DetokenizeMsg, TokenizeMsg, AbortMsg, UserReply), then three of them have no fixtures. Both the project's CLAUDE.md wire table and the architecture support the boundary reading.

### Prohibitions

| Plan | Prohibition | Tier | Disposition |
|------|-------------|------|-------------|
| 01-01 | Vendored LICENSE / copyright never removed or altered | test | ✓ VERIFIED. Enforcement is wired: LICENSE is in TIER_A, and the `LICENSE_MISSING` check is in check_upstream.py:287-290. `test_license_attribution_removed_fails` passes. |
| 01-06 | Tier A frozen frontend never modified, even when listed | test | ✓ VERIFIED. `test_tier_a_edit_fails_even_when_listed` and `test_tier_a_directory_edit_fails` pass. The online check is clean. |
| 01-03 | Rust mode runs the byte-identical upstream Scheduler; the handshake is not produced by patching vendored code | judgment | Non-authoritative LLM verdict: holds. `DEFAULT_SCHEDULER_FACTORY = "minisgl.scheduler:Scheduler"`. The handshake is read in `python/rsglang/backend.py`, which is outside vendor/. The vendored tree is pristine. **unverified-prohibition — human review recommended.** One note: the `RSGLANG_SCHEDULER_FACTORY` env seam can swap the class. The GPU script does not set it. |

### Required Artifacts

| Artifact | Status | Details |
|----------|--------|---------|
| vendor/mini-sglang/ (+LICENSE) | ✓ VERIFIED | pristine tree, 121 paths |
| UPSTREAM.md | ✓ VERIFIED | SHA, tiers, table header exact |
| vendor/UPSTREAM_SHA | ✓ VERIFIED | 9a91cfafe754aa85daee49998176275667eb58f2 |
| Cargo.toml / rust-toolchain.toml | ✓ VERIFIED | workspace builds and tests on 1.99.0 |
| pyproject.toml / requirements-mac.{in,txt} / scripts/bootstrap_mac_env.sh | ✓ VERIFIED | .venv works; pytest never collects vendor/ |
| crates/rsg-server/src/{main,handshake,transport}.rs + tests/cli.rs | ✓ VERIFIED | substantive, wired, 20 tests pass |
| crates/rsg-wire/src/lib.rs + tests/{fixtures,dump,common} | ✓ VERIFIED | substantive; 17 tests pass |
| fixtures/wire/*.msgpack + manifest.json | ✓ VERIFIED | 34 cases, fresh per `--check` |
| python/rsglang/{launch,backend,handshake,sockets}.py, testing/fake_scheduler.py | ✓ VERIFIED | wired. launch → backend.run_scheduler and encode_handshake_line; backend → minisgl.scheduler:Scheduler |
| scripts/{gen_wire_fixtures,check_upstream}.py, check_wire_decode.sh, check_all.sh | ✓ VERIFIED | all run green |
| scripts/gpu_phase1_check.sh | ✓ VERIFIED (exists, substantive) | not executed (no GPU) |

### Key Link Verification

| From | To | Via | Status |
|------|----|-----|--------|
| launch.py | backend.run_scheduler | `mp.Process(target=backend.run_scheduler, ...)` (launch.py:162-164) | ✓ WIRED |
| launch.py | rsg-server | `subprocess.Popen` with `rust_cli_args`, `stdin.write(encode_handshake_line(payload))` (launch.py:144-149, 254) | ✓ WIRED |
| backend.py | upstream Scheduler | `DEFAULT_SCHEDULER_FACTORY = "minisgl.scheduler:Scheduler"` | ✓ WIRED |
| sockets.py | upstream ServerArgs | `backend_create_detokenizer_link`, `zmq_*_addr` | ✓ WIRED |
| rsg-server main.rs | handshake.rs / transport.rs | `parse_handshake(..., EXPECTED_UPSTREAM_SHA)`, `ZmqTransport::open` | ✓ WIRED |
| handshake.rs, lib.rs | vendor/UPSTREAM_SHA | `include_str!("../../../vendor/UPSTREAM_SHA")` | ✓ WIRED |
| gen_wire_fixtures.py | upstream message/utils.py | `msgpack.packb(serialize_type(obj), use_bin_type=True)` | ✓ WIRED |
| tests/fixtures.rs | manifest.json | `common::manifest()` cases | ✓ WIRED |
| check_wire_decode.sh | tests/dump.rs | `DUMP_DIR=… cargo test -p rsg-wire --test dump` | ✓ WIRED |
| test_wire_decode.py | upstream decoders | `BaseBackendMsg.decoder` / `BaseTokenizerMsg.decoder` | ✓ WIRED |
| check_all.sh | check_upstream.py, gen_wire_fixtures --check | steps 3 and 5 | ✓ WIRED |
| gpu_phase1_check.sh | rsglang.launch | steps 2 and 3 | ✓ WIRED |

### Data-Flow Trace (Level 4)

| Artifact | Data | Source | Real data | Status |
|----------|------|--------|-----------|--------|
| rsg-server "handshake received" log | max_seq_len, eos, page_size, max_running_req, num_pages, sha | stdin line ← launcher ← ready_queue ← `extract_handshake(scheduler, …)` ← constructed scheduler object | Mac: FakeScheduler constants. GPU: real engine attributes (names checked against upstream) | ✓ FLOWING on Mac; GPU pending |
| Golden fixtures | msgpack bytes | vendored upstream `serialize_type` | yes (`--check` regenerates) | ✓ FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Phase gate (single full run) | `bash scripts/check_all.sh` (online) | exit 0. Cargo: 11+9+10+1+6 passed. Pytest: 59 passed, 36 skipped (dump-dependent; run in step 4). Fixtures fresh. Decode: 37 passed. check_upstream: 121 paths OK | ✓ PASS |
| Rust-mode tracer | `pytest python/tests/test_launch_rust_e2e.py::test_rust_mode_handshake_reaches_rsg_server` | 1 passed in 7.93 s | ✓ PASS |
| check_upstream under concurrency / interruption | 3 concurrent online runs, 3 SIGTERM-interrupted runs | vendor/ status and tree hash unchanged | ✓ PASS |
| CR-01 reproduction (Ctrl-C to the group) | `os.killpg(launcher, SIGINT)` after "handshake sent" (FakeScheduler) | `rsg-server exited with code 0` → failure tail → `exit code 1`. No leftover processes or sockets | ✗ confirms CR-01 (no must-have covers it; see below) |
| GPU checks | `scripts/gpu_phase1_check.sh` | not run (Mac, no CUDA) | ? SKIP → human |

### Probe Execution

No `scripts/*/tests/probe-*.sh` exists, and no plan declares one. Step 7c: N/A.

### Requirements Coverage

| Requirement | Source Plan(s) | Description | Status | Evidence |
|-------------|----------------|-------------|--------|----------|
| BASE-01 | 01-01, 01-06 | vendored @ 9a91cfa with LICENSE; UPSTREAM.md records commit and modified files | ✓ SATISFIED | SC1 evidence |
| BASE-02 | 01-02, 01-03, 01-05 | one launch command for `--frontend python` / `--frontend rust` | ✓ SATISFIED on Mac / ? NEEDS HUMAN on GPU | launcher + tests; GPU steps 2-3 |
| BASE-03 | 01-02, 01-03, 01-05 | backend readiness handshake (4 values); both frontends use same backend code | ✓ SATISFIED on Mac / ? NEEDS HUMAN on GPU | e2e handshake test; GPU step 3 |
| WIRE-01 | 01-04, 01-06 | byte-identical codec for all 7 types via golden fixtures | ✓ SATISFIED | SC4 evidence |
| WIRE-02 | 01-06 | every Rust message decodes through the real Python decoder | ✓ SATISFIED | SC5 evidence |

No orphaned requirements. REQUIREMENTS.md maps exactly these 5 IDs to Phase 1, and every one is claimed by a plan. Note: REQUIREMENTS.md already shows BASE-02 and BASE-03 as `[x] Complete`, but their GPU halves have not been verified yet.

### Anti-Patterns Found

No TBD/FIXME/XXX debt markers. The only match is the `mktemp …XXXXXX` template in gpu_phase1_check.sh:55. No TODO/HACK, `todo!()` or `unimplemented!()` in phase sources.

Code-review findings, re-weighed against the must-haves:

| Finding | File | Severity here | Defeats a must-have? |
|---------|------|---------------|----------------------|
| CR-01 group SIGINT → launcher exit 1 + failure report | python/rsglang/launch.py:236-242, 264-275 | ⚠️ Warning (human decision) | No. Reproduced. The 01-03 truth covers SIGTERM to the launcher pid only, which passes. D-12 says "if any child exits … exit non-zero", which the observed behavior literally satisfies. Children and sockets are still cleaned up. It does break the D-12 intent that a stop signal exits 0, and any Phase 3+ harness that stops rust mode by group signal will see spurious failures. No later phase in ROADMAP covers it, so it cannot be deferred by roadmap. |
| WR-01 `--shell` abbreviation bypasses the shell-mode guard | launch.py:101, 123 | ⚠️ Warning | No in Phase 1. Silently sets max_running_req=1 and would corrupt Phase 7 benchmarks. |
| WR-02 watchdog getppid read late | backend.py:61, 75 | ⚠️ Warning (human decision) | Partially. The 01-05 no-orphan truth holds on the tested post-ready path but not during child boot. |
| WR-03 setpgid detaches launcher from foreground group under wrappers | launch.py:128-129 | ⚠️ Warning | No. The GPU script uses setsid. Related to the open deferred-items pipeline entry. |
| WR-04 missing `eos_token_id` key accepted by Rust | crates/rsg-server/src/handshake.rs:25 | ⚠️ Warning | No. The 01-02 truth lists malformed JSON, unknown fields and version. The Python side always sends all 7 keys (`encode_handshake_line` enforces this). The doc comment's "every key is required" is still false. |
| WR-05 shutdown re-SIGINTs the group during upstream graceful shutdown | launch.py:193 | ⚠️ Warning | No in Phase 1. Matters for TP>1 on GPU. |
| IN-01..IN-09 | various | ℹ️ Info | No |
| (new) check_upstream.py leaves its `check_upstream-*` temp dir in $TMPDIR when killed by SIGTERM | scripts/check_upstream.py:308 | ℹ️ Info | No. vendor/ is untouched, which is what the truth claims. |
| (known) 1-in-~110 unexplained e2e escalation | deferred-items.md | ℹ️ Info | No |

### Human Verification Required

#### 1. GPU end-of-phase check (ROADMAP SC2 and SC3)
**Test:** On the Linux GPU box, run `bash scripts/gpu_phase1_check.sh` after the documented setup.
**Expected:** ALL PASS across the five steps: release build, python-mode chat completion, real handshake line (sha, max_running_req=256, num_pages>1, max_seq_len 1..40960, page_size 1 or 64, eos 151645), no orphan after kill -9, and check_upstream.
**Why human:** Needs CUDA and the real upstream Scheduler.

#### 2. CR-01 disposition
**Test:** Decide whether to fix now or defer with a target phase in 01-REVIEW-DISPOSITION.md.
**Expected:** The fix re-checks `stop_requested` right after each `ready_queue.get()` and before the `children()` scan, and adds an e2e test that SIGINTs the process group and asserts exit 0.
**Why human:** Policy call. No must-have covers it, but it is reproducible.

#### 3. WR-02 disposition
**Test:** Decide whether to pass the launcher pid to `start_parent_watchdog` (plus PR_SET_PDEATHSIG on Linux) now, or defer it.
**Expected:** Either the fix, or a recorded deferral.
**Why human:** The orphan only happens in a narrow boot window. Whether to accept that is a judgment call.

#### 4. Judgment-tier prohibition (01-03)
**Test:** Confirm that rust mode runs the unmodified upstream Scheduler and that the handshake comes from the launcher-side wrapper.
**Expected:** Agreement with the verifier verdict above.
**Why human:** unverified-prohibition — human review recommended.

#### 5. Package-approval ordering (01-01)
**Test:** Confirm you approved the PyPI pins before anything was installed.
**Expected:** Yes.
**Why human:** A past human act.

### Gaps Summary

There are no blocking gaps. All Mac-verifiable parts of the goal hold, and I checked them against the code rather than the summaries:
- The vendored tree is byte-identical to GitHub's 9a91cfa (online diff).
- The Rust codec matches upstream's encoder byte-for-byte on 34 fixtures, covering all 8 tags and every width boundary.
- Every Rust-emitted frame passes upstream's real `cls(**kwargs)` decoder.
- The one-command launcher delivers the readiness handshake end to end on the Mac using upstream's real ZMQ queues.

What remains:
- **The planned GPU run** for SC2 and SC3.
- **Two review findings that need a disposition.** CR-01 (Ctrl-C reports a failure) is reproducible, but no must-have covers it. WR-02 (watchdog boot window) weakens the 01-05 no-orphan truth outside its tested path. All 15 review findings are still `open` in the disposition ledger. I recommend resolving CR-01 and WR-01 before Phase 3 and Phase 7 respectively, because both affect harness behavior later.

---

_Verified: 2026-10-04T04:51:24Z_
_Verifier: Claude (gsd-verifier)_
