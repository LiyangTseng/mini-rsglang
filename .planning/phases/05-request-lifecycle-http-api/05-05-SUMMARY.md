---
phase: 05-request-lifecycle-http-api
plan: 05
subsystem: testing
tags: [python, fastapi, fixtures, golden-tests, api-parity, mock-scheduler]

# Dependency graph
requires:
  - phase: 05-02
    provides: Mac lock with fastapi/uvicorn/prompt_toolkit so upstream's frozen Python frontend module imports unmodified on the Mac
  - phase: 03
    provides: target/debug/mock-scheduler (handshake, echo-token engine, prefill/decode delays) as the scheduler stand-in
provides:
  - "python/rsglang/testing/python_frontend.py: runs upstream's unmodified api_server + tokenize_worker (scheduler ranks left out) against an externally started scheduler stand-in on --rsg-suffix's addresses"
  - "scripts/gen_api_fixtures.py (--out, --check): captures the 18-case API-01 golden fixture set from a live run of the above against mock-scheduler"
  - "committed fixtures/api/ (manifest.json + 15 .body files): the parity oracle plan 05-09's Rust api_parity.rs test replays and byte-diffs"
affects: [05-09]

# Actuals (#2632)
actuals:
  tokens: 13838
  tasks: 2
  commits: 3
  plan_head_before: 4198c5023bcae18c64fed7aff5beb4a3f814f3fa
  plan_head_after: 4ce3a25cf78163de056dbc947f7bc11ea57c87fe

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Golden-fixture-from-a-live-run pattern (D-02), the same shape as scripts/gen_wire_fixtures.py: capture real output from upstream's own frozen code (never a hand-port), normalize only the one genuinely non-deterministic field, exclude the generator-versions block from --check"
    - "A scheduler stand-in test harness reproduces upstream's start_subprocess topology minus the scheduler-rank processes, spawning only the detokenizer/tokenizer tokenize_worker processes against an externally started mock bound on the same ipc addresses"

key-files:
  created:
    - python/rsglang/testing/python_frontend.py
    - python/tests/test_python_frontend.py
    - scripts/gen_api_fixtures.py
    - python/tests/test_gen_api_fixtures.py
    - fixtures/api/manifest.json
    - fixtures/api/models_list.body
    - fixtures/api/v1_get.body
    - fixtures/api/v1_post.body
    - fixtures/api/v1_head.body
    - fixtures/api/v1_options.body
    - fixtures/api/generate_ascii.body
    - fixtures/api/generate_multibyte.body
    - fixtures/api/generate_one_token.body
    - fixtures/api/chat_stream_system_user.body
    - fixtures/api/chat_stream_default_max_tokens.body
    - fixtures/api/chat_nonstream_multibyte.body
    - fixtures/api/chat_nonstream_eos_final.body
    - fixtures/api/chat_nonstream_prompt_text.body
    - fixtures/api/chat_stream_empty_messages_prompt.body
    - fixtures/api/chat_stream_after_errors.body
  modified: []

key-decisions:
  - "The precondition's literal `HF_HUB_OFFLINE=1 AutoTokenizer.from_pretrained(...)` command fails in this environment on a transformers 4.57.3 bug unrelated to cache completeness: `_patch_mistral_regex` unconditionally calls `huggingface_hub.model_info()` for any large-vocab (>100k) repo-id tokenizer unless `_is_local`, and `HF_HUB_OFFLINE=1` turns that call into a hard failure. Verified the precondition's actual named fact (\"the tokenizer files load offline\") directly instead: `AutoTokenizer.from_pretrained('Qwen/Qwen3-0.6B', local_files_only=True)` exits 0 with no network use. Treated the precondition as met and proceeded — this is the same code path upstream's own `load_tokenizer` uses (plain `AutoTokenizer.from_pretrained`, no forced offline mode), so the runner and fixture generator need no special-casing."
  - "`minisgl.__file__` (named in the plan's action text) does not exist: minisgl is a namespace package (no `__init__.py`), so there is no `__file__` to assert against. Used `minisgl.__path__` instead, the same guard `scripts/gen_wire_fixtures.py` already uses for the identical reason (Rule 1 — the literal instruction would have raised `AttributeError` on every invocation)."
  - "chat_nonstream_eos_final's N (max_tokens) is represented in the static CASES table as a sentinel placeholder object, resolved to the real computed value only inside `generate()` once the tokenizer and chat template are loaded — keeps `test_case_table_invariants` a fast, no-IO structural check of the table shape."

patterns-established:
  - "gen_api_fixtures.py's capture/teardown helpers (_spawn_mock, _spawn_frontend, _wait_ready, _teardown) are the reusable, PID/port-scoped harness for any future script that needs a live python_frontend + mock-scheduler run on the Mac"

requirements-completed: [API-01]

