//! What players ask their characters to do.

use alloc::string::String;

use crate::character::CharacterId;

/// Something a character can do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Leave by the exit with this label.
    Go(String),
    /// Gather from the deposit here.
    Gather,
    /// Trade blows with someone in the same place.
    Attack(CharacterId),
    /// Pick up whatever lies dropped on the ground here.
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
}

/// An action and the character asked to take it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    /// Who acts.
    pub who: CharacterId,
    /// What they do.
    pub action: Action,
}

impl Command {
    /// `who` leaves by the exit labelled `label`.
    pub fn go(who: CharacterId, label: &str) -> Self {
        Self { who, action: Action::Go(label.into()) }
    }

    /// `who` trades blows with `target`.
    pub const fn attack(who: CharacterId, target: CharacterId) -> Self {
        Self { who, action: Action::Attack(target) }
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

    /// `who` gathers from the deposit where they stand.
    pub const fn gather(who: CharacterId) -> Self {
        Self { who, action: Action::Gather }
    }
}
