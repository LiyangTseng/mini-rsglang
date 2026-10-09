//! TTFT/ITL/E2E latency recording on `hdrhistogram`, microsecond resolution.

use std::fmt::Write as _;

use hdrhistogram::Histogram;
use hdrhistogram::serialization::{Deserializer, Serializer, V2DeflateSerializer};
use serde::{Deserialize, Serialize};

use crate::client::{Outcome, RequestRecord};

/// Upper bound in microseconds (1 hour): generously above any realistic
/// generation latency, so a long-running request never overflows the
/// histogram's range.
pub const HIST_MAX_US: u64 = 3_600_000_000;

fn new_histogram() -> Histogram<u64> {
    Histogram::<u64>::new_with_bounds(1, HIST_MAX_US, 3).expect("static histogram bounds are valid")
}

/// TTFT/ITL/E2E histograms, each in microseconds.
pub struct LatencyHistograms {
    pub ttft_us: Histogram<u64>,
    pub itl_us: Histogram<u64>,
    pub e2e_us: Histogram<u64>,
}

impl LatencyHistograms {
    pub fn new() -> Self {
        Self {
            ttft_us: new_histogram(),
            itl_us: new_histogram(),
            e2e_us: new_histogram(),
        }
    }

    /// Records TTFT and ITL gaps for `Completed`/`Cancelled` requests that
    /// observed at least one chunk, and E2E for `Completed` requests only.
    /// Values are clamped to `[1, HIST_MAX_US]` microseconds.
    pub fn record(&mut self, r: &RequestRecord) {
        let observed_prefill = matches!(r.outcome, Outcome::Completed | Outcome::Cancelled);
        if observed_prefill {
            if let Some(ttft) = r.ttft {
                record_clamped(&mut self.ttft_us, ttft);
            }
            for gap in &r.itl {
                record_clamped(&mut self.itl_us, *gap);
            }
        }
        if matches!(r.outcome, Outcome::Completed) {
            record_clamped(&mut self.e2e_us, r.e2e);
        }
    }

    /// Merges `other`'s counts into `self` (for combining per-worker
    /// histograms after a concurrent run).
    pub fn merge(&mut self, other: &LatencyHistograms) -> anyhow::Result<()> {
        self.ttft_us.add(&other.ttft_us)?;
        self.itl_us.add(&other.itl_us)?;
        self.e2e_us.add(&other.e2e_us)?;
        Ok(())
    }

    pub fn summary(&self) -> LatencySummary {
        LatencySummary {
            ttft_ms: percentiles(&self.ttft_us),
            itl_ms: percentiles(&self.itl_us),
            e2e_ms: percentiles(&self.e2e_us),
        }
    }

    /// Encodes all three histograms as hex-encoded HdrHistogram V2-deflate
    /// bytes (D-07), so a raw per-trial histogram can be stored in a run
    /// manifest and reloaded losslessly.
    pub fn encode(&self) -> anyhow::Result<EncodedHistograms> {
        Ok(EncodedHistograms {
            encoding: "hdrhistogram-v2-deflate+hex".to_string(),
            ttft_us: encode_histogram(&self.ttft_us)?,
            itl_us: encode_histogram(&self.itl_us)?,
            e2e_us: encode_histogram(&self.e2e_us)?,
        })
    }
}

impl Default for LatencyHistograms {
    fn default() -> Self {
        Self::new()
    }
}

fn record_clamped(h: &mut Histogram<u64>, d: std::time::Duration) {
    let us = (d.as_micros().max(1) as u64).min(HIST_MAX_US);
    // record() only errors when the value is out of the histogram's
    // configured range, which the clamp above already prevents.
    let _ = h.record(us);
}

/// `value_at_quantile(quantile) / 1000.0`, where `quantile` is in `[0.0,
/// 1.0]`. Returns `None` for an empty histogram.
pub fn percentile_ms(h: &Histogram<u64>, quantile: f64) -> Option<f64> {
    if h.is_empty() {
        return None;
    }
    Some(h.value_at_quantile(quantile) as f64 / 1000.0)
}

fn percentiles(h: &Histogram<u64>) -> Percentiles {
    Percentiles {
        count: h.len(),
        p50: percentile_ms(h, 0.50),
        p90: percentile_ms(h, 0.90),
        p99: percentile_ms(h, 0.99),
        max: percentile_ms(h, 1.0),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Percentiles {
    pub count: u64,
    pub p50: Option<f64>,
    pub p90: Option<f64>,
    pub p99: Option<f64>,
    pub max: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencySummary {
    pub ttft_ms: Percentiles,
    pub itl_ms: Percentiles,
    pub e2e_ms: Percentiles,
}

/// A histogram serialized as hex-encoded HdrHistogram V2-deflate bytes
/// (D-07). This is the standard binary form `hdrhistogram`'s own
/// `Deserializer` reads, and needs no base64 crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncodedHistograms {
    pub encoding: String,
    pub ttft_us: String,
    pub itl_us: String,
    pub e2e_us: String,
}

/// Encodes `h` as V2-deflate bytes, then lowercase hex (two chars/byte).
pub fn encode_histogram(h: &Histogram<u64>) -> anyhow::Result<String> {
    let mut buf = Vec::new();
    V2DeflateSerializer::new()
        .serialize(h, &mut buf)
        .map_err(|e| anyhow::anyhow!("serialize histogram: {e:?}"))?;
    Ok(to_hex(&buf))
}

/// Reverses [`encode_histogram`]: hex-decodes (erroring on odd length or a
/// non-hex digit — never panicking), then runs the V2/V2-deflate
/// `Deserializer`, which auto-detects the format from its leading cookie.
pub fn decode_histogram(hex: &str) -> anyhow::Result<Histogram<u64>> {
    let bytes = from_hex(hex)?;
    Deserializer::new()
        .deserialize(&mut bytes.as_slice())
        .map_err(|e| anyhow::anyhow!("deserialize histogram: {e:?}"))
}

fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        // `write!` to a `String` never fails.
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn from_hex(s: &str) -> anyhow::Result<Vec<u8>> {
    let bytes = s.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        anyhow::bail!("hex string has odd length ({})", bytes.len());
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    let mut i = 0;
    while i < bytes.len() {
        let hi = hex_val(bytes[i])?;
        let lo = hex_val(bytes[i + 1])?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Ok(out)
}

fn hex_val(b: u8) -> anyhow::Result<u8> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => anyhow::bail!("invalid hex digit: {:?}", b as char),
    }
}
