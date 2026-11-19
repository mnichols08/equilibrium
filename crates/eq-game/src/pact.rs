//! Pacts: how two lands stand to each other.
//!
//! Lands that never deal with each other have no pact, and nothing
//! follows from that. A pact is an alliance or a war. War can be
//! declared by one side; an alliance, and the truce that ends a war,
//! take both.
//!
//! Nothing here knows what a land is beyond a number, or what a pact
//! does: the rules in `game` decide that.

use alloc::vec::Vec;

/// How two lands stand to each other.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pact {
    /// They stand together.
    Alliance,
    /// They are at war.
    War,
}

/// What one founder asks of another's land.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Dealing {
    /// Stand together. Takes both.
    Ally,
    /// Make war. Takes one.
    War,
    /// Go back to having no pact. Leaving an alliance takes one; ending
    /// a war takes both.
    Truce,
}

/// What came of a dealing.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dealt {
    /// The offer stands until the other side makes the same one.
    Offered,
    /// The two lands are allies now.
    Allied,
    /// The two lands are at war now.
    AtWar,
    /// The war between them is over.
    Peace,
    /// The alliance between them is over.
    Parted,
}

/// Why a dealing came to nothing.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PactRefusal {
    /// Only the founder of a land speaks for it.
    NotFounder,
    /// Pacts are made on the Meeting Ground and nowhere else.
    NotOnMeetingGround,
    /// No land is known by that name, or by a founder of that name.
    NoSuchLand,
    /// A land has no dealings with itself.
    OwnLand,
    /// They are at war: there must be a truce before an alliance.
    AtWar,
    /// They are allies already.
    AlreadyAllied,
    /// They are at war already.
    AlreadyAtWar,
    /// There is no pact between them to end.
    NoPact,
}

/// Every pact in the world, and every offer waiting for an answer.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pacts {
    /// Each pair of lands with a pact, lower number first.
    sealed: Vec<(usize, usize, Pact)>,
    /// Offers made and not yet answered: who offered, to whom, what.
    offers: Vec<(usize, usize, Dealing)>,
}

/// A pair of lands in the order pacts are kept in.
const fn pair(a: usize, b: usize) -> (usize, usize) {
    if a < b {
        (a, b)
    } else {
        (b, a)
    }
}

impl Pacts {
    /// A world in which no land has had dealings with another.
    pub const fn new() -> Self {
        Self { sealed: Vec::new(), offers: Vec::new() }
    }

    /// How lands `a` and `b` stand to each other, if they have a pact.
    pub fn between(&self, a: usize, b: usize) -> Option<Pact> {
        let (low, high) = pair(a, b);
        self.sealed.iter().find(|(first, second, _)| (*first, *second) == (low, high)).map(|(_, _, pact)| *pact)
    }

    /// Whether `from` has made `to` this offer and still waits on it.
    pub fn has_offered(&self, from: usize, to: usize, dealing: Dealing) -> bool {
        self.offers.contains(&(from, to, dealing))
    }

    /// How many pairs of lands have `pact`.
    pub fn count(&self, pact: Pact) -> usize {
        self.sealed.iter().filter(|(_, _, kind)| *kind == pact).count()
    }

