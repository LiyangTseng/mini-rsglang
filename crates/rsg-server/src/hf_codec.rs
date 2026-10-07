//! The `TextCodec` adapter over Phase 4's `rsg-tokenizer` crate.
//!
//! Phase 4 owns tokenization, chat-template rendering and incremental
//! detokenization end to end (TOK-01..04); this module adapts its public
//! API onto the `TextCodec`/`IncrementalDecoder` seam `engine.rs` depends
//! on. No tokenization, template or detokenization logic is reimplemented
//! here -- every call below delegates straight into `rsg_tokenizer`.

use std::time::Duration;

use anyhow::Context;
use serde_json::Value;

use rsg_tokenizer::detokenize::Detokenizer;
use rsg_tokenizer::encode::{self, PromptInput};
use rsg_tokenizer::loader::{self, ModelAssets};
use rsg_tokenizer::{ModelSpec, TokenizerError, template};

use crate::codec::{ChatMessage, CodecError, IncrementalDecoder, Prompt, TextCodec};

/// How many times [`load_model_assets_with_retry`] retries a transient
/// loader error before giving up.
const LOAD_RETRY_ATTEMPTS: u32 = 5;

/// `true` for the two [`TokenizerError`] shapes a benign filesystem race in
/// hf-hub's own cache-pointer bookkeeping can produce, never for a genuine
/// config/content problem.
///
/// hf-hub's `create_pointer_symlink` (`cache/storage.rs`) does a bare
/// `remove_file` immediately followed by `symlink`, with no rename-based
/// atomicity: when two separate OS processes resolve the *same* cached file
/// concurrently (every `rsg-server` subprocess loads its own tokenizer
/// independently, with no cross-process coordination -- D-05/D-06's own
/// per-process loading design), one process's `remove_file` can land in
/// the instant between another process's own `remove_file` and `symlink`,
/// so a `std::fs::read_to_string` on the pointer path a moment later can
/// transiently see `ENOENT` even though the file is, and remains, fully
/// cached. This is the identical cache-race category Phase 4 already found
/// and deferred for `cargo test -p rsg-tokenizer` (STATE.md: "requires
/// --test-threads=1 to be deterministic"); Phase 5 cannot adopt that same
/// single-threaded workaround (multiple real `rsg-server` processes
/// concurrently loading one cached model is an expected, not a test-only,
/// shape -- e.g. Phase 7 benchmark runs), so a short bounded retry here is
/// the fix, not a test-harness restriction.
fn is_transient_cache_race(err: &TokenizerError) -> bool {
    matches!(err, TokenizerError::Io(_) | TokenizerError::HfHub(_))
}

/// Retries [`loader::load_model_assets`] up to [`LOAD_RETRY_ATTEMPTS`]
/// times on a transient cache-race error (see
/// [`is_transient_cache_race`]), with a short linear backoff. Any other
/// error (a real config/content problem, or a gated-access failure) returns
/// immediately on the first attempt -- retrying those would only delay a
/// deterministic failure.
fn load_model_assets_with_retry(spec: ModelSpec) -> Result<ModelAssets, TokenizerError> {
    let mut last_err = None;
    for attempt in 0..LOAD_RETRY_ATTEMPTS {
        match loader::load_model_assets(spec) {
            Ok(assets) => return Ok(assets),
            Err(e) if is_transient_cache_race(&e) && attempt + 1 < LOAD_RETRY_ATTEMPTS => {
                tracing::warn!(
                    slug = spec.slug,
                    attempt,
                    "transient error loading tokenizer assets, retrying: {e}"
                );
                std::thread::sleep(Duration::from_millis(20 * u64::from(attempt + 1)));
                last_err = Some(e);
            }
            Err(e) => return Err(e),
        }
    }
    Err(last_err
        .expect("the loop above always either returns or sets last_err before exhausting attempts"))
}

/// Adapts [`rsg_tokenizer`]'s model assets, chat-template environment and
/// resolved EOS/clean-up settings onto [`TextCodec`].
pub struct HfCodec {
    assets: ModelAssets,
    env: minijinja::Environment<'static>,
    eos_token_id: u32,
    clean_up_tokenization_spaces: bool,
}

