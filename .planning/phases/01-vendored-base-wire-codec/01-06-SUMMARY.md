---
phase: 01-vendored-base-wire-codec
plan: 06
subsystem: testing
tags: [integrity-gate, upstream-diff, frozen-frontend, msgpack, wire-02, pytest, cargo-test]

requires:
  - phase: 01-vendored-base-wire-codec (plan 01-01)
    provides: "vendor/mini-sglang at 9a91cfa (tree 02d3e4ad), UPSTREAM.md with the empty '## Modified files' table, uv .venv"
  - phase: 01-vendored-base-wire-codec (plan 01-03)
    provides: "python/tests suite and pytest config (testpaths, norecursedirs)"
  - phase: 01-vendored-base-wire-codec (plan 01-04)
    provides: "rsg-wire codec, crates/rsg-wire/tests/common/mod.rs case table, fixtures/wire + manifest, gen_wire_fixtures.py --check"
provides:
  - "scripts/check_upstream.py: online pristine diff and --offline tree-hash proof with Tier A/B/C enforcement (stdlib only, exit 0/1/2)"
  - "python/tests/test_check_upstream.py: 24 hermetic tests on temp git repos"
  - "crates/rsg-wire/tests/dump.rs + python/tests/test_wire_decode.py: WIRE-02 decode through upstream's real decoder with byte-identical re-encode"
  - "scripts/check_wire_decode.sh: one command for the Rust dump plus the pytest decode (D-15)"
  - "scripts/check_all.sh [--offline]: phase gate running cargo tests, pytest, fixture freshness, the decode check and check_upstream.py (D-14)"
affects: [phase-01-verification, gpu_phase1_check.sh step 5, phase-03-transport, any future vendored-code change]

actuals:
  tokens: 7596   # chars/4 over the 6 files created (30384 chars)
  tasks: 2
  commits: 3
plan_head_before: 65e16b87d73b7ead87dbee9652e9cd21be4b20d8
plan_head_after: 28cf2e1d6dfb8485ef5ce08ee2e0ab2c54795964

tech-stack:
  added: []
  patterns:
    - "Integrity check compares lstat-level identity (bytes, symlink-ness, symlink target, owner-exec bit) per path against a pristine extraction"
    - "Gate scripts: bash set -euo pipefail, cd to repo root, PYTHON env override, banner per step, final '<name>: OK' line"
    - "Cross-language oracle: Rust dumps encoded frames, upstream's own decoder plus serialize_type must reproduce the bytes"

key-files:
  created:
    - scripts/check_upstream.py
    - python/tests/test_check_upstream.py
    - crates/rsg-wire/tests/dump.rs
    - python/tests/test_wire_decode.py
    - scripts/check_wire_decode.sh
    - scripts/check_all.sh
  modified: []

key-decisions:
  - "check_upstream.py verifies the fetched upstream commit's root tree against KNOWN_TREES before archiving it (T-01-17), so a spoofed fetch fails with exit 2"
  - "check_upstream.py adds three categories beyond the plan's list: UPSTREAM_SHA_INVALID (bad vendor/UPSTREAM_SHA), OFFLINE_UNSUPPORTED (unknown SHA or non-empty table under --offline) and TREE_HASH_MISMATCH (offline committed tree differs)"
  - "A parse error in UPSTREAM.md or an invalid SHA file stops the check before any diff or fetch, so it can never pass silently"
  - "test_wire_decode.py runs inside the full pytest suite and skips without DUMP_DIR; check_wire_decode.sh sets RSGLANG_REQUIRE_DUMP=1 so the gate cannot pass by skipping"

patterns-established:
  - "Vendored-tree checks judge git ls-files --cached --others --exclude-standard under vendor/, so ignored build output (__pycache__, egg-info) and untracked files outside vendor/ never count"
  - "Every gate script ends with a single '<name>: OK' line that callers can grep"

requirements-completed: [BASE-01, WIRE-01, WIRE-02]

