//! A bounded value with named bands.
//!
//! A [`Meter`] is a number that can never leave its range: zone vitality,
//! corruption, infamy, a rested bonus. [`Bands`] give names to stretches of
//! that range, and report when a change moves the value from one band to
//! another, which is when the rest of the world needs to hear about it.
//!
//! ```
//! use eq_meter::{Bands, Meter};
//!
//! #[derive(Clone, Copy, Debug, PartialEq)]
//! enum Health { Collapsed, Balanced, Peak }
//!
//! const VITALITY: Bands<Health, 2> =
//!     Bands::new(Health::Collapsed, [(31, Health::Balanced), (81, Health::Peak)]);
//!
//! let mut vitality = Meter::full(0, 100);
//! assert_eq!(VITALITY.band_of(vitality.value()), Health::Peak);
//!
//! let change = vitality.adjust(-25);
//! assert_eq!(VITALITY.crossing(change), Some((Health::Peak, Health::Balanced)));
//! ```
#![no_std]

/// A value held between a minimum and a maximum, both included.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Meter {
    min: i64,
    max: i64,
    value: i64,
}

impl Meter {
    /// A meter over `min..=max`, starting at `value` clamped into range.
    ///
    /// # Panics
    /// Panics if `min > max`.
    pub const fn new(min: i64, max: i64, value: i64) -> Self {
        assert!(min <= max, "min must not exceed max");
        Self { min, max, value: clamp(value, min, max) }
    }

    /// A meter starting at its maximum.
    pub const fn full(min: i64, max: i64) -> Self {
        Self::new(min, max, max)
    }

    /// A meter starting at its minimum.
    pub const fn empty(min: i64, max: i64) -> Self {
        Self::new(min, max, min)
    }

    /// The current value.
    pub const fn value(&self) -> i64 {
        self.value
    }

    /// The lowest value the meter can hold.
    pub const fn min(&self) -> i64 {
        self.min
    }

    /// The highest value the meter can hold.
    pub const fn max(&self) -> i64 {
        self.max
    }

    /// Whether the meter is at its minimum.
    pub const fn is_empty(&self) -> bool {
        self.value == self.min
    }

    /// Whether the meter is at its maximum.
    pub const fn is_full(&self) -> bool {
        self.value == self.max
    }

    /// Moves the value by `delta`, stopping at either end.
    pub fn adjust(&mut self, delta: i64) -> Change {
        self.set(self.value.saturating_add(delta))
    }

    /// Sets the value, clamped into range.
    pub fn set(&mut self, value: i64) -> Change {
        let before = self.value;
        self.value = clamp(value, self.min, self.max);
        Change { before, after: self.value }
    }
}

const fn clamp(value: i64, min: i64, max: i64) -> i64 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// What a meter held before and after an update.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    /// The value before the update.
    pub before: i64,
    /// The value after the update.
    pub after: i64,
}

impl Change {
    /// How far the value actually moved, after clamping.
    pub const fn delta(self) -> i64 {
        self.after - self.before
    }

    /// Whether the update left the value where it was.
    pub const fn is_unchanged(self) -> bool {
        self.before == self.after
    }
}

/// Names for stretches of a meter's range.
///
/// Each step is `(threshold, band)`: the band applies from that threshold
/// upward, until the next step. Values below the first threshold fall in
/// the `lowest` band.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bands<B, const N: usize> {
    lowest: B,
    steps: [(i64, B); N],
}

impl<B: Copy, const N: usize> Bands<B, N> {
    /// Builds a set of bands. Declare it as a `const` and a bad table is a
    /// compile error.
    ///
    /// # Panics
    /// Panics if the thresholds are not in strictly increasing order.
    pub const fn new(lowest: B, steps: [(i64, B); N]) -> Self {
        let mut i = 1;
        while i < N {
            assert!(steps[i - 1].0 < steps[i].0, "thresholds must strictly increase");
            i += 1;
        }
        Self { lowest, steps }
    }

    /// The band that `value` falls in.
    pub fn band_of(&self, value: i64) -> B {
        self.steps
            .iter()
            .rev()
            .find(|(threshold, _)| value >= *threshold)
            .map_or(self.lowest, |&(_, band)| band)
    }

    /// The `(from, to)` bands if `change` moved the value across a
    /// boundary, or `None` if it stayed within one band.
    pub fn crossing(&self, change: Change) -> Option<(B, B)>
    where
        B: PartialEq,
    {
        let from = self.band_of(change.before);
        let to = self.band_of(change.after);
        (from != to).then_some((from, to))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Health {
        Collapsed,
        Balanced,
        Peak,
    }

    const VITALITY: Bands<Health, 2> =
        Bands::new(Health::Collapsed, [(31, Health::Balanced), (81, Health::Peak)]);

    #[test]
    fn new_clamps_starting_value() {
        assert_eq!(Meter::new(0, 100, 250).value(), 100);
        assert_eq!(Meter::new(0, 100, -5).value(), 0);
    }

    #[test]
    fn full_and_empty_start_at_the_ends() {
        assert!(Meter::full(0, 100).is_full());
        assert!(Meter::empty(0, 100).is_empty());
    }

    #[test]
    fn adjust_stops_at_both_ends() {
        let mut meter = Meter::new(0, 100, 90);
        let up = meter.adjust(50);
        assert_eq!((up.before, up.after, up.delta()), (90, 100, 10));
        let down = meter.adjust(-500);
        assert_eq!(down.after, 0);
        assert!(meter.adjust(-1).is_unchanged());
    }

    #[test]
    fn adjust_survives_extreme_deltas() {
        let mut meter = Meter::new(i64::MIN, i64::MAX, i64::MAX - 1);
        assert_eq!(meter.adjust(i64::MAX).after, i64::MAX);
    }

    #[test]
    fn negative_ranges_work() {
        let mut meter = Meter::new(-50, 50, 0);
        assert_eq!(meter.adjust(-80).after, -50);
    }

    #[test]
    fn band_edges_match_the_design_doc() {
        for (value, band) in [
            (0, Health::Collapsed),
            (30, Health::Collapsed),
            (31, Health::Balanced),
            (80, Health::Balanced),
            (81, Health::Peak),
            (100, Health::Peak),
        ] {
            assert_eq!(VITALITY.band_of(value), band, "value {value}");
        }
    }

    #[test]
    fn crossing_reports_only_band_changes() {
        let mut vitality = Meter::full(0, 100);
        assert_eq!(VITALITY.crossing(vitality.adjust(-10)), None);
        assert_eq!(
            VITALITY.crossing(vitality.adjust(-10)),
            Some((Health::Peak, Health::Balanced))
        );
        assert_eq!(
            VITALITY.crossing(vitality.adjust(-80)),
            Some((Health::Balanced, Health::Collapsed))
        );
        assert_eq!(
            VITALITY.crossing(vitality.set(100)),
            Some((Health::Collapsed, Health::Peak))
        );
    }

    #[test]
    fn bands_with_no_steps_have_one_band() {
        let single: Bands<Health, 0> = Bands::new(Health::Balanced, []);
        assert_eq!(single.band_of(i64::MIN), Health::Balanced);
    }

    #[test]
    #[should_panic(expected = "thresholds must strictly increase")]
    fn unordered_thresholds_panic() {
        let _ = Bands::new(Health::Collapsed, [(81, Health::Peak), (31, Health::Balanced)]);
    }

    #[test]
    #[should_panic(expected = "min must not exceed max")]
    fn inverted_range_panics() {
        let _ = Meter::new(10, 0, 5);
    }
}
