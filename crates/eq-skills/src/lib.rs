//! Use-based proficiency.
//!
//! A [`Proficiency`] is a level that rises by doing. Each use earns less
//! as the level nears its cap. The highest level ever reached is
//! remembered, and ground lost below it is regained faster than it was
//! first won: muscle memory.
//!
//! ```
//! use eq_skills::{Proficiency, Rules};
//!
//! const RULES: Rules = Rules { cap: 100, xp_per_level: 100, relearning_percent: 300 };
//!
//! let mut mining = Proficiency::new();
//! let practice = mining.practice(250, &RULES);
//! assert_eq!(practice.levels_gained, 2);
//! assert_eq!(mining.level(), 2);
//! ```
#![no_std]

use eq_curve::diminishing_returns;

/// How a proficiency grows. Plain numbers, meant to be tuned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rules {
    /// The highest level reachable.
    pub cap: u32,
    /// Experience needed for each level.
    pub xp_per_level: u64,
    /// How fast a level below the remembered peak is regained, as a
    /// percentage of the normal pace. 100 means no faster.
    pub relearning_percent: u32,
}

/// What one use earned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Practice {
    /// Experience earned after diminishing returns.
    pub xp_gained: u64,
    /// Whole levels gained.
    pub levels_gained: u32,
}

/// A level that rises with use.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Proficiency {
    level: u32,
    progress: u64,
    peak: u32,
}

impl Proficiency {
    /// A proficiency at level zero.
    pub const fn new() -> Self {
        Self { level: 0, progress: 0, peak: 0 }
    }

    /// The current level.
    pub const fn level(&self) -> u32 {
        self.level
    }

    /// The highest level ever reached.
    pub const fn peak(&self) -> u32 {
        self.peak
    }

    /// Experience banked toward the next level.
    pub const fn progress(&self) -> u64 {
        self.progress
    }

    /// Earns experience from one use worth `base_xp`.
    ///
    /// The amount is scaled by `(1 − level/cap)^1.5` at the level held
    /// when the use began, and again by `relearning_percent` if that
    /// level is below the remembered peak. Below the cap a use always
    /// earns at least one point, so the last levels are slow but never
    /// impossible.
    ///
    /// # Panics
    /// Panics if `rules.cap` or `rules.xp_per_level` is zero.
    pub fn practice(&mut self, base_xp: u64, rules: &Rules) -> Practice {
        assert!(rules.xp_per_level > 0, "xp_per_level must be greater than zero");
        if self.level >= rules.cap || base_xp == 0 {
            return Practice { xp_gained: 0, levels_gained: 0 };
        }
        let scaled = diminishing_returns(base_xp, u64::from(self.level), u64::from(rules.cap));
        let pace = if self.level < self.peak { u64::from(rules.relearning_percent) } else { 100 };
        let xp_gained = (scaled.saturating_mul(pace) / 100).max(1);
        let total = self.progress.saturating_add(xp_gained);
        let room = rules.cap - self.level;
        let levels_gained = u32::try_from(total / rules.xp_per_level).unwrap_or(u32::MAX).min(room);

        self.level += levels_gained;
        self.peak = self.peak.max(self.level);
        self.progress = if self.level >= rules.cap {
            0
        } else {
            total - u64::from(levels_gained) * rules.xp_per_level
        };
        Practice { xp_gained, levels_gained }
    }

    /// What carries over into a new life: `percent` of the level, rounded
    /// down, with nothing banked and no memory of the old peak.
    pub const fn reborn(&self, percent: u32) -> Self {
        let level = self.level * percent / 100;
        Self { level, progress: 0, peak: level }
    }

    /// Drops up to `levels` levels, never below `floor`, and returns how
    /// many were lost. The peak is kept.
    pub fn lose(&mut self, levels: u32, floor: u32) -> u32 {
        let lost = levels.min(self.level.saturating_sub(floor));
        self.level -= lost;
        if lost > 0 {
            self.progress = 0;
        }
        lost
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULES: Rules = Rules { cap: 100, xp_per_level: 100, relearning_percent: 300 };

    #[test]
    fn progress_carries_between_uses() {
        let mut skill = Proficiency::new();
        assert_eq!(skill.practice(60, &RULES), Practice { xp_gained: 60, levels_gained: 0 });
        assert_eq!(skill.practice(60, &RULES).levels_gained, 1);
        assert_eq!((skill.level(), skill.progress()), (1, 20));
    }

    #[test]
    fn each_level_is_slower_than_the_last() {
        let mut skill = Proficiency::new();
        let mut uses_per_level = [0u32; 4];
        for uses in &mut uses_per_level {
            let target = skill.level() + 25;
            while skill.level() < target {
                skill.practice(100, &RULES);
                *uses += 1;
            }
        }
        assert!(uses_per_level.windows(2).all(|pair| pair[0] < pair[1]), "{uses_per_level:?}");
        assert_eq!(skill.level(), 100);
    }

    #[test]
    fn stops_at_the_cap() {
        let mut skill = Proficiency::new();
        skill.practice(u64::MAX, &RULES);
        assert_eq!((skill.level(), skill.progress()), (100, 0));
        assert_eq!(skill.practice(1_000, &RULES), Practice { xp_gained: 0, levels_gained: 0 });
    }

    #[test]
    fn tiny_uses_near_the_cap_still_count() {
        let mut skill = Proficiency::new();
        skill.practice(u64::MAX, &Rules { cap: 99, ..RULES });
        assert_eq!(skill.level(), 99);
        assert_eq!(skill.practice(1, &RULES).xp_gained, 1);
    }

    #[test]
    fn lost_levels_come_back_faster_than_they_were_first_won() {
        let mut skill = Proficiency::new();
        let mut first_time = 0;
        while skill.level() < 20 {
            skill.practice(50, &RULES);
            first_time += 1;
        }
        skill.lose(10, 0);

        let mut second_time = 0;
        while skill.level() < 20 {
            skill.practice(50, &RULES);
            second_time += 1;
        }
        // Ten levels regained against twenty won: without muscle memory
        // that would take more than half as long.
        assert!(second_time * 4 < first_time, "{second_time} uses against {first_time}");

        // Past the old peak it is back to the normal pace.
        let before = skill.progress();
        assert_eq!(skill.practice(50, &RULES).xp_gained, 35);
        assert!(skill.progress() > before || skill.level() > 20);
    }

    #[test]
    fn a_new_life_keeps_a_share_of_the_level_and_forgets_the_peak() {
        let mut skill = Proficiency::new();
        skill.practice(u64::MAX, &Rules { cap: 40, ..RULES });
        skill.lose(10, 0);
        let reborn = skill.reborn(15);
        assert_eq!((reborn.level(), reborn.peak(), reborn.progress()), (4, 4, 0));
        assert_eq!(Proficiency::new().reborn(15), Proficiency::new());
    }

    #[test]
    fn zero_effort_earns_nothing() {
        let mut skill = Proficiency::new();
        assert_eq!(skill.practice(0, &RULES).xp_gained, 0);
    }

    #[test]
    fn losing_levels_keeps_the_peak_and_respects_the_floor() {
        let mut skill = Proficiency::new();
        skill.practice(1_000, &RULES);
        let reached = skill.level();
        assert_eq!(skill.lose(3, 0), 3);
        assert_eq!(skill.level(), reached - 3);
        assert_eq!(skill.peak(), reached);
        assert_eq!(skill.lose(100, 5), reached - 3 - 5);
        assert_eq!(skill.level(), 5);
        assert_eq!(skill.lose(1, 5), 0);
    }
}
