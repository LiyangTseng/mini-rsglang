//! The msgpack codec for the mini-sglang scheduler boundary.
//!
//! Upstream (`vendor/mini-sglang`, `message/utils.py`) serializes every message as a map whose
//! first key is `"__type__"` (the class name), followed by the dataclass fields in declaration
//! order, packed with `msgpack.packb(obj, use_bin_type=True)`. The scheduler decodes with
//! `cls(**kwargs)`, so an extra or renamed key raises `TypeError` and kills its loop. Byte
//! equality of the whole frame with upstream's encoder is therefore the parity contract.
//!
//! "All 7 message types" means the 7 classes upstream decodes with `cls(**kwargs)` on the
//! scheduler boundary: `UserMsg`, `AbortBackendMsg`, `ExitMsg`, `BatchBackendMsg`,
//! `DetokenizeMsg`, `BatchTokenizerMsg` and `SamplingParams`. `Tensor` is an 8th `__type__` tag
//! with a special-cased encoding (`buffer` as msgpack bin of int32 little-endian bytes, `dtype`
//! as the string `"torch.int32"`). [`WIRE_TYPE_TAGS`] lists all 8.
//!
//! Encoding rules that byte equality depends on:
//! - named maps (`rmp_serde::to_vec_named`), never the compact array encoding;
//! - struct field order is map key order, mirroring the upstream dataclasses exactly;
//! - floats are `f64` (msgpack float64), never `f32`;
//! - the tensor buffer goes through `serde_bytes` so it is msgpack bin, not an int array.
//!
//! Decoding ignores unknown keys; the decode-then-re-encode fixture tests catch schema drift.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// The upstream mini-sglang commit this codec mirrors. Single source: `vendor/UPSTREAM_SHA`.
pub const UPSTREAM_SHA: &str = include_str!("../../../vendor/UPSTREAM_SHA").trim_ascii();

/// The `dtype` string upstream writes for a `torch.int32` tensor (`str(tensor.dtype)`).
pub const TENSOR_DTYPE_INT32: &str = "torch.int32";

/// Every `__type__` tag that crosses the scheduler boundary: the 7 message types plus `Tensor`.
pub const WIRE_TYPE_TAGS: [&str; 8] = [
    "UserMsg",
    "AbortBackendMsg",
    "ExitMsg",
    "BatchBackendMsg",
    "DetokenizeMsg",
    "BatchTokenizerMsg",
    "SamplingParams",
    "Tensor",
];

