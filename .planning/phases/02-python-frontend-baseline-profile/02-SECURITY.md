---
phase: "02"
slug: "python-frontend-baseline-profile"
status: verified
threats_open: 0
asvs_level: 1
created: "2026-10-05"
verified: "2026-10-05"
---

# Phase 02 — Security

> Per-phase security contract: threat register, accepted risks, and audit trail.

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|----------------|
| PyPI / crates.io → Mac venv / GPU box | Third-party wheels and crates run code at install and import time | Package code, no secrets |
| profiling driver → profiled interpreters | Env vars + PYTHONPATH decide whether instrumentation runs inside the frozen frontend/backend processes | Environment variables only |
| profiled processes → work directory | Hook JSONL files written by profiled processes, read back by the driver | orig_argv, source file paths — no request content |
| driver → py-spy (ptrace) | py-spy reads server-process memory, needs elevated privilege on Linux | Process memory (read-only sampling) |
| driver → server process group | Driver launches GPU-holding processes and must end them | Process lifecycle signals |
| driver → docs/benchmarks/baseline-profile.json | Phase 7 and the RADIX-01 decision treat the sidecar as ground truth | Measured metrics, no secrets |
| load generator → server | Synthetic HTTP load sent to a local server | HTTP requests, no external network |
| hyperfine shell → driver subcommands | Command strings executed by hyperfine's shell | Argv built from driver's own flags |
| operator → GPU box privileges | Attaching py-spy needs CAP_SYS_PTRACE, root, or relaxed ptrace_scope on a shared machine | Privilege grant/revoke |
| wrapper → vendored tree | The run must leave upstream mini-sglang code byte-identical | Filesystem integrity check |
| GPU box → repo | A human carries the measured JSON from the GPU machine into the repo | Measured JSON, no secrets |
| report → Phase 7 / RADIX-01 | Later decisions cite this report's numbers | Narrative + numbers |

---

## Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation | Status |
|-----------|----------|-----------|----------|-------------|------------|--------|
| T-02-01 | Tampering | requirements-mac.txt relock | medium | mitigate | No `--upgrade`; uv keeps existing pins as preferences; Task 2 verify diff fails on any pre-existing pin change; sha256 hashes for every new package. Verified: 02-01-SUMMARY.md confirms all 31 pre-existing pins unchanged after relock. | closed |
| T-02-02 | Tampering | startup hook left active after the phase | medium | mitigate | No venv/site-packages write; shim lives only in a per-run dir on PYTHONPATH; `install()` requires `RSGLANG_PROFILE_DIR`. Verified: `test_env_gate_off_is_inert` passes. | closed |
| T-02-03 | Information Disclosure | hook JSONL files (orig_argv, allocation file paths) | low | accept | Local files in the operator's own work directory; contain only command lines and source paths, no request content or secrets. Accepted risk, no mitigation required. | closed |
| T-02-04 | Denial of Service | hook code raising inside the profiled server | medium | mitigate | gc callback only appends to a deque; thread body, atexit flush, shim `install()` call, and chain-load are each wrapped so no exception escapes into the host process. No crash observed across the real GPU run. | closed |
| T-02-05 | Elevation of Privilege | py-spy attach on the GPU box | high | mitigate | No sudo by default; `--py-spy-sudo` is opt-in, non-interactive `sudo -n`; permission error exits 2 with least-privilege remediation. Verified live: the real GPU box needed no privilege grant at all (ptrace unrestricted there), and the remediation path (`pyspy_can_attach`) was exercised and worked. | closed |
| T-02-06 | Denial of Service | leaked server / GPU processes after discover | high | mitigate | `teardown()` in a `finally` block, SIGINT then SIGKILL. **Code review (02-REVIEW.md CR-01) found `teardown()` caught `ProcessLookupError` but not `PermissionError` on the EPERM-on-reused-pgid race** — fixed in commit `0c78fe6` with regression test `test_teardown_survives_eperm_on_reused_pgid`. Mitigation now complete and verified. | closed |
| T-02-07 | Tampering | malformed discover JSON (ASVS V5) | medium | mitigate | `validate_sidecar` before write, `json allow_nan=False`, atomic `os.replace`; an invalid document is never written. Verified by test suite. | closed |
| T-02-08 | Tampering | malformed or non-GPU sidecar accepted downstream (ASVS V5) | high | mitigate | Full key-by-key schema with cross-field invariants; NaN/Infinity rejected; `require_gpu` demands a Linux run-mode document with a named GPU. Verified: the real GPU sidecar passes `validate --require-gpu`; a Mac/darwin document is rejected (tested). | closed |
| T-02-09 | Repudiation | numbers that cannot be traced to a run | medium | mitigate | `meta` keeps provenance (git_commit, git_dirty, upstream_sha, gpu, py_spy version/rate/flags, clock, created_utc), required by validation. Verified present in the real committed sidecar. | closed |
| T-02-10 | Tampering | speedscope format drift silently zeroing a bucket or the radix share (ASVS V5, RESEARCH A3) | medium | mitigate | `load_speedscope` rejects missing keys and samples/weights length mismatches with `SpeedscopeError`. **Extended during the real GPU run** to also decode with `errors="replace"` rather than crash on invalid UTF-8 in an unresolvable native-frame name (commit `d4272b3`). A related, narrower gap — out-of-range sample frame indices are not yet bounds-checked (02-REVIEW.md WR-02) — is tracked open as non-blocking (warning severity, below the `high` block threshold); it was outside this threat's originally-scoped mitigation (missing keys / length mismatch) and has not been observed to fire. | closed |
| T-02-11 | Denial of Service | load generator pointed at a non-local host | low | mitigate | Base URL is always built as `http://127.0.0.1:<port>`; no host flag exists. Verified by source inspection. | closed |
| T-02-12 | Denial of Service | cold-start runs leaking GPU servers | high | mitigate | `coldstart_once` self-heals a recorded group before launching; `coldstart_stop` sends SIGINT then SIGKILL to the whole tree, returns 1 if anything survives. Verified on the real GPU run (no pgid file or stand-in pid left alive). A narrower edge case — an unparseable/corrupted pgid file is abandoned rather than killed (02-REVIEW.md WR-01) — is tracked open as non-blocking; the common case (valid pgid file) is fully mitigated and was the case exercised. | closed |
| T-02-13 | Elevation of Privilege | CAP_SYS_PTRACE grant / ptrace_scope change left on the GPU box (wrapper-side) | high | mitigate | Wrapper never grants privileges itself; `--help` and the final summary print exact removal commands (`setcap -r`, `getcap` check, ptrace_scope restore). **Verified live on the real GPU run**: `getcap` printed nothing before and after the run — no grant was ever made, and the check that would have caught a lingering one ran and confirmed clean. | closed |
| T-02-14 | Tampering | vendored mini-sglang edited to ease profiling | high | mitigate | Step 4 runs `scripts/check_upstream.py`; the run fails if the tree differs. **Verified live**: step 4 reported PASS on the real GPU run, and `git status --porcelain -- vendor/mini-sglang` was empty at every checkpoint throughout the phase. | closed |
| T-02-15 | Tampering | non-GPU or partial numbers written to the canonical baseline | high | mitigate | `run` refuses the canonical `--out` off a Linux GPU box before launching anything (exit 2); `write_sidecar(require_gpu=True, require_scenarios=requested)` at the canonical path. Verified by `test_run_refuses_canonical_out_off_gpu` and by the real committed JSON passing `validate --require-gpu`. | closed |
| T-02-16 | Denial of Service | py-spy recorders, server trees, or cold-start servers left running | high | mitigate | `stop_all` stops every recorder before raising; `teardown` in `finally` per session; `run_coldstart` calls `coldstart_stop` in `finally` while a pgid file exists. Shares its core mechanism with T-02-06 (see CR-01 fix above). A narrower residual gap — `stop_all()` does not detect a py-spy recorder that somehow survives SIGKILL (02-REVIEW.md WR-05) — is tracked open as non-blocking; no survivor was observed on the real run. | closed |
| T-02-17 | Repudiation | narrative numbers not traceable to the measured JSON | medium | mitigate | `test_baseline_profile_report.py` requires every radix share and P99 TTFT in the JSON to appear verbatim in the markdown, and re-validates the JSON with `require_gpu=True`; the JSON is committed unedited (`git diff` check). Verified: the phase verifier independently re-derived numbers from the real JSON and matched the markdown exactly. | closed |
| T-02-18 | Tampering | shell injection through hyperfine command strings | low | mitigate | Commands are assembled with `shlex.join` from argv lists that come only from the driver's own flags. Verified by source inspection and `test_hyperfine_argv`. | closed |
| T-02-19 | Elevation of Privilege | ptrace grant made for the GPU run left in place afterwards (operator-side) | high | mitigate | Task 1 step 4 removes the setcap grant and restores ptrace_scope; human confirms `getcap` prints nothing. **Verified live**: no grant was ever made (this WSL2 box does not enforce ptrace_scope), and `getcap` was confirmed empty before and after — the threat did not materialize and the check that would have caught it ran clean. | closed |
| T-02-SC | Tampering | npm/pip/cargo installs (recurs across all 9 plans — 02-01 through 02-09) | high | mitigate | Blocking-human package-legitimacy checkpoint (02-01 Task 1) before any install — covers psutil, aiohttp, py-spy, hyperfine, with exact pins and hash-locked Mac lock. Every later plan's new code is either standard-library-only or draws from this same approved, already-installed set — no plan introduced an unapproved dependency. Verified: the human approved all four packages after independent registry-metadata verification (no typosquat variants); GPU-box installs used exactly the approved pins. | closed |

