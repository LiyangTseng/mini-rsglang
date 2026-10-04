//! The fixture case table, mirrored from `scripts/gen_wire_fixtures.py` row for row and in order.
//! `tests/fixtures.rs` checks the two tables agree with `fixtures/wire/manifest.json`.

use std::path::PathBuf;

use rsg_wire::{BackendMsg, SamplingParams, Tensor, TokenizerMsg};

/// The deterministic int32 token ids `(i * 7919) % 151936` for `i` in `0..n`.
pub fn ids(n: usize) -> Vec<i32> {
    (0..n)
        .map(|i| ((i as i64 * 7919) % 151936) as i32)
        .collect()
}

/// A hand-built wire value for one fixture case.
pub enum CaseValue {
    Backend(BackendMsg),
    Tokenizer(TokenizerMsg),
    Sampling(SamplingParams),
    Tensor(Tensor),
}

impl CaseValue {
    pub fn encode(&self) -> Vec<u8> {
        match self {
            CaseValue::Backend(m) => rsg_wire::encode_backend(m),
            CaseValue::Tokenizer(m) => rsg_wire::encode_tokenizer(m),
            CaseValue::Sampling(sp) => rsg_wire::encode(sp),
            CaseValue::Tensor(t) => rsg_wire::encode(t),
        }
        .expect("encode case value")
    }
}

fn user(uid: i64, n: usize, sampling_params: SamplingParams) -> CaseValue {
    CaseValue::Backend(BackendMsg::UserMsg {
        uid,
        input_ids: Tensor::from_i32_slice(&ids(n)),
        sampling_params,
    })
}

fn detok(uid: i64, next_token: i64, finished: bool) -> TokenizerMsg {
    TokenizerMsg::DetokenizeMsg {
        uid,
        next_token,
        finished,
    }
}

/// Every fixture case, in table order.
pub fn cases() -> Vec<(&'static str, CaseValue)> {
    let sp = SamplingParams::default;
    let mut cases = vec![
        (
            "base_user_msg",
            user(
                7,
                3,
                SamplingParams {
                    temperature: 0.0,
                    top_k: -1,
                    top_p: 1.0,
                    ignore_eos: false,
                    max_tokens: 128,
                },
            ),
        ),
        (
            "base_abort_backend_msg",
            CaseValue::Backend(BackendMsg::AbortBackendMsg { uid: 7 }),
        ),
        ("base_exit_msg", CaseValue::Backend(BackendMsg::ExitMsg {})),
        (
            "base_batch_backend_msg",
            CaseValue::Backend(BackendMsg::BatchBackendMsg {
                data: vec![
                    BackendMsg::UserMsg {
                        uid: 1,
                        input_ids: Tensor::from_i32_slice(&ids(3)),
                        sampling_params: sp(),
                    },
                    BackendMsg::AbortBackendMsg { uid: 2 },
                ],
            }),
        ),
        (
            "base_detokenize_msg",
            CaseValue::Tokenizer(detok(7, 151645, true)),
        ),
        (
            "base_batch_tokenizer_msg",
            CaseValue::Tokenizer(TokenizerMsg::BatchTokenizerMsg {
                data: vec![detok(1, 5, false)],
            }),
        ),
        (
            "base_sampling_params",
            CaseValue::Sampling(SamplingParams {
                temperature: 0.7,
                top_k: 50,
                top_p: 0.9,
                ignore_eos: true,
                max_tokens: 256,
            }),
        ),
        (
            "base_tensor",
            CaseValue::Tensor(Tensor::from_i32_slice(&ids(3))),
        ),
        (
            "batch_tokenizer_n",
            CaseValue::Tokenizer(TokenizerMsg::BatchTokenizerMsg {
                data: (1..=5).map(|i| detok(i, i + 4, i % 2 == 0)).collect(),
            }),
        ),
        (
            "batch_backend_many",
            CaseValue::Backend(BackendMsg::BatchBackendMsg {
                data: vec![
                    BackendMsg::UserMsg {
                        uid: 10,
                        input_ids: Tensor::from_i32_slice(&ids(5)),
                        sampling_params: SamplingParams {
                            max_tokens: 16,
                            ..sp()
                        },
                    },
                    BackendMsg::AbortBackendMsg { uid: 11 },
                    BackendMsg::UserMsg {
                        uid: 12,
                        input_ids: Tensor::from_i32_slice(&ids(1)),
                        sampling_params: sp(),
                    },
                    BackendMsg::AbortBackendMsg { uid: 13 },
                ],
            }),
        ),
    ];
    let uids: [(&str, i64); 7] = [
        ("int_uid_127", 127),
        ("int_uid_128", 128),
        ("int_uid_255", 255),
        ("int_uid_256", 256),
        ("int_uid_65535", 65535),
        ("int_uid_65536", 65536),
        ("int_uid_4294967296", 4294967296),
    ];
    for (name, uid) in uids {
        cases.push((
            name,
            CaseValue::Backend(BackendMsg::AbortBackendMsg { uid }),
        ));
    }
    let top_ks: [(&str, i64); 5] = [
        ("int_top_k_neg1", -1),
        ("int_top_k_neg32", -32),
        ("int_top_k_neg33", -33),
        ("int_top_k_neg128", -128),
        ("int_top_k_neg129", -129),
    ];
    for (name, top_k) in top_ks {
        cases.push((name, user(1, 1, SamplingParams { top_k, ..sp() })));
    }
    cases.extend([
        (
            "int_max_tokens_65536",
            user(
                1,
                1,
                SamplingParams {
                    max_tokens: 65536,
                    ..sp()
                },
            ),
        ),
        (
            "int_next_token_65536",
            CaseValue::Tokenizer(detok(1, 65536, false)),
        ),
        (
            "float_top_p_0_9",
            user(1, 1, SamplingParams { top_p: 0.9, ..sp() }),
        ),
        (
            "float_temperature_1_5",
            user(
                1,
                1,
                SamplingParams {
                    temperature: 1.5,
                    ..sp()
                },
            ),
        ),
        (
            "float_temperature_0_1",
            user(
                1,
                1,
                SamplingParams {
                    temperature: 0.1,
                    ..sp()
                },
            ),
        ),
        (
            "bool_ignore_eos_true",
            user(
                1,
                1,
                SamplingParams {
                    ignore_eos: true,
                    ..sp()
                },
            ),
        ),
        (
            "bool_finished_false",
            CaseValue::Tokenizer(detok(2, 9, false)),
        ),
    ]);
    let lens: [(&str, usize); 5] = [
        ("tensor_len_1", 1),
        ("tensor_len_63", 63),
        ("tensor_len_64", 64),
        ("tensor_len_16383", 16383),
        ("tensor_len_16384", 16384),
    ];
    for (name, n) in lens {
        cases.push((name, user(1, n, sp())));
    }
    cases
}

/// `fixtures/wire` at the repository root.
pub fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/wire")
}

/// Lowercase hex of a byte slice.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The parsed `fixtures/wire/manifest.json`.
pub fn manifest() -> serde_json::Value {
    let path = fixtures_dir().join("manifest.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}
