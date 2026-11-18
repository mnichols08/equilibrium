//! The six bloodlines. Every character is born into one and carries it
//! through every life.
//!
//! A bloodline is nameless until a founder of it gives it a name; until
//! then it goes by what the old stories call it. Each leans toward two
//! attributes, which those born to it learn faster.

use crate::types::Attribute;

/// One of the six bloodlines, in the order lands are founded for them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Bloodline {
    /// Coastal ritualists, bound to the tides. The old shoreline's own.
    #[default]
    Tidal,
    /// Builders and oath-keepers.
    Covenant,
    /// Survivors of the burned slopes.
    Ash,
    /// Deep dwellers, slow and immovable.
    Stone,
    /// Scholars and stargazers.
    Lunar,
    /// Wanderers who keep no settlement.
    Nomadic,
}

impl Bloodline {
    /// Every bloodline, in order.
    pub const ALL: [Self; 6] = [Self::Tidal, Self::Covenant, Self::Ash, Self::Stone, Self::Lunar, Self::Nomadic];

    /// Position in [`Bloodline::ALL`].
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The bloodline of the land founded `nth` (counting the first as
    /// 0). After six lands they come round again.
    pub const fn of_land(nth: usize) -> Self {
        Self::ALL[nth % Self::ALL.len()]
    }

    /// What it is called until a founder names it.
    pub const fn old_name(self) -> &'static str {
        match self {
            Self::Tidal => "the Tidal line",
            Self::Covenant => "the Covenant line",
            Self::Ash => "the Ash line",
            Self::Stone => "the Stone line",
            Self::Lunar => "the Lunar line",
            Self::Nomadic => "the Nomadic line",
        }
    }

    /// The two attributes those born to it learn faster.
    pub const fn leanings(self) -> [Attribute; 2] {
        match self {
            Self::Tidal => [Attribute::Attunement, Attribute::Finesse],
            Self::Covenant => [Attribute::Resilience, Attribute::Willpower],
            Self::Ash => [Attribute::Willpower, Attribute::Vigor],
            Self::Stone => [Attribute::Resilience, Attribute::Lineage],
            Self::Lunar => [Attribute::Attunement, Attribute::Willpower],
            Self::Nomadic => [Attribute::Finesse, Attribute::Lineage],
        }
    }

    /// Whether those born to it learn `attribute` faster.
    pub fn leans_to(self, attribute: Attribute) -> bool {
        self.leanings().contains(&attribute)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lands_take_the_bloodlines_in_turn_and_come_round_again() {
        assert_eq!(Bloodline::of_land(0), Bloodline::Tidal);
        assert_eq!(Bloodline::of_land(5), Bloodline::Nomadic);
        assert_eq!(Bloodline::of_land(6), Bloodline::Tidal);
        assert_eq!(Bloodline::default(), Bloodline::Tidal);
    }

    #[test]
    fn each_bloodline_leans_two_ways_and_no_two_lean_the_same() {
        for (index, line) in Bloodline::ALL.into_iter().enumerate() {
            assert_eq!(line.index(), index);
            let [first, second] = line.leanings();
            assert!(first != second && line.leans_to(first) && line.leans_to(second));
            assert!(Bloodline::ALL.into_iter().all(|other| other == line || other.leanings() != line.leanings()));
        }
        assert!(!Bloodline::Stone.leans_to(Attribute::Finesse));
    }
}
