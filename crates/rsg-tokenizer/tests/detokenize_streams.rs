//! TOK-03: Rust incremental detokenization matches the Python oracle's incremental chunks,
//! case by case, for every model in `common::MODELS` that has a `detok_streams.json` fixture,
//! against the committed `fixtures/tokenizer/{slug}/detok_streams.json`.
//!
//! Also includes a `proptest` no-panic property (ASVS V5 / this phase's success criterion 3):
//! `Detokenizer::step` must never panic on random `u32` token ids, including ids outside the
//! tokenizer's vocabulary range.

mod common;

use proptest::prelude::*;
use rsg_tokenizer::detokenize::Detokenizer;
use rsg_tokenizer::loader::{self, ModelAssets};
use serde_json::Value;

/// Maps a `common::ModelCase` slug to its `rsg_tokenizer::ModelSpec` constant.
fn model_spec(slug: &str) -> rsg_tokenizer::ModelSpec {
    match slug {
        "qwen3-0.6b" => rsg_tokenizer::QWEN3_0_6B,
        "llama-3.2-1b-instruct" => rsg_tokenizer::LLAMA_3_2_1B_INSTRUCT,
        other => panic!("no ModelSpec registered for slug {other:?}"),
    }
}

struct StreamCase {
    name: String,
    token_ids: Vec<u32>,
    chunks: Vec<String>,
}

/// The per-model detokenize-stream fixture cases, in committed order.
fn stream_cases(slug: &str) -> Vec<StreamCase> {
    let path = common::model_fixture(slug, "detok_streams.json");
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
            let token_ids = c["token_ids"]
                .as_array()
                .unwrap_or_else(|| panic!("case {name:?} missing `token_ids`"))
                .iter()
                .map(|v| v.as_u64().expect("id fits in u64") as u32)
                .collect();
            let chunks = c["chunks"]
                .as_array()
                .unwrap_or_else(|| panic!("case {name:?} missing `chunks`"))
                .iter()
                .map(|v| v.as_str().expect("chunk is a string").to_owned())
                .collect();
            StreamCase {
                name,
                token_ids,
                chunks,
            }
        })
        .collect()
}

/// Derives the model's real EOS token id from `tokenizer_config.json`'s `eos_token` string,
/// resolved through the tokenizer's own vocab -- mirrors how `AutoTokenizer` resolves
/// `eos_token_id` from `eos_token`.
fn eos_token_id(assets: &ModelAssets) -> u32 {
    let eos_token = assets
        .tokenizer_config
        .get("eos_token")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("tokenizer_config.json missing `eos_token`"));
    assets
        .tokenizer
        .token_to_id(eos_token)
        .unwrap_or_else(|| panic!("eos_token {eos_token:?} has no id in the tokenizer vocab"))
}

