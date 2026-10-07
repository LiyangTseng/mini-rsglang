---
phase: 04-tokenizer-detokenizer-parity
verified: 2026-10-07T01:34:24Z
status: passed
score: 4/4 must-haves verified
covered_files:
  - .planning/phases/04-tokenizer-detokenizer-parity/04-01-PLAN.md
  - .planning/phases/04-tokenizer-detokenizer-parity/04-01-SUMMARY.md
  - .planning/phases/04-tokenizer-detokenizer-parity/04-02-PLAN.md
  - .planning/phases/04-tokenizer-detokenizer-parity/04-02-SUMMARY.md
  - .planning/phases/04-tokenizer-detokenizer-parity/04-03-PLAN.md
  - .planning/phases/04-tokenizer-detokenizer-parity/04-03-SUMMARY.md
  - .planning/phases/04-tokenizer-detokenizer-parity/04-04-PLAN.md
  - .planning/phases/04-tokenizer-detokenizer-parity/04-04-SUMMARY.md
  - .planning/phases/04-tokenizer-detokenizer-parity/04-05-PLAN.md
  - .planning/phases/04-tokenizer-detokenizer-parity/04-05-SUMMARY.md
  - .planning/phases/04-tokenizer-detokenizer-parity/04-06-PLAN.md
  - .planning/phases/04-tokenizer-detokenizer-parity/04-06-SUMMARY.md
  - .planning/phases/04-tokenizer-detokenizer-parity/04-REVIEW-DISPOSITION.md
  - .planning/phases/04-tokenizer-detokenizer-parity/04-REVIEW.md
  - Cargo.toml
  - crates/rsg-tokenizer/Cargo.toml
  - crates/rsg-tokenizer/src/detokenize.rs
  - crates/rsg-tokenizer/src/encode.rs
  - crates/rsg-tokenizer/src/lib.rs
  - crates/rsg-tokenizer/src/loader.rs
  - crates/rsg-tokenizer/src/template.rs
  - crates/rsg-tokenizer/tests/chat_templates.rs
  - crates/rsg-tokenizer/tests/common/mod.rs
  - crates/rsg-tokenizer/tests/detokenize_streams.rs
  - crates/rsg-tokenizer/tests/token_ids.rs
  - scripts/check_all.sh
  - scripts/gen_tokenizer_fixtures.py
  - scripts/tokenizer_fixtures/__init__.py
  - scripts/tokenizer_fixtures/corpus_chat.py
  - scripts/tokenizer_fixtures/corpus_detok.py
  - scripts/tokenizer_fixtures/corpus_ids.py
  - scripts/tokenizer_fixtures/models.py
covered_digest: "v2:sha256:9118067aab4ca98d4b9620d4b628807a5c9dff93997070466eba3d52fb483dc5"
behavior_unverified: 0
overrides_applied: 0
---

# Phase 04: Tokenizer/Detokenizer Parity Verification Report

**Phase Goal:** Rust produces the same token ids from requests, and the same text from token
streams, as the Python frontend, for Qwen3-0.6B and one Llama-3.x model. Parity is proven entirely
on the Mac.
**Verified:** 2026-10-07T01:34:24Z
**Status:** passed
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

