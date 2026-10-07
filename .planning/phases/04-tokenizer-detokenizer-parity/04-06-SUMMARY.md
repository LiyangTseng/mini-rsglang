---
phase: 04-tokenizer-detokenizer-parity
plan: 06
subsystem: tokenizer
tags: [rsg-tokenizer, llama, qwen3, bos-token, clean-up-tokenization, parity-tests]

# Dependency graph
requires:
  - phase: 04-tokenizer-detokenizer-parity
    provides: "Plan 04-04's LLAMA_3_2_1B_INSTRUCT ModelSpec/GatedAccessUnavailable loader contract, Plan 04-05's three committed Llama-3.2-1B-Instruct golden fixtures (token_ids.json, chat_prompts.json frozen to '06 Oct 2026', detok_streams.json)"
provides:
  - "count_bos_occurrences(ids, bos_token_id) in encode.rs -- diagnostic-only BOS-occurrence counter, never called from encode_prompt itself"
  - "tests/common/mod.rs's skip_if_gated_unavailable + extended MODELS (now including llama-3.2-1b-instruct, gated: true) -- D-04's clean-skip pattern, reused by all three test files"
  - "All three Rust parity test files (token_ids.rs, chat_templates.rs, detokenize_streams.rs) parametrized over both models, green end to end with HF_TOKEN present and cleanly skipping Llama cases when gated access is unavailable"
  - "Empirical confirmation (this session, against the real meta-llama/Llama-3.2-1B-Instruct tokenizer via transformers 4.57.3) that the real Python oracle itself produces a BOS count of 2, not 1, for every chat-rendered Llama prompt -- documented as a discrepancy from D-10's assumed 'exactly once', asserted in the Rust test against the real count rather than silently normalized"
affects: []

# Actuals (#2632)
actuals:
  tokens: 4436
  tasks: 2
  commits: 3
  plan_head_before: 68bedbdf0d427a138ed558bd055848e6628c33b4
  plan_head_after: cf10ea8

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "strftime_now clock-freeze detection in the Rust test harness, by template content (chat_template.contains(\"strftime_now(\")), never by model slug -- mirrors corpus_chat.py's own detection method exactly, matching the project's 'never a parallel if model == llama code path' constraint in production code"
    - "Gated-model test skip: skip_if_gated_unavailable matches only the TokenizerError::GatedAccessUnavailable enum variant, never a bare Err(_), so a real regression can never be mistaken for D-04's clean skip (T-04-09)"

key-files:
  created:
    - ".planning/phases/04-tokenizer-detokenizer-parity/deferred-items.md"
  modified:
    - "crates/rsg-tokenizer/src/encode.rs"
    - "crates/rsg-tokenizer/src/detokenize.rs"
    - "crates/rsg-tokenizer/tests/common/mod.rs"
    - "crates/rsg-tokenizer/tests/token_ids.rs"
    - "crates/rsg-tokenizer/tests/chat_templates.rs"
    - "crates/rsg-tokenizer/tests/detokenize_streams.rs"

key-decisions:
  - "The real Llama BOS count (checked empirically against the canonical gated tokenizer this session, not assumed) is 2, not D-10's assumed 'exactly once'. This plan's test asserts equality to 2, documenting the discrepancy here rather than silently normalizing Rust's count to 1 or de-duplicating tokens in encode_prompt, per this plan's own must_haves.prohibitions."
  - "render_chat is now passed the model's real bos_token/eos_token strings (read from tokenizer_config.json) for every model, not a hardcoded None/None -- Llama's template interpolates {{- bos_token }} directly, so the prior hardcoded None rendered the literal text 'None' for any Llama case. Qwen3's bos_token is null, so its rendered output is byte-identical to before."
  - "tool_calling_case_prompt_contains_tool_call_tag's <tool_call> substring assertion is scoped to qwen3-0.6b only: Llama's template has no such tag convention (confirmed against the committed Llama chat_prompts.json fixture -- it renders raw function-call JSON with no wrapper), so the test's own documented intent (D-12 point 8, which is Qwen3-specific by its own doc comment) is preserved rather than incorrectly failing for a correctly-rendering Llama."
  - "clean_up_tokenization's 10-step chain (Task 1's other deliverable) was already implemented and wired into Detokenizer::step as of Plan 04-03's commit 9e63a0e -- confirmed via grep (exactly 10 non-comment .replace() calls) rather than assumed. No behavior change needed; only count_bos_occurrences was new work."

