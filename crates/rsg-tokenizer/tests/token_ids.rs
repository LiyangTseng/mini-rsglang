//! TOK-01: Rust `encode()` matches the Python oracle's token ids, case by case, for every model
//! in `common::MODELS`, against the committed `fixtures/tokenizer/{slug}/token_ids.json`.

mod common;

use serde_json::Value;

/// Maps a `common::ModelCase` slug to its `rsg_tokenizer::ModelSpec` constant.
fn model_spec(slug: &str) -> rsg_tokenizer::ModelSpec {
    match slug {
        "qwen3-0.6b" => rsg_tokenizer::QWEN3_0_6B,
        "llama-3.2-1b-instruct" => rsg_tokenizer::LLAMA_3_2_1B_INSTRUCT,
        other => panic!("no ModelSpec registered for slug {other:?}"),
    }
}

/// The shared, model-agnostic `(name, text)` corpus.
fn id_corpus() -> Vec<(String, String)> {
    let path = common::fixtures_dir().join("id_corpus.json");
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
            let text = c["text"]
                .as_str()
                .unwrap_or_else(|| panic!("case {name:?} missing `text`"))
                .to_owned();
            (name, text)
        })
        .collect()
}

/// The per-model expected ids, keyed by case name.
fn expected_ids(slug: &str) -> Vec<(String, Vec<u32>)> {
    let path = common::model_fixture(slug, "token_ids.json");
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
            let ids = c["ids"]
                .as_array()
                .unwrap_or_else(|| panic!("case {name:?} missing `ids`"))
                .iter()
                .map(|v| v.as_u64().expect("id fits in u64") as u32)
                .collect();
            (name, ids)
        })
        .collect()
}

#[test]
fn token_ids_match_python_oracle_for_every_model() {
    let corpus = id_corpus();
    for case in common::MODELS {
        let result = rsg_tokenizer::loader::load_model_assets(model_spec(case.slug));
        if common::skip_if_gated_unavailable(&result, case.slug) {
            continue;
        }
        let assets = result.unwrap_or_else(|e| panic!("load_model_assets({:?}): {e}", case.slug));
        let expected = expected_ids(case.slug);
        assert_eq!(
            expected.len(),
            corpus.len(),
            "model {:?}: fixture case count does not match id_corpus.json",
            case.slug
        );
        for ((corpus_name, text), (fixture_name, ids)) in corpus.iter().zip(expected.iter()) {
            assert_eq!(
                corpus_name, fixture_name,
                "model {:?}: case name order mismatch between id_corpus.json and token_ids.json",
                case.slug
            );
            let actual = rsg_tokenizer::encode::encode_text(&assets.tokenizer, text)
                .unwrap_or_else(|e| {
                    panic!(
                        "model {:?} case {corpus_name:?}: encode_text: {e}",
                        case.slug
                    )
                });
            assert_eq!(
                &actual, ids,
                "model {:?} case {corpus_name:?}: token ids differ from Python oracle",
                case.slug
            );
        }
    }
}
