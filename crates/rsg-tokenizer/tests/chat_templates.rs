//! TOK-02: Rust chat-template rendering matches the Python oracle's rendered prompt string,
//! case by case, for every model in `common::MODELS`, against the committed
//! `fixtures/tokenizer/{slug}/chat_prompts.json`.
//!
//! This test checks `template::render_chat`'s output directly (the rendered STRING), not
//! `encode::encode_prompt`'s token ids -- chat-template parity is about the string, per the
//! plan's objective.

mod common;

use rsg_tokenizer::loader::ModelAssets;
use rsg_tokenizer::{encode, loader, template};
use serde_json::Value;

/// Maps a `common::ModelCase` slug to its `rsg_tokenizer::ModelSpec` constant.
fn model_spec(slug: &str) -> rsg_tokenizer::ModelSpec {
    match slug {
        "qwen3-0.6b" => rsg_tokenizer::QWEN3_0_6B,
        "llama-3.2-1b-instruct" => rsg_tokenizer::LLAMA_3_2_1B_INSTRUCT,
        other => panic!("no ModelSpec registered for slug {other:?}"),
    }
}

/// Matches `scripts/tokenizer_fixtures/corpus_chat.py`'s own frozen-clock constant exactly (same
/// literal string, not re-derived) -- the committed Llama `chat_prompts.json` fixture was
/// generated with `strftime_now` frozen to this date (see 04-05-SUMMARY.md's "Next Phase
/// Readiness").
const FROZEN_NOW: &str = "06 Oct 2026";

/// Resolves a model's real `bos_token_id`, reading `bos_token` from `tokenizer_config.json`
/// (falling back to `special_tokens_map.json`) and looking it up in the tokenizer's own vocab --
/// `None` if the model has no `bos_token` at all (e.g. Qwen3-0.6B's `bos_token: null`).
fn bos_token_id(assets: &ModelAssets) -> Option<u32> {
    let bos_token_str = assets
        .tokenizer_config
        .get("bos_token")
        .and_then(Value::as_str)
        .or_else(|| {
            assets
                .special_tokens_map
                .get("bos_token")
                .and_then(Value::as_str)
        })?;
    assets.tokenizer.token_to_id(bos_token_str)
}

struct ChatCase {
    name: String,
    messages: Value,
    prompt: String,
}

