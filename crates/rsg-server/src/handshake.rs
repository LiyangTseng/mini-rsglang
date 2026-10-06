//! The readiness handshake the launcher writes to rsg-server's stdin (D-10, D-11).
//!
//! One JSON line, sent once the backend is ready:
//! `{"handshake_version":1,"upstream_sha":"<40 hex>","max_seq_len":..,"eos_token_id":..|null,
//!   "page_size":..,"max_running_req":..,"num_pages":..}`

use std::fmt;

use serde::{Deserialize, Serialize};

/// The only handshake schema version this binary understands.
pub const HANDSHAKE_VERSION: u32 = 1;

/// The upstream mini-sglang SHA the Rust wire fixtures were generated from.
/// Single source: `vendor/UPSTREAM_SHA`.
pub const EXPECTED_UPSTREAM_SHA: &str = include_str!("../../../vendor/UPSTREAM_SHA").trim_ascii();

/// The handshake payload. Every key is required (`eos_token_id` may be `null`);
/// any other key is rejected.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Handshake {
    pub handshake_version: u32,
    pub upstream_sha: String,
    pub max_seq_len: u64,
    // An explicit deserialize_with turns off serde's implicit missing-means-None
    // handling for Option, so the key itself is required while an explicit null stays allowed.
    #[serde(deserialize_with = "Option::deserialize")]
    pub eos_token_id: Option<u64>,
    pub page_size: u64,
    pub max_running_req: u64,
    pub num_pages: u64,
}

impl Handshake {
    /// `eos_token_id` as a log-friendly string: the number, or `null`.
    pub fn eos_display(&self) -> String {
        match self.eos_token_id {
            Some(id) => id.to_string(),
            None => "null".to_string(),
        }
    }

    /// Serializes this handshake as one JSON line (no trailing newline), the
    /// same schema `parse_handshake` accepts. Serialization of this plain,
    /// all-primitive struct cannot fail.
    pub fn to_json_line(&self) -> String {
        serde_json::to_string(self).expect("Handshake serialization is infallible")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandshakeError {
    Malformed(String),
    UnsupportedVersion { got: u32, expected: u32 },
    ShaMismatch { got: String, expected: String },
}

impl fmt::Display for HandshakeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HandshakeError::Malformed(msg) => write!(f, "malformed handshake line: {msg}"),
            HandshakeError::UnsupportedVersion { got, expected } => write!(
                f,
                "unsupported handshake_version {got} (this rsg-server understands {expected})"
            ),
            HandshakeError::ShaMismatch { got, expected } => write!(
                f,
                "upstream SHA mismatch: the Rust wire fixtures were generated for {expected}, \
                 but the launcher reported {got}"
            ),
        }
    }
}

impl std::error::Error for HandshakeError {}

