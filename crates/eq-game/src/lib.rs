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

pub mod balance;
pub mod bloodline;
pub mod board;
pub mod bots;
mod character;
pub mod combat;
mod command;
mod creature;
pub mod content;
mod event;
mod game;
mod haul;
pub mod narrate;
mod occupancy;
pub mod era;
pub mod escort;
pub mod pact;
pub mod parser;
pub mod rest;
pub mod seat;
mod solo;
pub mod spirit;
pub mod territory;
mod types;
#[cfg(feature = "serde")]
mod wire;
mod zone;

pub use board::{Board, Bounty, Contract, ContractId, Lot, TradeRefusal};
pub use character::{Character, CharacterId, Death, Forgotten};
pub use command::{Action, Command};
pub use creature::{Being, Creature, CreatureId, CreatureKind, Grave, Relic, RelicTier};
pub use event::{Event, Outcome};
pub use game::Game;
pub use haul::{pressure_percent, Haul};
pub use solo::{Reply, Solo};
pub use types::{Attribute, Cause, Deposit, Health, Passing, Place, Resource, Safety, SkillKind, Standing};
pub use zone::Zone;
