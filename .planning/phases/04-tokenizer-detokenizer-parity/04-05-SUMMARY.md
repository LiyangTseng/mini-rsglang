---
phase: 04-tokenizer-detokenizer-parity
plan: 05
subsystem: tokenizer
tags: [huggingface, transformers, fixtures, llama, gated-access, python, rsg-tokenizer]

# Dependency graph
requires:
  - phase: 04-tokenizer-detokenizer-parity
    provides: "Plan 04-04's LLAMA_3_2_1B_INSTRUCT ModelSpec/models.py entry, gated-access detection pattern, and the canonical tokenizer_config.json findings (add_bos_token absent, clean_up_tokenization_spaces=true, chat_template calls strftime_now)"
provides:
  - "Three committed Llama-3.2-1B-Instruct golden fixtures (token_ids.json, chat_prompts.json, detok_streams.json), generated from the real gated meta-llama/Llama-3.2-1B-Instruct repo via upstream's own load_tokenizer()"
  - "corpus_chat.py's FROZEN_NOW/_frozen_strftime_now: a generic (template-content-detected, not model-identity-branched) clock freeze for any chat template calling Jinja's strftime_now global"
  - "gen_tokenizer_fixtures.py's GatedAccessError + D-04 clean-skip wiring in generate()/check(): a gated model's auth-shaped load failure (401/403, GatedRepoError/RepositoryNotFoundError) skips cleanly with a SKIP log line and exit 0, while every other failure (any model) still hard-fails as EnvError/exit 2"
affects: ["04-06-PLAN (Rust-side TOK-04 parity tests consuming these three fixtures, plus BOS-count/clean_up_tokenization_spaces assertions)"]

# Actuals (#2632)
actuals:
  tokens: 8530
  tasks: 2
  commits: 2
  plan_head_before: b2f933988d69e8f61885b5053d3bd2b71f7f074c
  plan_head_after: dccbdbafb70dcc366ddc3a963072bf8fbe3b516f

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "strftime_now clock-freeze detection by template content (`\"strftime_now\" in tokenizer.chat_template`), never by model slug/identity -- Qwen3's path is untouched by construction, matching the project's stated 'never a parallel if model == llama code path' constraint"
    - "Auth-shaped-exception classification via cause-chain walk (`exc.__cause__`/`exc.__context__`) rather than a bare except, because transformers.AutoTokenizer.from_pretrained re-wraps huggingface_hub's GatedRepoError/RepositoryNotFoundError inside a plain OSError (`raise ... from e`) -- the original type survives only in `__cause__`"

key-files:
  created:
    - "fixtures/tokenizer/llama-3.2-1b-instruct/token_ids.json"
    - "fixtures/tokenizer/llama-3.2-1b-instruct/chat_prompts.json"
    - "fixtures/tokenizer/llama-3.2-1b-instruct/detok_streams.json"
  modified:
    - "scripts/tokenizer_fixtures/corpus_chat.py"
    - "scripts/gen_tokenizer_fixtures.py"

key-decisions:
  - "Frozen-clock mechanism monkeypatches the `datetime` name inside transformers.utils.chat_template_utils (the module strftime_now's nested closure resolves at call time), rather than passing a `date_string` kwarg to apply_chat_template -- upstream's real tokenize.py call never passes that kwarg, so parity requires matching its exact call signature, not adding a convenience kwarg it never uses."
  - "The override returns the frozen string unconditionally regardless of the requested format (fmt is ignored), mirroring the Rust build_environment(Some(fixed)) override's own 'returns fixed unconditionally' semantics from Plan 04-04, rather than trying to replicate chrono/strftime format-specifier parity inside the override itself."
  - "corpus_ids.py and corpus_detok.py needed no changes: both already read all model-specific values (eos_token_id, encode()) from the live tokenizer object rather than any hardcoded Qwen3 literal, so Task 1's conditional parametrization step was a no-op, confirmed by inspection rather than assumed."
  - "GatedAccessError is raised only when spec.gated is True AND the exception's cause/context chain contains huggingface_hub's GatedRepoError or RepositoryNotFoundError -- confirmed by reading huggingface_hub/utils/_http.py and transformers/utils/hub.py source directly this session, not assumed from the research doc."

