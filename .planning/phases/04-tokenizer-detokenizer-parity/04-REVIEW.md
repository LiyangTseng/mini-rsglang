---
phase: 04-tokenizer-detokenizer-parity
reviewed: 2026-10-06T00:00:00Z
depth: standard
files_reviewed: 26
files_reviewed_list:
  - Cargo.toml
  - Cargo.lock
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
  - fixtures/tokenizer/id_corpus.json
  - fixtures/tokenizer/llama-3.2-1b-instruct/chat_prompts.json
  - fixtures/tokenizer/llama-3.2-1b-instruct/detok_streams.json
  - fixtures/tokenizer/llama-3.2-1b-instruct/token_ids.json
  - fixtures/tokenizer/qwen3-0.6b/chat_prompts.json
  - fixtures/tokenizer/qwen3-0.6b/detok_streams.json
  - fixtures/tokenizer/qwen3-0.6b/token_ids.json
  - scripts/check_all.sh
  - scripts/gen_tokenizer_fixtures.py
  - scripts/tokenizer_fixtures/__init__.py
  - scripts/tokenizer_fixtures/corpus_chat.py
  - scripts/tokenizer_fixtures/corpus_detok.py
  - scripts/tokenizer_fixtures/corpus_ids.py
  - scripts/tokenizer_fixtures/models.py
findings:
  critical: 0
  warning: 3
  info: 2
  total: 5
status: issues_found
---

# Phase 04: Code Review Report

**Reviewed:** 2026-10-06
**Depth:** standard
**Files Reviewed:** 26
**Status:** issues_found

## Summary

Reviewed the Rust tokenizer/detokenizer crate (`rsg-tokenizer`), its Python fixture-generation
scripts, the committed golden fixtures for Qwen3-0.6B and Llama-3.2-1B-Instruct, and the phase
gate script. The core parity logic (`DecodeStatus`/`find_printable_text`/`clean_up_tokenization`
in `detokenize.rs`, the chat-template `tojson` separator override and `strftime_now` clock
override in `template.rs`, the gated-access detection in `loader.rs`) was traced carefully against
the documented upstream algorithms and found to be correctly ported — the surrogate-offset
bookkeeping in `Detokenizer::step` is subtle but self-consistent across the fast path and the
`\u{FFFD}` fallback path, and `cargo check`/`cargo clippy` on the crate produced no warnings beyond
trivial style nits (not reported here, per style-preference exclusion). No crate-local security,
correctness, or secret-handling issues were found in the production code paths.

Three issues worth fixing were found: a real test-reliability bug (unsynchronized global
env-var mutation across threads in three `loader.rs` unit tests, which the test runner does not
actually serialize by default, contrary to what the tests assume), a latent parity gap in
`encode_prompt`'s `bos_token`/`eos_token` resolution that only works for the two models currently
in scope, and a diagnostic quality gap in a hardcoded `"<unknown>"` error-message slug. None of
these are present in a form that currently breaks a committed fixture, but all three should be
fixed before the crate is extended to a third model or run under different test-parallelism
settings.

## Warnings

### WR-01: `EnvGuard`-based gated-credential tests race on shared process env vars under default `cargo test` parallelism

**File:** `crates/rsg-tokenizer/src/loader.rs:198-297`
**Issue:** `gated_access_unavailable_without_token`, `gated_access_unavailable_with_blank_token_file`,
and `gated_access_unavailable_when_implicit_token_disabled` each construct an `EnvGuard` that
mutates process-global environment variables (`HF_TOKEN`, `HF_TOKEN_PATH`, `HF_HOME`,
`HF_HUB_DISABLE_IMPLICIT_TOKEN`) for the duration of the test, restoring them on `Drop`. The doc
comment on `gated_access_unavailable_without_token` (line 246-248) explicitly says "Run with
`--test-threads=1`: `EnvGuard` mutates process-global env vars, so a concurrently-running test
could observe a torn-down state" — but nothing in the repository actually enforces single-threaded
execution. `scripts/check_all.sh` (the only test gate, step 1) runs plain `cargo test --workspace`
with no `--test-threads=1` flag, no `.cargo/config.toml` default, and there is no `serial_test`-style
mutex guarding these three tests. All three unit tests live in the same `--lib` test binary and
Rust's default test harness runs tests from one binary across multiple OS threads concurrently, so
two of these tests can genuinely interleave: one test's `EnvGuard::new()` can overwrite `HF_HOME`/
`HF_TOKEN_PATH` set up by another still-running test, and either guard's `Drop` can restore state
mid-flight for the other. This makes the three tests intermittently flaky in exactly the
"torn-down state" way the comment warns about, under the test command the project's own phase gate
actually runs.
**Fix:** Either add real cross-test serialization (a `static Mutex<()>` acquired for the lifetime
of `EnvGuard`, or the `serial_test` crate's `#[serial]` attribute on all three tests), or pin
`scripts/check_all.sh`'s step 1 to `cargo test --workspace -- --test-threads=1` (coarser, but
matches what the doc comment already assumes is happening). A `Mutex`-based guard is preferable
since it does not serialize the entire workspace's test suite for the sake of three tests.

