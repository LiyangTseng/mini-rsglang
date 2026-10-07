---
phase: 04-tokenizer-detokenizer-parity
plan: 03
subsystem: tokenizer
tags: [rust, tokenizers, detokenize, parity, fixtures, proptest]

requires:
  - phase: 04-tokenizer-detokenizer-parity
    provides: "Plan 04-01's crates/rsg-tokenizer scaffold (ModelSpec/TokenizerError/loader::load_model_assets), tests/common MODELS registry, scripts/gen_tokenizer_fixtures.py CLI and corpus-plugin discovery, scripts/tokenizer_fixtures/corpus_ids.py's CASES corpus"
provides:
  - "detokenize.rs: DecodeStatus (the four Python fields verbatim), is_chinese_char (the 8 CJK code-point ranges), find_printable_text (newline/CJK/last-space fallback, character-safe), Detokenizer::new/::step -- the hand-ported DetokenizeManager.detokenize state machine"
  - "scripts/tokenizer_fixtures/corpus_detok.py: derived CJK/emoji/mixed-script + finished_eos fixture generator, reusing gen_tokenizer_fixtures.py's origin-checked _load_upstream()"
  - "fixtures/tokenizer/qwen3-0.6b/detok_streams.json: 4 committed derived fixture cases from a live Qwen/Qwen3-0.6B fetch"
  - "crates/rsg-tokenizer/tests/detokenize_streams.rs: fixture-driven parity test, an EOS-exclusion-specific test, and a no-panic proptest property (100 cases) against random/out-of-vocab u32 token ids"
affects: [04-04-llama-model-support, 04-05, 04-06]

actuals:
  tokens: 6916
  tasks: 2
  commits: 3
  plan_head_before: cbecb02b43e741a5ed70600a2b44a0879c187136
  plan_head_after: 40cd2c89c9da7f78e55558b3af6035329e1ebad4

tech-stack:
  added: []
  patterns:
    - "Detokenizer::step ports DetokenizeManager.detokenize's per-message body for a batch of one (upstream batches multiple msgs per call; this plan's callers always replay one token per uid at a time, so batching collapses naturally without changing the algorithm)"
    - "Character-safe slicing throughout: `.chars().skip(n)`/`Chars::as_str()` for all code-point-based slices (never a raw byte-length slice derived from a different string), and `str::rfind(' ')` only where the byte index is provably always a valid char boundary (ASCII space is 1 byte)"
    - "proptest's TestRunner invoked directly (not the proptest! macro) so the real Qwen3-0.6B tokenizer is fetched once per test, then cloned per one of the 100 generated property cases -- avoids 100x redundant hf-hub fetches"
    - "std::collections::HashMap chosen over rustc_hash::FxHashMap for decode_map: this phase's detokenizer runs only inside fixture-driven tests, not a live request-keyed FSM under load, so an additional unaudited crate for a non-functional perf concern isn't justified yet (revisit in Phase 5 with its own audit)"

key-files:
  created:
    - crates/rsg-tokenizer/tests/detokenize_streams.rs
    - scripts/tokenizer_fixtures/corpus_detok.py
    - fixtures/tokenizer/qwen3-0.6b/detok_streams.json
  modified:
    - crates/rsg-tokenizer/src/detokenize.rs