All four roadmap success criteria were independently re-executed against the real codebase — not
inferred from SUMMARY.md — including the Llama-3.2-1B-Instruct gated path, run twice in this
session: once with the machine's cached HF credential present (full real assertions, zero skips)
and once with it explicitly disabled (`HF_TOKEN= HF_HUB_DISABLE_IMPLICIT_TOKEN=1`, confirming D-04's
clean-skip fires correctly instead of masking a failure as a pass).

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | On a test corpus, Rust tokenization produces token ids identical to the Python frontend's for Qwen3-0.6B | ✓ VERIFIED | `cargo test -p rsg-tokenizer --test token_ids -- --test-threads=1`: `token_ids_match_python_oracle_for_every_model` passed. 16-case `fixtures/tokenizer/id_corpus.json` (empty string, whitespace-only, CJK, emoji+ZWJ, NFD-normalized `café` via `unicodedata.normalize`, URL/code/JSON-like text, degenerate repeats) asserted byte-for-byte against `fixtures/tokenizer/qwen3-0.6b/token_ids.json`, produced by the vendored `load_tokenizer()` oracle. |
| 2 | For every chat conversation in the corpus, Rust chat-template rendering produces a prompt string identical to the Python frontend's for Qwen3-0.6B | ✓ VERIFIED | `chat_templates.rs`'s `chat_template_renders_match_python_oracle_for_every_model` passed for Qwen3 across all 9 fixture entries (D-12's 8 shapes, with case 4 split into two distinct entries: `system_empty_string` vs `system_missing_key`). Tool-calling case (`tool_calling`) exercises a real `tool_calls`-bearing message against Qwen3's live-fetched `chat_template.jinja`, asserted by `tool_calling_case_prompt_contains_tool_call_tag`. |
| 3 | Given the same token streams, the Rust incremental detokenizer and the Python frontend produce identical streamed text, including CJK and emoji split across tokens, with no UTF-8 breakage and no panics | ✓ VERIFIED | `detokenize_streams.rs`: `detokenize_streams_match_python_oracle_for_every_model` (CJK/emoji/mixed-script replay) and `finished_eos_case_excludes_eos_text_from_streamed_chunks` (EOS-exclusion + per-uid map cleanup) passed. `detokenizer_step_never_panics_on_random_or_out_of_vocab_token_ids` is a real `proptest::test_runner::TestRunner`-driven property over random `u32` ids (including out-of-vocab), not a fixture replay — passed, no panic. Char-safe slicing (`.chars().count()`/`.chars().skip(...)`) confirmed throughout `detokenize.rs`; no raw byte-length slice pattern found. |
| 4 | Criteria 1-3 also pass for one Llama-3.x model, including its BOS and space-cleanup cases | ✓ VERIFIED | Re-ran the full suite twice in this session. **With gated access available** (this machine's cached `~/.cache/huggingface/token`): zero `SKIP` lines logged; all three test files' Llama-3.2-1B-Instruct branches ran their real assertions, including `chat_templates.rs`'s live BOS-occurrence check (`count_bos_occurrences(&ids, bos_id) == 2`, the real-oracle-confirmed count, not D-10's assumed 1) and `detokenize.rs`'s `clean_up_tokenization` 10-step `.replace()` chain (applied because Llama's `tokenizer_config.json` sets `clean_up_tokenization_spaces: true`; confirmed inert for Qwen3, which sets it `false`). Result: 24 passed, 0 failed, 2 ignored (the two `#[ignore]`d canonical spot-checks, which also passed when re-run with `--ignored`). **With gated access forced unavailable**: 4 `SKIP llama-3.2-1b-instruct: gated access unavailable` lines logged (D-04's clean-skip), Qwen3 cases still ran and passed, exit 0. |

**Score:** 4/4 truths verified (0 present-but-behavior-unverified)

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `crates/rsg-tokenizer/Cargo.toml`, `src/lib.rs` | Workspace member crate | ✓ VERIFIED | `Cargo.toml` workspace `members = ["crates/*"]` includes it; crate compiles and its tests run under `cargo test -p rsg-tokenizer`. |
| `crates/rsg-tokenizer/src/loader.rs` | hf-hub fetch + gated-access detection | ✓ VERIFIED | `load_model_assets` returns `TokenizerError::GatedAccessUnavailable` distinctly (confirmed live, both branches exercised this session); no silent fallback to a different model on fetch failure (prohibition from 04-01 must_haves) — code path only ever raises `TokenizerError`, never substitutes. |
| `crates/rsg-tokenizer/src/encode.rs` | encode + chat-prompt + BOS counting | ✓ VERIFIED | `encode_text`, `encode_prompt`, `count_bos_occurrences` all present, unit-tested, and exercised by integration tests against real fixtures. |
| `crates/rsg-tokenizer/src/template.rs` | minijinja env, `raise_exception`/`strftime_now` | ✓ VERIFIED | `build_environment(now_override)`; `strftime_now_without_override_calls_the_real_clock` and `strftime_now_override_returns_fixed_string_regardless_of_format` both pass, confirming the override is opt-in only (prohibition from 04-04 honored — no silent production leak). |
| `crates/rsg-tokenizer/src/detokenize.rs` | DecodeStatus state machine, `find_printable_text`, `clean_up_tokenization` | ✓ VERIFIED | Hand-ported (not `tokenizers::DecodeStream`, explicit doc comment states why); char-safe slicing confirmed; 10-step `.replace()` chain present and gated by `clean_up_tokenization_spaces`. |
| `scripts/gen_tokenizer_fixtures.py` + `scripts/tokenizer_fixtures/*.py` | Python oracle fixture generator | ✓ VERIFIED | `.venv/bin/python scripts/gen_tokenizer_fixtures.py --check` → `gen_tokenizer_fixtures: fixtures match (7 files)`, exit 0, run directly in this session. Origin-check guard (`origin.is_relative_to(VENDOR_PY)`) confirmed present, raising `EnvError` on a non-vendored `minisgl`. |
| `fixtures/tokenizer/{qwen3-0.6b,llama-3.2-1b-instruct}/{token_ids,chat_prompts,detok_streams}.json` | Committed golden fixtures, both models | ✓ VERIFIED | All 6 files present; Llama's `chat_prompts.json` contains the frozen-clock literal `"06 Oct 2026"` from `corpus_chat.py`'s `FROZEN_NOW` (not system time at generation), confirming the clock-freeze mechanism is real, not merely claimed. |
| `scripts/check_all.sh` | Phase gate wiring fixture freshness | ✓ VERIFIED | Step 4 explicitly runs `gen_tokenizer_fixtures.py --check`, confirmed by direct read of the script. |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|----|--------|---------|
| `loader.rs` (hf-hub fetch) | `encode.rs::encode_text` | `tokenizers::Tokenizer::from_file` → `encode_text` | ✓ WIRED | `token_ids.rs` test calls `load_model_assets` then `encode::encode_text(&assets.tokenizer, text)`, asserting against fixtures. |
| `template.rs` minijinja `Environment` | `encode.rs::encode_prompt`'s `PromptInput::Chat` branch | direct call | ✓ WIRED | `chat_templates.rs` builds the env, calls `template::render_chat`, then separately calls `encode::encode_prompt` for the BOS-count check — both paths traced and exercised. |
| `tests/common/mod.rs` `MODELS` list | all three test files' per-model loops | `skip_if_gated_unavailable` | ✓ WIRED | Confirmed by direct test run: Llama branch runs for real when gated access is available, skips cleanly (matched on the specific `GatedAccessUnavailable` enum variant, never a bare `Err(_)`) when it is not. |
| `detokenize.rs`'s `clean_up_tokenization_spaces` field | `ModelAssets`/`tokenizer_config.json` | read, not hardcoded | ✓ WIRED | Confirmed generic (reads the model's own config value); Qwen3 (`false`) and Llama (`true`) both exercised in the same test run with no per-model branch in the test harness. |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| TOK-01..03, Qwen3 | `cargo test -p rsg-tokenizer -- --test-threads=1` | 24 passed, 0 failed, 2 ignored | ✓ PASS |
| TOK-04, Llama, gated access available | same command, implicit cached HF token present | 24 passed, 0 failed, 2 ignored; 0 `SKIP` lines | ✓ PASS |
| TOK-04, D-04 clean-skip, gated access forced unavailable | `HF_TOKEN= HF_HUB_DISABLE_IMPLICIT_TOKEN=1 cargo test -p rsg-tokenizer -- --test-threads=1 --nocapture` | 4 `SKIP llama-3.2-1b-instruct: gated access unavailable` lines; still exit 0, Qwen3 unaffected | ✓ PASS |
| Python oracle fixture freshness | `.venv/bin/python scripts/gen_tokenizer_fixtures.py --check` | `gen_tokenizer_fixtures: fixtures match (7 files)`, exit 0 | ✓ PASS |
| No debt markers in phase-scope files | `grep -rn -E "TBD\|FIXME\|XXX"` across `crates/rsg-tokenizer`, `scripts/tokenizer_fixtures`, `scripts/gen_tokenizer_fixtures.py`, `scripts/check_all.sh` | no matches | ✓ PASS |
| Clippy sanity | `cargo clippy -p rsg-tokenizer --tests` | 4 warnings, all pre-existing style nits already logged as IN-02 (open) | ✓ PASS (no new issues) |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|--------------|-------------|--------------|--------|----------|
| TOK-01 | 04-01 | Rust tokenization ids identical to Python oracle, Qwen3-0.6B | ✓ SATISFIED | `token_ids.rs` passed, 16-case corpus |
| TOK-02 | 04-02 | Rust chat-template rendering identical to Python oracle, Qwen3-0.6B | ✓ SATISFIED | `chat_templates.rs` passed, 9-entry corpus incl. tool-calling |
| TOK-03 | 04-03 | Rust incremental detokenization identical, no UTF-8 breakage/panics | ✓ SATISFIED | `detokenize_streams.rs` passed, incl. proptest no-panic property |
| TOK-04 | 04-04, 04-05, 04-06 | TOK-01..03 also pass for Llama-3.2-1B-Instruct | ✓ SATISFIED | Full suite re-run twice this session, both branches (available/unavailable gated access) behave correctly |

