//! Hand-written SplitMix64 (Vigna's reference algorithm), not the `rand`
//! crate (D-07): its output is a fixed published algorithm, so a seed
//! recorded in a run manifest regenerates the same workload regardless of
//! crate versions, and `rand` is not in the RESEARCH package audit.

/// A SplitMix64 PRNG stream. Cheap to construct and to `fork` into
/// decorrelated child streams (one per agent/worker), per Vigna's reference
/// `splitmix64.c`.
#[derive(Debug, Clone, Copy)]
pub struct SplitMix64 {
    state: u64,
}

const GOLDEN_GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;
const MIX1: u64 = 0xBF58_476D_1CE4_E5B9;
const MIX2: u64 = 0x94D0_49BB_1331_11EB;

impl SplitMix64 {
    /// Builds a fresh stream seeded with `seed`. The first call to
    /// `next_u64` advances past this seed, so `new(seed)` itself never
    /// leaks the raw seed as output.
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Vigna's reference step: advance the state by the golden-ratio
    /// increment, then mix with two fixed 64-bit multipliers and three
    /// xorshifts (30/27/31 bits).
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(GOLDEN_GAMMA);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(MIX1);
        z = (z ^ (z >> 27)).wrapping_mul(MIX2);
        z ^ (z >> 31)
    }

    /// A uniform `f64` in `[0, 1)`, built from the top 53 bits of
    /// `next_u64` (an `f64`'s mantissa width).
    pub fn next_f64(&mut self) -> f64 {
        let top53 = self.next_u64() >> 11;
        top53 as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// A uniform `f64` in `[lo, hi)`.
    pub fn uniform_f64(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next_f64()
    }

    /// A uniform `u32` in `[lo, hi]` (inclusive on both ends).
    pub fn range_inclusive_u32(&mut self, lo: u32, hi: u32) -> u32 {
        debug_assert!(lo <= hi, "range_inclusive_u32: lo {lo} > hi {hi}");
        let span = u64::from(hi - lo) + 1;
        lo + (self.next_u64() % span) as u32
    }

    /// An exponential-distributed `f64` (seconds, if `rate` is per-second),
    /// via inverse-CDF sampling: `-ln(1 - u) / rate`.
    pub fn exp(&mut self, rate: f64) -> f64 {
        let u = self.next_f64();
        -(1.0 - u).ln() / rate
    }

    /// Forks a decorrelated child stream for `stream` (e.g. an agent or
    /// worker index). One SplitMix64 step is run over
    /// `self.state ^ stream * GOLDEN_GAMMA` to produce the child's seed, so
    /// sibling streams (same base seed, different `stream`) never alias.
    pub fn fork(&self, stream: u64) -> SplitMix64 {
        let mixed = self.state ^ stream.wrapping_mul(GOLDEN_GAMMA);
        let mut tmp = SplitMix64::new(mixed);
        let child_seed = tmp.next_u64();
        SplitMix64::new(child_seed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vigna's reference vector for seed 1234567 (also used by many
    /// independent SplitMix64 implementations as a cross-check).
    #[test]
    fn reference_vector_seed_1234567() {
        let mut rng = SplitMix64::new(1234567);
        assert_eq!(rng.next_u64(), 6457827717110365317);
        assert_eq!(rng.next_u64(), 3203168211198807973);
        assert_eq!(rng.next_u64(), 9817491932198370423);
    }

    #[test]
    fn next_f64_stays_in_unit_interval() {
        let mut rng = SplitMix64::new(42);
        for _ in 0..10_000 {
            let x = rng.next_f64();
            assert!((0.0..1.0).contains(&x), "next_f64 out of range: {x}");
        }
    }

    #[test]
    fn fork_streams_are_decorrelated() {
        let base = SplitMix64::new(42);
        let mut a = base.fork(0);
        let mut b = base.fork(1);
        assert_ne!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn exp_is_never_negative() {
        let mut rng = SplitMix64::new(7);
        for _ in 0..10_000 {
            let x = rng.exp(50.0);
            assert!(x >= 0.0, "exp produced a negative value: {x}");
        }
    }
}
