//! The balance of the world: how far it leans toward stillness or fury.
//!
//! Lands that keep too much peace still the world: water fails, and
//! what is taken from the ground does not come back. Lands that make
//! too much war rouse it: storms tear at everything that can be worked.
//! In between, and for lands that have no dealings at all, the world
//! is even and none of this is felt.
//!
//! Nothing here knows what a pact or a deposit is. It is one number,
//! what pushes it, and what it stands for.

/// The furthest the world can lean either way.
const MOST: i64 = 100;

/// How far it must lean before anything is felt.
const FELT_AT: i64 = 20;

/// How far it must lean before the worst is felt.
const WORST_AT: i64 = 60;

/// What the world is like to live in.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mood {
    /// Too much peace, for too long. Nothing grows back.
    Parched,
    /// Peace is winning. What is taken comes back slowly.
    Dry,
    /// Peace and war in balance, or no dealings at all.
    Even,
    /// War is winning. Storms take a little of everything.
    Restless,
    /// Too much war, for too long. Storms take a lot.
    Raging,
}

impl Mood {
    /// The name shown to players.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Parched => "parched",
            Self::Dry => "dry",
            Self::Even => "even",
            Self::Restless => "restless",
            Self::Raging => "raging",
        }
    }

    /// How fast what is taken from the ground comes back, as a
    /// percentage of the usual pace.
    pub const fn growth_percent(self) -> u32 {
        match self {
            Self::Parched => 0,
            Self::Dry => 50,
            Self::Even | Self::Restless | Self::Raging => 100,
        }
    }

    /// The share of what every deposit holds that each storm tears
    /// away, as a percentage.
    /// How long a tidal way stays clear each tide, in percent of the
    /// usual, or `None` if the water never comes in at all.
    pub const fn tide_open_percent(self) -> Option<u64> {
        match self {
            Self::Parched => None,
            Self::Dry => Some(150),
            Self::Even => Some(100),
            Self::Restless => Some(75),
            Self::Raging => Some(50),
        }
    }

    /// Whether the tide crashes in hard enough to hurt whoever is
    /// caught on the shore when it comes.
    pub const fn tide_crashes(self) -> bool {
        matches!(self, Self::Raging)
    }

    /// How much a storm tears from every deposit, as a percentage.
    pub const fn storm_percent(self) -> u32 {
        match self {
            Self::Parched | Self::Dry | Self::Even => 0,
            Self::Restless => 3,
            Self::Raging => 10,
        }
    }
}

impl Mood {
    /// Whether the creatures that live in the wild can live at all.
    /// In a parched world they die where they are and none come back.
    pub const fn sustains_animals(self) -> bool {
        !matches!(self, Self::Parched)
    }

    /// How hard creatures strike, as a percentage of their usual
    /// strength. A roused world rouses what lives in it.
    pub const fn ferocity_percent(self) -> u32 {
        match self {
            Self::Parched | Self::Dry | Self::Even => 100,
            Self::Restless => 125,
            Self::Raging => 150,
        }
    }

    /// What each storm costs every land, in the same coin as a death:
    /// vitality that only time gives back. A land wins back ten in the
    /// time between two storms, so a restless world only holds a land a
    /// little under its best, and a raging one grinds it down over the
    /// best part of a day.
    pub const fn storm_wear(self) -> i64 {
        match self {
            Self::Parched | Self::Dry | Self::Even => 0,
            Self::Restless => 5,
            Self::Raging => 12,
        }
    }

    /// The health each storm takes from anyone caught without shelter.
    /// A storm batters; it does not kill.
    pub const fn storm_harm(self) -> u32 {
        match self {
            Self::Parched | Self::Dry | Self::Even => 0,
            Self::Restless => 5,
            Self::Raging => 15,
        }
    }
}

/// How far the world leans: below zero toward stillness, above toward
/// fury.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Balance {
    lean: i64,
}

impl Balance {
    /// A world in balance.
    pub const fn new() -> Self {
        Self { lean: 0 }
    }

    /// How far it leans, from -100 (stillness) to 100 (fury).
    pub const fn lean(self) -> i64 {
        self.lean
    }

    /// What that is like to live in.
    pub const fn mood(self) -> Mood {
        match self.lean {
            lean if lean <= -WORST_AT => Mood::Parched,
            lean if lean <= -FELT_AT => Mood::Dry,
            lean if lean >= WORST_AT => Mood::Raging,
            lean if lean >= FELT_AT => Mood::Restless,
            _ => Mood::Even,
        }
    }

    /// Pushes the world `by`: toward fury if positive, stillness if
    /// negative. Returns the moods before and after if it changed.
    pub fn tip(&mut self, by: i64) -> Option<(Mood, Mood)> {
        let before = self.mood();
        self.lean = self.lean.saturating_add(by).clamp(-MOST, MOST);
        let after = self.mood();
        (before != after).then_some((before, after))
    }

    /// Lets the world ease one step back toward balance, as it does
    /// when nothing is pushing it.
    pub fn settle(&mut self) -> Option<(Mood, Mood)> {
        self.tip(-self.lean.signum())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_world_nobody_pushes_is_even_and_stays_so() {
        let mut balance = Balance::new();
        assert_eq!((balance.mood(), balance.settle(), balance.lean()), (Mood::Even, None, 0));
        assert_eq!((Mood::Even.growth_percent(), Mood::Even.storm_percent()), (100, 0));
    }

    #[test]
    fn peace_dries_the_world_and_war_rouses_it() {
        let mut balance = Balance::new();
        assert_eq!(balance.tip(-19), None);
        assert_eq!(balance.tip(-1), Some((Mood::Even, Mood::Dry)));
        assert_eq!(balance.tip(-40), Some((Mood::Dry, Mood::Parched)));
        assert_eq!(balance.tip(-1_000), None);
        assert_eq!(balance.lean(), -100, "it only leans so far");
        assert_eq!(Mood::Parched.growth_percent(), 0, "nothing comes back");

        assert_eq!(balance.tip(160), Some((Mood::Parched, Mood::Raging)));
        assert!(Mood::Raging.storm_percent() > Mood::Restless.storm_percent());
        assert!(Mood::Raging.storm_harm() > Mood::Restless.storm_harm() && Mood::Even.storm_harm() == 0);
        assert!(Mood::Raging.storm_wear() > Mood::Restless.storm_wear() && Mood::Dry.storm_wear() == 0);
        assert_eq!((Mood::Even.ferocity_percent(), Mood::Raging.ferocity_percent()), (100, 150));
        assert!(!Mood::Parched.sustains_animals() && Mood::Dry.sustains_animals() && Mood::Raging.sustains_animals());
        assert_eq!(Mood::Raging.growth_percent(), 100);
    }

    #[test]
    fn left_alone_it_eases_back_a_step_at_a_time() {
        let mut balance = Balance::new();
        balance.tip(21);
        assert_eq!(balance.settle(), None);
        assert_eq!(balance.settle(), Some((Mood::Restless, Mood::Even)));
        balance.tip(-40);
        balance.settle();
        assert_eq!(balance.lean(), -20);
    }
}
