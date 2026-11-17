//! The Spirit Layer as plain sums.
//!
//! The dead walk the same places as the living and can draw ectoplasm
//! from them. Ectoplasm is the one thing that survives a death: it is
//! spent at rebirth to carry more of the old life into the new one.
//! Drawing it also moves a spirit up the spirit tiers, life after life.
//!
//! Nothing here touches the world.

use crate::content::{
    ECTOPLASM_PER_PERCENT, LINEAGE_PER_PERCENT, MOST_CARRYOVER_PERCENT, MOST_ECTOPLASM_BONUS_PERCENT, SPIRIT_TIER_AT,
};

/// How far a spirit has come, from all the ectoplasm it has ever drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SpiritTier {
    /// The base form: slow and of little account.
    LostShade,
    /// Moves through the Spirit Layer at twice the pace.
    WanderingPhantom,
    /// Can shield the living. (Not built yet; the name is earned.)
    SpiritSentinel,
    /// Can take a body for a moment. (Not built yet; the name is earned.)
    AncestralAspect,
}

impl SpiritTier {
    /// Every tier, lowest first.
    pub const ALL: [Self; 4] =
        [Self::LostShade, Self::WanderingPhantom, Self::SpiritSentinel, Self::AncestralAspect];

    /// The tier reached after drawing `drawn` ectoplasm over every life.
    pub fn of(drawn: u64) -> Self {
        let reached = SPIRIT_TIER_AT.iter().filter(|&&needed| drawn >= needed).count();
        Self::ALL[reached.saturating_sub(1).min(Self::ALL.len() - 1)]
    }

    /// The name shown to players.
    pub const fn name(self) -> &'static str {
        match self {
            Self::LostShade => "Lost Shade",
            Self::WanderingPhantom => "Wandering Phantom",
            Self::SpiritSentinel => "Spirit Sentinel",
            Self::AncestralAspect => "Ancestral Aspect",
        }
    }

    /// How many times faster than a Lost Shade this tier travels.
    pub const fn pace(self) -> u64 {
        match self {
            Self::LostShade => 1,
            _ => 2,
        }
    }
}

/// Ectoplasm one draw gives a spirit of this Attunement.
pub const fn ectoplasm_per_draw(attunement: u32) -> u64 {
    1 + attunement as u64 / 10
}

/// What a new life keeps of the old.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inheritance {
    /// The share of each skill and attribute kept, as a percentage.
    pub percent: u32,
    /// The ectoplasm spent to raise it.
    pub spent: u64,
}

/// Works out an inheritance: the `base` share, raised by Lineage and by
/// as much ectoplasm as is held and will count.
pub fn inheritance(base: u32, lineage: u32, ectoplasm: u64) -> Inheritance {
    let from_lineage = lineage / LINEAGE_PER_PERCENT;
    let room = MOST_CARRYOVER_PERCENT.saturating_sub(base + from_lineage).min(MOST_ECTOPLASM_BONUS_PERCENT);
    // The bonus is at most a few dozen percent, so the cast cannot truncate.
    let bought = (ectoplasm / ECTOPLASM_PER_PERCENT).min(u64::from(room)) as u32;
    Inheritance {
        percent: (base + from_lineage + bought).min(MOST_CARRYOVER_PERCENT),
        spent: u64::from(bought) * ECTOPLASM_PER_PERCENT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_come_with_ectoplasm_drawn() {
        assert_eq!(SpiritTier::of(0), SpiritTier::LostShade);
        assert_eq!(SpiritTier::of(99), SpiritTier::LostShade);
        assert_eq!(SpiritTier::of(100), SpiritTier::WanderingPhantom);
        assert_eq!(SpiritTier::of(400), SpiritTier::SpiritSentinel);
        assert_eq!(SpiritTier::of(u64::MAX), SpiritTier::AncestralAspect);
    }

    #[test]
    fn only_the_first_tier_is_slow() {
        assert_eq!(SpiritTier::LostShade.pace(), 1);
        assert_eq!(SpiritTier::WanderingPhantom.pace(), 2);
    }

    #[test]
    fn attunement_makes_each_draw_richer() {
        assert_eq!(ectoplasm_per_draw(0), 1);
        assert_eq!(ectoplasm_per_draw(9), 1);
        assert_eq!(ectoplasm_per_draw(30), 4);
    }

    #[test]
    fn a_bare_rebirth_keeps_the_base_share() {
        assert_eq!(inheritance(15, 0, 0), Inheritance { percent: 15, spent: 0 });
    }

    #[test]
    fn ectoplasm_buys_a_better_inheritance_up_to_a_limit() {
        assert_eq!(inheritance(15, 0, 45), Inheritance { percent: 17, spent: 40 });
        assert_eq!(inheritance(15, 0, 10_000), Inheritance { percent: 30, spent: 300 });
    }

    #[test]
    fn lineage_raises_it_for_nothing_and_the_whole_is_capped() {
        assert_eq!(inheritance(15, 10, 0), Inheritance { percent: 20, spent: 0 });
        assert_eq!(inheritance(15, 100, 10_000), Inheritance { percent: 50, spent: 0 });
        assert_eq!(inheritance(5, 0, 0), Inheritance { percent: 5, spent: 0 });
    }
}