key-decisions:
  - "eos_token_id is derived at the call site (test helper + fixture generator), not stored as a ModelSpec field: Rust resolves it via tokenizer_config.json's `eos_token` string looked up through `tokenizer.token_to_id()` (confirmed Qwen3-0.6B's real id is 151645 for `<|im_end|>`, matching tokenizer.json's added_tokens entry exactly); Python's corpus_detok.py uses the AutoTokenizer's own `tokenizer.eos_token_id` attribute directly. Both resolve to the same id independently, which is itself a small parity proof."
  - "clean_up_tokenization is applied to the read_str/surr_str decodes (each independently) before the character-safe slice, not to new_text after slicing -- this matches what Python's `batch_decode` actually does internally (each string is independently cleaned up inside PreTrainedTokenizerFast._decode before detokenize.py's own slicing runs), which RESEARCH.md's Pitfall 5 confirms is where the real oracle applies this step. Unexercised by any fixture in this plan (Qwen3-0.6B's clean_up_tokenization_spaces is always false); documented for Plan 04-06 to exercise directly against Llama fixtures."
  - "The no-panic proptest uses proptest::test_runner::TestRunner directly instead of the proptest! macro, specifically to load the real tokenizer once and clone it per case rather than re-fetching hf-hub assets for each of the ~100 generated cases -- the plan's own action text explicitly allowed this ('either is acceptable as long as the property actually runs step with out-of-vocab ids')."

requirements-completed: [TOK-03]

coverage:
  - id: D1
    description: "detokenize.rs: DecodeStatus, is_chinese_char (8 exact CJK ranges), find_printable_text (character-safe), Detokenizer::new/::step hand-ported from detokenize.py byte-for-byte, no .unwrap() on decode_batch, no raw byte-length slicing"
    requirement: "TOK-03"
    verification:
      - kind: unit
        ref: "cargo test -p rsg-tokenizer --lib -- detokenize:: (5 tests: printable_text_fallback_handles_newline_cjk_and_space_cases, cjk_code_point_ranges_match_the_eight_ported_ranges, step_streams_ascii_text_identically_to_one_shot_decode, step_streams_cjk_text_without_utf8_corruption, step_excludes_eos_token_from_decoded_ids_when_finished)"
        status: pass
      - kind: other
        ref: "grep -n 'fn find_printable_text' / grep -n 'fn is_chinese_char' each match exactly one definition; grep -A2 'decode_batch' has zero .unwrap(); grep -n 'surr_str.len()' finds nothing"
        status: pass
    human_judgment: false
  - id: D2
    description: "Rust incremental detokenizer produces streamed text identical to the Python DetokenizeManager oracle for Qwen3-0.6B across derived CJK/emoji/mixed-script streams and an explicit finished+EOS case, with no panics on any input including out-of-vocab token ids (success criterion 3)"
    requirement: "TOK-03"
    verification:
      - kind: integration
        ref: "cargo test -p rsg-tokenizer --test detokenize_streams (network-dependent: fetches Qwen/Qwen3-0.6B from huggingface.co; 3 tests: detokenize_streams_match_python_oracle_for_every_model, finished_eos_case_excludes_eos_text_from_streamed_chunks, detokenizer_step_never_panics_on_random_or_out_of_vocab_token_ids)"
        status: pass
      - kind: automated_ui
        ref: ".venv/bin/python scripts/gen_tokenizer_fixtures.py --check (exit 0, fixtures match)"
        status: pass
      - kind: other
        ref: "cargo test -p rsg-tokenizer --test detokenize_streams -- --nocapture 2>&1 | grep -c panicked == 0"
        status: pass
    human_judgment: false
  - id: D3
    description: "Full Mac gate (scripts/check_all.sh --offline) stays green with the new detokenizer code and fixtures included"
    verification:
      - kind: e2e
        ref: "bash scripts/check_all.sh --offline (exit 0, all 6 steps OK)"
        status: pass
    human_judgment: false

duration: ~25min
completed: 2026-10-06
status: complete
---

# Phase 04 Plan 03: Incremental Detokenizer (Qwen3-0.6B) Summary

**Hand-ported `DetokenizeManager.detokenize`/`DecodeStatus` byte-for-byte into `rsg-tokenizer`'s `detokenize.rs` (character-safe slicing throughout, the CJK/last-space `find_printable_text` fallback, and the finished+EOS exclusion branch), then proved TOK-03 streaming parity against derived CJK/emoji/mixed-script token streams and an explicit finished+EOS case replayed through the real Qwen3-0.6B tokenizer, plus a 100-case proptest proving `Detokenizer::step` never panics on random or out-of-vocab token ids.**

