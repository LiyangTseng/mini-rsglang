---
phase: "1"
slug: "vendored-base-wire-codec"
status: verified
# threats_open = count of OPEN threats at or above workflow.security_block_on severity (the blocking gate)
threats_open: 0
asvs_level: 1
block_on: high
register_authored_at_plan_time: true
created: "2026-10-03"
---

# Phase 1 — Security

> Per-phase security contract: threat register, accepted risks, and audit trail.
> Register merged from the `<threat_model>` blocks of plans 01-01 to 01-06; verified at ASVS L1 (grep depth) by `/gsd-secure-phase 1`.

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|---------------|
| github.com → vendor/ | Upstream source enters the repo and later runs as the backend in both modes | Source code (integrity-critical) |
| PyPI / crates.io → .venv, Cargo.lock | Third-party packages execute at install, build and import time | Wheels and crates (integrity-critical) |
| launcher → rsg-server stdin | Handshake JSON configures the Rust frontend | One JSON line: engine limits, eos, upstream SHA (non-secret) |
| local users → /tmp ipc sockets | Upstream's socket paths are predictable files in /tmp | ZMQ msgpack frames (non-secret) |
| user CLI / environment → launcher | CLI args are forwarded to upstream `parse_args`; `RSGLANG_SCHEDULER_FACTORY` selects the scheduler class | Args and env (same-user) |
| launcher → /tmp filesystem | The launcher deletes this run's socket files in a shared directory | File paths |
| launcher ↔ child processes | The launcher owns the lifetime of GPU-holding scheduler processes and rsg-server | Signals, exit codes, stderr tails |
| Rust frontend → Python scheduler | Every frame Rust emits is decoded by upstream's `cls(**kwargs)`; a malformed frame kills the scheduler loop | msgpack frames |
| vendored encoder → committed fixtures | Fixtures are the oracle for byte-exactness | msgpack bytes |
| github.com → pristine comparison tree | The reference copy is fetched on each online `check_upstream.py` run | Git objects (SHA-addressed) |

---

## Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation | Status |
|-----------|----------|-----------|----------|-------------|------------|--------|
| T-01-01 | Tampering | vendor/mini-sglang/ import | high | mitigate | Root tree `02d3e4ad…` pinned in `UPSTREAM.md:11` and `scripts/check_upstream.py:37`; `check_upstream.py --offline` and the online diff both OK on 2026-10-03 | closed |
| T-01-02 | Repudiation | vendor/mini-sglang/LICENSE attribution | medium | mitigate | Copyright line at `vendor/mini-sglang/LICENSE:3`; LICENSE is Tier A and its attribution is asserted in `scripts/check_upstream.py:48,63,287` | closed |
| T-01-03 | Tampering | .venv editable installs pointing at another checkout | low | accept | See Accepted Risks Log AR-01 | closed |
| T-01-04 | Tampering | handshake.rs `parse_handshake` | high | mitigate | `#[serde(deny_unknown_fields)]` at `crates/rsg-server/src/handshake.rs:21`, version check at `:55`, compiled-in SHA at `:16`; CLI tests `extra_key_exits_2`, `handshake_version_2_exits_2`, `sha_mismatch_exits_2_naming_both_shas` green | closed |
| T-01-05 | Denial of Service | rsg-server stdin lifecycle | medium | mitigate | `EXIT_STDIN_EOF = 3` (`crates/rsg-server/src/main.rs:23,161,165`); stdin read on `std::thread::spawn` (`:60`); `stdin_eof_*_exits_3` tests green | closed |
| T-01-06 | Spoofing | /tmp/minisgl_* ipc paths | low | accept | See Accepted Risks Log AR-02 | closed |
| T-01-07 | Elevation of Privilege | `RSGLANG_SCHEDULER_FACTORY` test seam | low | accept | See Accepted Risks Log AR-03 | closed |
| T-01-08 | Denial of Service | `sockets.unlink_run_sockets` | medium | mitigate | Suffix validated by `^[A-Za-z0-9._=-]+$` (`python/rsglang/sockets.py:17`); only this run's five exact paths are unlinked, never a glob (`:28`) | closed |
| T-01-09 | Tampering | `launch.build_parser` | medium | mitigate | `allow_abbrev=False` (`python/rsglang/launch.py:51`); launcher-only flags stripped before forwarding, covered by `test_launch_args.py` | closed |
| T-01-10 | Denial of Service | launcher shutdown | medium | mitigate | `os.setpgid(0, 0)` (`launch.py:129`), group SIGINT (`:193`), group SIGKILL escalation (`:213-220`). Residual: when the launcher is already a pipeline's group leader, `killpg` also signals sibling pipeline members; workaround `setsid` or file redirection; structural fix awaiting decision in `deferred-items.md` | closed |
| T-01-11 | Denial of Service | rsg-wire encode (extra/renamed key, wrong width) | high | mitigate | `rmp_serde::to_vec_named` (`crates/rsg-wire/src/lib.rs:162`), `serde_bytes` buffer (`:61`); `every_fixture_roundtrips_byte_exact` and `hand_built_cases_match_fixtures` (`crates/rsg-wire/tests/fixtures.rs:45,62`) green | closed |
| T-01-12 | Tampering | fixtures/wire freshness | medium | mitigate | `gen_wire_fixtures.py --check` (`scripts/gen_wire_fixtures.py:303`) runs as `check_all.sh` step 3; SHA single-source test (`crates/rsg-wire/tests/fixtures.rs:87-92`) | closed |
| T-01-13 | Denial of Service | orphaned scheduler / rsg-server after launcher crash | high | mitigate | Group SIGKILL escalation (`launch.py:213-220`), `start_parent_watchdog` (`python/rsglang/backend.py:54`), rsg-server exit 3 on stdin EOF; `test_launcher_sigkill_leaves_no_orphans` (`python/tests/test_launch_rust_e2e.py:228`) green on the Mac. The GPU-box `nvidia-smi` confirmation is part of the pending end-of-phase human check (`scripts/gpu_phase1_check.sh`) | closed |
| T-01-14 | Denial of Service | backend that never becomes ready | medium | mitigate | `--ready-timeout` default 900 s (`launch.py:58`); `test_ready_timeout` (`test_launch_rust_e2e.py:198`) green | closed |
| T-01-15 | Information Disclosure | printing child stderr tails | low | accept | See Accepted Risks Log AR-04 | closed |
| T-01-16 | Tampering | vendor/mini-sglang frozen paths | high | mitigate | Tier enforcement plus `UNLISTED_CHANGE` / `STALE_LISTING` (`scripts/check_upstream.py:11-12,196`); run by `check_all.sh` step 5 | closed |
| T-01-17 | Spoofing | pristine fetch from github.com | medium | mitigate | Pristine content addressed by the full 40-char SHA read from `vendor/UPSTREAM_SHA` (`check_upstream.py:260`) and cross-checked against the pinned root tree (`:37`); `--offline` needs no network (`:14,225`) | closed |
| T-01-18 | Repudiation | undocumented vendored edits | medium | mitigate | Every differing path needs an `UPSTREAM.md` row; untracked and uncommitted edits included via `git ls-files --cached --others --exclude-standard` (`check_upstream.py:132`) | closed |
| T-01-19 | Denial of Service | Rust frame rejected by the scheduler | high | mitigate | `test_wire_decode.py` decodes every Rust-encoded case through `BaseBackendMsg`/`BaseTokenizerMsg.decoder` (`:24`) and re-encodes byte-identically (37 passed via `check_wire_decode.sh`); negative control `test_upstream_decoder_rejects_extra_key` (`:64-67`) | closed |
| T-01-SC | Tampering | npm/pip/cargo installs (registered by every plan) | high | mitigate | Blocking-human package gate in 01-01 (approved); `requirements-mac.txt` hash-locked (895 `--hash=sha256` lines) and applied with `uv pip sync` (`scripts/bootstrap_mac_env.sh:38`); crates pinned in `[workspace.dependencies]` (`Cargo.toml:11`) with `Cargo.lock` committed; 01-03, 01-05 and 01-06 add no packages | closed |

