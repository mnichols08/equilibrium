//! The era the whole world is in, from the design document's World
//! State System. It follows what its people have done to it: how well
//! their lands are, and whether they are at war.
//!
//! Nothing here knows what a land is. It is told two numbers and says
//! which era that makes, and what an era changes.

/// Below this average vitality across the lands, the Void is consuming
/// the world.
const VOID_BELOW: i64 = 20;

/// Below this average vitality the lands are over-worked enough to be
/// an era of conflict, war or no war.
const CONFLICT_BELOW: i64 = 50;

/// The era the world is in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Era {
    /// The lands are well and nobody is at war. Building is cheap.
    #[default]
    Rebuilding,
    /// The lands are over-worked, or at war. Fighting teaches twice as much.
    Conflict,
    /// The lands are failing everywhere. What lives in the wild comes
    /// back as fast as it is put down.
    VoidConsumption,
}

impl Era {
    /// The era of a world whose lands have `vitality` on average (0 to
    /// 100) and `wars` wars standing between them.
    pub const fn of(vitality: i64, wars: usize) -> Self {
        if vitality < VOID_BELOW {
            Self::VoidConsumption
        } else if vitality < CONFLICT_BELOW || wars > 0 {
            Self::Conflict
        } else {
            Self::Rebuilding
        }
    }

    /// The name shown to players.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Rebuilding => "the Era of Rebuilding",
            Self::Conflict => "the Era of Conflict",
            Self::VoidConsumption => "the Void Consumption",
        }
    }

    /// What building costs, as a percentage of the usual price.
    pub const fn building_percent(self) -> u64 {
        match self {
            Self::Rebuilding => 75,
            Self::Conflict | Self::VoidConsumption => 100,
        }
    }

    /// How much a blow teaches, as a percentage of the usual.
    pub const fn fighting_xp_percent(self) -> u64 {
        match self {
            Self::Conflict => 200,
            Self::Rebuilding | Self::VoidConsumption => 100,
        }
    }

    /// Whether creatures come back to an empty lair at once.
    pub const fn lairs_never_rest(self) -> bool {
        matches!(self, Self::VoidConsumption)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_era_follows_the_lands_and_their_wars() {
        assert_eq!(Era::default(), Era::Rebuilding);
        assert_eq!(Era::of(100, 0), Era::Rebuilding);
        assert_eq!(Era::of(50, 0), Era::Rebuilding);
        assert_eq!(Era::of(49, 0), Era::Conflict, "over-worked lands");
        assert_eq!(Era::of(100, 1), Era::Conflict, "war");
        assert_eq!(Era::of(19, 0), Era::VoidConsumption);
        assert_eq!(Era::of(19, 3), Era::VoidConsumption, "the Void does not care who is at war");
    }

    #[test]
    fn each_era_changes_one_thing() {
        assert_eq!((Era::Rebuilding.building_percent(), Era::Conflict.building_percent()), (75, 100));
        assert_eq!((Era::Rebuilding.fighting_xp_percent(), Era::Conflict.fighting_xp_percent()), (100, 200));
        assert!(Era::VoidConsumption.lairs_never_rest() && !Era::Conflict.lairs_never_rest());
    }
}