patterns-established:
  - "D-04 clean-skip, generalized: skip_if_gated_unavailable<T>(result: &Result<T, TokenizerError>, slug) with no Debug bound on T, since ModelAssets (holding a non-Debug tokenizers::Tokenizer) must be usable with it -- the plan's literal signature (impl std::fmt::Debug bound) would not have compiled against the crate's real ModelAssets type; fixed as a Rule 1 deviation."

requirements-completed: [TOK-04]

coverage:
  - id: D1
    description: "count_bos_occurrences added to encode.rs via RED-GREEN TDD (test(04-06) RED commit against a deliberately-wrong 0-returning stub, feat(04-06) GREEN commit with the real filter-count implementation); never called from encode_prompt itself"
    requirement: "TOK-04"
    verification:
      - kind: unit
        ref: "crates/rsg-tokenizer/src/encode.rs#count_bos_occurrences_counts_matching_elements"
        status: pass
      - kind: unit
        ref: "crates/rsg-tokenizer/src/encode.rs#count_bos_occurrences_returns_zero_when_bos_token_id_is_none"
        status: pass
      - kind: unit
        ref: "crates/rsg-tokenizer/src/encode.rs#count_bos_occurrences_returns_zero_when_no_elements_match"
        status: pass
    human_judgment: false
  - id: D2
    description: "clean_up_tokenization's 10-step chain confirmed already correctly wired (Plan 04-03), conditional on clean_up_tokenization_spaces, with no regression to Qwen3's existing detokenize behavior"
    requirement: "TOK-04"
    verification:
      - kind: other
        ref: "grep -v '^\\s*//' crates/rsg-tokenizer/src/detokenize.rs | grep -c '\\.replace(' == 10"
        status: pass
      - kind: integration
        ref: "cargo test -p rsg-tokenizer --test detokenize_streams qwen3 (0 tests matched by the literal filter, exit 0 -- Qwen3's full detokenize suite re-run separately below confirms no regression)"
        status: pass
    human_judgment: false
  - id: D3
    description: "All three Rust parity test files (token_ids.rs, chat_templates.rs, detokenize_streams.rs) parametrized over Llama-3.2-1B-Instruct, with D-04's clean-skip wired via skip_if_gated_unavailable, green end to end with HF_TOKEN present and with Llama cases cleanly skipped (not failing) when gated credentials are genuinely unavailable"
    requirement: "TOK-04"
    verification:
      - kind: integration
        ref: "cargo test -p rsg-tokenizer -- --test-threads=1 (both models, HF_TOKEN present via this machine's cached credential): 24 passed, 0 failed, 2 ignored"
        status: pass
      - kind: integration
        ref: "HF_TOKEN= HF_HUB_DISABLE_IMPLICIT_TOKEN=1 cargo test -p rsg-tokenizer -- --test-threads=1 --nocapture: 4 'SKIP llama-3.2-1b-instruct: gated access unavailable' lines, exit 0"
        status: pass
    human_judgment: false
  - id: D4
    description: "D-10's BOS-count expectation checked against the real oracle, not assumed: a live empirical check this session against the real canonical meta-llama/Llama-3.2-1B-Instruct tokenizer (via .venv/bin/python, transformers 4.57.3 -- the exact tokenize.py pipeline: apply_chat_template then encode(prompt, add_special_tokens=True)) confirms the real oracle produces BOS count 2, not 1, for all 9 chat-rendered cases. The Rust test asserts equality to 2 and documents this as a discrepancy from D-10's assumed 'exactly once', per this plan's prohibition on silent correction."
    requirement: "TOK-04"
    verification:
      - kind: other
        ref: ".venv/bin/python one-off check this session: tok.encode(case['prompt'], add_special_tokens=True) for all 9 chat_prompts.json cases -> bos_count=2 in every case, first5 ids == [128000, 128000, 128006, ...]"
        status: pass
      - kind: integration
        ref: "crates/rsg-tokenizer/tests/chat_templates.rs#chat_template_renders_match_python_oracle_for_every_model (Llama-only BOS-count assertion, asserts == 2)"
        status: pass
    human_judgment: false