/// Parse one handshake line and check its version and upstream SHA.
pub fn parse_handshake(line: &str, expected_sha: &str) -> Result<Handshake, HandshakeError> {
    let line = line.trim_end_matches(['\r', '\n']);
    let hs: Handshake =
        serde_json::from_str(line).map_err(|e| HandshakeError::Malformed(e.to_string()))?;
    if hs.handshake_version != HANDSHAKE_VERSION {
        return Err(HandshakeError::UnsupportedVersion {
            got: hs.handshake_version,
            expected: HANDSHAKE_VERSION,
        });
    }
    if hs.upstream_sha != expected_sha {
        return Err(HandshakeError::ShaMismatch {
            got: hs.upstream_sha,
            expected: expected_sha.to_string(),
        });
    }
    Ok(hs)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "9a91cfafe754aa85daee49998176275667eb58f2";

    fn line(sha: &str, eos: &str) -> String {
        format!(
            "{{\"handshake_version\":1,\"upstream_sha\":\"{sha}\",\"max_seq_len\":4096,\
             \"eos_token_id\":{eos},\"page_size\":16,\"max_running_req\":8,\"num_pages\":1024}}\n"
        )
    }

    #[test]
    fn parses_contract_line() {
        let hs = parse_handshake(&line(SHA, "151645"), SHA).expect("valid handshake");
        assert_eq!(
            hs,
            Handshake {
                handshake_version: 1,
                upstream_sha: SHA.to_string(),
                max_seq_len: 4096,
                eos_token_id: Some(151645),
                page_size: 16,
                max_running_req: 8,
                num_pages: 1024,
            }
        );
        assert_eq!(hs.eos_display(), "151645");
    }

    #[test]
    fn null_eos_parses_to_none() {
        let hs = parse_handshake(&line(SHA, "null"), SHA).expect("valid handshake");
        assert_eq!(hs.eos_token_id, None);
        assert_eq!(hs.eos_display(), "null");
    }

    #[test]
    fn sha_mismatch_names_both_shas() {
        let bad = "0000000000000000000000000000000000000000";
        let err = parse_handshake(&line(bad, "151645"), EXPECTED_UPSTREAM_SHA).unwrap_err();
        assert!(matches!(err, HandshakeError::ShaMismatch { .. }), "{err:?}");
        let msg = err.to_string();
        assert!(msg.contains(bad), "{msg}");
        assert!(msg.contains(EXPECTED_UPSTREAM_SHA), "{msg}");
    }

    #[test]
    fn unsupported_version() {
        let l = line(SHA, "151645").replace("\"handshake_version\":1", "\"handshake_version\":2");
        let err = parse_handshake(&l, SHA).unwrap_err();
        assert_eq!(
            err,
            HandshakeError::UnsupportedVersion {
                got: 2,
                expected: 1
            }
        );
    }

    #[test]
    fn extra_key_is_malformed() {
        let l =
            line(SHA, "151645").replace("\"num_pages\":1024}", "\"num_pages\":1024,\"extra\":1}");
        assert!(matches!(
            parse_handshake(&l, SHA),
            Err(HandshakeError::Malformed(_))
        ));
    }

    #[test]
    fn missing_key_is_malformed() {
        let l = line(SHA, "151645").replace(",\"num_pages\":1024", "");
        assert!(matches!(
            parse_handshake(&l, SHA),
            Err(HandshakeError::Malformed(_))
        ));
    }

    #[test]
    fn missing_eos_key_is_malformed() {
        let l = line(SHA, "151645").replace("\"eos_token_id\":151645,", "");
        let err = parse_handshake(&l, SHA).unwrap_err();
        match err {
            HandshakeError::Malformed(msg) => {
                assert!(msg.contains("eos_token_id"), "{msg}");
            }
            other => panic!("expected Malformed, got {other:?}"),
        }
    }

    #[test]
    fn not_json_is_malformed() {
        assert!(matches!(
            parse_handshake("not json", SHA),
            Err(HandshakeError::Malformed(_))
        ));
    }

    #[test]
    fn to_json_line_round_trips_through_parse_handshake() {
        let hs = Handshake {
            handshake_version: HANDSHAKE_VERSION,
            upstream_sha: EXPECTED_UPSTREAM_SHA.to_string(),
            max_seq_len: 4096,
            eos_token_id: Some(151645),
            page_size: 16,
            max_running_req: 8,
            num_pages: 1024,
        };
        let line = hs.to_json_line();
        assert!(!line.ends_with('\n'), "{line:?}");
        let parsed = parse_handshake(&line, EXPECTED_UPSTREAM_SHA).expect("valid handshake");
        assert_eq!(parsed, hs);
    }

    #[test]
    fn expected_sha_is_vendor_file() {
        assert_eq!(EXPECTED_UPSTREAM_SHA.len(), 40);
        assert!(
            EXPECTED_UPSTREAM_SHA
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
        );
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../vendor/UPSTREAM_SHA");
        let on_disk = std::fs::read_to_string(path).expect("read vendor/UPSTREAM_SHA");
        assert_eq!(EXPECTED_UPSTREAM_SHA, on_disk.trim());
    }
}
