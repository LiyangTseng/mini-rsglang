//! The tokenizer/detokenizer seam (Phase 4 plugs into this, plan 05-08).
//!
//! `encode` mirrors upstream's `tokenizer/tokenize.py`: a plain text prompt
//! is encoded as-is with special tokens added, and a chat prompt is first
//! rendered through the chat template (`add_generation_prompt=True`) and
//! then encoded the same way. `IncrementalDecoder::step` mirrors one
//! `DetokenizeMsg` through `DetokenizeManager.detokenize`
//! (`tokenizer/detokenize.py`): it returns that message's
//! `incremental_output`, which may be empty, with the EOS-on-finish
//! exclusion upstream applies.
//!
//! This module defines only the seam: `TestServer`'s `ByteCodec` (in
//! `tests/common/test_server.rs`) is the only implementation until Phase
//! 4's real tokenizer/detokenizer lands behind the same trait.

use serde::{Deserialize, Serialize};

/// One chat message's role. Upstream restricts this to exactly these three
/// values (`Literal["system", "user", "assistant"]` in `api_server.py`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    User,
    Assistant,
}

impl ChatRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            ChatRole::System => "system",
            ChatRole::User => "user",
            ChatRole::Assistant => "assistant",
        }
    }
}

/// One message in a chat-completions request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

/// What a request asks to encode: a plain text prompt (`/generate`), or a
/// list of chat messages to render through the chat template
/// (`/v1/chat/completions`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Prompt {
    Text(String),
    Chat(Vec<ChatMessage>),
}

/// An error from encoding a prompt or decoding a token.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct CodecError(pub String);

/// The per-request incremental-decode state (mirrors upstream's
/// `DecodeStatus`: `decoded_ids`, `read_offset`, `surr_offset`,
/// `sent_offset`). Each request gets its own decoder from
/// [`TextCodec::decoder`].
pub trait IncrementalDecoder: Send {
    /// Feeds one more token through the decoder and returns the text this
    /// token newly contributes (upstream's `incremental_output`), which may
    /// be empty (e.g. a token that only completes a multi-token UTF-8
    /// sequence has already been credited, or further bytes are still
    /// pending). `finished` signals the last token of the request, so any
    /// implementation that buffers partial output can flush it here.
    fn step(&mut self, next_token: i64, finished: bool) -> Result<String, CodecError>;
}

/// The tokenizer/detokenizer seam this plan's engine depends on. Phase 4
/// (plan 05-08) plugs the real Hugging Face tokenizer in behind this trait;
/// tests use `ByteCodec` (`tests/common/test_server.rs`) as a test double.
pub trait TextCodec: Send + Sync + 'static {
    /// Encodes a prompt into token ids. `Prompt::Text` is encoded as-is with
    /// special tokens added; `Prompt::Chat` is first rendered through the
    /// chat template with `add_generation_prompt=true`.
    fn encode(&self, prompt: &Prompt) -> Result<Vec<i32>, CodecError>;

    /// Builds a fresh incremental decoder for one request.
    fn decoder(&self) -> Box<dyn IncrementalDecoder>;
}