patterns-established:
  - "Gated-model fixture generation: treat 'gated access unavailable' as a typed, narrowly-scoped exception (GatedAccessError) produced by walking the real exception's cause chain, never a bare except Exception -- preserves the ability for a genuine bug to still hard-fail the generator."

requirements-completed: [TOK-04]

coverage:
  - id: D1
    description: "Three Llama-3.2-1B-Instruct golden fixtures (token_ids, chat_prompts, detok_streams) generated from the real canonical gated repo via upstream's load_tokenizer(), committed to fixtures/tokenizer/llama-3.2-1b-instruct/"
    requirement: "TOK-04"
    verification:
      - kind: other
        ref: ".venv/bin/python scripts/gen_tokenizer_fixtures.py --check (confirms byte-identical fixtures against a fresh regeneration)"
        status: pass
    human_judgment: false
  - id: D2
    description: "chat_prompts.json's 9 Llama cases are deterministic: strftime_now is frozen to a fixed date string, confirmed by running --check twice in a row with byte-identical output, and by direct inspection that every rendered prompt contains the frozen date substring"
    requirement: "TOK-04"
    verification:
      - kind: other
        ref: "scripts/gen_tokenizer_fixtures.py --check run twice in a row (this session) -- 0 diffs both times"
        status: pass
    human_judgment: false
  - id: D3
    description: "gen_tokenizer_fixtures.py's main CLI skips Llama cleanly (SKIP log line, exit 0) when gated access is genuinely unavailable, while still validating Qwen3's fixtures and never masking a non-auth-shaped failure (e.g. a corrupted Qwen3 fixture) as a false skip"
    requirement: "TOK-04"
    verification:
      - kind: other
        ref: "Live negative test this session: local HF cache for meta-llama/Llama-3.2-1B-Instruct moved aside + HF_HUB_DISABLE_IMPLICIT_TOKEN=1 HF_TOKEN= -- produced a real 401 GatedRepoError, SKIP line printed, exit 0; cache restored, --check then re-validated all three Llama fixtures with no SKIP line"
        status: pass
      - kind: other
        ref: "Deliberately corrupted fixtures/tokenizer/qwen3-0.6b/token_ids.json (temporary, reverted via git checkout) -- --check reported DIFF and exit 1, confirming the skip logic never swallows a genuine Qwen3 failure"
        status: pass
    human_judgment: false

duration: ~30min
completed: 2026-10-07
status: complete
---

# Phase 4 Plan 5: Llama Golden Fixtures + Gated-Access Clean-Skip Summary

**Three real Llama-3.2-1B-Instruct golden fixtures generated via a frozen-clock chat corpus, plus a typed `GatedAccessError`/D-04 clean-skip path wired into `gen_tokenizer_fixtures.py`'s main CLI**

## Performance

- **Duration:** ~30 min
- **Completed:** 2026-10-07T00:49:04Z
- **Tasks:** 2
- **Files modified:** 5 (3 new fixture files, 2 modified scripts)

## Accomplishments

