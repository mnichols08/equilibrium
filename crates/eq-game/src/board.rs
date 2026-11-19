//! The Contract Board: trades between people who need not meet.
//!
//! Someone pins a contract to the board: this much of one thing, for
//! that much of another. What they offer leaves their pack at once and
//! is held by the board, so the offer is always good. Whoever accepts
//! pays the asking price and takes what was held; the price waits at
//! the board for the one who posted to collect.
//!
//! Nothing here touches the world. The rules in `game` decide who is
//! standing at the board and what is in whose pack.

use alloc::vec::Vec;

use crate::character::CharacterId;
use crate::content::MOST_CONTRACTS_EACH;
use crate::types::Resource;

eq_ids::define_id!(
    /// Identifies a contract on one [`Board`]. Ids are never reused.
    ContractId
);

#[cfg(feature = "serde")]
crate::wire::as_number!(ContractId);

/// Some amount of one resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Lot {
    /// What.
    pub resource: Resource,
    /// How much.
    pub amount: u64,
}

/// An offer pinned to the board.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Contract {
    /// Its number on the board.
    pub id: ContractId,
    /// Who posted it.
    pub by: CharacterId,
    /// What the board holds for whoever accepts.
    pub give: Lot,
    /// What accepting costs.
    pub want: Lot,
}

/// Why a trade at the board was not allowed.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TradeRefusal {
    /// The board is at the zone's anchor, and they are not.
    NotAtBoard,
    /// There is no contract with that number.
    NoSuchContract,
    /// They do not carry enough to post or to pay.
    CannotPay,
    /// The contract is their own.
    YourOwn,
    /// The contract is someone else's.
    NotYours,
    /// They already have as many contracts up as one person may.
    TooMany,
    /// A contract must offer something and ask something.
    Empty,
    /// The board holds nothing for them.
    NothingOwed,
    /// Nobody puts a price on their own head.
    OnYourself,
    /// They have asked for an escort already.
    AlreadyHiring,
    /// That person has not asked for an escort, or has one.
    NotHiring,
    /// No land goes by that name. (The Meeting Ground is "ground".)
    NoSuchPlace,
    /// A bounty has to pay at least this much, and more on a newcomer.
    TooLittle(u64),
}

/// A price on someone's head. The board holds the reward for whoever
/// kills them.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounty {
    /// Whose death it pays for.
    pub on: CharacterId,
    /// Who put it up.
    pub by: CharacterId,
    /// What it pays.
    pub reward: Lot,
}

/// Every open contract, and what the board holds for those whose
/// contracts were accepted.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Board {
    contracts: Vec<Contract>,
    bounties: Vec<Bounty>,
    next: u64,
    /// What is waiting to be collected, by who it is for.
    owed: Vec<(CharacterId, [u64; Resource::ALL.len()])>,
}

impl Board {
    /// An empty board.
    pub const fn new() -> Self {
        Self { contracts: Vec::new(), bounties: Vec::new(), next: 0, owed: Vec::new() }
    }

    /// The open contracts, oldest first.
    pub fn contracts(&self) -> &[Contract] {
        &self.contracts
    }

    /// The prices on people's heads, oldest first.
    pub fn bounties(&self) -> &[Bounty] {
        &self.bounties
    }

    /// Puts a price on someone's head. The caller has already taken the
    /// reward from the poster's pack.
    pub(crate) fn post_bounty(&mut self, bounty: Bounty) {
        self.bounties.push(bounty);
    }

    /// Pays every bounty on `on` to `killer`: the rewards wait here for
    /// them to collect. Returns what was paid.
    pub(crate) fn pay_bounties(&mut self, on: CharacterId, killer: CharacterId) -> Vec<Lot> {
        let (paid, kept): (Vec<Bounty>, Vec<Bounty>) = self.bounties.iter().partition(|bounty| bounty.on == on);
        self.bounties = kept;
        let rewards: Vec<Lot> = paid.iter().map(|bounty| bounty.reward).collect();
        for &reward in &rewards {
            self.hold_for(killer, reward);
        }
        rewards
    }

    /// One open contract.
    pub fn contract(&self, id: ContractId) -> Option<&Contract> {
        self.contracts.iter().find(|contract| contract.id == id)
    }

    /// What the board holds for `who`, by resource in pack order.
    pub fn owed_to(&self, who: CharacterId) -> [u64; Resource::ALL.len()] {
        self.owed.iter().find(|(owner, _)| *owner == who).map_or([0; Resource::ALL.len()], |(_, pile)| *pile)
    }