coverage:
  - id: D1
    description: "python/rsglang/testing/python_frontend.py runs upstream's unmodified api_server + tokenize_worker (only the scheduler-rank processes left out) against a spawned mock-scheduler stand-in, and serves real HTTP (/v1/models, /generate) on the Mac"
    requirement: "API-01"
    verification:
      - kind: integration
        ref: "python/tests/test_python_frontend.py#test_tracer_python_frontend_serves_generate_against_mock"
        status: pass
    human_judgment: false
  - id: D2
    description: "scripts/gen_api_fixtures.py captures the 18 fixed-order API-01 cases from one fresh live run into fixtures/api/, normalizing only the `created` field (checked within 1 day of capture time), and --check regenerates into a temp dir and byte-diffs with exit 0/1/2"
    requirement: "API-01"
    verification:
      - kind: unit
        ref: "python/tests/test_gen_api_fixtures.py#test_normalize_created_replaces_exactly_one"
        status: pass
      - kind: unit
        ref: "python/tests/test_gen_api_fixtures.py#test_case_table_invariants"
        status: pass
      - kind: integration
        ref: "python/tests/test_gen_api_fixtures.py#test_committed_fixtures_are_fresh"
        status: pass
      - kind: other
        ref: ".venv/bin/python scripts/gen_api_fixtures.py --check (exit 0, 'fixtures match (18 cases)')"
        status: pass
    human_judgment: false
  - id: D3
    description: "The 3 status-only error cases (generate_missing_max_tokens 422, chat_bad_role 422, chat_missing_prompt 500) consume no uid, proven by chat_stream_after_errors landing on uid 9 immediately after them with no gap"
    requirement: "API-01"
    verification:
      - kind: other
        ref: "fixtures/api/manifest.json cases[] uid sequence: 0..9 across the 10 uid-consuming cases in table order, with the 3 status-only cases interleaved consuming none"
        status: pass
    human_judgment: false

duration: 55min
completed: 2026-10-06
status: complete
---

# Phase 05 Plan 05: Python-Frontend API-01 Golden Fixtures Summary

**Upstream's frozen Python frontend (api_server + tokenize_worker) now runs on the Mac against mock-scheduler via `rsglang.testing.python_frontend`, and `scripts/gen_api_fixtures.py` captured 18 fixed request/response pairs from one live run into committed `fixtures/api/` — the byte-exact parity oracle for API-01 that plan 05-09's Rust test replays.**

## Performance

- **Duration:** ~55 min
- **Started:** 2026-10-06
- **Completed:** 2026-10-06
- **Tasks:** 2
- **Files modified:** 20 (19 created, 1 later rewritten in place for GREEN)

## Accomplishments
- `rsglang.testing.python_frontend` reproduces upstream's `start_subprocess` spawn (the detokenizer and, if configured, separate tokenizer `tokenize_worker` processes) minus the scheduler-rank processes, against an externally started scheduler stand-in on the caller's `--rsg-suffix` addresses — nothing upstream is patched or monkeypatched.
- The tracer test proves it end to end: `GET /v1/models` returns the configured model id, and `POST /generate` with `max_tokens=4` streams exactly 4 `data: ` lines before a single-newline `data: [DONE]\n`, with full process-group and socket teardown verified via `psutil`.
- `scripts/gen_api_fixtures.py` captured all 18 cases from one fresh run: 5 `/v1` + `/v1/models` variants, 3 `/generate` variants, 7 `/v1/chat/completions` success variants (streaming and non-streaming, multibyte, EOS-exclusion, prompt-fallback, empty-messages-fallback), and 3 validation/assertion error cases (422/422/500) proven to consume no uid.
- The `chat_nonstream_eos_final` case's `max_tokens` is computed at capture time from the real chat template + tokenizer (index of the first `eos_token_id` in the rendered+encoded prompt, plus one), landing the echoed final token exactly on EOS and exercising detokenize.py's finished+EOS exclusion — the captured body shows the echoed prefix text with no trailing EOS token.
- `--check` regenerates into a temp dir and byte-diffs every `.body` file plus the `model`/`mock_args`/`upstream_sha`/`cases` manifest keys (excluding only the `generator` versions block), matching `scripts/gen_wire_fixtures.py`'s established convention.

## Task Commits

1. **Task 1: Tracer — upstream's unmodified Python frontend served against the mock-scheduler** - `6b2d842` (feat)
2. **Task 2: gen_api_fixtures.py (RED)** - `ea599ea` (test)
3. **Task 2: gen_api_fixtures.py (GREEN)** - `4ce3a25` (feat)

**Plan metadata:** (this commit)

