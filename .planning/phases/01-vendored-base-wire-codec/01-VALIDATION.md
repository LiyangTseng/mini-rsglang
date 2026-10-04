---
phase: "1"
slug: "vendored-base-wire-codec"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: validated
nyquist_compliant: true
wave_0_complete: true
created: "2026-10-03"
validated: "2026-10-03"
---

# Phase 1 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.
> Seeded from `01-RESEARCH.md` § Validation Architecture.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | `cargo test` (Rust 1.99, built-in harness) + pytest 9.x |
| **Config file** | root `Cargo.toml` workspace + `rust-toolchain.toml`; root `pyproject.toml` `[tool.pytest.ini_options]` (`testpaths`, `norecursedirs`, `slow` marker, `--strict-markers`) |
| **Quick run command** | `cargo test -p rsg-wire && .venv/bin/python -m pytest python/tests -x -q -m "not slow"` |
| **Full suite command** | `cargo test --workspace && .venv/bin/python -m pytest python/tests -q && .venv/bin/python scripts/gen_wire_fixtures.py --check && bash scripts/check_wire_decode.sh && .venv/bin/python scripts/check_upstream.py` (one command: `bash scripts/check_all.sh`, plan 01-06) |
| **Estimated runtime** | ~60 seconds (excluding first cargo build); measured 50 s for `check_all.sh` on 2026-10-03 |

---

## Sampling Rate

- **After every task commit:** Run the quick run command
- **After every plan wave:** Run the full suite command
- **Before `/gsd-verify-work`:** Full suite must be green on the Mac AND the GPU checklist signed off
- **Max feedback latency:** 120 seconds

---

## Per-Task Verification Map

Filled in by the planner/executor per task. Requirement → test map from research:

| Req ID | Behavior | Test Type | Automated Command | File Exists | Status |
|--------|----------|-----------|-------------------|-------------|--------|
| BASE-01 | Vendored tree == pristine 9a91cfa; LICENSE present; UPSTREAM.md lists every diff; frozen tiers enforced | integration (script) | `.venv/bin/python scripts/check_upstream.py` / `.venv/bin/python scripts/check_upstream.py --offline` | ✅ | ✅ green |
| BASE-01 | Check script detects an unlisted edit and a listed-but-frozen edit | unit | `.venv/bin/python -m pytest python/tests/test_check_upstream.py -x` | ✅ | ✅ green |
| BASE-02 | Python mode builds the exact upstream launch argv (exec injected) | unit | `.venv/bin/python -m pytest python/tests/test_launch_args.py -x` | ✅ | ✅ green |
| BASE-02 | Rust mode e2e on Mac: launcher + fake scheduler (real upstream ZMQ queues) + real Rust skeleton logs handshake | integration | `cargo build -p rsg-server && .venv/bin/python -m pytest python/tests/test_launch_rust_e2e.py -x` | ✅ | ✅ green |
| BASE-02 | Failure handling: child crash → killpg + non-zero + stderr tail; ready timeout; launcher SIGKILL → Rust exits on stdin EOF | integration | same file, separate tests | ✅ | ✅ green (one unexplained ~10 s escalation in ~110 runs before the non-zero assertion; see `deferred-items.md`) |
| BASE-03 | `extract_handshake` reads the right attributes; JSON schema; SHA mismatch → Rust exit≠0 | unit | `.venv/bin/python -m pytest python/tests/test_handshake.py -x` + `cargo test -p rsg-server` | ✅ | ✅ green |
| BASE-03 | Rust socket roles derived from `ServerArgs` (num_tokenizer 0 → bind `_1`, N>0 → connect) | unit | `.venv/bin/python -m pytest python/tests/test_topology.py -x` | ✅ | ✅ green |
| WIRE-01 | Every fixture: Rust decode→encode == bytes; hand-built base cases encode == bytes; SHA consistency | unit | `cargo test -p rsg-wire` | ✅ | ✅ green |
| WIRE-01 | Fixtures fresh against vendored code | integration | `.venv/bin/python scripts/gen_wire_fixtures.py --check` | ✅ | ✅ green |
| WIRE-02 | Rust-encoded messages decode through real upstream decoder and re-encode identically | integration | `scripts/check_wire_decode.sh` | ✅ | ✅ green |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [x] Root `Cargo.toml` (workspace) + `rust-toolchain.toml` (1.99.0)
- [x] Root `pyproject.toml` for `rsglang`, with pytest `testpaths`/`norecursedirs` excluding `vendor/`
- [x] Root `.gitignore`: `target/`, `.venv/`, `__pycache__/`, `*.egg-info/`
- [x] Launcher test fixtures (Rust binary build fixture `rust_bin`, `LauncherRun` helper with per-run suffix and `/tmp/minisgl_*<suffix>` cleanup) — planned as module fixtures inside `python/tests/test_launch_rust_e2e.py` (plan 01-03) instead of a separate `conftest.py`, since only that file spawns processes
- [x] `python/rsglang/testing/fake_scheduler.py` (plan 01-03)
- [x] Mac env bootstrap script `scripts/bootstrap_mac_env.sh` (uv venv Python 3.12 + hash-pinned lock + `--no-deps -e vendor/mini-sglang`) (plan 01-01)

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| `--frontend python` serves a chat completion through the unmodified Python frontend | BASE-02 | Needs GPU (Linux, CUDA) | `python -m rsglang.launch --frontend python --model Qwen/Qwen3-0.6B`; `curl localhost:1919/v1/chat/completions ...` returns a completion |
| `--frontend rust` logs real handshake values from the real backend | BASE-02, BASE-03 | Needs GPU | Rust log shows `max_seq_len`, `eos_token_id`, `page_size` (1, or 64 on SM100), `max_running_req`, `num_pages>1`, SHA |
| Launcher SIGKILL leaves no GPU processes | BASE-02 | Needs GPU | `kill -9 <launcher pid>`; `nvidia-smi` shows no leftover process |
| Frozen frontend proof on GPU checkout | BASE-03 | Runs on GPU box checkout | `.venv/bin/python scripts/check_upstream.py` passes |

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies (01-01 Task 2 is the blocking-human package gate, approved)
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 120s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** validated 2026-10-03 (`/gsd-validate-phase 1`, Mac side). The four GPU rows under Manual-Only still need the Linux sign-off via `scripts/gpu_phase1_check.sh`.

---

## Validation Audit 2026-10-03

| Metric | Count |
|--------|-------|
| Gaps found | 0 |
| Resolved | 0 |
| Escalated | 0 |

Evidence: `bash scripts/check_all.sh` OK online in 50 s. Steps: `cargo test --workspace` green, pytest 59 passed + 36 skipped, fixture freshness OK, `check_wire_decode.sh` 37 passed, `check_upstream.py` OK with 121 paths and 0 listed modifications. The 36 skips are all `test_wire_decode.py` without `DUMP_DIR`; step 4 runs them with the dump. Also green: `check_upstream.py --offline` (tree `02d3e4ad`), `cargo clippy --workspace --all-targets -- -D warnings`, and the `gpu_phase1_check.sh` syntax and `--help` checks.
