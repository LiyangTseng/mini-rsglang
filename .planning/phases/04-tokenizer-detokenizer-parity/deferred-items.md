# Deferred Items — Phase 04 (tokenizer-detokenizer-parity)

Out-of-scope discoveries logged per the executor's deviation-rules scope boundary: issues not
directly caused by the current task's changes are recorded here, not fixed.

## 1. Pre-existing concurrent-test-execution race in `crates/rsg-tokenizer`'s lib unit tests

- **Found during:** Plan 04-06, Task 2 verification (`cargo test -p rsg-tokenizer` with default
  parallel test threads).
- **Symptom:** With cargo's default parallel test execution, 1-3 of the following intermittently
  `FAILED` (never with `--test-threads=1`, which is always green):
  - `loader::tests::gated_access_unavailable_with_blank_token_file` — `expected
    GatedAccessUnavailable, got Ok(_)`. The test's own doc comment (written in Plan 04-04)
    already names the cause: `EnvGuard` mutates process-global env vars, so a concurrently
    running test can observe a torn-down state mid-test.
  - `detokenize::tests::step_streams_ascii_text_identically_to_one_shot_decode` /
    `step_excludes_eos_token_from_decoded_ids_when_finished` /
    `step_streams_cjk_text_without_utf8_corruption` — `load_model_assets(QWEN3_0_6B) (network
    required): Io(Os { code: 2, kind: NotFound, ... })`. Several tests fetch the same
    `Qwen/Qwen3-0.6B` repo concurrently via hf-hub's local cache/lock files; this is a cache-lock
    race between threads, not a Llama-related or TOK-04 issue (Qwen3-only, pre-existing since
    Plan 04-01/04-03).
- **Scope:** `crates/rsg-tokenizer/src/loader.rs` and `crates/rsg-tokenizer/src/detokenize.rs`'s
  `#[cfg(test)]` modules — neither file is in this plan's `files_modified`, and neither failure
  is caused by this plan's Llama additions (reproducible by running the pre-existing Qwen3-only
  lib tests alone with default parallel threads).
- **Not fixed here:** out of scope per the scope-boundary rule ("only auto-fix issues directly
  caused by the current task's changes"). `cargo test -p rsg-tokenizer -- --test-threads=1` is
  deterministic and green (including both models' full parity suite); this is the reliable
  invocation until a future plan adds proper test-level serialization (e.g. a
  `once_cell`/`std::sync::Once`-backed shared-fetch cache, or a `#[serial]`-style attribute on the
  affected tests).
- **Recommended follow-up:** a small, separately-scoped fix (not part of TOK-04) — either add a
  `cargo-nextest` config entry running these tests single-threaded, or share one already-loaded
  `ModelAssets`/`Tokenizer` across the affected tests instead of each one independently calling
  `load_model_assets`.
