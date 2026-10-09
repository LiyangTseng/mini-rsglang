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

**Resolved by 07-10 Task 3** (WINDOWS.md entry 4, marked `fixed`): both
tests' PATH composition was the actual bug -- each prepended its stub
`bin_dir` but kept the rest of `os.environ["PATH"]`, so a real `hyperfine`
elsewhere on PATH (not in the stub dir) was still found by `shutil.which`/
`command -v` after the stub was removed, and the "missing" branch they
meant to simulate never fired. Fixed by excluding, in each PATH, any
directory other than the test's own stub `bin_dir` that itself contains a
real `hyperfine` binary. `.venv/bin/python -m pytest python/tests -q`
passed in full (197 passed, 37 skipped, 0 failed) after this fix plus a
`scripts/bootstrap_mac_env.sh` rerun (this worktree's `.venv` predated
Phase 5 landing via the reconcile and was missing fastapi/uvicorn, needed
by two unrelated Phase 5 tests to spawn the real upstream Python frontend
-- a venv-sync, not a code change).

## 2026-10-07 -- 07-10, running `cargo test --workspace` as part of `check_all.sh --offline`

`cargo test -p rsg-tokenizer --lib` is intermittently flaky:
`loader::tests::gated_access_unavailable_with_blank_token_file` sometimes
panics with "expected GatedAccessUnavailable, got Ok(_)" under the
workspace-wide run, but passes reliably when run alone
(`-- --exact`). Confirmed non-deterministic by re-running
`cargo test -p rsg-tokenizer --lib` three times in a row: 1 failed, 2
passed, no code change between runs.

Root cause: `loader.rs`'s test-only `EnvGuard` mutates process-global
`HF_TOKEN`/`HF_TOKEN_PATH`/`HF_HOME`/`HF_HUB_DISABLE_IMPLICIT_TOKEN` env
vars with no cross-test mutex, and Rust's default test runner executes
multiple `#[test]` functions in the same binary concurrently across
threads -- two EnvGuard-using tests (`gated_access_unavailable_with_blank_
token_file` and `gated_access_unavailable_when_implicit_token_disabled`)
can interleave their save/mutate/restore cycles. This is a pre-existing,
already-documented Phase 4 limitation, not introduced by 07-10:
`crates/rsg-server/src/hf_codec.rs`'s own comment already notes
rsg-tokenizer's test suite "requires --test-threads=1 to be
deterministic" for an identical category of race Phase 4 found and
deferred.

**Resolved by 07-10 Task 3** (WINDOWS.md entry 5, marked `fixed`):
re-running `cargo test --workspace` unchanged did not reliably pass --
two re-runs under full workspace load both reproduced the failure
deterministically, ruling out "just retry" as a viable path. Rule 1
(race condition) applies: fixed surgically inside
`crates/rsg-tokenizer/src/loader.rs`'s own test module with a `static
Mutex<()>` held for each `EnvGuard`'s full lifetime, serializing the
save/mutate/run/restore cycle across the binary's parallel test
threads -- narrower than forcing `--test-threads=1` on
`scripts/check_all.sh`'s shared `cargo test --workspace` step, which
would have slowed every future phase's test gate to fix a bug local to
one test module. Verified with 6 repeated `cargo test -p rsg-tokenizer
--lib` runs, all green.
