//! Run-to-run confidence-interval statistics (D-05, BENCH-07).
//!
//! CIs here come from N repeated alternating runs per arm — e.g. N
//! `run_s2`/`run_closed` trials for the Python arm, interleaved with N
//! trials for the Rust arm — not from bootstrapping within a single run's
//! per-request latencies. A single run (`n < 2`) therefore yields no
//! interval; the report states why rather than inventing one.

use serde::Serialize;

fn mean(xs: &[f64]) -> f64 {
    xs.iter().sum::<f64>() / xs.len() as f64
}

/// Sample variance (divides by `n - 1`). Callers must ensure `xs.len() >= 2`.
fn sample_variance(xs: &[f64], m: f64) -> f64 {
    let sum_sq: f64 = xs.iter().map(|x| (x - m) * (x - m)).sum();
    sum_sq / (xs.len() as f64 - 1.0)
}

/// Two-sided 97.5th-percentile Student-t critical value (for a 95% CI), for
/// degrees of freedom `df`. Exact lookup for `df` in `1..=30`; stepped
/// approximations above that, converging to the normal value 1.960.
const T975_TABLE_1_TO_30: [f64; 30] = [
    12.706, 4.303, 3.182, 2.776, 2.571, 2.447, 2.365, 2.306, 2.262, 2.228, 2.201, 2.179, 2.160,
    2.145, 2.131, 2.120, 2.110, 2.101, 2.093, 2.086, 2.080, 2.074, 2.069, 2.064, 2.060, 2.056,
    2.052, 2.048, 2.045, 2.042,
];

/// `t975(df)`: the two-sided 97.5th-percentile Student-t value for `df`
/// degrees of freedom. `df == 0` is not a meaningful call site (every
/// caller guards `n >= 2`, so `df >= 1`); it falls back to the `df == 1`
/// value rather than panicking.
pub fn t975(df: usize) -> f64 {
    match df {
        0 => T975_TABLE_1_TO_30[0],
        1..=30 => T975_TABLE_1_TO_30[df - 1],
        31..=40 => 2.021,
        41..=60 => 2.000,
        61..=120 => 1.980,
        _ => 1.960,
    }
}

/// A two-sided 95% confidence interval for a sample mean (Student-t).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Ci95 {
    pub mean: f64,
    pub half_width: f64,
    pub lo: f64,
    pub hi: f64,
    pub n: usize,
}

/// `mean ± t975(n-1) * sd/sqrt(n)`. `None` for `n < 2` (BENCH-07's empty
/// edge: never a fabricated interval from a single run).
pub fn mean_ci95(xs: &[f64]) -> Option<Ci95> {
    let n = xs.len();
    if n < 2 {
        return None;
    }
    let m = mean(xs);
    let sd = sample_variance(xs, m).sqrt();
    let se = sd / (n as f64).sqrt();
    let half_width = t975(n - 1) * se;
    Some(Ci95 {
        mean: m,
        half_width,
        lo: m - half_width,
        hi: m + half_width,
        n,
    })
}

/// A two-sided 95% confidence interval for the Welch (unequal-variance)
/// difference of two means, `mean(b) - mean(a)`.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct WelchCi95 {
    pub diff: f64,
    pub half_width: f64,
    pub lo: f64,
    pub hi: f64,
    pub df: f64,
}

/// Welch's t-test interval for `mean(b) - mean(a)`, with
/// Welch-Satterthwaite degrees of freedom (floored for the `t975` lookup,
/// which is conservative). `None` if either side has `n < 2`. Never `NaN`:
/// when both sides have zero sample variance, `df` falls back to
/// `na + nb - 2` and `half_width` is `0.0` rather than `0.0 / 0.0`.
pub fn welch_diff_ci95(a: &[f64], b: &[f64]) -> Option<WelchCi95> {
    let na = a.len();
    let nb = b.len();
    if na < 2 || nb < 2 {
        return None;
    }

    let ma = mean(a);
    let mb = mean(b);
    let var_a = sample_variance(a, ma);
    let var_b = sample_variance(b, mb);
    let se2_a = var_a / na as f64;
    let se2_b = var_b / nb as f64;
    let se_combined = (se2_a + se2_b).sqrt();
    let diff = mb - ma;

    if se_combined == 0.0 {
        let df = (na + nb - 2) as f64;
        return Some(WelchCi95 {
            diff,
            half_width: 0.0,
            lo: diff,
            hi: diff,
            df,
        });
    }

    let numerator = (se2_a + se2_b) * (se2_a + se2_b);
    let denom = (se2_a * se2_a) / (na as f64 - 1.0) + (se2_b * se2_b) / (nb as f64 - 1.0);
    let df = numerator / denom;
    let t = t975((df.floor() as usize).max(1));
    let half_width = t * se_combined;

    Some(WelchCi95 {
        diff,
        half_width,
        lo: diff - half_width,
        hi: diff + half_width,
        df,
    })
}

