//! Byte-exactness of the Rust codec against the golden fixtures exported from upstream's
//! Python encoder (`scripts/gen_wire_fixtures.py`). Runs without Python.

mod common;

use std::path::PathBuf;

use rsg_wire::{SamplingParams, Tensor, WIRE_TYPE_TAGS};
use serde_json::Value;

fn manifest_cases(manifest: &Value) -> &Vec<Value> {
    manifest["cases"].as_array().expect("manifest cases array")
}

fn field<'a>(case: &'a Value, key: &str) -> &'a str {
    case[key]
        .as_str()
        .unwrap_or_else(|| panic!("manifest case is missing string field {key:?}: {case}"))
}

fn read_fixture(file: &str) -> Vec<u8> {
    let path = common::fixtures_dir().join(file);
    std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Decodes a fixture by its manifest metadata and re-encodes it.
fn reencode(top_type: &str, decoder: &str, bytes: &[u8]) -> Vec<u8> {
    match (top_type, decoder) {
        ("SamplingParams", _) => {
            rsg_wire::encode(&rsg_wire::decode::<SamplingParams>(bytes).expect("decode"))
        }
        ("Tensor", _) => rsg_wire::encode(&rsg_wire::decode::<Tensor>(bytes).expect("decode")),
        (_, "backend") => {
            rsg_wire::encode_backend(&rsg_wire::decode_backend(bytes).expect("decode"))
        }
        (_, "tokenizer") => {
            rsg_wire::encode_tokenizer(&rsg_wire::decode_tokenizer(bytes).expect("decode"))
        }
        _ => panic!("unknown decoder {decoder:?}"),
    }
    .expect("re-encode")
}

#[test]
fn every_fixture_roundtrips_byte_exact() {
    let manifest = common::manifest();
    let cases = manifest_cases(&manifest);
    assert!(!cases.is_empty(), "manifest lists no cases");
    for case in cases {
        let name = field(case, "name");
        let bytes = read_fixture(field(case, "file"));
        let again = reencode(field(case, "top_type"), field(case, "decoder"), &bytes);
        assert_eq!(
            common::hex(&again),
            common::hex(&bytes),
            "case {name}: decode then encode changed the bytes"
        );
    }
}

#[test]
fn hand_built_cases_match_fixtures() {
    for (name, value) in common::cases() {
        let expected = read_fixture(&format!("{name}.msgpack"));
        assert_eq!(
            common::hex(&value.encode()),
            common::hex(&expected),
            "case {name}: hand-built value does not encode to the fixture bytes"
        );
    }
}

#[test]
fn case_tables_agree() {
    let manifest = common::manifest();
    let from_manifest: Vec<&str> = manifest_cases(&manifest)
        .iter()
        .map(|c| field(c, "name"))
        .collect();
    let from_rust: Vec<&str> = common::cases().into_iter().map(|(n, _)| n).collect();
    assert_eq!(from_rust, from_manifest);
}

#[test]
fn sha_single_source() {
    let manifest = common::manifest();
    let vendor_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/UPSTREAM_SHA");
    let vendor = std::fs::read_to_string(&vendor_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", vendor_path.display()));
    assert_eq!(rsg_wire::UPSTREAM_SHA, vendor.trim());
    assert_eq!(
        Some(rsg_wire::UPSTREAM_SHA),
        manifest["upstream_sha"].as_str()
    );
    assert_eq!(rsg_wire::UPSTREAM_SHA.len(), 40);
    assert!(
        rsg_wire::UPSTREAM_SHA
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
}

#[test]
fn all_type_tags_covered() {
    let manifest = common::manifest();
    let tags: Vec<&str> = manifest["type_tags"]
        .as_array()
        .expect("type_tags array")
        .iter()
        .map(|t| t.as_str().expect("type tag string"))
        .collect();
    assert_eq!(tags, WIRE_TYPE_TAGS);
    let cases = manifest_cases(&manifest);
    for tag in WIRE_TYPE_TAGS {
        assert!(
            cases
                .iter()
                .any(|c| field(c, "category") == "base" && field(c, "top_type") == tag),
            "no base_ case has top_type {tag}"
        );
    }
}

#[test]
fn manifest_lengths_match_files() {
    let manifest = common::manifest();
    for case in manifest_cases(&manifest) {
        let len = read_fixture(field(case, "file")).len() as u64;
        assert_eq!(
            Some(len),
            case["len"].as_u64(),
            "case {}: manifest len differs from the file size",
            field(case, "name")
        );
    }
}