No orphaned requirements: REQUIREMENTS.md's Phase 4 traceability lists exactly TOK-01 through
TOK-04, all four are claimed by this phase's plans, and all four are marked `Complete` in
REQUIREMENTS.md, consistent with the evidence above.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| `crates/rsg-tokenizer/src/loader.rs:198-297` | WR-01 (04-REVIEW.md) | `EnvGuard`-based gated-credential unit tests race under default parallel `cargo test` | ⚠️ Warning (logged, open) | Does not affect parity correctness; mitigated by the documented `--test-threads=1` reliable invocation, which this verification used and confirmed deterministic. `scripts/check_all.sh` step 1 still runs plain `cargo test --workspace` without the flag — a latent CI-flakiness risk, not a goal blocker. |
| `crates/rsg-tokenizer/src/encode.rs:50-57` | WR-02 (04-REVIEW.md) | `encode_prompt` reads `bos_token`/`eos_token` only from `tokenizer_config.json`, no `special_tokens_map.json` fallback | ⚠️ Warning (logged, open) | Unexercised by both current fixture models (Qwen3, Llama both set these directly); would only bite a future third model, out of this phase's scope. |
| `crates/rsg-tokenizer/src/encode.rs:49` | WR-03 (04-REVIEW.md) | `MissingChatTemplate` error always reports `slug: "<unknown>"` | ⚠️ Warning (logged, open) | Diagnostic-quality only, never triggered by a passing fixture. |
| `crates/rsg-tokenizer/tests/chat_templates.rs:28`, `scripts/tokenizer_fixtures/corpus_chat.py:30` | IN-01 (04-REVIEW.md) | Duplicated `FROZEN_NOW` literal across Rust/Python | ℹ️ Info (logged, open) | Fails loudly (not silently) if the two literals ever diverge; confirmed real and correctly wired in this session. |
| `crates/rsg-tokenizer/src/detokenize.rs:142`, `src/loader.rs:56-60` | IN-02 (04-REVIEW.md) | Clippy style nits (`unwrap_or_default`, `collapsible_if`) | ℹ️ Info (logged, open) | No behavioral effect; reproduced independently in this session's `cargo clippy` run. |

