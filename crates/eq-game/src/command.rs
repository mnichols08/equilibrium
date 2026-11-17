//! What players ask their characters to do.

use alloc::string::String;

use crate::board::{ContractId, Lot};
use crate::character::CharacterId;
use crate::creature::CreatureId;

/// Something a character can do.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Action {
    /// Come into the world under this name. The command must be for the
    /// next free id ([`Game::next_id`](crate::Game::next_id)).
    Join(String),
    /// Leave by the exit with this label.
    Go(String),
    /// Gather from the deposit here.
    Gather,
    /// Trade blows with someone in the same place.
    Attack(CharacterId),
    /// Strike a creature in the same place.
    Fight(CreatureId),
    /// Open every relic here that will open, and take what is in them.
    Take,
    /// Settle down to sleep where you stand.
    Rest,
    /// Wake up, or get up again before sleep comes.
    Wake,
    /// Go through the pack of someone asleep in the same place.
    Rob(CharacterId),
    /// As a spirit, go back into your body.
    Return,
    /// As a spirit, give up your body and be reborn.
    Relinquish,
    /// As a forgotten spirit, search this place for a memory of yourself.
    Seek,
    /// At the shrine, speak the name of a forgotten spirit to redeem it.
    Name(CharacterId),
    /// As a founder, give your land its name. It keeps it for good.
    NameLand(String),
    /// As a founder, give your house its name. It keeps it for good.
    NameHouse(String),
    /// As a founder, invite someone by name. Whoever joins the world
    /// under that name arrives in your land, as one of your house.
    Invite(String),
    /// Take part in a Restoration Rite over the deposit here. One
    /// person begins it; a second, answering in time, completes it.
    Rite,
    /// Tend someone asleep in the same place, so they can sleep longer
    /// before the long sleep takes them.
    Tend(CharacterId),
    /// At the board, pin up an offer: `give`, for `want`.
    Post {
        /// What is offered. It leaves the pack at once.
        give: Lot,
        /// What is asked for it.
        want: Lot,
    },
    /// At the board, pay a contract's price and take what it offers.
    Accept(ContractId),
    /// At the board, take down a contract of your own and have back
    /// what it held.
    Withdraw(ContractId),
    /// At the board, take what your accepted contracts earned.
    Collect,
}

/// An action and the character asked to take it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Command {
    /// Who acts.
    pub who: CharacterId,
    /// What they do.
    pub action: Action,
}

impl Command {
    /// A newcomer called `name` comes into the world as `who`, which
    /// must be the world's next free id.
    pub fn join(who: CharacterId, name: &str) -> Self {
        Self { who, action: Action::Join(name.into()) }
    }

    /// `who` leaves by the exit labelled `label`.
    pub fn go(who: CharacterId, label: &str) -> Self {
        Self { who, action: Action::Go(label.into()) }
    }

    /// `who` trades blows with `target`.
    pub const fn attack(who: CharacterId, target: CharacterId) -> Self {
        Self { who, action: Action::Attack(target) }
    }

    /// `who` strikes `creature`.
    pub const fn fight(who: CharacterId, creature: CreatureId) -> Self {
        Self { who, action: Action::Fight(creature) }
    }

    /// `who` picks up what lies on the ground where they stand.
    pub const fn take(who: CharacterId) -> Self {
        Self { who, action: Action::Take }
    }

    /// `who` settles down to sleep.
    pub const fn rest(who: CharacterId) -> Self {
        Self { who, action: Action::Rest }
    }

    /// `who` wakes up.
    pub const fn wake(who: CharacterId) -> Self {
        Self { who, action: Action::Wake }
    }

    /// `who` goes through `target`'s pack as they sleep.
    pub const fn rob(who: CharacterId, target: CharacterId) -> Self {
        Self { who, action: Action::Rob(target) }
    }

    /// `who`, a spirit, goes back into their body.
    pub const fn return_to_body(who: CharacterId) -> Self {
        Self { who, action: Action::Return }
    }

    /// `who`, a spirit, gives up their body and is reborn.
    pub const fn relinquish(who: CharacterId) -> Self {
        Self { who, action: Action::Relinquish }
    }

    /// `who`, a forgotten spirit, searches where they stand for a memory.
    pub const fn seek(who: CharacterId) -> Self {
        Self { who, action: Action::Seek }
    }

    /// `who` speaks `target`'s name at the shrine.
    pub const fn name(who: CharacterId, target: CharacterId) -> Self {
        Self { who, action: Action::Name(target) }
    }

    /// `who`, a founder, names their land.
    pub fn name_land(who: CharacterId, name: &str) -> Self {
        Self { who, action: Action::NameLand(name.into()) }
    }

    /// `who`, a founder, names their house.
    pub fn name_house(who: CharacterId, name: &str) -> Self {
        Self { who, action: Action::NameHouse(name.into()) }
    }

    /// `who`, a founder, invites whoever will join as `name`.
    pub fn invite(who: CharacterId, name: &str) -> Self {
        Self { who, action: Action::Invite(name.into()) }
    }

    /// `who` takes part in a rite where they stand.
    pub const fn rite(who: CharacterId) -> Self {
        Self { who, action: Action::Rite }
    }

    /// `who` tends `target` as they sleep.
    pub const fn tend(who: CharacterId, target: CharacterId) -> Self {
        Self { who, action: Action::Tend(target) }
    }

    /// `who` pins up an offer of `give` for `want`.
    pub const fn post(who: CharacterId, give: Lot, want: Lot) -> Self {
        Self { who, action: Action::Post { give, want } }
    }

    /// `who` accepts contract `id`.
    pub const fn accept(who: CharacterId, id: ContractId) -> Self {
        Self { who, action: Action::Accept(id) }
    }

    /// `who` takes down their own contract `id`.
    pub const fn withdraw(who: CharacterId, id: ContractId) -> Self {
        Self { who, action: Action::Withdraw(id) }
    }

    /// `who` collects what the board holds for them.
    pub const fn collect(who: CharacterId) -> Self {
        Self { who, action: Action::Collect }
    }

    /// `who` gathers from the deposit where they stand.
    pub const fn gather(who: CharacterId) -> Self {
        Self { who, action: Action::Gather }
    }
}