## Files Created/Modified
- `python/rsglang/testing/python_frontend.py` - Runner: `--rsg-suffix`, `_load_upstream` (vendored-import guard), `_make_start_backend` (upstream's tokenizer/detokenizer spawn minus scheduler ranks)
- `python/tests/test_python_frontend.py` - `test_tracer_python_frontend_serves_generate_against_mock`: spawns mock-scheduler then the frontend, proves `/v1/models` and a 4-token `/generate` stream, tears down and asserts no survivor processes/sockets via `psutil`
- `scripts/gen_api_fixtures.py` - `CASES` (18-case table), `normalize_created`, `generate`/`check`/`main`, and the spawn/wait/teardown helpers (`_spawn_mock`, `_spawn_frontend`, `_wait_ready`, `_teardown`)
- `python/tests/test_gen_api_fixtures.py` - `test_normalize_created_replaces_exactly_one`, `test_case_table_invariants`, `test_committed_fixtures_are_fresh`
- `fixtures/api/manifest.json` + 15 `.body` files - the committed API-01 parity oracle

## Decisions Made
- Treated the Task 1 precondition as met via a direct, read-only verification of its named fact rather than its literal suggested command, which fails in this environment on an unrelated transformers bug (see `key-decisions` in frontmatter for the full reasoning).
- Used `minisgl.__path__` instead of the plan's literal `minisgl.__file__` for the vendored-import guard, since `minisgl` is a namespace package with no `__file__` — the same pattern `scripts/gen_wire_fixtures.py` already uses.
- Represented `chat_nonstream_eos_final`'s dynamically-computed `max_tokens` as a sentinel placeholder in the static `CASES` table, resolved only at capture time, so the case-table-invariants test stays a fast structural check with no tokenizer load.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] `minisgl.__file__` does not exist on this namespace package**
- **Found during:** Task 1
- **Issue:** The plan's action text says "Assert `minisgl.__file__` resolves under vendor/mini-sglang/python, else exit 2" — but `minisgl` has no `__init__.py` (it is a PEP 420 namespace package), so `minisgl.__file__` raises `AttributeError` on every invocation, not just the error path.
- **Fix:** Used `minisgl.__path__` (a `_NamespacePath`) instead, checking `Path(list(minisgl.__path__)[0]).resolve().is_relative_to(VENDOR_PY.resolve())` — the identical guard `scripts/gen_wire_fixtures.py` already uses for the same reason.
- **Files modified:** `python/rsglang/testing/python_frontend.py`, `scripts/gen_api_fixtures.py`
- **Verification:** Both the tracer test and the fixture-freshness test exercise this guard path and pass.
- **Committed in:** `6b2d842` (Task 1), `4ce3a25` (Task 2 GREEN)

**2. [Rule 3 - Blocking] Precondition command fails on an unrelated transformers bug; verified the named fact directly instead**
- **Found during:** Task 1, before any task work (precondition check)
- **Issue:** `HF_HUB_OFFLINE=1 .venv/bin/python -c "from transformers import AutoTokenizer; AutoTokenizer.from_pretrained('Qwen/Qwen3-0.6B')"` exits 1 with `OfflineModeIsEnabled`, even though the tokenizer files are fully present in the local HF cache. Root cause: transformers 4.57.3's `_patch_mistral_regex` unconditionally calls `huggingface_hub.model_info(model_id)` for any tokenizer with `vocab_size > 100000` (Qwen3 is 151936) unless the path is literally local (`_is_local`) — a repo-id load always hits this, and `HF_HUB_OFFLINE=1` turns the resulting network call into a hard failure. This is independent of cache completeness.
- **Fix:** Verified the precondition's actual prose fact ("the Qwen/Qwen3-0.6B tokenizer files load offline") with a read-only check that isolates cache-only loading from this unrelated bug: `AutoTokenizer.from_pretrained('Qwen/Qwen3-0.6B', local_files_only=True)` (no `HF_HUB_OFFLINE`), which exits 0 and makes no network call. Treated the precondition as met. The runner and fixture generator both call the tokenizer exactly the way upstream's own `load_tokenizer` does (plain `AutoTokenizer.from_pretrained`, no forced offline mode), so no workaround was needed in shipped code — this was purely a precondition-verification-command issue, not a functional gap.
- **Files modified:** None (verification-only; no shipped code changed as a result)
- **Verification:** Both tasks' tests pass using plain `AutoTokenizer.from_pretrained` calls with real (available) network access in this environment.
- **Committed in:** N/A (no code change)

---

**Total deviations:** 2 auto-fixed (1 bug fix in shipped code, 1 precondition-verification adjustment)
**Impact on plan:** Neither affects API-01 parity scope or the committed fixture content. No scope creep.

## Issues Encountered
- Confirmed Starlette's `JSONResponse` default serialization uses compact `(",", ":")` separators (no spaces), which the `normalize_created` regex (`rb'"created":(\d+)'`, no space after the colon) assumes — verified against the actual captured bytes rather than assumed from memory.
- Confirmed HEAD `/v1` correctly returns a zero-byte body (`v1_head.body` is 0 bytes), matching HTTP HEAD semantics as actually produced by FastAPI/Starlette/uvicorn, not assumed.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- `fixtures/api/manifest.json` and its 15 `.body` files are the committed parity oracle plan 05-09's `crates/rsg-server/tests/api_parity.rs` will replay in the same case order and byte-compare.
- `scripts/gen_api_fixtures.py --check` is ready to be wired into the Mac gate (`scripts/check_all.sh`) by a later plan, the same way `gen_wire_fixtures.py --check` already is.
- No blockers. The case table's `compare`/`normalize`/`uid` fields give plan 05-09 everything it needs to know which bytes to compare, which field to zero out, and which cases are status-only.

---
*Phase: 05-request-lifecycle-http-api*
*Completed: 2026-10-06*

## Self-Check: PASSED
