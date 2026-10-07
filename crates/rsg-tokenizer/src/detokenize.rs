//! Incremental detokenization: the `DecodeStatus` state machine, hand-ported byte-for-byte from
//! `vendor/mini-sglang/python/minisgl/tokenizer/detokenize.py`'s `DetokenizeManager.detokenize`
//! and `DecodeStatus`, including `find_printable_text`'s CJK/last-space fallback and
//! `_is_chinese_char`'s exact code-point ranges.
//!
//! Deliberately NOT built on `tokenizers::DecodeStream`: `DecodeStream` shares the same
//! incomplete-UTF-8 (`'\u{FFFD}'`-suffix) heuristic as upstream, but has no `finished`+EOS
//! exclusion branch and no `find_printable_text` CJK fallback -- see CLAUDE.md and
//! 04-RESEARCH.md Pitfall 4. Porting the four-field `DecodeStatus` directly is simpler than
//! wrapping `DecodeStream` and patching in the missing branches.

use std::collections::HashMap;

use crate::TokenizerError;

/// Mirrors `detokenize.py`'s `DecodeStatus` dataclass field-for-field.
#[derive(Debug, Clone, Default)]
pub struct DecodeStatus {
    pub decoded_ids: Vec<u32>,
    pub decoded_str: String,
    /// Length (in `decoded_ids`) of ids whose decode has been confirmed and flushed into
    /// `decoded_str`.
    pub read_offset: usize,
    /// Length (in `decoded_ids`) of ids used only as left-context for the surrogate decode.
    pub surr_offset: usize,
    /// Length, in characters (matching Python's `len(str)`, never bytes), of `decoded_str`
    /// already returned to the caller.
    pub sent_offset: usize,
}

/// Checks whether `cp` is a CJK Unified Ideographs code point, per `detokenize.py`'s
/// `_is_chinese_char` (the exact 8 code-point ranges, ported verbatim -- this is NOT all
/// Japanese/Korean characters, see upstream's own comment).
pub fn is_chinese_char(cp: u32) -> bool {
    matches!(
        cp,
        0x4E00..=0x9FFF
            | 0x3400..=0x4DBF
            | 0x20000..=0x2A6DF
            | 0x2A700..=0x2B73F
            | 0x2B740..=0x2B81F
            | 0x2B820..=0x2CEAF
            | 0xF900..=0xFAFF
            | 0x2F800..=0x2FA1F
    )
}

/// Returns the longest printable prefix of `text` that contains only entire words, ported
/// verbatim from `detokenize.py`'s `find_printable_text`. Character-safe throughout (never a
/// raw byte-index slice): uses `Chars::as_str()` (always a valid UTF-8 boundary) to drop the
/// trailing character, and `str::rfind(' ')` only (ASCII space is always 1 byte, so the byte
/// index returned is always a valid char boundary to slice at).
pub fn find_printable_text(text: &str) -> String {
    if text.ends_with('\n') {
        return text.to_string();
    }

    let mut without_last = text.chars();
    let Some(last) = without_last.next_back() else {
        // Empty text: Python's `text[: text.rfind(" ") + 1]` on "" is also "".
        return String::new();
    };
    if is_chinese_char(last as u32) {
        return text.to_string();
    }

    // `without_last` now holds every char except the last one; its own last char (if any) is
    // the ORIGINAL text's second-to-last char -- exactly Python's `text[-2]`.
    if second_to_last_char_is_chinese(without_last.clone()) {
        // Python's `text[:-1]`: `Chars::as_str()` is always a valid boundary.
        return without_last.as_str().to_string();
    }

    match text.rfind(' ') {
        Some(byte_idx) => text[..byte_idx + 1].to_string(),
        None => String::new(),
    }
}

fn second_to_last_char_is_chinese(mut chars_without_last: std::str::Chars<'_>) -> bool {
    match chars_without_last.next_back() {
        Some(second_last) => is_chinese_char(second_last as u32),
        None => false,
    }
}

