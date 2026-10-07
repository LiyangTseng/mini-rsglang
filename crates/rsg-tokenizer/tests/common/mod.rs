//! Shared test helpers: the model registry (mirrors `scripts/tokenizer_fixtures/models.py`'s
//! `MODELS` list by slug) and fixture-path helpers reused by every `tests/*.rs` file in this crate.

use std::path::PathBuf;

/// One model this crate's test suite parametrizes over.
pub struct ModelCase {
    pub slug: &'static str,
    /// Documents which models require gated HF Hub access (mirrors
    /// `tokenizer_fixtures.models.MODELS`'s own `gated` field by slug), kept alongside `slug` for
    /// readability. The actual skip decision (`skip_if_gated_unavailable`) matches on the real
    /// `TokenizerError::GatedAccessUnavailable` variant returned by `load_model_assets`, not on
    /// this static flag, so this field is never read by test logic itself — `#[allow(dead_code)]`
    /// documents that intentionally, rather than leaving an unexplained compiler warning.
    #[allow(dead_code)]
    pub gated: bool,
}

/// The Rust mirror of `tokenizer_fixtures.models.MODELS`. Extended by Plan 04-06 to include
/// Llama-3.2-1B-Instruct (gated); never reduced.
pub const MODELS: &[ModelCase] = &[
    ModelCase {
        slug: "qwen3-0.6b",
        gated: false,
    },
    ModelCase {
        slug: "llama-3.2-1b-instruct",
        gated: true,
    },
];

/// Returns `true` (and logs a `SKIP {slug}: gated access unavailable` line to stderr) when
/// `result` is `Err(TokenizerError::GatedAccessUnavailable { .. })` — D-04's clean-skip path.
/// Any other error is left for the caller's own `.expect()`/`.unwrap_or_else()` to fail the test
/// normally: this must never swallow a real bug as a false "gated access unavailable" skip
/// (T-04-09), so it matches on that one specific enum variant only, never a bare `Err(_)`.
///
/// Generic over `T` with no `Debug` bound: `loader::ModelAssets` does not implement `Debug` (it
/// holds a `tokenizers::Tokenizer`, which doesn't either — see `loader.rs`'s own
/// `assert_gated_unavailable` test helper for the same constraint), so this function never
/// formats the `Ok` side, only matches the `Err` side's enum variant.
pub fn skip_if_gated_unavailable<T>(
    result: &Result<T, rsg_tokenizer::TokenizerError>,
    slug: &str,
) -> bool {
    match result {
        Err(rsg_tokenizer::TokenizerError::GatedAccessUnavailable { .. }) => {
            eprintln!("SKIP {slug}: gated access unavailable");
            true
        }
        _ => false,
    }
}

/// `fixtures/tokenizer` relative to the crate root.
pub fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/tokenizer")
}

/// `fixtures/tokenizer/{slug}/{filename}` — the one generic helper every test file reuses by
/// filename alone.
pub fn model_fixture(slug: &str, filename: &str) -> PathBuf {
    fixtures_dir().join(slug).join(filename)
}