No `TBD`/`FIXME`/`XXX` debt markers found in any phase-scope file. All five code-review findings
(0 Critical / 3 Warning / 2 Info) are recorded `open` in `04-REVIEW-DISPOSITION.md` — not silently
dropped, not fixed, and none of them contradict or undermine the phase's observable truths above.

### Human Verification Required

None. All four success criteria were independently exercised with real command output in this
session, including both branches of the gated-access path for the Llama model (available and
forced-unavailable), so no item needed to be routed to human verification.

### Gaps Summary

No gaps. All four roadmap success criteria are independently verified against real test execution,
not SUMMARY.md narration: token-id parity, chat-template parity, and detokenization parity all hold
for Qwen3-0.6B, and the same three categories hold for Llama-3.2-1B-Instruct when gated access is
available, with D-04's clean-skip path behaving correctly (not silently masking a would-be failure)
when it is not. The known BOS-count discrepancy (real oracle produces 2, not D-10's assumed 1) is
correctly asserted as 2 in the test, not papered over. Three open code-review Warnings (WR-01..03)
and two Info items are real but do not block goal achievement: they are either test-infrastructure
reliability concerns already worked around by a documented invocation, or latent gaps that only
matter for a hypothetical future third model outside this phase's scope.

---

_Verified: 2026-10-07T01:34:24Z_
_Verifier: Claude (gsd-verifier)_