/// Python's `clean_up_tokenization` post-step (ported verbatim from `transformers`
/// `tokenization_utils_base.py`), applied only when the model's `clean_up_tokenization_spaces`
/// is true. Qwen3-0.6B always has this false, so no fixture in this plan exercises it; wired now
/// (cheap, self-contained, no new state) so Plan 04-06's Llama detokenize fixtures
/// (`clean_up_tokenization_spaces: true`) can exercise it directly.
fn clean_up_tokenization(text: &str) -> String {
    text.replace(" .", ".")
        .replace(" ?", "?")
        .replace(" !", "!")
        .replace(" ,", ",")
        .replace(" ' ", "'")
        .replace(" n't", "n't")
        .replace(" 'm", "'m")
        .replace(" 's", "'s")
        .replace(" 've", "'ve")
        .replace(" 're", "'re")
}

/// Incremental per-`uid` detokenizer, hand-ported from `DetokenizeManager`/`DecodeStatus`.
pub struct Detokenizer {
    tokenizer: tokenizers::Tokenizer,
    eos_token_id: u32,
    clean_up_tokenization_spaces: bool,
    decode_map: HashMap<i64, DecodeStatus>,
}

impl Detokenizer {
    pub fn new(
        tokenizer: tokenizers::Tokenizer,
        eos_token_id: u32,
        clean_up_tokenization_spaces: bool,
    ) -> Self {
        Self {
            tokenizer,
            eos_token_id,
            clean_up_tokenization_spaces,
            decode_map: HashMap::new(),
        }
    }

