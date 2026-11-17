//! Game time: ticks, durations, repeating cycles and pausable stopwatches.
//!
//! Nothing here reads a wall clock. Time only moves when the caller says
//! which tick it is, so a server can run ticks in real time while tests
//! run them as fast as they like.
//!
//! ```
//! use eq_clock::{Cycle, Duration, Tick};
//!
//! // A 20-minute tide: caves are open for the first half.
//! let tide = Cycle::new(Duration::minutes(20));
//! assert!(tide.is_within(Tick::new(0), Duration::minutes(10)));
//! assert!(!tide.is_within(Tick::new(700), Duration::minutes(10)));
//! ```
#![no_std]

use core::ops::{Add, Sub};

/// A span of time, in ticks. One tick is one second of world time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Duration(u64);

impl Duration {
    /// No time at all.
    pub const ZERO: Self = Self(0);

    /// A span of `ticks` ticks.
    pub const fn ticks(ticks: u64) -> Self {
        Self(ticks)
    }

    /// A span of `seconds` seconds of world time.
    pub const fn seconds(seconds: u64) -> Self {
        Self(seconds)
    }

    /// A span of `minutes` minutes of world time.
    pub const fn minutes(minutes: u64) -> Self {
        Self(minutes * 60)
    }

    /// A span of `hours` hours of world time.
    pub const fn hours(hours: u64) -> Self {
        Self(hours * 3_600)
    }

    /// A span of `days` days of world time.
    pub const fn days(days: u64) -> Self {
        Self(days * 86_400)
    }

    /// The span as a number of ticks.
    pub const fn as_ticks(self) -> u64 {
        self.0
    }

    /// What is left of this span after `spent`, or nothing if more than
    /// all of it has been spent.
    pub const fn saturating_sub(self, spent: Self) -> Self {
        Self(self.0.saturating_sub(spent.0))
    }
}

impl Add for Duration {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

/// A moment in world time, counted in ticks since the world began.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tick(u64);

impl Tick {
    /// The first tick of a world.
    pub const ZERO: Self = Self(0);

    /// The moment `ticks` ticks after the world began.
    pub const fn new(ticks: u64) -> Self {
        Self(ticks)
    }

    /// Ticks since the world began.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// The moment one tick later.
    pub const fn next(self) -> Self {
        Self(self.0 + 1)
    }

    /// Time elapsed since `earlier`, or zero if `earlier` is in the future.
    pub const fn since(self, earlier: Self) -> Duration {
        Duration(self.0.saturating_sub(earlier.0))
    }
}

impl Add<Duration> for Tick {
    type Output = Self;

    fn add(self, span: Duration) -> Self {
        Self(self.0 + span.0)
    }
}

impl Sub for Tick {
    type Output = Duration;

    /// Same as [`Tick::since`]: never negative.
    fn sub(self, earlier: Self) -> Duration {
        self.since(earlier)
    }
}

/// Something that repeats on a fixed period, such as a tide.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cycle {
    period: Duration,
    offset: Duration,
}

impl Cycle {
    /// A cycle that starts a fresh period at tick zero.
    ///
    /// # Panics
    /// Panics if `period` is zero.
    pub const fn new(period: Duration) -> Self {
        assert!(period.0 > 0, "period must be greater than zero");
        Self { period, offset: Duration::ZERO }
    }

    /// The same cycle, already `offset` into its period at tick zero.
    pub const fn with_offset(self, offset: Duration) -> Self {
        Self { period: self.period, offset: Duration(offset.0 % self.period.0) }
    }

    /// The length of one full turn.
    pub const fn period(self) -> Duration {
        self.period
    }

    /// How far into the current turn `tick` is: `0..period`.
    pub const fn phase_at(self, tick: Tick) -> Duration {
        let period = self.period.0;
        Duration((tick.0 % period + self.offset.0) % period)
    }

    /// How many full turns have completed by `tick`.
    pub const fn turns_at(self, tick: Tick) -> u64 {
        let period = self.period.0;
        tick.0 / period + (tick.0 % period + self.offset.0) / period
    }

    /// Whether `tick` falls in the first `window` of the current turn.
    pub const fn is_within(self, tick: Tick, window: Duration) -> bool {
        self.phase_at(tick).0 < window.0
    }
}

/// Measures elapsed time and can be paused.
///
/// Used for clocks that only count some of the time, such as a timer that
/// runs only while its owner is playing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stopwatch {
    banked: Duration,
    running_since: Option<Tick>,
}

impl Stopwatch {
    /// A stopped stopwatch reading zero.
    pub const fn new() -> Self {
        Self { banked: Duration::ZERO, running_since: None }
    }

    /// A stopwatch reading zero, running from `now`.
    pub const fn started(now: Tick) -> Self {
        Self { banked: Duration::ZERO, running_since: Some(now) }
    }

