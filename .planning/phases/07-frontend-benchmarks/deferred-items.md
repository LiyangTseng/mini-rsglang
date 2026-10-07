# Deferred Items — Phase 07 (Frontend Benchmarks)

Out-of-scope discoveries logged during execution, per the executor's scope
boundary rule (only auto-fix issues directly caused by the current task's
changes). Not fixed here.

## 2026-10-07 — 07-10, running `scripts/check_all.sh --offline`

Two pre-existing, unrelated `pytest python/tests` failures on this Mac,
both from Phase 2 (commits `c35508b`/`eddbf06`/`c53a4b3`), neither touched
by 07-10's Task 1 or Task 2 changes:

- `python/tests/test_baseline_profile.py::test_run_s3_hyperfine_missing_exits_2`
  — expects exit code 2 (hyperfine missing) but gets exit code 1 (a real
  `hyperfine` on this machine's PATH ran and failed its first warmup run
  instead of being absent).
- `python/tests/test_gpu_profile_script.py::test_hyperfine_ok` — after the
  test unlinks its `hyperfine` stub, `hyperfine_ok` still finds a real
  `hyperfine` binary elsewhere on PATH (rc=0) instead of reporting it
  missing (expected rc=1).

Root cause in both cases: this Mac now has a real `hyperfine` installed and
reachable on PATH outside the test's `tmp_path/bin` stub directory (likely
via Homebrew, added in an earlier phase per CLAUDE.md's `hyperfine` 1.20.0
recommendation), which these two tests' "simulate hyperfine missing/broken"
scenarios did not anticipake when written in Phase 2.

Because of these two failures, `scripts/check_all.sh --offline` (which runs
`pytest python/tests -q` as step 2/5 under `set -e`) does not currently
reach `check_all: OK` on this machine. `cargo test --workspace` (step 1/5)
passed cleanly, including 07-10's new `mock_stack` tests.

Status: deferred, not fixed by 07-10 (out of scope: neither file nor the
scripts they test were touched by this plan).
