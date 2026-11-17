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
//! How the crate is laid out:
//!
//! - [`content`]: the map and every tuning number.
//! - [`Command`] and [`Event`]: what goes in and what comes out.
//! - [`Game`]: the rules, as an [`eq_events::World`]. It holds a [`Zone`]
//!   (map, deposits, vitality) and the [`Character`]s in it.
//! - [`Haul`]: the sums behind a harvest.
//! - [`combat`]: who may be attacked, and how hard a blow lands.
//! - [`parser`]: text to intent.
//! - [`narrate`]: events and state to text, for one viewer.
//! - [`bots`]: simple characters that play by themselves.
//! - [`Solo`]: one player at the controls; what every single-player client
//!   wraps.
//!
//! [`Game::audience`] says who should be told about an event.
//!
//! The crate does no I/O. `eq-cli` wraps it in a terminal.
#![no_std]

extern crate alloc;

pub mod bots;
mod character;
pub mod combat;
mod command;
pub mod content;
mod event;
mod game;
mod haul;
pub mod narrate;
mod occupancy;
pub mod parser;
mod solo;
mod types;
mod zone;

pub use character::{Character, CharacterId};
pub use command::{Action, Command};
pub use event::{Event, Outcome};
pub use game::Game;
pub use haul::{pressure_percent, Haul};
pub use solo::{Reply, Solo};
pub use types::{Attribute, Deposit, Health, Place, Resource, Safety, SkillKind, Standing};
pub use zone::Zone;
