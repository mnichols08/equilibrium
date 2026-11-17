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

    /// `who` gathers from the deposit where they stand.
    pub const fn gather(who: CharacterId) -> Self {
        Self { who, action: Action::Gather }
    }
}