/// The per-model chat-prompt fixture cases, in committed order.
fn chat_cases(slug: &str) -> Vec<ChatCase> {
    let path = common::model_fixture(slug, "chat_prompts.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let manifest: Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    manifest["cases"]
        .as_array()
        .unwrap_or_else(|| panic!("{}: missing `cases` array", path.display()))
        .iter()
        .map(|c| {
            let name = c["name"]
                .as_str()
                .unwrap_or_else(|| panic!("case missing `name`: {c:?}"))
                .to_owned();
            let messages = c["messages"]
                .as_array()
                .unwrap_or_else(|| panic!("case {name:?} missing `messages`"))
                .clone();
            let prompt = c["prompt"]
                .as_str()
                .unwrap_or_else(|| panic!("case {name:?} missing `prompt`"))
                .to_owned();
            ChatCase {
                name,
                messages: Value::Array(messages),
                prompt,
            }
        })
        .collect()
}

#[test]
fn chat_template_renders_match_python_oracle_for_every_model() {
    for case in common::MODELS {
        let result = loader::load_model_assets(model_spec(case.slug));
        if common::skip_if_gated_unavailable(&result, case.slug) {
            continue;
        }
        let assets = result.unwrap_or_else(|e| panic!("load_model_assets({:?}): {e}", case.slug));
        let chat_template = assets
            .chat_template
            .as_deref()
            .unwrap_or_else(|| panic!("model {:?}: no chat_template in fixture assets", case.slug));

        // Detected generically by template content, never by model slug/identity (mirrors
        // corpus_chat.py's own `"strftime_now" in tokenizer.chat_template` check) -- Qwen3's
        // template has no `strftime_now` call, so its render path is byte-for-byte unaffected.
        let now_override = if chat_template.contains("strftime_now(") {
            Some(FROZEN_NOW.to_string())
        } else {
            None
        };
        let env = template::build_environment(now_override);

        let cases = chat_cases(case.slug);
        assert!(
            !cases.is_empty(),
            "model {:?}: chat_prompts.json has no cases",
            case.slug
        );

        let bos_id = bos_token_id(&assets);
        // Read the model's real `bos_token`/`eos_token` strings generically (every model, not
        // just Llama) -- mirrors `encode::encode_prompt`'s own reads from `tokenizer_config`.
        // Qwen3's `bos_token` is `null`, so this naturally resolves to `None` for Qwen3, matching
        // prior behavior exactly; Llama's template actually interpolates `{{- bos_token }}`, so
        // passing `None` there previously rendered the literal text "None" instead of
        // `<|begin_of_text|>`.
        let bos_token_str = assets
            .tokenizer_config
            .get("bos_token")
            .and_then(Value::as_str);
        let eos_token_str = assets
            .tokenizer_config
            .get("eos_token")
            .and_then(Value::as_str);

        for chat_case in &cases {
            let rendered = template::render_chat(
                &env,
                chat_template,
                &chat_case.messages,
                bos_token_str,
                eos_token_str,
            )
            .unwrap_or_else(|e| {
                panic!(
                    "model {:?} case {:?}: render_chat failed: {e}",
                    case.slug, chat_case.name
                )
            });
            assert_eq!(
                rendered, chat_case.prompt,
                "model {:?} case {:?}: rendered prompt differs from Python oracle",
                case.slug, chat_case.name
            );

            // D-10, Llama only: check the BOS-occurrence count in the final encoded `input_ids`
            // against the real oracle, never assume "exactly once". A live empirical check
            // against the real canonical tokenizer this session (`.venv/bin/python`, transformers
            // 4.57.3) confirmed the real Python oracle itself produces a BOS count of 2 for every
            // chat-rendered prompt -- the template's own `{{- bos_token }}` prepend plus
            // `tokenizer.json`'s post-processor both fire, exactly the mechanism Pitfall 2/D-10
            // describe. This is a documented discrepancy from D-10's "exactly once" expectation,
            // not a Rust bug: see this plan's own SUMMARY.
            if case.slug == "llama-3.2-1b-instruct" {
                let ids = encode::encode_prompt(
                    &assets,
                    &env,
                    encode::PromptInput::Chat(chat_case.messages.clone()),
                )
                .unwrap_or_else(|e| {
                    panic!(
                        "model {:?} case {:?}: encode_prompt failed: {e}",
                        case.slug, chat_case.name
                    )
                });
                let bos_count = encode::count_bos_occurrences(&ids, bos_id);
                assert_eq!(
                    bos_count, 2,
                    "model {:?} case {:?}: expected the real oracle's documented BOS count (2, \
                     not D-10's assumed 1) in final input_ids, got {bos_count}",
                    case.slug, chat_case.name
                );
            }
        }
    }
}

/// D-12 point 4: `system_empty_string` and `system_missing_key` must both exist as genuinely
/// distinct fixture entries, asserted independently of whether Qwen3's template happens to
/// render them identically or differently (it renders them differently -- see the fixture).
#[test]
fn case_4_sub_cases_are_both_present_and_independently_asserted() {
    for case in common::MODELS {
        let cases = chat_cases(case.slug);
        let names: Vec<&str> = cases.iter().map(|c| c.name.as_str()).collect();
        assert!(
            names.contains(&"system_empty_string"),
            "model {:?}: missing system_empty_string case",
            case.slug
        );
        assert!(
            names.contains(&"system_missing_key"),
            "model {:?}: missing system_missing_key case",
            case.slug
        );
    }
}

/// D-12 point 8: the tool-calling case must actually exercise Qwen3's tool-call rendering
/// branch, not silently fall through to the plain-message branch.
///
/// The `<tool_call>` tag substring check is Qwen3-specific, matching this test's own documented
/// intent -- Llama-3.2-1B-Instruct's template has no such tag convention (confirmed against the
/// committed `fixtures/tokenizer/llama-3.2-1b-instruct/chat_prompts.json`: its `tool_calling`
/// case renders the raw function-call JSON directly, with no `<tool_call>` wrapper at all), so
/// asserting the substring for every model in `common::MODELS` would incorrectly fail for Llama
/// despite Llama's template rendering correctly. Every model still gets the weaker "the
/// tool_calling case exists at all" check below.
#[test]
fn tool_calling_case_prompt_contains_tool_call_tag() {
    for case in common::MODELS {
        let cases = chat_cases(case.slug);
        let tool_case = cases
            .iter()
            .find(|c| c.name == "tool_calling")
            .unwrap_or_else(|| panic!("model {:?}: missing tool_calling case", case.slug));
        if case.slug != "qwen3-0.6b" {
            continue;
        }
        assert!(
            tool_case.prompt.contains("<tool_call>"),
            "model {:?}: tool_calling case prompt missing <tool_call> substring: {:?}",
            case.slug,
            tool_case.prompt
        );
    }
}