*Status: open · closed · open — below high threshold (non-blocking)*
*Severity: critical > high > medium > low — only open threats at or above workflow.security_block_on count toward threats_open*
*Disposition: mitigate (implementation required) · accept (documented risk) · transfer (third-party)*

---

## Accepted Risks Log

| Risk ID | Threat Ref | Rationale | Accepted By | Date |
|---------|------------|-----------|-------------|------|
| AR-01 | T-01-03 | Bootstrap is rerun per checkout, and 01-01 Task 3's verify asserts `minisgl` resolves to this checkout's `vendor/`. Residual risk is a developer manually reusing a venv across checkouts | Plan-time disposition, 01-01-PLAN.md | 2026-10-03 |
| AR-02 | T-01-06 | Upstream's predictable `/tmp/minisgl_*` paths are kept for byte-for-byte topology parity; dev and GPU boxes are single-user; libzmq bind fails loudly if the path is taken | Plan-time disposition, 01-02-PLAN.md | 2026-10-03 |
| AR-03 | T-01-07 | Anyone who can set the launcher's environment can already run code as that user; the default is the unmodified upstream Scheduler and the variable is a documented test seam. No privilege boundary is crossed | Plan-time disposition, 01-03-PLAN.md | 2026-10-03 |
| AR-04 | T-01-15 | Stderr tails go to the operator's own terminal and contain only local log lines; Phase 1 handles no secrets | Plan-time disposition, 01-05-PLAN.md | 2026-10-03 |

*Accepted risks do not resurface in future audit runs.*

---

## Security Audit Trail

| Audit Date | Threats Total | Closed | Open | Run By |
|------------|---------------|--------|------|--------|
| 2026-10-03 | 20 | 20 | 0 | `/gsd-secure-phase 1` (orchestrator, L1 grep depth; short-circuit, no auditor spawned) |

## Security Audit 2026-10-03

| Metric | Count |
|--------|-------|
| Threats found | 20 |
| Closed | 20 |
| Open | 0 |

---

## Sign-Off

- [x] All threats have a disposition (mitigate / accept / transfer)
- [x] Accepted risks documented in Accepted Risks Log
- [x] `threats_open: 0` confirmed
- [x] `status: verified` set in frontmatter

**Approval:** verified 2026-10-03