fn clean_up_tokenization_spaces(assets: &ModelAssets) -> bool {
    assets
        .tokenizer_config
        .get("clean_up_tokenization_spaces")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

#[test]
fn detokenize_streams_match_python_oracle_for_every_model() {
    let mut exercised_any_model = false;

    for case in common::MODELS {
        let fixture_path = common::model_fixture(case.slug, "detok_streams.json");
        if !fixture_path.exists() {
            continue; // not every model has detok_streams.json fixtures committed yet
        }
        let result = loader::load_model_assets(model_spec(case.slug));
        if common::skip_if_gated_unavailable(&result, case.slug) {
            continue;
        }
        exercised_any_model = true;

        let assets = result.unwrap_or_else(|e| panic!("load_model_assets({:?}): {e}", case.slug));
        let eos = eos_token_id(&assets);
        let clean_up = clean_up_tokenization_spaces(&assets);

        let cases = stream_cases(case.slug);
        assert!(
            !cases.is_empty(),
            "model {:?}: detok_streams.json has no cases",
            case.slug
        );

        let mut detokenizer = Detokenizer::new(assets.tokenizer, eos, clean_up);

        for (uid, stream_case) in cases.iter().enumerate() {
            assert!(
                !stream_case.token_ids.is_empty(),
                "model {:?} case {:?}: empty token_ids",
                case.slug,
                stream_case.name
            );
            let last_index = stream_case.token_ids.len() - 1;
            let mut actual_chunks = Vec::with_capacity(stream_case.token_ids.len());
            for (i, &token_id) in stream_case.token_ids.iter().enumerate() {
                let finished = i == last_index;
                let chunk = detokenizer
                    .step(uid as i64, token_id, finished)
                    .unwrap_or_else(|e| {
                        panic!(
                            "model {:?} case {:?}: step(token_id={token_id}, finished={finished}) failed: {e}",
                            case.slug, stream_case.name
                        )
                    });
                actual_chunks.push(chunk);
            }
            assert_eq!(
                &actual_chunks, &stream_case.chunks,
                "model {:?} case {:?}: incremental chunks differ from the Python oracle",
                case.slug, stream_case.name
            );
        }
    }

    assert!(
        exercised_any_model,
        "no model in common::MODELS has a detok_streams.json fixture -- nothing was tested"
    );
}

/// D-09 / this phase's success criterion 3 (no panics): the `finished_eos` fixture case's last
/// token id must be the model's real EOS id, and the concatenation of its streamed chunks must
/// never contain the EOS token's own decoded text -- confirms the EOS-exclusion branch actually
/// fired for the committed fixture, not just that the Rust port happens to match whatever the
/// fixture says.
#[test]
fn finished_eos_case_excludes_eos_text_from_streamed_chunks() {
    for case in common::MODELS {
        let fixture_path = common::model_fixture(case.slug, "detok_streams.json");
        if !fixture_path.exists() {
            continue;
        }

        let result = loader::load_model_assets(model_spec(case.slug));
        if common::skip_if_gated_unavailable(&result, case.slug) {
            continue;
        }
        let assets = result.unwrap_or_else(|e| panic!("load_model_assets({:?}): {e}", case.slug));
        let eos = eos_token_id(&assets);
        let eos_text = assets
            .tokenizer
            .decode(&[eos], false)
            .unwrap_or_else(|e| panic!("model {:?}: decode(eos): {e}", case.slug));

        let cases = stream_cases(case.slug);
        let finished_eos = cases
            .iter()
            .find(|c| c.name == "finished_eos")
            .unwrap_or_else(|| panic!("model {:?}: missing finished_eos case", case.slug));

        assert_eq!(
            *finished_eos.token_ids.last().expect("non-empty token_ids"),
            eos,
            "model {:?}: finished_eos case's last token id must be the real eos_token_id",
            case.slug
        );

        let concatenated: String = finished_eos.chunks.concat();
        assert!(
            !concatenated.contains(&eos_text),
            "model {:?}: finished_eos streamed text unexpectedly contains the EOS token's own decoded text {:?}: {:?}",
            case.slug,
            eos_text,
            concatenated
        );
    }
}

/// Security Domain (ASVS V5, DoS mitigation): `Detokenizer::step` must never panic, even on
/// out-of-vocab `u32` token ids or adversarial sequences engineered to straddle multi-byte UTF-8
/// boundaries. A separate `#[test]` (not the fixture loop above) using `proptest`'s `TestRunner`
/// directly -- rather than the `proptest!` macro -- so the real Qwen3-0.6B tokenizer is fetched
/// exactly once for the whole property (not once per one of the ~100 generated cases): each case
/// clones the already-loaded, lightweight `tokenizers::Tokenizer` instead of re-running
/// `load_model_assets`.
#[test]
fn detokenizer_step_never_panics_on_random_or_out_of_vocab_token_ids() {
    use proptest::test_runner::{Config as RunnerConfig, TestRunner};

    let assets = loader::load_model_assets(rsg_tokenizer::QWEN3_0_6B)
        .expect("load_model_assets(QWEN3_0_6B) for the no-panic property");
    let eos = eos_token_id(&assets);

    let mut runner = TestRunner::new(RunnerConfig {
        cases: 100,
        ..RunnerConfig::default()
    });
    let strategy = proptest::collection::vec(any::<u32>(), 1..32usize);

    let result = runner.run(&strategy, |ids| {
        let mut detokenizer = Detokenizer::new(assets.tokenizer.clone(), eos, false);
        let last_index = ids.len() - 1;
        for (i, &id) in ids.iter().enumerate() {
            let finished = i == last_index;
            // Deliberately ignore the Result: an ordinary decode `Err` is not a panic and not
            // the property under test here -- only an actual panic should fail this property.
            let _ = detokenizer.step(0, id, finished);
        }
        Ok(())
    });

    result.unwrap_or_else(|e| panic!("proptest found an input that panics `step`: {e}"));
}
