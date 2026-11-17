//! What players ask their characters to do.

use alloc::string::String;

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

    /// `who` gathers from the deposit where they stand.
    pub const fn gather(who: CharacterId) -> Self {
        Self { who, action: Action::Gather }
    }
}
