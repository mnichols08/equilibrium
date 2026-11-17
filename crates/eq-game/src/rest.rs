//! Rest as plain sums over time asleep.
//!
//! Nothing here touches the world. Each function takes how long someone
//! slept and says what that did: how rested they are, and how far the
//! body wasted if they slept too long.
//!
//! | Asleep | What happens |
//! | --- | --- |
//! | up to 8 hours | the rested bonus refills |
//! | up to 3 days | nothing gained, nothing lost |
//! | beyond | body skills and attributes slip, a level per 6 hours, to a floor |
//! | 14 days | the body dies where it lies (the rules in `game` see to that) |

use eq_clock::Duration;

use crate::content::{ATROPHY_EVERY, ATROPHY_FLOOR_PERCENT, FORGIVING_STRETCH, FULL_REST, RESTED_BONUS_PERCENT};

/// The most rested a character can be.
pub const FULLY_RESTED: u32 = 100;

/// Rest gained by sleeping `asleep`, in points out of [`FULLY_RESTED`].
pub const fn rest_gained(asleep: Duration) -> u32 {
    let gained = asleep.as_ticks() * FULLY_RESTED as u64 / FULL_REST.as_ticks();
    if gained > FULLY_RESTED as u64 {
        FULLY_RESTED
    } else {
        gained as u32
    }
}

/// Levels every body skill and attribute loses after sleeping `asleep`.
pub const fn atrophy(asleep: Duration) -> u32 {
    let wasting = asleep.as_ticks().saturating_sub(FORGIVING_STRETCH.as_ticks());
    let levels = wasting / ATROPHY_EVERY.as_ticks();
    if levels > u32::MAX as u64 {
        u32::MAX
    } else {
        levels as u32
    }
}

/// The level atrophy cannot push a skill or attribute below, given the
/// highest level it ever reached.
pub const fn atrophy_floor(peak: u32) -> u32 {
    peak * ATROPHY_FLOOR_PERCENT / 100
}

/// Experience after the rested bonus, for a character `rested` points
/// rested.
pub const fn with_rested_bonus(xp: u64, rested: u32) -> u64 {
    let rested = if rested > FULLY_RESTED { FULLY_RESTED } else { rested } as u64;
    xp + xp * RESTED_BONUS_PERCENT * rested / (100 * FULLY_RESTED as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nights_sleep_restores_everything_and_a_nap_restores_some() {
        assert_eq!(rest_gained(Duration::ZERO), 0);
        assert_eq!(rest_gained(Duration::hours(2)), 25);
        assert_eq!(rest_gained(Duration::hours(8)), 100);
        assert_eq!(rest_gained(Duration::days(30)), 100);
    }

    #[test]
    fn nothing_wastes_inside_the_forgiving_stretch() {
        assert_eq!(atrophy(Duration::hours(8)), 0);
        assert_eq!(atrophy(Duration::days(3)), 0);
        assert_eq!(atrophy(Duration::days(3) + Duration::hours(5)), 0);
    }

    #[test]
    fn beyond_it_a_level_goes_every_six_hours() {
        assert_eq!(atrophy(Duration::days(3) + Duration::hours(6)), 1);
        assert_eq!(atrophy(Duration::days(4)), 4);
        assert_eq!(atrophy(Duration::days(13)), 40);
    }

    #[test]
    fn the_floor_is_half_the_peak() {
        assert_eq!(atrophy_floor(40), 20);
        assert_eq!(atrophy_floor(1), 0);
        assert_eq!(atrophy_floor(0), 0);
    }

    #[test]
    fn the_rested_bonus_shrinks_evenly_to_nothing() {
        assert_eq!(with_rested_bonus(100, 100), 150);
        assert_eq!(with_rested_bonus(100, 50), 125);
        assert_eq!(with_rested_bonus(100, 0), 100);
        assert_eq!(with_rested_bonus(100, 999), 150);
    }
}