- Generated all three Llama-3.2-1B-Instruct golden fixtures (`token_ids.json` — 16 cases, `chat_prompts.json` — 9 cases, `detok_streams.json`) from the real gated `meta-llama/Llama-3.2-1B-Instruct` repo, via upstream's own `load_tokenizer()`, with live gated access confirmed working in this execution environment before generating anything (a read-only `hf_hub_download` spot-check, matching the task's precondition).
- Added `FROZEN_NOW`/`_frozen_strftime_now` to `corpus_chat.py`: detects whether a model's chat template calls Jinja's `strftime_now` global by inspecting `tokenizer.chat_template`'s text — never by branching on model slug — so Llama's chat-prompt fixture is deterministic (re-running generation twice produces byte-identical output) while Qwen3's generation path (whose template never calls `strftime_now`) is completely unaffected.
- Verified the frozen-clock mechanism directly: monkeypatching the `datetime` name inside `transformers.utils.chat_template_utils` (not passing a `date_string` kwarg, which upstream's real `tokenize.py` call never does) makes two consecutive `apply_chat_template` calls produce byte-identical output containing the frozen date string.
- Added `GatedAccessError` to `gen_tokenizer_fixtures.py`, raised only when a gated model's `load_tokenizer()` failure's cause/context chain contains huggingface_hub's `GatedRepoError` or `RepositoryNotFoundError` — confirmed by reading `huggingface_hub/utils/_http.py` and `transformers/utils/hub.py` source directly (not assumed): `AutoTokenizer.from_pretrained` re-wraps both into a plain `OSError` via `raise ... from e`, so the original exception type survives only in `__cause__`.
- Wired the skip into both `generate()` (returns `(count, skipped_slugs)` now) and `check()` (excludes a skipped model's files from the diff entirely, so its committed fixtures are neither falsely reported "missing" nor falsely "stale").
- Live-verified the full skip path end-to-end (not just a `HF_TOKEN=` env-var check — see Deviations): moved the local HF cache for the Llama repo aside and disabled implicit-token resolution, which produced a genuine 401 `GatedRepoError`; the generator printed `SKIP llama-3.2-1b-instruct: gated access unavailable (...)` and exited 0, with Qwen3's 4 files still validated. Restored the cache, confirmed `--check` validates all three Llama fixtures again with no SKIP line. A deliberately corrupted (then git-reverted) Qwen3 fixture still produced `DIFF`/exit 1, confirming the skip path never masks a genuine Qwen3 regression.

## Task Commits

Each task was committed atomically:

1. **Task 1: Generate Llama fixtures across all three corpora, with a frozen clock for chat prompts** — `901de09` (feat)
2. **Task 2: Wire D-04's clean-skip behavior into the main CLI** — `dccbdba` (feat)

**Plan metadata:** (this commit, created after this SUMMARY)

## Files Created/Modified

- `fixtures/tokenizer/llama-3.2-1b-instruct/token_ids.json` — 16 TOK-01 corpus cases encoded against the real canonical Llama tokenizer.
- `fixtures/tokenizer/llama-3.2-1b-instruct/chat_prompts.json` — 9 TOK-02 chat corpus cases, each containing the frozen `06 Oct 2026` date string instead of the real wall-clock date.
- `fixtures/tokenizer/llama-3.2-1b-instruct/detok_streams.json` — derived CJK/emoji/mixed-script + `finished_eos` detokenize-stream cases (D-09), replayed through upstream's real `DetokenizeManager`.
- `scripts/tokenizer_fixtures/corpus_chat.py` — `FROZEN_NOW` constant, `_frozen_strftime_now` context manager, `generate(tokenizer, now_override=FROZEN_NOW)` signature; `_render_all` extracted as the shared render helper.
- `scripts/gen_tokenizer_fixtures.py` — `GatedAccessError`, `_is_auth_shaped`, `_load_tokenizer_for_fixture`, `_is_under_skipped_model`; `generate()` now returns `(count, skipped_slugs)`; `check()` excludes skipped models' files from its diff; `main()` updated for the new return shape.

## Decisions Made

- Frozen-clock mechanism monkeypatches the `datetime` name inside `transformers.utils.chat_template_utils` rather than passing a `date_string` kwarg to `apply_chat_template` — preserves the exact upstream call signature (`tokenize=False, add_generation_prompt=True`, no extra kwargs), matching `tokenize.py`'s real call rather than injecting a convenience parameter upstream never uses.
- The override returns the frozen string unconditionally regardless of the requested `strftime` format, mirroring the Rust `build_environment(Some(fixed))` override's "returns fixed unconditionally" semantics from Plan 04-04, rather than attempting Python/chrono format-specifier parity inside the override itself (that parity concern belongs to Plan 04-04's `chrono_strftime_matches_python_format` test, not this fixture generator).
- `corpus_ids.py` and `corpus_detok.py` required no changes: inspection confirmed both already read every model-specific value (`eos_token_id`, `encode()`) from the live tokenizer object, never a hardcoded Qwen3 literal — Task 1's conditional "parametrize if needed" instruction found nothing to parametrize.
- `GatedAccessError` is raised only when `spec.gated is True` AND the failure's cause/context chain contains `GatedRepoError`/`RepositoryNotFoundError` — confirmed against real huggingface_hub/transformers source this session, not assumed from RESEARCH.md.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] The plan's literal verify command (`HF_TOKEN=` alone) does not reproduce a gated-access failure in this environment**
- **Found during:** Task 2's verification
- **Issue:** This machine resolves hf-hub credentials via an implicit cached token file at `~/.cache/huggingface/token` (confirmed present), independent of the `HF_TOKEN` environment variable. Running the plan's literal `<automated>` verify command (`HF_TOKEN= .venv/bin/python scripts/gen_tokenizer_fixtures.py --check`) therefore still authenticated successfully and produced no `SKIP` line — it would have reported a false pass/fail mismatch against the plan's stated `fails_when` condition, but more importantly it never actually exercised the skip code path at all. The local HF cache already holding the Llama repo's files compounded this: even adding `HF_HUB_DISABLE_IMPLICIT_TOKEN=1` alone still resolved from cache without hitting the network.
- **Fix:** Temporarily moved the cached `~/.cache/huggingface/hub/models--meta-llama--Llama-3.2-1B-Instruct` directory aside (outside the repo, fully reversible) and re-ran with both `HF_TOKEN=` and `HF_HUB_DISABLE_IMPLICIT_TOKEN=1` set. This produced a genuine live 401 `GatedRepoError`, confirming `GatedAccessError`/`_is_auth_shaped` actually fire correctly on real auth failure, not just in theory. Restored the cache directory immediately after and re-confirmed the normal authenticated path still validates all three Llama fixtures.
- **Files modified:** none (test-environment manipulation only, no repo changes)
- **Verification:** `SKIP llama-3.2-1b-instruct: gated access unavailable (...401 Client Error...)` printed to stderr, exit 0, Qwen3's 4 files still validated during the simulated-unavailable run; normal run afterward showed no `SKIP` line and validated all 7 files.
- **Committed in:** n/a (verification-only; no code change required beyond what Task 2's commit `dccbdba` already contains)

---

**Total deviations:** 1 auto-fixed (1 blocking/verification-methodology)
**Impact on plan:** The deviation was purely in how the skip path was *verified* in this specific environment (which has durable cached credentials/cache files) — the shipped code (`GatedAccessError`, `_is_auth_shaped`, the skip wiring) is unchanged from what Task 2's action text specified. No scope creep.

## Issues Encountered

None beyond the verification-methodology deviation documented above.

## User Setup Required

None — no external service configuration required beyond the already-completed HF gated-access grant (D-02), reused from Plan 04-04.

## Next Phase Readiness

- Plan 04-06 can now parametrize its Rust `token_ids.rs`/`chat_templates.rs`/`detokenize_streams.rs` tests over `llama-3.2-1b-instruct` using these three committed fixtures as the real oracle output.
- The frozen `06 Oct 2026` date string embedded in `chat_prompts.json`'s Llama cases must be matched by Plan 04-06's Rust test harness `strftime_now` override (per Plan 04-04's `build_environment(now_override: Option<String>)`, already built) — use the same literal string, not a re-derived one.
- `gen_tokenizer_fixtures.py --check` is green with gated access available in this environment; the clean-skip path is now live-verified to work correctly when gated access genuinely is not available (e.g. a future CI runner without the author's cached credentials).
- No blockers for Plan 04-06.

---
*Phase: 04-tokenizer-detokenizer-parity*
*Completed: 2026-10-07*

## Self-Check: PASSED

All 6 claimed files (3 new Llama fixtures, `corpus_chat.py`, `gen_tokenizer_fixtures.py`, this SUMMARY.md) confirmed present on disk via direct `[ -f ... ]` checks. Commits `901de09`, `dccbdba`, and the final metadata commit `5db09f9` confirmed present via `git log --oneline --all | grep`.