/// A percent-delta confidence interval, derived from [`welch_diff_ci95`] by
/// scaling by `100 / mean(a)` and treating that denominator as fixed (a
/// documented approximation — the proper ratio-distribution CI is not
/// normal, but this is a reasonable report-friendly band for BENCH-07).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct PctDelta {
    pub pct: f64,
    pub lo_pct: f64,
    pub hi_pct: f64,
}

/// `(mean(b) - mean(a)) / mean(a) * 100`, with a CI band scaled from the
/// Welch CI. `None` when either side has `n < 2`, or when `mean(a) == 0`
/// (division by zero).
pub fn pct_delta(a: &[f64], b: &[f64]) -> Option<PctDelta> {
    let welch = welch_diff_ci95(a, b)?;
    let ma = mean(a);
    if ma == 0.0 {
        return None;
    }
    let scale = 100.0 / ma;
    Some(PctDelta {
        pct: welch.diff * scale,
        lo_pct: welch.lo * scale,
        hi_pct: welch.hi * scale,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64, tol: f64) {
        assert!((a - b).abs() <= tol, "{a} not within {tol} of {b}");
    }

    #[test]
    fn mean_ci95_known_example() {
        let ci = mean_ci95(&[1.0, 2.0, 3.0, 4.0, 5.0]).expect("n=5 >= 2");
        approx(ci.mean, 3.0, 1e-9);
        approx(ci.half_width, 1.963, 0.001);
        assert_eq!(ci.n, 5);
    }

    #[test]
    fn mean_ci95_empty_edge() {
        assert!(mean_ci95(&[4.0]).is_none());
        assert!(mean_ci95(&[]).is_none());
    }

    #[test]
    fn welch_diff_ci95_known_example() {
        let w = welch_diff_ci95(&[10.0, 12.0, 14.0], &[20.0, 22.0, 24.0, 26.0])
            .expect("n>=2 both sides");
        approx(w.diff, 11.0, 1e-9);
        approx(w.df, 4.959, 0.001);
        approx(w.half_width, 4.808, 0.001);
    }

    #[test]
    fn welch_diff_ci95_empty_edge() {
        assert!(welch_diff_ci95(&[1.0], &[1.0, 2.0]).is_none());
        assert!(welch_diff_ci95(&[1.0, 2.0], &[1.0]).is_none());
    }

    #[test]
    fn welch_diff_ci95_zero_variance_never_nan() {
        let w = welch_diff_ci95(&[5.0, 5.0], &[7.0, 7.0]).expect("n=2 both sides");
        approx(w.diff, 2.0, 1e-9);
        approx(w.half_width, 0.0, 1e-9);
        approx(w.df, 2.0, 1e-9);
        assert!(!w.diff.is_nan() && !w.half_width.is_nan() && !w.df.is_nan());
    }

    #[test]
    fn pct_delta_known_example() {
        let d = pct_delta(&[100.0, 102.0, 98.0], &[97.0, 99.0, 98.0]).expect("valid inputs");
        approx(d.pct, -2.0, 1e-9);
    }

    #[test]
    fn pct_delta_zero_mean_a_is_none() {
        assert!(pct_delta(&[0.0, 0.0], &[1.0, 2.0]).is_none());
    }

    #[test]
    fn t975_reference_values() {
        approx(t975(1), 12.706, 1e-9);
        approx(t975(4), 2.776, 1e-9);
        approx(t975(30), 2.042, 1e-9);
        approx(t975(31), 2.021, 1e-9);
        approx(t975(40), 2.021, 1e-9);
        approx(t975(41), 2.000, 1e-9);
        approx(t975(60), 2.000, 1e-9);
        approx(t975(61), 1.980, 1e-9);
        approx(t975(120), 1.980, 1e-9);
        approx(t975(121), 1.960, 1e-9);
    }
}
