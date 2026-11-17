//! The first playable build of Equilibrium: characters sharing the Rocky
//! Shoreline.
//!
//! It exists to test one claim from the design: that systems feed each
//! other. Gathering trains a skill and an attribute; the attribute raises
//! yields; taking too much wears the deposit and drags down the zone's
//! vitality; low vitality slows regrowth and cuts yields. Whoever does the
//! over-harvesting is tainted for it: their own yields fall and the land
//! marks them.
//!
//! - [`content`]: the map and every tuning number.
//! - [`Game`]: the rules, as an [`eq_events::World`].
//! - [`parser`]: text to commands.
//! - [`narrate`]: events and state to text.
//!
//! The crate does no I/O. `eq-cli` wraps it in a terminal.
#![no_std]

extern crate alloc;

pub mod content;
mod game;
pub mod narrate;
pub mod parser;
mod types;

pub use game::{pressure_percent, Action, Character, CharacterId, Command, Event, Game, Haul, Outcome};
pub use types::{Attribute, Deposit, Health, Place, Resource, Safety, SkillKind, Standing};