/// Errors from encoding, decoding or interpreting wire values.
#[derive(Debug, thiserror::Error)]
pub enum WireError {
    #[error("msgpack encode failed: {0}")]
    Encode(#[from] rmp_serde::encode::Error),
    #[error("msgpack decode failed: {0}")]
    Decode(#[from] rmp_serde::decode::Error),
    #[error("tensor dtype is {got:?}, expected {TENSOR_DTYPE_INT32:?}")]
    TensorDtype { got: String },
    #[error("tensor buffer length {len} is not a multiple of 4")]
    TensorLength { len: usize },
}

/// A 1-D tensor as upstream serializes it: `{"__type__": "Tensor", "buffer": <bin>, "dtype": ...}`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "__type__", rename = "Tensor")]
pub struct Tensor {
    #[serde(with = "serde_bytes")]
    pub buffer: Vec<u8>,
    pub dtype: String,
}

impl Tensor {
    /// Builds an int32 tensor; the buffer is little-endian, like numpy `tobytes()` on the
    /// little-endian hosts upstream supports.
    pub fn from_i32_slice(ids: &[i32]) -> Tensor {
        let mut buffer = Vec::with_capacity(ids.len() * 4);
        for id in ids {
            buffer.extend_from_slice(&id.to_le_bytes());
        }
        Tensor {
            buffer,
            dtype: TENSOR_DTYPE_INT32.to_owned(),
        }
    }

    /// Reads the buffer back as int32 values. Rejects any dtype other than `torch.int32` and
    /// any buffer whose length is not a multiple of 4.
    pub fn to_i32_vec(&self) -> Result<Vec<i32>, WireError> {
        if self.dtype != TENSOR_DTYPE_INT32 {
            return Err(WireError::TensorDtype {
                got: self.dtype.clone(),
            });
        }
        if !self.buffer.len().is_multiple_of(4) {
            return Err(WireError::TensorLength {
                len: self.buffer.len(),
            });
        }
        Ok(self
            .buffer
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| i32::from_le_bytes(*c))
            .collect())
    }
}

/// `minisgl.core.SamplingParams`, fields in declaration order.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "__type__", rename = "SamplingParams")]
pub struct SamplingParams {
    pub temperature: f64,
    pub top_k: i64,
    pub top_p: f64,
    pub ignore_eos: bool,
    pub max_tokens: i64,
}

impl Default for SamplingParams {
    /// Upstream defaults: `temperature=0.0, top_k=-1, top_p=1.0, ignore_eos=False, max_tokens=1024`.
    fn default() -> Self {
        SamplingParams {
            temperature: 0.0,
            top_k: -1,
            top_p: 1.0,
            ignore_eos: false,
            max_tokens: 1024,
        }
    }
}

/// Messages the scheduler receives (`minisgl.message.backend`). Variant names are the wire tags.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "__type__")]
pub enum BackendMsg {
    UserMsg {
        uid: i64,
        input_ids: Tensor,
        sampling_params: SamplingParams,
    },
    AbortBackendMsg {
        uid: i64,
    },
    /// A braced empty variant, so it encodes as the 1-entry map `{"__type__": "ExitMsg"}`.
    ExitMsg {},
    BatchBackendMsg {
        data: Vec<BackendMsg>,
    },
}

/// Messages the scheduler sends to the detokenizer (`minisgl.message.tokenizer`).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "__type__")]
pub enum TokenizerMsg {
    DetokenizeMsg {
        uid: i64,
        next_token: i64,
        finished: bool,
    },
    BatchTokenizerMsg {
        data: Vec<TokenizerMsg>,
    },
}

/// Encodes any wire value as a named msgpack map (the same bytes as upstream's encoder).
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, WireError> {
    Ok(rmp_serde::to_vec_named(value)?)
}

/// Decodes any wire value from msgpack bytes.
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, WireError> {
    Ok(rmp_serde::from_slice(bytes)?)
}

/// Encodes a message for the scheduler's backend socket.
pub fn encode_backend(msg: &BackendMsg) -> Result<Vec<u8>, WireError> {
    encode(msg)
}

/// Decodes a message read from the scheduler's backend socket.
pub fn decode_backend(bytes: &[u8]) -> Result<BackendMsg, WireError> {
    decode(bytes)
}

/// Encodes a message for the detokenizer socket.
pub fn encode_tokenizer(msg: &TokenizerMsg) -> Result<Vec<u8>, WireError> {
    encode(msg)
}

/// Decodes a message read from the detokenizer socket.
pub fn decode_tokenizer(bytes: &[u8]) -> Result<TokenizerMsg, WireError> {
    decode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ABORT_PREFIX: &str = "82a85f5f747970655f5faf41626f72744261636b656e644d7367a3756964";
    const BUFFER_KEY: &str = "a6627566666572";
    const DTYPE_ENTRY: &str = "a56474797065ab746f7263682e696e743332";

    fn ids(n: usize) -> Vec<i32> {
        (0..n)
            .map(|i| ((i as i64 * 7919) % 151936) as i32)
            .collect()
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn backend_hex(msg: &BackendMsg) -> String {
        hex(&encode_backend(msg).expect("encode backend"))
    }

    #[test]
    fn abort_backend_msg_uid_7_known_bytes() {
        assert_eq!(
            backend_hex(&BackendMsg::AbortBackendMsg { uid: 7 }),
            format!("{ABORT_PREFIX}07")
        );
    }

    #[test]
    fn exit_msg_is_one_entry_map() {
        assert_eq!(
            backend_hex(&BackendMsg::ExitMsg {}),
            "81a85f5f747970655f5fa7457869744d7367"
        );
    }

    #[test]
    fn uid_integer_widths_match_python() {
        let cases: [(i64, &str); 7] = [
            (127, "7f"),
            (128, "cc80"),
            (255, "ccff"),
            (256, "cd0100"),
            (65535, "cdffff"),
            (65536, "ce00010000"),
            (4294967296, "cf0000000100000000"),
        ];
        for (uid, tail) in cases {
            assert_eq!(
                backend_hex(&BackendMsg::AbortBackendMsg { uid }),
                format!("{ABORT_PREFIX}{tail}"),
                "uid {uid}"
            );
        }
    }

    #[test]
    fn sampling_params_floats_are_f64_and_negative_ints_match() {
        let sp = |top_k: i64| SamplingParams {
            top_p: 0.9,
            top_k,
            ..SamplingParams::default()
        };
        let h = hex(&encode(&sp(-1)).expect("encode"));
        assert!(
            h.contains("a5746f705f70cb3feccccccccccccd"),
            "top_p 0.9 as f64: {h}"
        );
        assert!(h.contains("a5746f705f6bff"), "top_k -1: {h}");
        let h = hex(&encode(&sp(-33)).expect("encode"));
        assert!(h.contains("a5746f705f6bd0df"), "top_k -33: {h}");
        let h = hex(&encode(&sp(-129)).expect("encode"));
        assert!(h.contains("a5746f705f6bd1ff7f"), "top_k -129: {h}");
    }

    #[test]
    fn tensor_bin_header_widths_match_python() {
        let cases: [(usize, &str); 4] = [
            (63, "c4fc"),
            (64, "c50100"),
            (16383, "c5fffc"),
            (16384, "c600010000"),
        ];
        for (n, header) in cases {
            let h = hex(&encode(&Tensor::from_i32_slice(&ids(n))).expect("encode"));
            assert!(
                h.contains(&format!("{BUFFER_KEY}{header}")),
                "tensor of {n} ids: bin header {header} missing"
            );
            assert!(
                h.contains(DTYPE_ENTRY),
                "tensor of {n} ids: dtype entry missing"
            );
        }
    }

    #[test]
    fn tensor_buffer_is_little_endian_int32() {
        let t = Tensor::from_i32_slice(&[1, -2]);
        assert_eq!(t.buffer, vec![1, 0, 0, 0, 0xfe, 0xff, 0xff, 0xff]);
        assert_eq!(t.dtype, TENSOR_DTYPE_INT32);
        assert_eq!(t.to_i32_vec().expect("int32 tensor"), vec![1, -2]);
    }

    #[test]
    fn tensor_to_i32_vec_rejects_wrong_dtype_and_length() {
        let wrong_dtype = Tensor {
            buffer: vec![0; 8],
            dtype: "torch.int64".to_owned(),
        };
        assert!(matches!(
            wrong_dtype.to_i32_vec(),
            Err(WireError::TensorDtype { .. })
        ));
        let wrong_len = Tensor {
            buffer: vec![0; 5],
            dtype: TENSOR_DTYPE_INT32.to_owned(),
        };
        assert!(matches!(
            wrong_len.to_i32_vec(),
            Err(WireError::TensorLength { len: 5 })
        ));
    }

    #[test]
    fn every_variant_round_trips() {
        let user = BackendMsg::UserMsg {
            uid: 1,
            input_ids: Tensor::from_i32_slice(&ids(3)),
            sampling_params: SamplingParams::default(),
        };
        let backend = [
            user.clone(),
            BackendMsg::AbortBackendMsg { uid: 2 },
            BackendMsg::ExitMsg {},
            BackendMsg::BatchBackendMsg {
                data: vec![
                    user,
                    BackendMsg::AbortBackendMsg { uid: 3 },
                    BackendMsg::BatchBackendMsg { data: vec![] },
                ],
            },
        ];
        for msg in backend {
            let bytes = encode_backend(&msg).expect("encode");
            assert_eq!(decode_backend(&bytes).expect("decode"), msg);
        }

        let detok = TokenizerMsg::DetokenizeMsg {
            uid: 4,
            next_token: 151645,
            finished: true,
        };
        let tokenizer = [
            detok.clone(),
            TokenizerMsg::BatchTokenizerMsg {
                data: vec![detok, TokenizerMsg::BatchTokenizerMsg { data: vec![] }],
            },
        ];
        for msg in tokenizer {
            let bytes = encode_tokenizer(&msg).expect("encode");
            assert_eq!(decode_tokenizer(&bytes).expect("decode"), msg);
        }

        let sp = SamplingParams {
            temperature: 0.7,
            top_k: 50,
            top_p: 0.9,
            ignore_eos: true,
            max_tokens: 256,
        };
        assert_eq!(
            decode::<SamplingParams>(&encode(&sp).expect("encode")).expect("decode"),
            sp
        );
        let t = Tensor::from_i32_slice(&ids(5));
        assert_eq!(
            decode::<Tensor>(&encode(&t).expect("encode")).expect("decode"),
            t
        );
    }

    #[test]
    fn unknown_type_tag_is_rejected() {
        // {"__type__": "NotAMsg", "uid": 7}
        let mut bytes = vec![0x82, 0xa8];
        bytes.extend_from_slice(b"__type__");
        bytes.push(0xa7);
        bytes.extend_from_slice(b"NotAMsg");
        bytes.push(0xa3);
        bytes.extend_from_slice(b"uid");
        bytes.push(0x07);
        assert!(decode_backend(&bytes).is_err());
    }

    #[test]
    fn type_tags_and_upstream_sha() {
        assert_eq!(
            WIRE_TYPE_TAGS,
            [
                "UserMsg",
                "AbortBackendMsg",
                "ExitMsg",
                "BatchBackendMsg",
                "DetokenizeMsg",
                "BatchTokenizerMsg",
                "SamplingParams",
                "Tensor",
            ]
        );
        assert_eq!(UPSTREAM_SHA.len(), 40);
        assert!(
            UPSTREAM_SHA
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
    }
}