    /// Every land that has `pact` with `land`.
    pub fn with(&self, land: usize, pact: Pact) -> impl Iterator<Item = usize> + '_ {
        self.sealed.iter().filter(move |(_, _, kind)| *kind == pact).filter_map(move |(first, second, _)| {
            if *first == land {
                Some(*second)
            } else if *second == land {
                Some(*first)
            } else {
                None
            }
        })
    }

    /// Land `from` deals with land `to`.
    ///
    /// # Errors
    /// Returns why the dealing came to nothing.
    pub fn deal(&mut self, from: usize, to: usize, dealing: Dealing) -> Result<Dealt, PactRefusal> {
        if from == to {
            return Err(PactRefusal::OwnLand);
        }
        match (dealing, self.between(from, to)) {
            (Dealing::Ally, Some(Pact::War)) => Err(PactRefusal::AtWar),
            (Dealing::Ally, Some(Pact::Alliance)) => Err(PactRefusal::AlreadyAllied),
            (Dealing::War, Some(Pact::War)) => Err(PactRefusal::AlreadyAtWar),
            (Dealing::Truce, None) => Err(PactRefusal::NoPact),
            (Dealing::War, _) => Ok(self.seal(from, to, Some(Pact::War), Dealt::AtWar)),
            (Dealing::Truce, Some(Pact::Alliance)) => Ok(self.seal(from, to, None, Dealt::Parted)),
            (Dealing::Ally, None) if self.answers(from, to, dealing) => {
                Ok(self.seal(from, to, Some(Pact::Alliance), Dealt::Allied))
            }
            (Dealing::Truce, Some(Pact::War)) if self.answers(from, to, dealing) => Ok(self.seal(from, to, None, Dealt::Peace)),
            (Dealing::Ally | Dealing::Truce, _) => {
                if !self.offers.contains(&(from, to, dealing)) {
                    self.offers.push((from, to, dealing));
                }
                Ok(Dealt::Offered)
            }
        }
    }

    /// Whether `to` has already made `from` the offer `from` is making.
    fn answers(&self, from: usize, to: usize, dealing: Dealing) -> bool {
        self.offers.contains(&(to, from, dealing))
    }

    /// Sets how two lands stand, forgetting whatever either had offered
    /// the other, and returns `dealt`.
    fn seal(&mut self, a: usize, b: usize, pact: Option<Pact>, dealt: Dealt) -> Dealt {
        let (low, high) = pair(a, b);
        self.sealed.retain(|(first, second, _)| (*first, *second) != (low, high));
        self.offers.retain(|(from, to, _)| pair(*from, *to) != (low, high));
        self.sealed.extend(pact.map(|pact| (low, high, pact)));
        dealt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lands_that_never_deal_have_no_pact() {
        let pacts = Pacts::new();
        assert_eq!(pacts.between(1, 2), None);
        assert_eq!(pacts.with(1, Pact::War).count(), 0);
    }

    #[test]
    fn an_alliance_takes_both_and_war_takes_one() {
        let mut pacts = Pacts::new();
        assert_eq!(pacts.deal(1, 1, Dealing::Ally), Err(PactRefusal::OwnLand));
        assert_eq!(pacts.deal(1, 2, Dealing::Ally), Ok(Dealt::Offered));
        assert_eq!(pacts.deal(1, 2, Dealing::Ally), Ok(Dealt::Offered), "asking twice is still one offer");
        assert_eq!(pacts.between(1, 2), None);
        assert_eq!(pacts.deal(2, 1, Dealing::Ally), Ok(Dealt::Allied));
        assert_eq!((pacts.between(1, 2), pacts.between(2, 1)), (Some(Pact::Alliance), Some(Pact::Alliance)));
        assert_eq!(pacts.deal(2, 1, Dealing::Ally), Err(PactRefusal::AlreadyAllied));

        // War breaks an alliance at a word.
        assert_eq!(pacts.deal(2, 1, Dealing::War), Ok(Dealt::AtWar));
        assert_eq!(pacts.deal(1, 2, Dealing::War), Err(PactRefusal::AlreadyAtWar));
        assert_eq!(pacts.deal(1, 2, Dealing::Ally), Err(PactRefusal::AtWar));
        assert_eq!(pacts.with(1, Pact::War).collect::<Vec<_>>(), [2]);
        assert_eq!(pacts.with(3, Pact::War).count(), 0);
    }

    #[test]
    fn a_truce_ends_a_war_only_when_both_ask_and_an_alliance_when_either_does() {
        let mut pacts = Pacts::new();
        assert_eq!(pacts.deal(1, 2, Dealing::Truce), Err(PactRefusal::NoPact));
        pacts.deal(1, 2, Dealing::War).unwrap();
        assert_eq!(pacts.deal(1, 2, Dealing::Truce), Ok(Dealt::Offered));
        assert_eq!(pacts.between(1, 2), Some(Pact::War));
        assert_eq!(pacts.deal(2, 1, Dealing::Truce), Ok(Dealt::Peace));
        assert_eq!(pacts.between(1, 2), None);

        pacts.deal(1, 2, Dealing::Ally).unwrap();
        pacts.deal(2, 1, Dealing::Ally).unwrap();
        assert_eq!(pacts.deal(1, 2, Dealing::Truce), Ok(Dealt::Parted));
        assert_eq!(pacts.between(1, 2), None);
    }

    #[test]
    fn an_old_offer_does_not_outlive_a_change_between_the_two() {
        let mut pacts = Pacts::new();
        pacts.deal(1, 2, Dealing::Ally).unwrap();
        pacts.deal(1, 2, Dealing::War).unwrap();
        pacts.deal(1, 2, Dealing::Truce).unwrap();
        pacts.deal(2, 1, Dealing::Truce).unwrap();
        assert_eq!(pacts.deal(2, 1, Dealing::Ally), Ok(Dealt::Offered), "the offer made before the war is gone");
    }
}
