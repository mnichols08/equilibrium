//! Fighting and theft: who may be attacked or robbed, and how hard a
//! blow lands.
//!
//! The rule for who may be attacked is the middle ground between open
//! PvP and none:
//!
//! - In a haven, nobody.
//! - In the wilderness, only those the land has marked. A player who
//!   never over-harvests can never be attacked there.
//! - In contested places, anybody.

use crate::content::{
    FIGHTING_PER_PERCENT_DAMAGE,
    BASE_DAMAGE, DAMAGE_LOST_AT_FULL_TAINT, DAMAGE_PERCENT_PER_VIGOR, MOST_DAMAGE_RESISTED, RESILIENCE_PER_PERCENT_RESISTED,
};
use crate::types::{Safety, Standing};

/// Why an attack was not allowed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Refusal {
    /// The target is the attacker.
    Yourself,
    /// The target is not in the same place, or is a spirit and cannot
    /// be touched.
    NotHere,
    /// This place is a haven.
    Haven,
    /// This is wilderness and the target carries no mark.
    Unmarked,
}

/// Why a spirit could not return to its body.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReturnRefusal {
    /// The spirit is not where its body lies.
    NotAtBody,
    /// The spirit is held back until this tick.
    TooSoon(eq_clock::Tick),
    /// There is no body: it died in its sleep and is gone.
    NoBody,
    /// The body was left too long and has risen as a Body Guard.
    Risen,
    /// The body has been buried.
    Buried,
    /// There is no giving up and starting again: the way back to the
    /// living is through the afterlife, memory by memory.
    Unredeemed,
}

/// Why something only a founder can do was not done.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FoundRefusal {
    /// They did not found the territory they belong to.
    NotFounder,
    /// It has a name already. Names are for good.
    AlreadyNamed,
    /// The name will not do.
    Name(eq_naming::Problem),
    /// Someone of that name is already in the world.
    AlreadyHere,
    /// They have as many invitations waiting as a founder may.
    TooManyInvites,
}

/// Why a Restoration Rite could not be held.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RiteRefusal {
    /// There is nothing here that could be mended.
    NothingToMend,
    /// The deposit here is not hard pressed enough to need a rite.
    NoNeed,
    /// The land will not answer someone it has marked.
    Marked,
    /// A ward already lies on this place.
    Warded,
}

/// Why a sleeper could not be tended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TendRefusal {
    /// The target is the one tending.
    Yourself,
    /// The target is somewhere else, or a spirit.
    NotHere,
    /// The target is awake.
    Awake,
    /// The sleeper has been tended as much as tending can help.
    Enough,
}

/// Why a name could not be spoken at the shrine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NameRefusal {
    /// Names are only heard at the zone's anchor.
    NotAtShrine,
    /// The one named is not a forgotten spirit.
    NotForgotten,
}

/// Why a robbery was not allowed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RobRefusal {
    /// The target is the robber.
    Yourself,
    /// The target is not in the same place.
    NotHere,
    /// The target is awake.
    Awake,
    /// This place is a haven.
    Haven,
    /// The target carries nothing.
    NothingToSteal,
}

/// Whether a sleeper may be robbed in a place of this `safety`. Sleep is
/// safe only in a haven.
pub const fn may_rob(safety: Safety) -> Result<(), RobRefusal> {
    match safety {
        Safety::Haven => Err(RobRefusal::Haven),
        Safety::Buffer | Safety::Contested => Ok(()),
    }
}

/// Whether someone of `target` standing may be attacked in a place of
/// this `safety`.
pub const fn may_attack(safety: Safety, target: Standing) -> Result<(), Refusal> {
    match (safety, target) {
        (Safety::Haven, _) => Err(Refusal::Haven),
        (Safety::Buffer, Standing::Clean) => Err(Refusal::Unmarked),
        (Safety::Buffer, Standing::Marked) | (Safety::Contested, _) => Ok(()),
    }
}

/// Everything that decides how hard one blow lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Blow {
    /// The random part of the blow, from `0` up to the damage spread.
    pub roll: u32,
    /// The attacker's Vigor.
    pub vigor: u32,
    /// The attacker's fighting skill.
    pub fighting: u32,
    /// The attacker's taint, from 0 to 100. The land's mark weakens the arm.
    pub taint: u32,
    /// The defender's Resilience.
    pub resilience: u32,
}

impl Blow {
    /// The health this blow takes:
    ///
    /// `(base + roll) × (1 + vigor / 100) × (1 + fighting / 200) × (1 − resisted) × (1 − taint loss)`
    ///
    /// rounded down, but never less than 1.
    pub fn damage(&self) -> u32 {
        const WHOLE: u64 = 100;
        let resisted = (self.resilience / RESILIENCE_PER_PERCENT_RESISTED).min(MOST_DAMAGE_RESISTED);
        let weakened = self.taint.min(100) * DAMAGE_LOST_AT_FULL_TAINT / 100;
        let parts = u64::from(BASE_DAMAGE + self.roll)
            * (WHOLE + u64::from(self.vigor * DAMAGE_PERCENT_PER_VIGOR))
            * (WHOLE * FIGHTING_PER_PERCENT_DAMAGE + u64::from(self.fighting))
            * (WHOLE - u64::from(resisted))
            * (WHOLE - u64::from(weakened));
        // A base blow is small and levels stop at 100, so this fits easily.
        ((parts / (WHOLE.pow(4) * FIGHTING_PER_PERCENT_DAMAGE)) as u32).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn blow(vigor: u32, taint: u32, resilience: u32) -> Blow {
        Blow { roll: 0, vigor, fighting: 0, taint, resilience }
    }

    #[test]
    fn havens_are_safe_wilderness_protects_the_clean_and_contested_ground_protects_nobody() {
        for standing in [Standing::Clean, Standing::Marked] {
            assert_eq!(may_attack(Safety::Haven, standing), Err(Refusal::Haven));
            assert_eq!(may_attack(Safety::Contested, standing), Ok(()));
        }
        assert_eq!(may_attack(Safety::Buffer, Standing::Clean), Err(Refusal::Unmarked));
        assert_eq!(may_attack(Safety::Buffer, Standing::Marked), Ok(()));
    }

    #[test]
    fn sleepers_can_be_robbed_anywhere_but_a_haven() {
        assert_eq!(may_rob(Safety::Haven), Err(RobRefusal::Haven));
        assert_eq!(may_rob(Safety::Buffer), Ok(()));
        assert_eq!(may_rob(Safety::Contested), Ok(()));
    }

    #[test]
    fn damage_follows_vigor_resilience_and_taint() {
        assert_eq!(blow(0, 0, 0).damage(), 12);
        assert_eq!(blow(50, 0, 0).damage(), 18);
        assert_eq!(blow(0, 0, 50).damage(), 9);
        assert_eq!(blow(0, 100, 0).damage(), 6);
        assert_eq!(Blow { roll: 6, vigor: 0, fighting: 0, taint: 0, resilience: 0 }.damage(), 18);
        assert_eq!(Blow { fighting: 50, ..blow(0, 0, 0) }.damage(), 15, "a quarter more at fighting 50");
    }

    #[test]
    fn resistance_is_capped_and_a_blow_always_lands() {
        assert_eq!(blow(0, 0, 100).damage(), blow(0, 0, 1_000).damage());
        assert!(blow(0, 100, 100).damage() >= 1);
    }
}