    /// Starts or resumes counting. Does nothing if already running.
    pub fn resume(&mut self, now: Tick) {
        if self.running_since.is_none() {
            self.running_since = Some(now);
        }
    }

    /// Stops counting and keeps the time so far. Does nothing if paused.
    pub fn pause(&mut self, now: Tick) {
        self.banked = self.elapsed(now);
        self.running_since = None;
    }

    /// Whether the stopwatch is counting.
    pub const fn is_running(&self) -> bool {
        self.running_since.is_some()
    }

    /// Total time counted as of `now`.
    pub fn elapsed(&self, now: Tick) -> Duration {
        match self.running_since {
            Some(start) => self.banked + now.since(start),
            None => self.banked,
        }
    }

    /// Whether at least `limit` has been counted as of `now`.
    pub fn has_reached(&self, now: Tick, limit: Duration) -> bool {
        self.elapsed(now) >= limit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_left_of_a_span_never_goes_below_nothing() {
        assert_eq!(Duration::hours(10).saturating_sub(Duration::hours(4)), Duration::hours(6));
        assert_eq!(Duration::hours(1).saturating_sub(Duration::hours(4)), Duration::ZERO);
    }

    #[test]
    fn duration_units_convert_to_ticks() {
        assert_eq!(Duration::minutes(20).as_ticks(), 1_200);
        assert_eq!(Duration::hours(10).as_ticks(), 36_000);
        assert_eq!(Duration::days(14).as_ticks(), 1_209_600);
    }

    #[test]
    fn tick_arithmetic() {
        let start = Tick::new(100);
        let later = start + Duration::seconds(15);
        assert_eq!(later.get(), 115);
        assert_eq!(later - start, Duration::ticks(15));
        assert_eq!(start - later, Duration::ZERO);
        assert_eq!(Tick::ZERO.next(), Tick::new(1));
    }

    #[test]
    fn cycle_phase_wraps_at_period() {
        let ebb = Cycle::new(Duration::seconds(15));
        assert_eq!(ebb.phase_at(Tick::new(0)), Duration::ZERO);
        assert_eq!(ebb.phase_at(Tick::new(14)), Duration::ticks(14));
        assert_eq!(ebb.phase_at(Tick::new(15)), Duration::ZERO);
        assert_eq!(ebb.turns_at(Tick::new(14)), 0);
        assert_eq!(ebb.turns_at(Tick::new(45)), 3);
    }

    #[test]
    fn cycle_offset_shifts_phase() {
        let tide = Cycle::new(Duration::minutes(20)).with_offset(Duration::minutes(5));
        assert_eq!(tide.phase_at(Tick::ZERO), Duration::minutes(5));
        assert_eq!(tide.phase_at(Tick::new(900)), Duration::ZERO);
        assert_eq!(tide.turns_at(Tick::new(899)), 0);
        assert_eq!(tide.turns_at(Tick::new(900)), 1);
    }

    #[test]
    fn cycle_survives_huge_ticks() {
        let cycle = Cycle::new(Duration::ticks(u64::MAX)).with_offset(Duration::ticks(u64::MAX - 1));
        assert_eq!(cycle.phase_at(Tick::new(0)), Duration::ticks(u64::MAX - 1));
    }

    #[test]
    fn cycle_window_marks_open_part_of_turn() {
        let tide = Cycle::new(Duration::minutes(20));
        let open = Duration::minutes(10);
        assert!(tide.is_within(Tick::new(599), open));
        assert!(!tide.is_within(Tick::new(600), open));
        assert!(tide.is_within(Tick::new(1_200), open));
    }

    #[test]
    #[should_panic(expected = "period must be greater than zero")]
    fn zero_period_panics() {
        let _ = Cycle::new(Duration::ZERO);
    }

    #[test]
    fn stopwatch_counts_only_while_running() {
        let mut watch = Stopwatch::new();
        assert_eq!(watch.elapsed(Tick::new(50)), Duration::ZERO);

        watch.resume(Tick::new(100));
        watch.pause(Tick::new(130));
        assert_eq!(watch.elapsed(Tick::new(9_999)), Duration::ticks(30));

        watch.resume(Tick::new(10_000));
        assert_eq!(watch.elapsed(Tick::new(10_020)), Duration::ticks(50));
        assert!(watch.has_reached(Tick::new(10_020), Duration::ticks(50)));
        assert!(!watch.has_reached(Tick::new(10_019), Duration::ticks(50)));
    }

    #[test]
    fn stopwatch_ignores_repeated_resume_and_pause() {
        let mut watch = Stopwatch::started(Tick::new(0));
        watch.resume(Tick::new(5));
        assert_eq!(watch.elapsed(Tick::new(10)), Duration::ticks(10));
        watch.pause(Tick::new(10));
        watch.pause(Tick::new(20));
        assert!(!watch.is_running());
        assert_eq!(watch.elapsed(Tick::new(30)), Duration::ticks(10));
    }
}
