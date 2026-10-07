# Deferred Items — Phase 06 (gpu-end-to-end-parity)

Out-of-scope discoveries logged per the executor's deviation-rules scope boundary: issues not
directly caused by the current task's changes are recorded here, not fixed.

## 1. `.venv` missing `uvicorn` on this Mac dev box

- **Found during:** Plan 06-06, Task 1 verification (`.venv/bin/python -m pytest python/tests -q`,
  the full suite -- run after the targeted parity test files all passed, to check for broader
  regressions from this plan's changes).
- **Symptom:** `test_gen_api_fixtures.py::test_committed_fixtures_are_fresh` and
  `test_python_frontend.py::test_tracer_python_frontend_serves_generate_against_mock` both fail
  with `No module named 'uvicorn'` when the Python-frontend stand-in (`rsglang.testing.python_frontend`
  / upstream's `minisgl.server.api_server`) tries to import it. Confirmed directly:
  `.venv/bin/python -c "import uvicorn"` raises `ModuleNotFoundError` on this checkout right now.
- **Scope:** Neither test file, nor `uvicorn`, nor any Python-frontend-serving code is in this
  plan's `files_modified` (`scripts/gpu_phase6_parity.sh`, `python/rsglang/parity/sweep.py`,
  `python/tests/test_gpu_phase6_parity_script.py`). `uvicorn==0.54.0` was already approved and
  pinned in `requirements-mac.txt` back in Phase 5 (Plan 05-01's package-legitimacy checkpoint);
  this is a local `.venv` sync gap on this specific checkout, not a regression this plan introduced.
- **Not fixed here:** out of scope per the scope-boundary rule, and a `uv pip install`/`uv sync`
  invocation is exactly the kind of environment mutation a plan execution should not perform as a
  side effect of unrelated work. `scripts/gpu_phase6_parity.sh` and the rest of this plan's Task 1
  deliverables do not depend on `uvicorn` at all (they only invoke `fake_parity_server`/mock-scheduler
  stand-ins, never the real upstream Python frontend) and are unaffected.
- **Recommended follow-up:** re-run `uv venv --python=3.12` + `uv pip sync requirements-mac.txt`
  (or whatever this project's Mac bootstrap step is) on this checkout before trusting a full
  `scripts/check_all.sh`/`pytest python/tests -q` run that needs the real Python frontend.

## 2. Pre-existing `cargo test -p rsg-tokenizer` default-parallel-threads flake (confirmed still present)

- **Found during:** Plan 06-06, Task 1 verification (`bash scripts/check_all.sh --offline`).
- **Symptom:** `loader::tests::gated_access_unavailable_with_blank_token_file` failed under
  cargo's default parallel test threads (`expected GatedAccessUnavailable, got Ok(_)`);
  `cargo test -p rsg-tokenizer --lib -- --test-threads=1` passes 17/17 on the same checkout.
- **Scope:** Already logged in `.planning/phases/04-tokenizer-detokenizer-parity/deferred-items.md`
  item 1, and in `STATE.md`'s Blockers/Concerns. Re-confirmed here only because this plan's own
  `check_all.sh` run hit it; no new information, no files in this plan's scope involved.
- **Not fixed here:** already tracked; see the Phase 04 deferred-items.md entry for the
  recommended follow-up.
