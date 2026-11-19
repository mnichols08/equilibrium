//! Diminishing returns: the more someone works one deposit, the less
//! each haul gives them there.
//!
//! Every haul at a place leaves its gatherer a little more worked out
//! there, and less of each one after it is worth keeping, though the
//! deposit gives up as much as ever and the work teaches as much. It
//! wears off with time spent elsewhere. Someone who moves between
//! deposits gathers at full value and pays for it in walking; someone
//! who stays at one wastes the land for less and less. It is the
//! gatherer's own: anyone else at the same deposit gathers as if fresh.

use alloc::vec::Vec;

use eq_clock::Tick;
use eq_world_graph::PlaceId;

use crate::content::{WORKED_OUT_FADES_EVERY, WORKED_OUT_FLOOR_PERCENT, WORKED_OUT_STEP_PERCENT};

/// Where someone has been working lately, and how hard.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Worked {
    /// Each place, how many hauls' weariness it held, and when it last grew.
    recent: Vec<(PlaceId, u32, Tick)>,
}

impl Worked {
    /// Nowhere worked.
    pub const fn new() -> Self {
        Self { recent: Vec::new() }
    }

    /// How many hauls' weariness is left at `place` by `now`.
    pub fn hauls(&self, place: PlaceId, now: Tick) -> u32 {
        self.recent.iter().find(|entry| entry.0 == place).map_or(0, |&(_, hauls, at)| faded(hauls, at, now))
    }

    /// The share, in percent, of a full haul that working `place` at
    /// `now` would give.
    pub fn returns_percent(&self, place: PlaceId, now: Tick) -> u32 {
        100u32.saturating_sub(self.hauls(place, now) * WORKED_OUT_STEP_PERCENT).max(WORKED_OUT_FLOOR_PERCENT)
    }

    /// One more haul at `place` at `now`. Places worn off entirely are forgotten.
    pub(crate) fn work(&mut self, place: PlaceId, now: Tick) {
        let hauls = self.hauls(place, now) + 1;
        self.recent.retain(|&(other, held, at)| other != place && faded(held, at, now) > 0);
        self.recent.push((place, hauls, now));
    }
}

/// `hauls` of weariness from `at`, after fading until `now`.
fn faded(hauls: u32, at: Tick, now: Tick) -> u32 {
    let gone = now.get().saturating_sub(at.get()) / WORKED_OUT_FADES_EVERY.as_ticks();
    hauls.saturating_sub(u32::try_from(gone).unwrap_or(u32::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEDGE: PlaceId = PlaceId::new(1);
    const BEACH: PlaceId = PlaceId::new(2);

    #[test]
    fn each_haul_at_one_place_gives_less_down_to_a_floor() {
        let mut worked = Worked::new();
        let mut returns = Vec::new();
        for haul in 0..9 {
            let now = Tick::new(haul * 5);
            returns.push(worked.returns_percent(LEDGE, now));
            worked.work(LEDGE, now);
        }
        assert_eq!(returns, [100, 85, 70, 55, 40, 25, 10, 10, 10]);
        // Somewhere else is as fresh as ever.
        assert_eq!(worked.returns_percent(BEACH, Tick::new(45)), 100);
    }

    #[test]
    fn it_wears_off_with_time_away() {
        let mut worked = Worked::new();
        for haul in 0..4 {
            worked.work(LEDGE, Tick::new(haul * 5));
        }
        let last = Tick::new(15);
        assert_eq!(worked.returns_percent(LEDGE, last), 40);
        assert_eq!(worked.returns_percent(LEDGE, last + WORKED_OUT_FADES_EVERY), 55);
        let rested = Tick::new(15 + 4 * WORKED_OUT_FADES_EVERY.as_ticks());
        assert_eq!(worked.returns_percent(LEDGE, rested), 100);
        // Working elsewhere then forgets the place altogether.
        worked.work(BEACH, rested);
        assert_eq!(worked.recent.len(), 1);
    }
}