    /// Whether `by` may pin up a contract of `give` for `want`.
    pub fn may_post(&self, by: CharacterId, give: Lot, want: Lot) -> Result<(), TradeRefusal> {
        if give.amount == 0 || want.amount == 0 {
            return Err(TradeRefusal::Empty);
        }
        if self.contracts.iter().filter(|contract| contract.by == by).count() >= MOST_CONTRACTS_EACH {
            return Err(TradeRefusal::TooMany);
        }
        Ok(())
    }

    /// Pins up a contract. The caller has already taken `give` from the
    /// poster's pack.
    pub(crate) fn post(&mut self, by: CharacterId, give: Lot, want: Lot) -> ContractId {
        let id = ContractId::new(self.next);
        self.next += 1;
        self.contracts.push(Contract { id, by, give, want });
        id
    }

    /// Takes a contract down and returns it.
    pub(crate) fn take_down(&mut self, id: ContractId) -> Option<Contract> {
        let slot = self.contracts.iter().position(|contract| contract.id == id)?;
        Some(self.contracts.remove(slot))
    }

    /// Holds `lot` for `who` to collect.
    pub(crate) fn hold_for(&mut self, who: CharacterId, lot: Lot) {
        let slot = match self.owed.iter().position(|(owner, _)| *owner == who) {
            Some(slot) => slot,
            None => {
                self.owed.push((who, [0; Resource::ALL.len()]));
                self.owed.len() - 1
            }
        };
        self.owed[slot].1[lot.resource.index()] += lot.amount;
    }

    /// Hands over everything held for `who`.
    pub(crate) fn collect(&mut self, who: CharacterId) -> [u64; Resource::ALL.len()] {
        match self.owed.iter().position(|(owner, _)| *owner == who) {
            Some(slot) => self.owed.remove(slot).1,
            None => [0; Resource::ALL.len()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASH: CharacterId = CharacterId::new(0);
    const BRYN: CharacterId = CharacterId::new(1);

    const fn lot(resource: Resource, amount: u64) -> Lot {
        Lot { resource, amount }
    }

    #[test]
    fn contracts_are_numbered_in_order_and_never_renumbered() {
        let mut board = Board::new();
        let first = board.post(ASH, lot(Resource::Ore, 10), lot(Resource::Kelp, 30));
        let second = board.post(BRYN, lot(Resource::Kelp, 5), lot(Resource::Ore, 1));
        assert_eq!((first.get(), second.get()), (0, 1));
        assert_eq!(board.take_down(first).map(|contract| contract.by), Some(ASH));
        assert_eq!(board.take_down(first), None);
        assert_eq!(board.post(ASH, lot(Resource::Ore, 1), lot(Resource::Kelp, 1)).get(), 2);
        assert_eq!(board.contracts().iter().map(|contract| contract.id.get()).collect::<Vec<_>>(), [1, 2]);
    }

    #[test]
    fn a_contract_must_offer_and_ask_something_and_each_person_has_a_limit() {
        let mut board = Board::new();
        let (ore, kelp) = (lot(Resource::Ore, 1), lot(Resource::Kelp, 1));
        assert_eq!(board.may_post(ASH, lot(Resource::Ore, 0), kelp), Err(TradeRefusal::Empty));
        assert_eq!(board.may_post(ASH, ore, lot(Resource::Kelp, 0)), Err(TradeRefusal::Empty));
        for _ in 0..MOST_CONTRACTS_EACH {
            assert_eq!(board.may_post(ASH, ore, kelp), Ok(()));
            board.post(ASH, ore, kelp);
        }
        assert_eq!(board.may_post(ASH, ore, kelp), Err(TradeRefusal::TooMany));
        assert_eq!(board.may_post(BRYN, ore, kelp), Ok(()));
    }

    #[test]
    fn what_is_held_for_someone_adds_up_and_is_handed_over_once() {
        let mut board = Board::new();
        board.hold_for(ASH, lot(Resource::Kelp, 30));
        board.hold_for(ASH, lot(Resource::Kelp, 12));
        board.hold_for(ASH, lot(Resource::Ore, 2));
        assert_eq!(board.owed_to(ASH), [2, 0, 42, 0]);
        assert_eq!(board.owed_to(BRYN), [0; 4]);
        assert_eq!(board.collect(ASH), [2, 0, 42, 0]);
        assert_eq!(board.collect(ASH), [0; 4]);
    }
}
