//! Text encoding, ported from `tokenizer/tokenize.py`'s `TokenizeManager.tokenize`: a plain-text
//! branch (`else: prompt = msg.text`) and a chat branch (`isinstance(msg.text, list)` ->
//! `apply_chat_template(...)`), both followed by the shared `tokenizer.encode(prompt, ...)` call.

use crate::TokenizerError;
use crate::loader::ModelAssets;
use crate::template;

/// Encodes already-rendered prompt text, matching `tokenize.py`'s
/// `tokenizer.encode(prompt, return_tensors="pt")` call — HF's default `add_special_tokens=True`
/// (`return_tensors` only changes the output container, never which ids are produced).
pub fn encode_text(
    tokenizer: &tokenizers::Tokenizer,
    text: &str,
) -> Result<Vec<u32>, TokenizerError> {
    let encoding = tokenizer.encode(text, true)?;
    Ok(encoding.get_ids().to_vec())
}

/// The two shapes `tokenize.py`'s `TokenizeManager.tokenize` branches on: `msg.text` is either a
/// plain string, or a list of chat messages (`isinstance(msg.text, list)`).
pub enum PromptInput {
    /// Already-rendered prompt text — encoded directly, no template rendering.
    Raw(String),
    /// A chat conversation (`messages` array) — rendered through the model's chat template
    /// before encoding, matching `apply_chat_template(msg.text, tokenize=False,
    /// add_generation_prompt=True)`.
    Chat(serde_json::Value),
}

/// Encodes `input` into token ids, taking the chat-template-render-then-encode path for
/// `PromptInput::Chat` or the direct-encode path for `PromptInput::Raw` — mirrors
/// `tokenize.py`'s `isinstance(msg.text, list)` branch exactly.
///
/// Never caches or memoizes the rendered chat-template string across calls: `PromptInput::Chat`
/// always re-renders via [`template::render_chat`] on this call alone (see this plan's
/// `must_haves.prohibitions`).
pub fn encode_prompt(
    assets: &ModelAssets,
    env: &minijinja::Environment<'static>,
    input: PromptInput,
) -> Result<Vec<u32>, TokenizerError> {
    match input {
        PromptInput::Raw(text) => encode_text(&assets.tokenizer, &text),
        PromptInput::Chat(messages) => {
            let template_src = assets
                .chat_template
                .as_deref()
                .ok_or(TokenizerError::MissingChatTemplate { slug: "<unknown>" })?;
            let bos_token = assets
                .tokenizer_config
                .get("bos_token")
                .and_then(serde_json::Value::as_str);
            let eos_token = assets
                .tokenizer_config
                .get("eos_token")
                .and_then(serde_json::Value::as_str);
            let rendered =
                template::render_chat(env, template_src, &messages, bos_token, eos_token)?;
            encode_text(&assets.tokenizer, &rendered)
        }
    }
}

/// Counts how many elements of `ids` equal `bos_token_id`, returning 0 if `bos_token_id` is
/// `None`. Purely diagnostic/assertive — consumed only by Plan 04-06's chat-template parity test
/// to check D-10's BOS-count expectation against the real oracle fixture; never called from
/// [`encode_prompt`] itself, and must never be used to de-duplicate or mutate the ids it counts
/// (see this plan's `must_haves.prohibitions`).
pub fn count_bos_occurrences(ids: &[u32], bos_token_id: Option<u32>) -> usize {
    match bos_token_id {
        Some(bos) => ids.iter().filter(|&&id| id == bos).count(),
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_bos_occurrences_counts_matching_elements() {
        let ids = [128000u32, 128000, 128006, 9125];
        assert_eq!(count_bos_occurrences(&ids, Some(128000)), 2);
    }

    #[test]
    fn count_bos_occurrences_returns_zero_when_bos_token_id_is_none() {
        let ids = [1u32, 2, 3];
        assert_eq!(count_bos_occurrences(&ids, None), 0);
    }

    #[test]
    fn count_bos_occurrences_returns_zero_when_no_elements_match() {
        let ids = [1u32, 2, 3];
        assert_eq!(count_bos_occurrences(&ids, Some(99)), 0);
    }
}
