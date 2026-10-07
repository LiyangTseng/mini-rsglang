//! Rust port of mini-sglang's tokenization frontend: `tokenizer/tokenize.py`,
//! `tokenizer/detokenize.py`, and `utils/hf.py`.
//!
//! Parity contract: for a given [`ModelSpec`], Rust token ids, chat-template-rendered prompts,
//! and incrementally-streamed detokenized text must equal the frozen Python frontend's output,
//! for Qwen3-0.6B (hard gate) and Llama-3.2-1B-Instruct. This plan (04-01) proves the encode-only
//! slice end to end; `template.rs` (chat-template rendering) and `detokenize.rs` (incremental
//! decode) are filled in by Plan 04-02 and 04-03 respectively.

pub mod detokenize;
pub mod encode;
pub mod loader;
pub mod template;

/// Identifies a tokenizer/model pair this crate supports, by HF Hub repo id.
///
/// Every loader/encode/template/detokenize function takes a `ModelSpec`-shaped parameter rather
/// than being hardcoded to one model, so adding Llama-3.2-1B-Instruct (Plan 04-04/04-05) extends
/// the shared `MODELS` list instead of duplicating code paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelSpec {
    /// Short, filesystem-safe identifier used for fixture directories and test parametrization.
    pub slug: &'static str,
    /// The exact HF Hub repo id (`owner/name`) passed to `hf-hub`.
    pub repo_id: &'static str,
    /// Whether this repo requires an authenticated, license-accepted HF token to fetch.
    pub gated: bool,
}

/// Qwen3-0.6B: the hard-gate parity model (TOK-01..03 must always pass for this model).
pub const QWEN3_0_6B: ModelSpec = ModelSpec {
    slug: "qwen3-0.6b",
    repo_id: "Qwen/Qwen3-0.6B",
    gated: false,
};

/// Llama-3.2-1B-Instruct: the TOK-04 Llama parity target (D-01). Gated — fetching it requires a
/// local `HF_TOKEN`/cached credential with license acceptance (D-02); see
/// `loader::load_model_assets`'s `GatedAccessUnavailable` clean-skip path (D-04).
///
/// Plan 04-04's own action text says to place this "next to `QWEN3_0_6B`" but names
/// `loader.rs` as the file — `QWEN3_0_6B` actually lives here in `lib.rs` (set by Plan 04-01).
/// Keeping both `ModelSpec` constants together in one place (deviation, Rule 1: the plan's
/// file-location assumption was stale) takes priority over the literal file name.
pub const LLAMA_3_2_1B_INSTRUCT: ModelSpec = ModelSpec {
    slug: "llama-3.2-1b-instruct",
    repo_id: "meta-llama/Llama-3.2-1B-Instruct",
    gated: true,
};

/// Errors from loading tokenizer assets, encoding, rendering chat templates, or detokenizing.
#[derive(Debug, thiserror::Error)]
pub enum TokenizerError {
    #[error("hf-hub fetch failed: {0}")]
    HfHub(#[from] hf_hub::HFError),
    #[error("tokenizer error: {0}")]
    Tokenizer(#[from] Box<dyn std::error::Error + Send + Sync>),
    #[error("chat-template render error: {0}")]
    Template(#[from] minijinja::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("model {slug:?} has no chat_template (neither tokenizer_config.json nor chat_template.json provided one)")]
    MissingChatTemplate { slug: &'static str },
    #[error("gated access unavailable for model {slug:?}: {detail}")]
    GatedAccessUnavailable { slug: &'static str, detail: String },
    #[error("model {slug:?} produced {count} BOS tokens in input_ids, expected exactly 1")]
    BosCountMismatch { slug: &'static str, count: usize },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qwen3_model_spec_is_well_formed() {
        assert_eq!(QWEN3_0_6B.slug, "qwen3-0.6b");
        assert_eq!(QWEN3_0_6B.repo_id, "Qwen/Qwen3-0.6B");
        assert!(!QWEN3_0_6B.gated);
    }

    #[test]
    fn llama_model_spec_is_well_formed() {
        assert_eq!(LLAMA_3_2_1B_INSTRUCT.slug, "llama-3.2-1b-instruct");
        assert_eq!(LLAMA_3_2_1B_INSTRUCT.repo_id, "meta-llama/Llama-3.2-1B-Instruct");
        assert!(LLAMA_3_2_1B_INSTRUCT.gated);
    }
}
