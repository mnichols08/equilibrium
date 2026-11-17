//! The Equilibrium server: one world, many players.
//!
//! - [`Realm`]: the world, its saved command log and its players, with
//!   no network in it. Everything the server does to the game goes
//!   through here, so it can all be tested without a socket.
//! - [`net`]: WebSockets in front of a realm.
//!
//! The server holds the rules and does the narrating. A client sends
//! the lines its player types and shows the lines that come back
//! (`eq-protocol`).

pub mod net;
mod realm;

pub use realm::{Config, Delivery, Entered, Realm, RealmError, Refusal};
