//! Known-distribution percentile fixture and the histogram encode/decode
//! round trip (D-07): raw per-trial histograms must be storable and
//! reloadable losslessly.

use hdrhistogram::Histogram;

use rsg_bench::metrics::{self, HIST_MAX_US};

fn thousand_ms_histogram() -> Histogram<u64> {
    let mut h = Histogram::<u64>::new_with_bounds(1, HIST_MAX_US, 3).expect("valid bounds");
    for ms in 1..=1000u64 {
        h.record(ms * 1000).expect("record within range");
    }
    h
}

#[test]
fn known_distribution_p99() {
    let h = thousand_ms_histogram();

    let p99 = metrics::percentile_ms(&h, 0.99).expect("non-empty histogram");
    assert!((989.0..=991.0).contains(&p99), "p99 = {p99}");

    let p50 = metrics::percentile_ms(&h, 0.50).expect("non-empty histogram");
    assert!((499.0..=501.0).contains(&p50), "p50 = {p50}");
}

#[test]
fn histogram_round_trip() {
    let h = thousand_ms_histogram();

    let encoded = metrics::encode_histogram(&h).expect("encode");
    let decoded = metrics::decode_histogram(&encoded).expect("decode");

    assert_eq!(decoded.len(), h.len());
    assert_eq!(decoded.value_at_quantile(0.99), h.value_at_quantile(0.99));
}

#[test]
fn decode_garbage_hex_never_panics() {
    assert!(metrics::decode_histogram("not-hex-at-all").is_err());
    assert!(metrics::decode_histogram("abc").is_err(), "odd-length hex should error");
    assert!(metrics::decode_histogram("").is_err(), "empty bytes have no cookie");
}