### WR-02: `encode_prompt`'s chat path resolves `bos_token`/`eos_token` only from `tokenizer_config.json`, with no `special_tokens_map.json` fallback

**File:** `crates/rsg-tokenizer/src/encode.rs:50-57`
**Issue:** `encode_prompt`'s `PromptInput::Chat` branch reads `bos_token`/`eos_token` exclusively via
`assets.tokenizer_config.get("bos_token"/"eos_token")`. Two other places in this same crate resolve
the identical value with a fallback to `special_tokens_map.json` when `tokenizer_config.json` is
missing the key: `tests/chat_templates.rs`'s own `bos_token_id()` helper (lines 33-45, `.or_else(||
assets.special_tokens_map.get("bos_token")...)`), and the documented upstream fallback pattern
`loader::load_model_assets` itself already uses for `chat_template` (falling back to
`chat_template.json`). Both current fixture models (Qwen3, Llama) happen to set `bos_token`/
`eos_token` directly in `tokenizer_config.json`, so this gap is currently unexercised by any
fixture — but `lib.rs`'s own module doc states the explicit design goal that "adding [a model]
extends the shared `MODELS` list instead of duplicating code paths" (line 18-19). A future model
whose `bos_token`/`eos_token` live only in `special_tokens_map.json` would silently render its
chat template with `bos_token`/`eos_token` passed as `None` into the Jinja context (e.g. a template
doing `{{- bos_token }}` would render the literal absence instead of the real token), producing a
wrong-but-non-crashing prompt — exactly the failure mode Pitfall 2 in this phase's own research
notes is concerned about.
**Fix:** Factor the fallback lookup used in `chat_templates.rs`'s `bos_token_id`/`bos_token_str`
logic into a shared helper in `rsg-tokenizer`'s production code (e.g. `ModelAssets::bos_token_str()`
/ `eos_token_str()` that check `tokenizer_config` then `special_tokens_map`), and have both
`encode_prompt` and the test file call it, rather than keeping the fallback logic test-only.

### WR-03: `MissingChatTemplate` error always reports `slug: "<unknown>"`

**File:** `crates/rsg-tokenizer/src/encode.rs:49`
**Issue:** `encode_prompt` raises `TokenizerError::MissingChatTemplate { slug: "<unknown>" }` when
`assets.chat_template` is `None`, because `loader::ModelAssets` (`crates/rsg-tokenizer/src/loader.rs:23-28`)
has no `slug`/model-identifying field for `encode_prompt` to read. `TokenizerError::MissingChatTemplate`'s
own `#[error]` message (`lib.rs:64-65`) is specifically formatted to name the offending model
(`"model {slug:?} has no chat_template..."`), but this call site can never actually supply that
information — every occurrence of this error, for every model, prints the literal string
`"<unknown>"` instead of e.g. `"qwen3-0.6b"`. This degrades the error's usefulness exactly in the
scenario it exists for: diagnosing which model's asset fetch produced incomplete data.
**Fix:** Add a `pub slug: &'static str` field to `ModelAssets` (populated from `spec.slug` in
`load_model_assets`), and have `encode_prompt` take the `ModelSpec`/slug it already receives
indirectly, e.g. `assets.chat_template.as_deref().ok_or(TokenizerError::MissingChatTemplate { slug:
assets.slug })?`.

## Info

### IN-01: Duplicated magic frozen-date literal between Rust test and Python fixture generator

**File:** `crates/rsg-tokenizer/tests/chat_templates.rs:28`, `scripts/tokenizer_fixtures/corpus_chat.py:30`
**Issue:** `FROZEN_NOW = "06 Oct 2026"` is hand-duplicated as an identical literal string in two
different languages/files, with only a comment linking them ("Matches
`scripts/tokenizer_fixtures/corpus_chat.py`'s own frozen-clock constant exactly (same literal
string, not re-derived)"). If the fixture is ever regenerated with a different frozen date in the
Python script without updating the Rust constant (or vice versa), the test would fail loudly
(the rendered prompt would differ from the stale fixture), so this is not a silent-failure risk —
but it is an avoidable single point of truth violation for two files already in this review's
scope.
**Fix:** Consider writing the frozen date into the fixture JSON itself (e.g. a top-level
`"frozen_now"` key alongside `"cases"`) so the Rust test reads it directly from the fixture rather
than hardcoding a parallel literal.

### IN-02: `clippy` flags idiomatic simplifications not addressed in this phase

**File:** `crates/rsg-tokenizer/src/detokenize.rs:142`, `crates/rsg-tokenizer/src/loader.rs:56-60`
**Issue:** `cargo clippy -p rsg-tokenizer --tests --all-targets` reports `clippy::unwrap_or_default`
(`.or_insert_with(DecodeStatus::default)` should be `.or_default()`) and `clippy::collapsible_if`
(the nested `if let Ok(path) = ... { if ... { return true } }` in `has_gated_credentials` should
collapse to a single `if let ... && ...`). Neither affects behavior.
**Fix:** Apply `cargo clippy --fix -p rsg-tokenizer` or make the two edits by hand.

---

_Reviewed: 2026-10-06_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