impl HfCodec {
    /// Loads `model` (an HF Hub repo id, e.g. `"Qwen/Qwen3-0.6B"`) through
    /// Phase 4's `loader::load_model_assets`, then resolves the eos token
    /// id and `clean_up_tokenization_spaces` flag from
    /// `tokenizer_config.json`, exactly as `detokenize.rs`'s own test
    /// helper and `encode.rs`'s chat path do.
    ///
    /// `model` always comes from the `--model` CLI flag at process
    /// startup, never per-request input, so this runs exactly once per
    /// process. [`ModelSpec`]'s `slug`/`repo_id` fields require
    /// `&'static str` -- a shape built for Phase 4's own two hardcoded
    /// model constants, not a runtime CLI value. Leaking the owned
    /// `String` once here (a one-time startup cost, not a per-request
    /// leak) satisfies that signature without duplicating Phase 4's
    /// loader/fallback-chain logic for a "CLI model id" special case.
    pub fn load(model: &str) -> anyhow::Result<HfCodec> {
        let spec = ModelSpec {
            slug: Box::leak(model.to_string().into_boxed_str()),
            repo_id: Box::leak(model.to_string().into_boxed_str()),
            // Every model this plan's `--model` flag loads is public
            // (Qwen3-0.6B); `gated` only changes which error variant an
            // auth failure maps to (see `loader::gated_or`) -- a genuinely
            // gated model would surface as an ordinary `TokenizerError`
            // auth error here instead of `GatedAccessUnavailable`, not a
            // silent misbehavior.
            gated: false,
        };
        let assets = load_model_assets_with_retry(spec)
            .with_context(|| format!("loading tokenizer assets for {model:?}"))?;

        let eos_token = assets
            .tokenizer_config
            .get("eos_token")
            .and_then(Value::as_str)
            .with_context(|| format!("{model:?}: tokenizer_config.json has no eos_token"))?;
        let eos_token_id = assets
            .tokenizer
            .token_to_id(eos_token)
            .with_context(|| format!("{model:?}: eos_token {eos_token:?} has no vocab id"))?;
        let clean_up_tokenization_spaces = assets
            .tokenizer_config
            .get("clean_up_tokenization_spaces")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        Ok(HfCodec {
            assets,
            env: template::build_environment(None),
            eos_token_id,
            clean_up_tokenization_spaces,
        })
    }
}

/// Converts chat messages into the `messages` JSON value `rsg_tokenizer`'s
/// chat-template renderer expects -- the same `{role, content}` shape
/// upstream's `apply_chat_template` receives.
fn chat_messages_to_json(messages: &[ChatMessage]) -> Value {
    Value::Array(
        messages
            .iter()
            .map(|m| serde_json::json!({"role": m.role.as_str(), "content": m.content}))
            .collect(),
    )
}

impl TextCodec for HfCodec {
    fn encode(&self, prompt: &Prompt) -> Result<Vec<i32>, CodecError> {
        let input = match prompt {
            Prompt::Text(text) => PromptInput::Raw(text.clone()),
            Prompt::Chat(messages) => PromptInput::Chat(chat_messages_to_json(messages)),
        };
        let ids = encode::encode_prompt(&self.assets, &self.env, input)
            .map_err(|e| CodecError(e.to_string()))?;
        ids.into_iter()
            .map(|id| {
                i32::try_from(id)
                    .map_err(|_| CodecError(format!("token id {id} does not fit in i32")))
            })
            .collect()
    }

    fn decoder(&self) -> Box<dyn IncrementalDecoder> {
        Box::new(HfDecoder {
            detokenizer: Detokenizer::new(
                self.assets.tokenizer.clone(),
                self.eos_token_id,
                self.clean_up_tokenization_spaces,
            ),
        })
    }
}

/// Wraps one request's [`Detokenizer`]. Each [`HfCodec::decoder`] call
/// builds a fresh `Detokenizer` dedicated to exactly one request, so every
/// call here uses the same arbitrary local uid (`0`) -- `Detokenizer`'s own
/// uid-keyed map (upstream's batching shape) never needs to distinguish
/// requests this struct only ever drives one of.
struct HfDecoder {
    detokenizer: Detokenizer,
}

impl IncrementalDecoder for HfDecoder {
    fn step(&mut self, next_token: i64, finished: bool) -> Result<String, CodecError> {
        let token = u32::try_from(next_token)
            .map_err(|_| CodecError(format!("token id {next_token} does not fit in u32")))?;
        self.detokenizer
            .step(0, token, finished)
            .map_err(|e| CodecError(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const QWEN3: &str = "Qwen/Qwen3-0.6B";

    #[test]
    fn load_then_encode_round_trips_through_decoder() {
        let codec = HfCodec::load(QWEN3).expect("load Qwen3-0.6B (network/HF cache required)");
        let ids = codec
            .encode(&Prompt::Text("Hello world".to_string()))
            .expect("encode");
        assert!(!ids.is_empty());

        let mut decoder = codec.decoder();
        let mut streamed = String::new();
        let last = ids.len() - 1;
        for (i, &id) in ids.iter().enumerate() {
            let chunk = decoder
                .step(i64::from(id), i == last)
                .expect("decoder step");
            streamed.push_str(&chunk);
        }
        assert_eq!(streamed, "Hello world");
    }

    #[test]
    fn chat_prompt_renders_through_the_real_chat_template() {
        let codec = HfCodec::load(QWEN3).expect("load Qwen3-0.6B (network/HF cache required)");
        let ids = codec
            .encode(&Prompt::Chat(vec![ChatMessage {
                role: crate::codec::ChatRole::User,
                content: "hi".to_string(),
            }]))
            .expect("encode chat prompt");
        assert!(!ids.is_empty());
    }
}
