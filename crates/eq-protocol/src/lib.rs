//! What a server and its clients say to each other.
//!
//! Every message is one JSON object with a `type`. The first thing a
//! client sends is [`ClientMessage::Hello`], carrying [`VERSION`]; a
//! server that speaks another version refuses it, so a stale client
//! fails at the door and not halfway through a game.
//!
//! The protocol carries lines of text in both directions. The server
//! holds the rules and does the narrating; a client only reads lines
//! and shows what comes back. A client that can draw may also ask for
//! scenes: what its character can see, as data.
//!
//! ```
//! use eq_protocol::{ClientMessage, ServerMessage};
//!
//! let hello = ClientMessage::hello("Mikey", "a passphrase");
//! let wire = hello.to_json();
//! assert_eq!(ClientMessage::from_json(&wire), Ok(hello));
//!
//! let reply = ServerMessage::Lines { lines: vec!["You gather 12 kelp.".into()] };
//! assert_eq!(reply.to_json(), r#"{"type":"lines","lines":["You gather 12 kelp."]}"#);
//! ```

use serde::{Deserialize, Serialize};

/// The protocol version this crate speaks.
pub const VERSION: u32 = 1;

/// The longest line of input a server will read, in bytes.
pub const MAX_LINE: usize = 200;

/// Something a client sends.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// The first message: who is playing.
    Hello {
        /// The protocol version the client speaks.
        version: u32,
        /// The character's name. A new name makes a new character.
        name: String,
        /// A passphrase. The first to use a name sets it; after that the
        /// name only answers to the same passphrase.
        key: String,
        /// Whether the client wants [`ServerMessage::Scene`]s to draw.
        /// Left out, it is `false`: a terminal only wants lines.
        #[serde(default)]
        scenes: bool,
    },
    /// One line typed by the player, such as `north` or `attack bryn`.
    Line {
        /// The line.
        text: String,
    },
}

/// Something a server sends.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// The hello was accepted.
    Welcome {
        /// The protocol version the server speaks.
        version: u32,
        /// The character's name as the world has it.
        name: String,
        /// Whether this hello made the character.
        new: bool,
    },
    /// The hello was refused, or the connection is being closed. Nothing
    /// follows.
    Refused {
        /// Why, in words fit to show the player.
        reason: String,
    },
    /// Lines to show the player, in order.
    Lines {
        /// The lines.
        lines: Vec<String>,
    },
    /// What the player's character can see now, for clients that asked
    /// for scenes. The protocol does not say what is in one: that is
    /// between the game and whatever draws it.
    Scene {
        /// The scene.
        scene: serde_json::Value,
    },
}

/// Why some text was not a message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotAMessage(pub String);

impl std::fmt::Display for NotAMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "not a message: {}", self.0)
    }
}

impl std::error::Error for NotAMessage {}

impl ClientMessage {
    /// A hello in this crate's protocol version, from a client that
    /// only wants lines.
    pub fn hello(name: &str, key: &str) -> Self {
        Self::Hello { version: VERSION, name: name.into(), key: key.into(), scenes: false }
    }

    /// One typed line.
    pub fn line(text: &str) -> Self {
        Self::Line { text: text.into() }
    }

    /// The message as JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a message holds only plain data")
    }

    /// Reads a message from JSON.
    pub fn from_json(text: &str) -> Result<Self, NotAMessage> {
        serde_json::from_str(text).map_err(|error| NotAMessage(error.to_string()))
    }
}

impl ServerMessage {
    /// Lines to show, from anything that yields strings.
    pub fn lines(lines: impl IntoIterator<Item = String>) -> Self {
        Self::Lines { lines: lines.into_iter().collect() }
    }

    /// The message as JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a message holds only plain data")
    }

    /// Reads a message from JSON.
    pub fn from_json(text: &str) -> Result<Self, NotAMessage> {
        serde_json::from_str(text).map_err(|error| NotAMessage(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_message_survives_the_wire() {
        let from_client = [ClientMessage::hello("Ash", "secret"), ClientMessage::line("go north")];
        for message in from_client {
            assert_eq!(ClientMessage::from_json(&message.to_json()), Ok(message));
        }
        let from_server = [
            ServerMessage::Welcome { version: VERSION, name: "Ash".into(), new: true },
            ServerMessage::Refused { reason: "That name is taken.".into() },
            ServerMessage::lines(["one".to_string(), "two".to_string()]),
        ];
        for message in from_server {
            assert_eq!(ServerMessage::from_json(&message.to_json()), Ok(message));
        }
    }

    #[test]
    fn the_wire_format_is_plain_and_stable() {
        assert_eq!(
            ClientMessage::hello("Ash", "k").to_json(),
            r#"{"type":"hello","version":1,"name":"Ash","key":"k","scenes":false}"#
        );
        // A client that does not mention scenes gets none.
        let plain = ClientMessage::from_json(r#"{"type":"hello","version":1,"name":"Ash","key":"k"}"#);
        assert_eq!(plain, Ok(ClientMessage::hello("Ash", "k")));
        let scene = ServerMessage::Scene { scene: serde_json::json!({ "clock": "0:00:05" }) };
        assert_eq!(scene.to_json(), r#"{"type":"scene","scene":{"clock":"0:00:05"}}"#);
        assert_eq!(ServerMessage::from_json(&scene.to_json()), Ok(scene));
        assert_eq!(ClientMessage::line("look").to_json(), r#"{"type":"line","text":"look"}"#);
    }

    #[test]
    fn nonsense_is_refused_with_a_reason() {
        assert!(ClientMessage::from_json("hello").is_err());
        assert!(ClientMessage::from_json(r#"{"type":"dance"}"#).is_err());
        let problem = ServerMessage::from_json("{}").unwrap_err();
        assert!(problem.to_string().starts_with("not a message: "));
    }
}