    /// Ports `DetokenizeManager.detokenize`'s per-message body exactly, for a single message at
    /// a time (upstream batches multiple `msgs` per call; this plan's callers always replay one
    /// token per `uid` at a time, so batching collapses to a batch of one).
    pub fn step(
        &mut self,
        uid: i64,
        next_token: u32,
        finished: bool,
    ) -> Result<String, TokenizerError> {
        let exclude_from_decoded_ids = finished && next_token == self.eos_token_id;

        let (read_ids, surr_ids) = {
            let status = self.decode_map.entry(uid).or_default();
            if !exclude_from_decoded_ids {
                status.decoded_ids.push(next_token);
            }
            let read_ids = status.decoded_ids[status.surr_offset..].to_vec();
            let surr_ids = status.decoded_ids[status.surr_offset..status.read_offset].to_vec();
            (read_ids, surr_ids)
        };

        // Never `.unwrap()` a decode call: propagate failure through `TokenizerError::Tokenizer`
        // via `?` (its `#[from] Box<dyn Error + Send + Sync>` matches `tokenizers`' own Result
        // alias exactly).
        let decoded = self
            .tokenizer
            .decode_batch(&[read_ids.as_slice(), surr_ids.as_slice()], false)?;

        let mut read_str = decoded[0].clone();
        let mut surrogate_str = decoded[1].clone();
        if self.clean_up_tokenization_spaces {
            read_str = clean_up_tokenization(&read_str);
            surrogate_str = clean_up_tokenization(&surrogate_str);
        }

        // Character-safe slice (the exact Pitfall 1 fix): Python's `read_str[len(surr_str):]`
        // slices by Unicode code point, never by byte count. Skip exactly as many `char`s as
        // the surrogate decode contains -- never a raw byte-length slice derived from a
        // different string's byte length, which would panic or corrupt multi-byte UTF-8 on
        // CJK/emoji input.
        let surrogate_char_count = surrogate_str.chars().count();
        let new_text: String = read_str.chars().skip(surrogate_char_count).collect();

        let status = self
            .decode_map
            .get_mut(&uid)
            .expect("uid was inserted into decode_map above, in this same call");

        let output_str = if !new_text.is_empty() && !new_text.ends_with('\u{FFFD}') {
            let output_str = format!("{}{}", status.decoded_str, new_text);
            status.decoded_str = output_str.clone();
            status.surr_offset = status.read_offset;
            status.read_offset = status.decoded_ids.len();
            output_str
        } else {
            let fallback = find_printable_text(&new_text);
            format!("{}{}", status.decoded_str, fallback)
        };

        let incremental_output: String = output_str.chars().skip(status.sent_offset).collect();
        status.sent_offset = output_str.chars().count();

        if finished {
            self.decode_map.remove(&uid);
        }

        Ok(incremental_output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{QWEN3_0_6B, loader};

    fn eos_token_id(assets: &loader::ModelAssets) -> u32 {
        let eos_token = assets
            .tokenizer_config
            .get("eos_token")
            .and_then(serde_json::Value::as_str)
            .expect("tokenizer_config.json has an `eos_token` key");
        assets
            .tokenizer
            .token_to_id(eos_token)
            .expect("eos_token resolves to an id in the tokenizer vocab")
    }

    #[test]
    fn printable_text_fallback_handles_newline_cjk_and_space_cases() {
        assert_eq!(find_printable_text("hello\n"), "hello\n");
        assert_eq!(find_printable_text("foo\u{4F60}"), "foo\u{4F60}"); // ends with CJK -> as-is
        assert_eq!(find_printable_text("foo\u{4F60}a"), "foo\u{4F60}"); // 2nd-to-last CJK -> drop last char
        assert_eq!(find_printable_text("foo bar"), "foo "); // truncate at last space, inclusive
        assert_eq!(find_printable_text("foobar"), ""); // no space -> empty
        assert_eq!(find_printable_text(""), "");
    }

    #[test]
    fn cjk_code_point_ranges_match_the_eight_ported_ranges() {
        assert!(is_chinese_char(0x4E00));
        assert!(is_chinese_char(0x9FFF));
        assert!(is_chinese_char(0x3400));
        assert!(!is_chinese_char(0x4DC0)); // just above the 0x3400..=0x4DBF range
        assert!(!is_chinese_char('a' as u32));
        assert!(is_chinese_char(0x2F800));
    }

    #[test]
    fn step_streams_ascii_text_identically_to_one_shot_decode() {
        let assets = loader::load_model_assets(QWEN3_0_6B)
            .expect("load_model_assets(QWEN3_0_6B) (network required)");
        let eos = eos_token_id(&assets);
        let text = "The quick brown fox jumps over the lazy dog.";
        let ids = assets
            .tokenizer
            .encode(text, false)
            .expect("encode")
            .get_ids()
            .to_vec();
        let expected = assets.tokenizer.decode(&ids, false).expect("decode");

        let mut detokenizer = Detokenizer::new(assets.tokenizer, eos, false);
        let mut streamed = String::new();
        let last_index = ids.len() - 1;
        for (i, &id) in ids.iter().enumerate() {
            let finished = i == last_index;
            let chunk = detokenizer.step(0, id, finished).expect("step");
            streamed.push_str(&chunk);
        }
        assert_eq!(streamed, expected);
    }

    #[test]
    fn step_streams_cjk_text_without_utf8_corruption() {
        let assets = loader::load_model_assets(QWEN3_0_6B)
            .expect("load_model_assets(QWEN3_0_6B) (network required)");
        let eos = eos_token_id(&assets);
        let text = "今天天气很好,我们一起去公园散步吧。";
        let ids = assets
            .tokenizer
            .encode(text, false)
            .expect("encode")
            .get_ids()
            .to_vec();
        let expected = assets.tokenizer.decode(&ids, false).expect("decode");

        let mut detokenizer = Detokenizer::new(assets.tokenizer, eos, false);
        let mut streamed = String::new();
        let last_index = ids.len() - 1;
        for (i, &id) in ids.iter().enumerate() {
            let finished = i == last_index;
            let chunk = detokenizer.step(1, id, finished).expect("step");
            streamed.push_str(&chunk);
        }
        assert_eq!(streamed, expected);
    }

    #[test]
    fn step_excludes_eos_token_from_decoded_ids_when_finished() {
        let assets = loader::load_model_assets(QWEN3_0_6B)
            .expect("load_model_assets(QWEN3_0_6B) (network required)");
        let eos = eos_token_id(&assets);
        let text = "Hi there.";
        let base_ids = assets
            .tokenizer
            .encode(text, false)
            .expect("encode")
            .get_ids()
            .to_vec();
        // The expected text excludes EOS entirely -- it must never be decoded or appear in the
        // streamed output, per `detokenize.py`'s `if not (msg.finished and msg.next_token ==
        // self.eos_token_id)` guard.
        let expected = assets.tokenizer.decode(&base_ids, false).expect("decode");

        let mut ids = base_ids;
        ids.push(eos);

        let mut detokenizer = Detokenizer::new(assets.tokenizer, eos, false);
        let mut streamed = String::new();
        let last_index = ids.len() - 1;
        for (i, &id) in ids.iter().enumerate() {
            let finished = i == last_index;
            let chunk = detokenizer.step(2, id, finished).expect("step");
            streamed.push_str(&chunk);
        }
        assert_eq!(
            streamed, expected,
            "EOS token text must not appear in the streamed output"
        );
    }
}