coverage:
  - id: D1
    description: "check_upstream.py enforces D-03/D-04: unlisted edits, additions, deletions, symlink and exec-bit changes fail; Tier A/C fail even when listed; Tier B needs shared fix 'yes'; stale listings and UPSTREAM.md parse errors fail; output sorted by path"
    requirement: BASE-01
    verification:
      - kind: unit
        ref: ".venv/bin/python -m pytest python/tests/test_check_upstream.py -q (24 passed)"
        status: pass
    human_judgment: false
  - id: D2
    description: "The real vendored tree is pristine: online diff against freshly fetched 9a91cfa and offline tree-hash check both pass, and nothing under vendor/ was written"
    requirement: BASE-01
    verification:
      - kind: integration
        ref: ".venv/bin/python scripts/check_upstream.py (121 paths compared, 0 listed modifications)"
        status: pass
      - kind: integration
        ref: ".venv/bin/python scripts/check_upstream.py --offline"
        status: pass
      - kind: other
        ref: "test \"$(git write-tree --prefix=vendor/mini-sglang/)\" = 02d3e4ad34ec00c88f549fd9d287a4588958d824"
        status: pass
    human_judgment: false
  - id: D3
    description: "WIRE-02: all 34 Rust-encoded cases decode through upstream's real cls(**kwargs) decoder and re-encode byte-identically; dump is complete, batch order is kept, and the extra-key negative control raises TypeError"
    requirement: WIRE-02
    verification:
      - kind: integration
        ref: "bash scripts/check_wire_decode.sh (cargo dump 34 cases, pytest 37 passed)"
        status: pass
      - kind: unit
        ref: ".venv/bin/python -m pytest python/tests/test_wire_decode.py -q -k rejects_extra_key"
        status: pass
    human_judgment: false
  - id: D4
    description: "scripts/check_all.sh runs cargo tests, pytest, fixture freshness, the decode check and the vendored-tree check together and ends with check_all: OK"
    requirement: WIRE-01
    verification:
      - kind: integration
        ref: "bash scripts/check_all.sh --offline"
        status: pass
      - kind: integration
        ref: "bash scripts/check_all.sh (online)"
        status: pass
    human_judgment: false

duration: 8min
completed: 2026-10-04
status: complete
---

# Phase 1 Plan 06: Integrity Gates Summary

**Stdlib-only `check_upstream.py` proves vendor/mini-sglang equals pristine 9a91cfa (online diff or offline tree hash 02d3e4ad) and locks the frozen frontend tiers. A Rust dump plus pytest decode proves all 34 Rust-encoded frames pass upstream's real decoder byte-for-byte. `check_all.sh` runs the whole phase suite from one command.**

## Performance

- **Duration:** 8 min
- **Started:** 2026-10-04T04:00:03Z
- **Completed:** 2026-10-04T04:08:04Z
- **Tasks:** 2
- **Files modified:** 6 (all new)

## Accomplishments
- `scripts/check_upstream.py`:
  - Online mode fetches upstream into a per-run temp dir with `git clone --filter=blob:none --no-checkout`, checks the commit's root tree, and runs `git archive`.
  - It compares all 121 vendored paths by bytes, symlink-ness, symlink target and owner-exec bit.
  - It enforces Tier A/C (never), Tier B (shared fix "yes" only), UNLISTED_CHANGE, STALE_LISTING and LICENSE_MISSING. It also checks the UPSTREAM.md SHA and table format.
  - `--offline` checks the committed tree hash and that vendor/ is clean.
- 24 hermetic tests in `python/tests/test_check_upstream.py`. They run on temp git repos built from `git archive` of the committed vendored tree, so they keep the `.dockerignore` symlink and need no network.
- WIRE-02 runs from one command (`scripts/check_wire_decode.sh`). The Rust dump writes 34 frames, and pytest decodes each through `BaseBackendMsg`/`BaseTokenizerMsg.decoder`, re-encodes it with `serialize_type`, and compares the bytes to the frame and the committed fixture. It also checks dump completeness, batch element order, and the extra-key negative control.
- `scripts/check_all.sh [--offline]` is the phase gate. It is green both offline and online on the Mac.

## Task Commits

1. **Task 1 (RED): failing hermetic tests for check_upstream.py** - `23fb181` (test)
2. **Task 1 (GREEN): check_upstream.py** - `230f133` (feat)
3. **Task 2: WIRE-02 decode check and check_all.sh** - `28cf2e1` (feat)

No REFACTOR commit was needed.

## Files Created/Modified
- `scripts/check_upstream.py` - pristine diff, offline tree-hash proof, tier enforcement (exit 0 OK, 1 violations, 2 environment)
- `python/tests/test_check_upstream.py` - 24 hermetic tests, including the offline case on the real repo
- `crates/rsg-wire/tests/dump.rs` - `dump_rust_encodings`: writes `<DUMP_DIR>/<case>.msgpack` from hand-built Rust values and asserts the count matches the manifest
- `python/tests/test_wire_decode.py` - decode, re-encode equality, completeness, batch order, negative control; `RSGLANG_REQUIRE_DUMP=1` turns a missing dump into a failure
- `scripts/check_wire_decode.sh` - temp `DUMP_DIR`, cargo dump, then pytest; prints `check_wire_decode: OK`
- `scripts/check_all.sh` - 5-step phase gate; prints `check_all: OK`

