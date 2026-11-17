//! Deterministic progression maths.
//!
//! Everything here is integer arithmetic. Floating-point `powf` can differ
//! between platforms, and the same world must come out bit-identical on the
//! server, in wasm and in tests. Fractions are held as parts per million
//! and results round down.
//!
//! ```
//! use eq_curve::{diminishing_returns, stepped_bonus, Staged};
//!
//! // XP = base × (1 − level/100)^1.5
//! assert_eq!(diminishing_returns(100, 0, 100), 100);
//! assert_eq!(diminishing_returns(100, 50, 100), 35);
//! assert_eq!(diminishing_returns(100, 100, 100), 0);
//!
//! // Attribute XP = base × (1 + tier/10)
//! assert_eq!(stepped_bonus(50, 3, 10), 65);
//!
//! // A value that changes in stages over time.
//! const BONUS: Staged<3> = Staged::new([(0, 0), (10, 100), (20, 100)]);
//! assert_eq!(BONUS.at(5), 50);
//! ```
#![no_std]

/// A non-negative fraction, held as parts per million.
///
/// Can exceed one, so it also serves as a multiplier such as 1.3×.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ratio(u64);

impl Ratio {
    /// How many parts make one whole.
    pub const SCALE: u64 = 1_000_000;

    /// Zero.
    pub const ZERO: Self = Self(0);

    /// One whole.
    pub const ONE: Self = Self(Self::SCALE);

    /// `numerator / denominator`, rounded down to the nearest millionth.
    ///
    /// # Panics
    /// Panics if `denominator` is zero.
    pub const fn new(numerator: u64, denominator: u64) -> Self {
        assert!(denominator > 0, "denominator must be greater than zero");
        let parts = numerator as u128 * Self::SCALE as u128 / denominator as u128;
        Self(saturate(parts))
    }

    /// A ratio from a raw count of millionths.
    pub const fn from_parts_per_million(parts: u64) -> Self {
        Self(parts)
    }

    /// The ratio as a count of millionths.
    pub const fn parts_per_million(self) -> u64 {
        self.0
    }

    /// `1 − self`, or zero if `self` is one or more.
    pub const fn complement(self) -> Self {
        Self(Self::SCALE.saturating_sub(self.0))
    }

    /// `self` raised to the power 1.5, computed as `self × √self`.
    pub const fn pow_three_halves(self) -> Self {
        let scale = Self::SCALE as u128;
        let root = (self.0 as u128 * scale).isqrt();
        Self(saturate(self.0 as u128 * root / scale))
    }

    /// `amount × self`, rounded down.
    pub const fn apply(self, amount: u64) -> u64 {
        saturate(amount as u128 * self.0 as u128 / Self::SCALE as u128)
    }
}

const fn saturate(value: u128) -> u64 {
    if value > u64::MAX as u128 {
        u64::MAX
    } else {
        value as u64
    }
}

/// Gain that shrinks as a level nears its cap:
/// `base × (1 − level/cap)^1.5`, rounded down.
///
/// Returns zero at or beyond the cap.
///
/// # Panics
/// Panics if `cap` is zero.
pub const fn diminishing_returns(base: u64, level: u64, cap: u64) -> u64 {
    Ratio::new(level, cap).complement().pow_three_halves().apply(base)
}

/// Gain that grows by a fixed fraction per step:
/// `base × (1 + steps/steps_per_whole)`, rounded down.
///
/// # Panics
/// Panics if `steps_per_whole` is zero.
pub const fn stepped_bonus(base: u64, steps: u64, steps_per_whole: u64) -> u64 {
    assert!(steps_per_whole > 0, "steps_per_whole must be greater than zero");
    let whole = steps_per_whole as u128;
    saturate(base as u128 * (whole + steps as u128) / whole)
}

/// A value that changes in straight-line stages.
///
/// Built from `(x, y)` points. Between two points the value moves evenly
/// from one `y` to the next; before the first point and after the last it
/// holds steady. `x` is usually elapsed ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Staged<const N: usize> {
    points: [(u64, i64); N],
}

impl<const N: usize> Staged<N> {
    /// Builds a staged curve. Declare it as a `const` and a bad table is a
    /// compile error.
    ///
    /// # Panics
    /// Panics if there are no points, or if the `x` values are not in
    /// strictly increasing order.
    pub const fn new(points: [(u64, i64); N]) -> Self {
        assert!(N > 0, "a staged curve needs at least one point");
        let mut i = 1;
        while i < N {
            assert!(points[i - 1].0 < points[i].0, "x values must strictly increase");
            i += 1;
        }
        Self { points }
    }