*Status: open · closed · open — below `high` threshold (non-blocking)*
*Severity: critical > high > medium > low — only open threats at or above `workflow.security_block_on` (`high`) count toward `threats_open`*
*Disposition: mitigate (implementation required) · accept (documented risk) · transfer (third-party)*

---

## Accepted Risks Log

| Risk ID | Threat Ref | Rationale | Accepted By | Date |
|---------|------------|-----------|--------------|------|
| AR-02-01 | T-02-03 | Hook JSONL files contain only local command lines and source file paths (no request content, no secrets), written to the operator's own work directory on a dev or GPU box they already control. | Project author (via plan 02-02's authored disposition) | 2026-10-05 |

---

## Security Audit Trail

| Audit Date | Threats Total | Closed | Open | Run By |
|------------|----------------|--------|------|--------|
| 2026-10-05 | 19 (+ T-02-SC recurring across 9 plans) | 19 | 0 | Claude (orchestrator, L1 grep-depth per ASVS level 1 + register_authored_at_plan_time=true short-circuit; register independently cross-checked against the real GPU run's live output — getcap, check_upstream, validate --require-gpu — not just plan-time claims) |

**Note on two threats found genuinely open during code review and fixed before this audit:** T-02-06/T-02-16's shared `teardown()` mechanism had a real gap (CR-01: missing `PermissionError` handling on the EPERM-on-reused-pgid race) — fixed in commit `0c78fe6` with a regression test before this audit ran. Three narrower, non-blocking residual gaps remain open at `warning` severity (below the `high` block threshold) and are tracked in `02-REVIEW-DISPOSITION.md`: WR-01 (coldstart self-heal on a corrupted pgid file), WR-02 (speedscope frame-index bounds), WR-05 (stop_all not detecting a SIGKILL-surviving py-spy process). None were observed to fire on the real GPU run, and none block this phase's `threats_open: 0`.

---

## Sign-Off

- [x] All threats have a disposition (mitigate / accept / transfer)
- [x] Accepted risks documented in Accepted Risks Log
- [x] `threats_open: 0` confirmed
- [x] `status: verified` set in frontmatter

**Approval:** verified 2026-10-05