## Verification Results
- `pytest python/tests/test_check_upstream.py -q`: 24 passed
- `scripts/check_upstream.py --offline`: `check_upstream: OK (offline, tree 02d3e4ad... matches pristine 9a91cfa, 0 listed modifications)`
- `scripts/check_upstream.py` (online): `check_upstream: OK (121 paths compared, 0 listed modifications)`. The temp dir was removed afterwards.
- `git write-tree --prefix=vendor/mini-sglang/` = `02d3e4ad34ec00c88f549fd9d287a4588958d824`
- `scripts/check_wire_decode.sh`: dumped 34 cases, 37 passed
- Sanity checks done on throwaway dumps:
  - Injecting an extra key into one dumped frame makes that case fail with `TypeError`.
  - `RSGLANG_REQUIRE_DUMP=1` without `DUMP_DIR` errors every dump-dependent test.
- `scripts/check_all.sh --offline` and `scripts/check_all.sh`: both exit 0 and end with `check_all: OK`. The full pytest step reports 59 passed and 36 skipped. The skips are the dump-dependent decode tests, which step 4 runs with a dump.
- `cargo fmt --check` and `cargo clippy -p rsg-wire --all-targets -D warnings` are clean.

## Decisions Made
- The fetched commit's root tree is checked against `KNOWN_TREES` before use. This is the T-01-17 mitigation at fetch time, not only in offline mode.
- Offline mode reuses `UNLISTED_CHANGE` for each uncommitted or untracked path under vendor/. It has its own categories for the cases it cannot judge (`OFFLINE_UNSUPPORTED`) and for a committed-tree mismatch (`TREE_HASH_MISMATCH`).
- A parse error or an invalid SHA file stops the check before any fetch.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Critical] Named categories for failure modes the plan described but did not name**
- **Found during:** Task 1
- **Issue:** The plan says an invalid `vendor/UPSTREAM_SHA`, an unknown SHA or non-empty table under `--offline`, and a wrong offline tree hash must fail, but its category list has no names for these.
- **Fix:** Added `UPSTREAM_SHA_INVALID`, `OFFLINE_UNSUPPORTED` and `TREE_HASH_MISMATCH`. All of them exit 1. The planned categories are unchanged.
- **Files modified:** scripts/check_upstream.py
- **Committed in:** 230f133

**2. [Rule 2 - Missing Critical] Verify the fetched commit's root tree**
- **Found during:** Task 1
- **Issue:** T-01-17 calls for SHA-addressed pristine content. The online path trusted the clone without checking the tree.
- **Fix:** `git rev-parse <sha>^{tree}` must equal `KNOWN_TREES[sha]`. Otherwise the check exits 2.
- **Files modified:** scripts/check_upstream.py
- **Committed in:** 230f133

**3. [Rule 3 - Blocking] clippy dead_code on the shared test helper**
- **Found during:** Task 2
- **Issue:** `cargo clippy -D warnings` failed because the dump test binary does not use `common::hex`.
- **Fix:** Put `#[allow(dead_code)]` on `mod common;` in dump.rs. The shared module from 01-04 was not changed.
- **Files modified:** crates/rsg-wire/tests/dump.rs
- **Committed in:** 28cf2e1

**4. [Rule 2 - Missing Critical] Tests for must-have truths not covered by the task's test list**
- **Found during:** Tasks 1 and 2
- **Issue:** Several must-have truths had no test in the task's behavior list:
  - exec-bit change
  - a Tier A directory member
  - an unlisted Tier B edit
  - a bad yes/no value
  - the UPSTREAM.md SHA check and the LICENSE check
  - offline failure modes on temp repos
  - "never writes under vendor/"
  - "decoded batch data keeps element order"
- **Fix:** Added tests for each (`test_check_upstream.py` has 24 tests; `test_batch_decode_keeps_element_order`).
- **Committed in:** 23fb181, 28cf2e1

---

**Total deviations:** 4 auto-fixed (3 missing critical, 1 blocking)
**Impact on plan:** Stricter checks and more coverage. No scope creep and no new dependencies.

## TDD Gate Compliance
Task 1 (`tdd="true"`):
- RED `23fb181`: all 24 tests failed because the script was absent.
- GREEN `230f133`: all 24 pass.
- No refactor was needed.

## Issues Encountered
None.

## User Setup Required
None. No external service configuration is required. The online check needs network access to github.com, and it exits 2 when that is unreachable.

## Next Phase Readiness
- Phase 1's Mac-side gates are complete. `scripts/check_all.sh` is the phase gate.
- Step 5 of `scripts/gpu_phase1_check.sh` now has its `scripts/check_upstream.py`.
- Phase 1 sign-off still waits on the human GPU run. That is WINDOWS.md entry 1, which stays open.

---
*Phase: 01-vendored-base-wire-codec*
*Completed: 2026-10-04*

## Self-Check: PASSED