duration: ~25min
completed: 2026-10-07
status: complete
---

# Phase 4 Plan 6: Full Qwen3 + Llama Tokenizer/Detokenizer Parity Suite Summary

**`cargo test -p rsg-tokenizer` green end to end for both Qwen3-0.6B and Llama-3.2-1B-Instruct, with a live-confirmed real-oracle BOS count of 2 (not D-10's assumed 1) for every Llama chat-rendered prompt, and D-04's clean-skip path verified to actually skip (not fail) when gated access is genuinely unavailable**

## Performance

- **Duration:** ~25 min
- **Completed:** 2026-10-07T01:09:46Z
- **Tasks:** 2
- **Files modified:** 6 (4 production/test files + 1 deferred-items.md + this SUMMARY.md)

## Accomplishments

- Added `count_bos_occurrences(ids, bos_token_id)` to `encode.rs` via genuine RED-GREEN TDD: a deliberately-wrong stub (`0` unconditionally) produced a real assertion failure (`left: 0, right: 2`) before the real filter-count implementation made it pass -- not a compile-error RED.
- Confirmed `clean_up_tokenization`'s 10-step `.replace()` chain (Task 1's other named deliverable) was already correctly implemented and wired into `Detokenizer::step` as of Plan 04-03's commit `9e63a0e` -- verified by direct `grep` count (exactly 10 non-comment `.replace()` calls) rather than assumed from the plan text, since re-reading the actual file showed the work already done. No behavior change was needed there.
- Extended `tests/common/mod.rs`'s `MODELS` to include `llama-3.2-1b-instruct` (gated: true), and added `skip_if_gated_unavailable`, matching only the specific `TokenizerError::GatedAccessUnavailable` enum variant (never a bare `Err(_)`), so a real regression can never be mistaken for D-04's clean skip (T-04-09's mitigation).
- Parametrized all three test files (`token_ids.rs`, `chat_templates.rs`, `detokenize_streams.rs`) over both models using the new skip helper. `chat_templates.rs` additionally: detects the `strftime_now` clock-freeze requirement generically by template content (never by model slug, mirroring `corpus_chat.py`'s own detection method) and freezes to the same literal `"06 Oct 2026"` string the committed Llama fixture used; reads the model's real `bos_token`/`eos_token` strings from `tokenizer_config.json` for every model (fixing a pre-existing hardcoded `None, None` that would have rendered the literal text `"None"` for Llama, since Llama's template interpolates `{{- bos_token }}` directly -- Qwen3 is unaffected since its `bos_token` is `null`).
- **Empirically confirmed the real D-10 BOS count, not assumed:** ran a one-off check this session against the real canonical `meta-llama/Llama-3.2-1B-Instruct` tokenizer via `.venv/bin/python` (transformers 4.57.3, the author's cached gated credentials, the exact `tokenize.py` pipeline -- `apply_chat_template` then `tokenizer.encode(prompt, add_special_tokens=True)`). All 9 chat-rendered cases produced **BOS count 2**, with `ids[0] == ids[1] == 128000` (`<|begin_of_text|>`) -- exactly the "double-BOS" mechanism RESEARCH.md's Pitfall 2 and the 04-04 canonical spot-check predicted, now confirmed through the *full* chat-template-then-encode pipeline rather than just the bare-string-encode case 04-04 tested. The Rust `chat_template_renders_match_python_oracle_for_every_model` test's Llama-only assertion checks `count_bos_occurrences` equals 2, documenting this as a discrepancy from D-10's assumed "exactly once" rather than silently normalizing to 1.
- Scoped `tool_calling_case_prompt_contains_tool_call_tag`'s `<tool_call>` substring assertion to `qwen3-0.6b` only (Rule 1 fix): Llama's chat template has no such tag convention at all (confirmed against the committed `chat_prompts.json` fixture -- its `tool_calling` case renders the raw function-call JSON directly, with no wrapper tag), so looping the substring assertion over every model in the now-extended `MODELS` list would have incorrectly failed for a correctly-rendering Llama. The test's own doc comment already named this as a Qwen3-specific check (D-12 point 8); every model still gets the weaker "the `tool_calling` case exists at all" check.
- `cargo test -p rsg-tokenizer -- --test-threads=1` is green end to end: 24 passed (17 lib + 3 chat_templates + 3 detokenize_streams + 1 token_ids), 0 failed, 2 ignored (the two `#[ignore]`d canonical spot-checks from Plan 04-04, which require `--ignored` to run manually). With `HF_TOKEN=` and `HF_HUB_DISABLE_IMPLICIT_TOKEN=1` set (the only combination that actually disables this machine's ambient cached credential -- same deviation 04-05-SUMMARY already documented for this environment), the suite is still green with 4 `SKIP llama-3.2-1b-instruct: gated access unavailable` lines logged, confirming D-04's clean-skip path fires correctly rather than failing the build.

## Task Commits

Each task was committed atomically:

1. **Task 1: clean_up_tokenization chain and BOS-count instrumentation** — `bbef317` (test, RED) + `b5f76b8` (feat, GREEN)
2. **Task 2: Parametrize all three test files over Llama, with D-04 clean-skip** — `cf10ea8` (feat)

**Plan metadata:** (this commit, created after this SUMMARY)

## Files Created/Modified

- `crates/rsg-tokenizer/src/encode.rs` — `count_bos_occurrences`, 3 new unit tests.
- `crates/rsg-tokenizer/src/detokenize.rs` — trivial `cargo fmt` reformat of one call site (no behavior change); `clean_up_tokenization` itself was unchanged (already correct from Plan 04-03).
- `crates/rsg-tokenizer/tests/common/mod.rs` — `MODELS` extended with `llama-3.2-1b-instruct`; new `skip_if_gated_unavailable`.
- `crates/rsg-tokenizer/tests/token_ids.rs` — Llama `ModelSpec` mapping; skip wiring in the per-model loop.
- `crates/rsg-tokenizer/tests/chat_templates.rs` — Llama `ModelSpec` mapping; skip wiring; generic `strftime_now`-freeze detection; real `bos_token`/`eos_token` strings threaded into `render_chat` for every model; Llama-only BOS-count assertion (`== 2`); `tool_calling_case_prompt_contains_tool_call_tag` scoped to Qwen3.
- `crates/rsg-tokenizer/tests/detokenize_streams.rs` — Llama `ModelSpec` mapping; skip wiring in both fixture-driven tests.
- `.planning/phases/04-tokenizer-detokenizer-parity/deferred-items.md` — new: logs a pre-existing, out-of-scope concurrent-test-execution race (see Issues Encountered).

## Decisions Made

- The real Llama BOS count is 2, not D-10's assumed "exactly once" — checked against the live canonical tokenizer this session, documented as a discrepancy rather than silently normalized or fixed via de-duplication in `encode_prompt` (explicitly prohibited by this plan's `must_haves.prohibitions`).
- `render_chat` now receives every model's real `bos_token`/`eos_token` strings (read from `tokenizer_config.json`), not a hardcoded `None, None` — a model-generic change (not an `if slug == llama` branch), since Qwen3's `bos_token: null` makes the new code path produce byte-identical output to before.
- `tool_calling_case_prompt_contains_tool_call_tag`'s tag-substring check is scoped to `qwen3-0.6b`, matching the test's own pre-existing documented intent (D-12 point 8 is specifically about Qwen3's tool-call rendering branch) rather than treating it as a cross-model parity assertion it was never designed to be.
- `skip_if_gated_unavailable`'s actual signature drops the plan's literal `impl std::fmt::Debug` bound on the `Ok` type: `loader::ModelAssets` (the real type this function is called with) holds a `tokenizers::Tokenizer`, which does not implement `Debug` — the plan's literal signature would not have compiled against the crate's real types. Fixed as a Rule 1 deviation (stale signature spec).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] `skip_if_gated_unavailable`'s signature dropped the plan's literal `Debug` bound**
- **Found during:** Task 2
- **Issue:** The plan's action text specified `result: &Result<impl std::fmt::Debug, rsg_tokenizer::TokenizerError>`, but `loader::ModelAssets` (the actual `Ok` type every call site passes) does not implement `Debug` — it holds a `tokenizers::Tokenizer`, which doesn't either (the same constraint `loader.rs`'s own `assert_gated_unavailable` test helper documents from Plan 04-04). The literal signature would not compile against the crate's real types.
- **Fix:** Implemented `skip_if_gated_unavailable<T>(result: &Result<T, TokenizerError>, slug: &str) -> bool` with no `Debug` bound at all — the function never formats the `Ok` side, only matches the `Err` side's specific enum variant, so the bound was never actually needed.
- **Files modified:** `crates/rsg-tokenizer/tests/common/mod.rs`
- **Verification:** Compiles and passes against `Result<ModelAssets, TokenizerError>` at all three call sites.
- **Committed in:** `cf10ea8`

**2. [Rule 1 - Bug] `render_chat`'s hardcoded `None, None` for `bos_token`/`eos_token` would have rendered the literal text "None" for Llama**
- **Found during:** Task 2, first `cargo test -p rsg-tokenizer --test chat_templates` run (`chat_template_renders_match_python_oracle_for_every_model` failed: `left: "None<|start_header_id|>..."`)
- **Issue:** The pre-existing test (written against Qwen3 only, whose `bos_token` is `null`) hardcoded `None, None` for `render_chat`'s `bos_token`/`eos_token` parameters. Llama's chat template interpolates `{{- bos_token }}` directly, so minijinja rendered the literal string `"None"` where `<|begin_of_text|>` belonged.
- **Fix:** Read the model's real `bos_token`/`eos_token` strings from `tokenizer_config.json` for every model (generic, not an `if slug == llama` branch) and pass them into `render_chat`. Qwen3's rendered output is unaffected (its `bos_token` is `null`, resolving to the same `None` as before).
- **Files modified:** `crates/rsg-tokenizer/tests/chat_templates.rs`
- **Verification:** `chat_template_renders_match_python_oracle_for_every_model` passes for both models after the fix.
- **Committed in:** `cf10ea8`

**3. [Rule 1 - Bug] `tool_calling_case_prompt_contains_tool_call_tag` would have incorrectly failed for a correctly-rendering Llama**
- **Found during:** Task 2, reasoning about the extended `MODELS` loop before running the test (confirmed by inspecting the committed Llama `chat_prompts.json` fixture's `tool_calling` case directly)
- **Issue:** The test loops over `common::MODELS` and asserts every model's `tool_calling` case prompt contains the literal substring `<tool_call>`. Llama's template has no such tag convention — it renders the raw function-call JSON with no wrapper at all — so looping the assertion over the newly-extended `MODELS` would fail for Llama despite correct rendering.
- **Fix:** Scoped the substring assertion to `qwen3-0.6b` only, matching the test's own pre-existing doc comment ("the tool-calling case must actually exercise **Qwen3's** tool-call rendering branch") rather than treating it as a universal parity check it was never designed to be. Every model still gets the weaker "the `tool_calling` case exists at all" check.
- **Files modified:** `crates/rsg-tokenizer/tests/chat_templates.rs`
- **Verification:** `tool_calling_case_prompt_contains_tool_call_tag` passes for both models after the fix.
- **Committed in:** `cf10ea8`

---

**Total deviations:** 3 auto-fixed (all Rule 1 - bugs found while wiring the second model into pre-existing, Qwen3-only-shaped test code)
**Impact on plan:** All three were necessary for correctness — the plan's own text anticipated some of this friction ("no case-matching logic needs to be added beyond the skip check" turned out to be only true for two of the three test files; `chat_templates.rs` needed the `bos_token`/`eos_token` and tool-call-tag fixes to actually achieve parity rather than a false pass). No scope creep: all fixes stayed inside this plan's declared `files_modified`.

## Issues Encountered

**Pre-existing concurrent-test-execution race, out of scope (logged, not fixed):** running `cargo test -p rsg-tokenizer` with cargo's default parallel test threads intermittently fails 1-3 of: `loader::tests::gated_access_unavailable_with_blank_token_file` (its own Plan-04-04-authored doc comment already names the cause — `EnvGuard` mutates global env vars, racing a concurrently-running test) and/or `detokenize::tests::step_streams_*`/`step_excludes_eos_token_from_decoded_ids_when_finished` (a cache-lock race between threads concurrently fetching the same `Qwen/Qwen3-0.6B` repo). Neither file is in this plan's `files_modified`, and neither failure is caused by this plan's Llama additions — confirmed reproducible with Llama entirely absent from `MODELS` by running the pre-existing Qwen3-only lib tests alone with default threads. `cargo test -p rsg-tokenizer -- --test-threads=1` is deterministic and green for the full suite (both models). Logged to `.planning/phases/04-tokenizer-detokenizer-parity/deferred-items.md` per the scope-boundary rule, with a recommended follow-up (test-level serialization or a shared fetch cache) for a future, separately-scoped plan.

This machine's HF credential resolution needed both `HF_TOKEN=` AND `HF_HUB_DISABLE_IMPLICIT_TOKEN=1` to actually simulate "no gated credentials" (an ambient cached token at `~/.cache/huggingface/token` makes `HF_TOKEN=` alone insufficient) — the same environment quirk 04-05-SUMMARY already documented. Used both when verifying the D-04 skip path; no code change was needed for this, only the verification command.

## User Setup Required

None — no external service configuration required.

## Next Phase Readiness

- **TOK-04 is now complete** (confirmed via `requirements.mark-complete`): all three declaring plans (04-04, 04-05, 04-06) have finished, and TOK-01 through TOK-03's criteria now also pass for Llama-3.2-1B-Instruct, including the real (not assumed) BOS-count behavior and `clean_up_tokenization_spaces` space-cleanup handling.
- Phase 4 (tokenizer-detokenizer-parity)'s full success criteria are met: `cargo test -p rsg-tokenizer -- --test-threads=1` is green end to end for both models, with Llama cleanly skipping (not failing) when gated access is genuinely unavailable.
- Any future consumer of this crate's Llama BOS-count behavior should use **2** as the documented real-oracle value for a chat-template-rendered-then-encoded prompt, not D-10's originally assumed "exactly once" — this is upstream's real behavior (confirmed against the canonical gated repo via the actual `tokenize.py` pipeline), not a Rust-vs-Python parity gap.
- The pre-existing concurrent-test-execution race (see Issues Encountered / `deferred-items.md`) should be picked up as a small, separately-scoped fix before this crate's CI configuration (if any) assumes default-parallel `cargo test` is reliable.

---
*Phase: 04-tokenizer-detokenizer-parity*
*Completed: 2026-10-07*

## Self-Check: PASSED

All 6 claimed files (encode.rs, detokenize.rs, tests/common/mod.rs, tests/token_ids.rs,
tests/chat_templates.rs, tests/detokenize_streams.rs) plus deferred-items.md and this SUMMARY.md
confirmed present on disk via direct `[ -f ... ]` checks. Commits `bbef317`, `b5f76b8`, and
`cf10ea8` confirmed present via `git log --oneline --all | grep`.
