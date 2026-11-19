//! A seeded, splittable random source.
//!
//! Integer-only (no floats), so results are bit-identical on every
//! platform, including wasm. The generator is plain data: keep it in world
//! state and a replay from the same seed makes the same rolls.
//!
//! ```
//! use eq_rng::Rng;
//!
//! let mut a = Rng::from_seed(1);
//! let mut b = Rng::from_seed(1);
//! assert_eq!(a.below(100), b.below(100));
//! ```
#![no_std]

const GOLDEN_GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

/// SplitMix64 finaliser: scrambles a counter into a well-mixed value.
const fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A deterministic random source (SplitMix64).
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// Creates a generator from a seed. Same seed, same sequence.
    pub const fn from_seed(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Returns the next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(GOLDEN_GAMMA);
        mix(self.state)
    }

    /// Returns a uniform value in `0..bound`, without modulo bias.
    ///
    /// # Panics
    /// Panics if `bound` is zero.
    pub fn below(&mut self, bound: u64) -> u64 {
        assert!(bound > 0, "bound must be greater than zero");
        // Reject the few low values that would make some results likelier.
        let threshold = bound.wrapping_neg() % bound;
        loop {
            let value = self.next_u64();
            if value >= threshold {
                return value % bound;
            }
        }
    }

    /// Returns a uniform value in `low..=high`.
    ///
    /// # Panics
    /// Panics if `low > high`.
    pub fn between(&mut self, low: u64, high: u64) -> u64 {
        assert!(low <= high, "low must not exceed high");
        match (high - low).checked_add(1) {
            Some(span) => low + self.below(span),
            None => self.next_u64(),
        }
    }

    /// Returns `true` with probability `numerator / denominator`.
    ///
    /// # Panics
    /// Panics if `denominator` is zero.
    pub fn chance(&mut self, numerator: u64, denominator: u64) -> bool {
        self.below(denominator) < numerator
    }

    /// Derives an independent generator for a labelled stream.
    ///
    /// Does not advance `self`, so adding a new stream never shifts the
    /// rolls of existing ones. Use a distinct label per purpose (a place
    /// id, a system number).
    pub fn split(&self, label: u64) -> Self {
        Self::from_seed(mix(self.state ^ mix(label.wrapping_add(GOLDEN_GAMMA))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_splitmix64_reference_values() {
        let mut rng = Rng::from_seed(0);
        assert_eq!(rng.next_u64(), 0xE220_A839_7B1D_CDAF);
        assert_eq!(rng.next_u64(), 0x6E78_9E6A_A1B9_65F4);
        assert_eq!(rng.next_u64(), 0x06C4_5D18_8009_454F);
    }

    #[test]
    fn same_seed_gives_same_sequence() {
        let mut a = Rng::from_seed(99);
        let mut b = Rng::from_seed(99);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn below_stays_in_range_and_reaches_every_value() {
        let mut rng = Rng::from_seed(5);
        let mut seen = [false; 6];
        for _ in 0..1000 {
            seen[rng.below(6) as usize] = true;
        }
        assert!(seen.iter().all(|&hit| hit));
    }

    #[test]
    fn between_includes_both_ends() {
        let mut rng = Rng::from_seed(5);
        let mut seen = [false; 3];
        for _ in 0..200 {
            seen[(rng.between(10, 12) - 10) as usize] = true;
        }
        assert!(seen.iter().all(|&hit| hit));
        assert_eq!(rng.between(7, 7), 7);
        rng.between(0, u64::MAX);
    }

    #[test]
    fn chance_handles_certain_and_impossible() {
        let mut rng = Rng::from_seed(1);
        for _ in 0..100 {
            assert!(rng.chance(1, 1));
            assert!(!rng.chance(0, 1));
        }
    }

    #[test]
    fn chance_is_roughly_fair() {
        let mut rng = Rng::from_seed(3);
        let hits = (0..10_000).filter(|_| rng.chance(1, 4)).count();
        assert!((2300..2700).contains(&hits), "hits = {hits}");
    }

    #[test]
    fn split_leaves_parent_untouched() {
        let parent = Rng::from_seed(8);
        let before = parent.clone();
        let _ = parent.split(1);
        assert_eq!(parent, before);
    }

    #[test]
    fn split_streams_differ_by_label_and_from_parent() {
        let mut parent = Rng::from_seed(8);
        let mut one = parent.split(1);
        let mut two = parent.split(2);
        let (a, b, c) = (one.next_u64(), two.next_u64(), parent.next_u64());
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_ne!(b, c);
    }

    #[test]
    #[should_panic(expected = "bound must be greater than zero")]
    fn below_zero_panics() {
        Rng::from_seed(0).below(0);
    }
}
