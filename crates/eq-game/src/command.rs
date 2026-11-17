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

    /// `who` gathers from the deposit where they stand.
    pub const fn gather(who: CharacterId) -> Self {
        Self { who, action: Action::Gather }
    }
}