    /// The value at `x`, rounded down.
    pub fn at(&self, x: u64) -> i64 {
        let after = self.points.partition_point(|&(px, _)| px <= x);
        if after == 0 {
            return self.points[0].1;
        }
        let (x0, y0) = self.points[after - 1];
        let Some(&(x1, y1)) = self.points.get(after) else {
            return y0;
        };
        let rise = i128::from(y1) - i128::from(y0);
        let run = i128::from(x1 - x0);
        let progress = i128::from(x - x0);
        let value = i128::from(y0) + (rise * progress).div_euclid(run);
        // Always between y0 and y1, so it fits.
        value as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratio_basics() {
        assert_eq!(Ratio::new(1, 4).parts_per_million(), 250_000);
        assert_eq!(Ratio::new(3, 3), Ratio::ONE);
        assert_eq!(Ratio::new(1, 4).complement(), Ratio::new(3, 4));
        assert_eq!(Ratio::new(5, 4).complement(), Ratio::ZERO);
        assert_eq!(Ratio::new(13, 10).apply(50), 65);
    }

    #[test]
    fn pow_three_halves_hits_exact_squares() {
        assert_eq!(Ratio::ZERO.pow_three_halves(), Ratio::ZERO);
        assert_eq!(Ratio::ONE.pow_three_halves(), Ratio::ONE);
        // 0.25^1.5 = 0.125, 4^1.5 = 8
        assert_eq!(Ratio::new(1, 4).pow_three_halves(), Ratio::new(1, 8));
        assert_eq!(Ratio::new(4, 1).pow_three_halves(), Ratio::new(8, 1));
    }

    #[test]
    fn diminishing_returns_matches_the_formula() {
        // base 1000 × (1 − level/100)^1.5, rounded down
        for (level, expected) in [(0, 1000), (10, 853), (25, 649), (50, 353), (75, 125), (99, 1), (100, 0)] {
            assert_eq!(diminishing_returns(1000, level, 100), expected, "level {level}");
        }
    }

    #[test]
    fn diminishing_returns_never_rises_with_level() {
        let mut previous = u64::MAX;
        for level in 0..=120 {
            let gain = diminishing_returns(10_000, level, 100);
            assert!(gain <= previous, "rose at level {level}");
            previous = gain;
        }
        assert_eq!(previous, 0);
    }

    #[test]
    fn stepped_bonus_matches_the_craft_formula() {
        assert_eq!(stepped_bonus(100, 0, 10), 100);
        assert_eq!(stepped_bonus(100, 5, 10), 150);
        assert_eq!(stepped_bonus(7, 1, 10), 7); // 7.7 rounds down
    }

    #[test]
    fn extremes_saturate_instead_of_overflowing() {
        assert_eq!(stepped_bonus(u64::MAX, 10, 10), u64::MAX);
        assert_eq!(Ratio::new(u64::MAX, 1).apply(u64::MAX), u64::MAX);
        assert_eq!(Ratio::new(u64::MAX, 1).pow_three_halves().parts_per_million(), u64::MAX);
    }

    #[test]
    fn staged_holds_steady_outside_its_points() {
        let curve = Staged::new([(10, 5), (20, 15)]);
        assert_eq!(curve.at(0), 5);
        assert_eq!(curve.at(10), 5);
        assert_eq!(curve.at(20), 15);
        assert_eq!(curve.at(u64::MAX), 15);
    }

    #[test]
    fn staged_moves_evenly_between_points() {
        let curve = Staged::new([(0, 0), (10, 100), (20, 100), (30, -50)]);
        assert_eq!(curve.at(5), 50);
        assert_eq!(curve.at(15), 100);
        assert_eq!(curve.at(25), 25);
        assert_eq!(curve.at(30), -50);
    }

    #[test]
    fn staged_rounds_down_when_falling_too() {
        let curve = Staged::new([(0, 0), (3, -1)]);
        assert_eq!(curve.at(1), -1);
        let curve = Staged::new([(0, 0), (3, 1)]);
        assert_eq!(curve.at(2), 0);
    }

    #[test]
    fn staged_survives_the_full_number_range() {
        let curve = Staged::new([(0, i64::MIN), (u64::MAX, i64::MAX)]);
        assert_eq!(curve.at(0), i64::MIN);
        assert_eq!(curve.at(u64::MAX), i64::MAX);
        assert!(curve.at(u64::MAX / 2).abs() <= 1);
    }

    #[test]
    fn single_point_is_constant() {
        let curve = Staged::new([(100, 7)]);
        assert_eq!((curve.at(0), curve.at(100), curve.at(500)), (7, 7, 7));
    }

    #[test]
    #[should_panic(expected = "x values must strictly increase")]
    fn unordered_points_panic() {
        let _ = Staged::new([(5, 0), (5, 1)]);
    }
}