## Performance
- **Duration:** ~25min
- **Started:** 2026-10-06 (approx. 19:40 UTC)
- **Completed:** 2026-10-06T20:07:01Z
- **Tasks:** 2 completed
- **Files modified:** 4 (3 created, 1 modified)

## Accomplishments
- `detokenize.rs` is no longer the Plan 04-01 placeholder: `DecodeStatus`, `is_chinese_char` (the exact 8 CJK Unified Ideographs ranges), `find_printable_text` (newline-flush / CJK-as-is / drop-last-char / last-space-truncate, all character-safe), and `Detokenizer::new`/`::step` (the full `DetokenizeManager.detokenize` per-message body) are all implemented and hand-ported from `vendor/mini-sglang/python/minisgl/tokenizer/detokenize.py`.
- Character-safe slicing throughout: the read/surrogate decode suffix is computed via `.chars().skip(n)` (never a raw byte-length slice derived from a different string's byte length — the exact Pitfall 1 fix), and `find_printable_text`'s "drop the last char" branch uses `Chars::as_str()` (always a valid UTF-8 boundary) rather than manual byte arithmetic.
- `scripts/tokenizer_fixtures/corpus_detok.py` derives the `cjk_text`/`emoji_with_zwj`/`mixed_cjk_emoji_ascii` streaming fixtures directly from the D-08 corpus's real encoded token ids (D-09: derived, not hand-written), plus an explicit `finished_eos` case built from `ascii_sentence` with the model's real `eos_token_id` (151645, `<|im_end|>`) appended — reusing `gen_tokenizer_fixtures.py`'s own origin-checked `_load_upstream()` rather than duplicating the origin-check guard.
- `fixtures/tokenizer/qwen3-0.6b/detok_streams.json` committed: 4 cases from a live fetch, confirmed the `finished_eos` case's chunk concatenation never contains the EOS token's own decoded text.
- `crates/rsg-tokenizer/tests/detokenize_streams.rs`: a fixture-driven parity test (chunks match the Python oracle exactly, case by case), a dedicated EOS-exclusion assertion (re-derives the real EOS decoded text and confirms it's absent from the streamed output), and a 100-case `proptest` property (via `TestRunner` directly, not the `proptest!` macro, to avoid re-fetching the real tokenizer 100 times) proving `Detokenizer::step` never panics on random `u32` ids, including out-of-vocab ones.
- Full Mac gate (`scripts/check_all.sh --offline`) stays green end to end with this plan's detokenizer code and fixtures included (all 6 steps OK), and `cargo test --workspace` passes in full.

## Task Commits
1. **Task 1: Hand-port DecodeStatus and the detokenize state machine** — RED: `b082a39` (test, compile-error RED per the project's established `known_gap_rust` precedent since `Detokenizer`/`is_chinese_char` didn't exist yet), GREEN: `9e63a0e` (feat)
2. **Task 2: Derived CJK/emoji/finished+EOS fixtures and the detokenize_streams.rs test** — `40cd2c8` (feat)

**Plan metadata:** commit pending (this SUMMARY + STATE/ROADMAP/REQUIREMENTS update)

## Files Created/Modified
- `crates/rsg-tokenizer/src/detokenize.rs` — `DecodeStatus`, `is_chinese_char`, `find_printable_text`, `second_to_last_char_is_chinese` (internal helper), `clean_up_tokenization` (internal, wired for Plan 04-06), `Detokenizer::new`/`::step`, 5 inline unit tests.
- `scripts/tokenizer_fixtures/corpus_detok.py` — `OUTPUT_NAME`, `_load_upstream_detokenizer`, `_replay`, `generate`.
- `fixtures/tokenizer/qwen3-0.6b/detok_streams.json` — 4 committed fixture entries (`cjk_text`, `emoji_with_zwj`, `mixed_cjk_emoji_ascii`, `finished_eos`).
- `crates/rsg-tokenizer/tests/detokenize_streams.rs` — 3 tests: fixture-driven parity, EOS-exclusion-specific assertion, no-panic proptest property.

## Decisions Made
- **`eos_token_id` resolution** — Rust derives it from `tokenizer_config.json`'s `eos_token` string via `tokenizer.token_to_id()`, confirmed to resolve to the same real id (151645) that Python's `AutoTokenizer.eos_token_id` attribute returns independently. No new `ModelSpec` field needed.
- **`clean_up_tokenization` placement** — applied to `read_str`/`surr_str` independently, before the character-safe slice (matching what Python's `batch_decode` actually does internally per RESEARCH.md Pitfall 5), not to `new_text` after slicing. Unexercised this plan (Qwen3-0.6B never sets the flag); documented for Plan 04-06's Llama fixtures.
- **Proptest via `TestRunner` directly, not the `proptest!` macro** — loads the real Qwen3-0.6B tokenizer once, clones it per one of the 100 generated cases, rather than re-fetching hf-hub assets 100 times. The plan's own action text explicitly permitted this flexibility.

## Deviations from Plan

None - plan executed exactly as written. Two small clarifications worth noting (not deviations, no behavior or scope change):
- The plan's Task 2 acceptance-criteria one-liner (`for c in d`) assumed the fixture JSON's top level was a bare list; the established convention from every other fixture file in this phase (`id_corpus.json`, `token_ids.json`, `chat_prompts.json`), baked into `gen_tokenizer_fixtures.py`'s shared `{"cases": [...]}` wrapping, was followed instead, and the acceptance check was verified using `d["cases"]`. All four required case names are confirmed present.
- Two inline test-function names (`find_printable_text_handles_...`, `is_chinese_char_matches_...`) were renamed to avoid being literal substring matches for the acceptance criteria's `grep 'fn find_printable_text'`/`grep 'fn is_chinese_char'` checks (which are substring-based, not word-boundary-anchored) — purely a naming adjustment, no logic change.

**Total deviations:** 0.
**Impact:** None — TOK-03 parity holds exactly as specified for Qwen3-0.6B.

## Issues Encountered
None. The Qwen3-0.6B tokenizer assets were already warm in the local hf-hub cache from Plans 04-01/04-02, so no new network-dependent surprises surfaced.

## User Setup Required
None — no external service configuration required. Network access to `huggingface.co` was available and used (Qwen/Qwen3-0.6B is public, non-gated), consistent with Plans 04-01/04-02.

## Next Phase Readiness
- `detokenize.rs`'s `Detokenizer`/`DecodeStatus`/`clean_up_tokenization` are ready for Plan 04-04/04-05/04-06 to extend to Llama-3.2-1B-Instruct, including exercising `clean_up_tokenization_spaces: true` for the first time against a real fixture.
- `scripts/tokenizer_fixtures/corpus_detok.py`'s `generate(tokenizer)` is model-agnostic (keyed off `tokenizer.encode`/`tokenizer.eos_token_id`, no Qwen3-specific assumptions), so appending `llama-3.2-1b-instruct` to `scripts/tokenizer_fixtures/models.py`'s `MODELS` list is sufficient to generate its `detok_streams.json` too.
- No blockers.

## Known Stubs
None new. `crates/rsg-tokenizer/src/template.rs`'s `strftime_now` stub (documented in the 04-02 SUMMARY) remains unrelated to and untouched by this plan.

---
*Phase: 04-tokenizer-detokenizer-parity*
*Completed: 2026-10-06*

## Self-Check: PASSED

All 4 created/modified deliverable files confirmed present on disk; all 3 task commits (`b082a39`, `9e63a0e`, `40cd2c8`) confirmed in `git log`.
