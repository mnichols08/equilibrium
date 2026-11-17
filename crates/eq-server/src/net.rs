//! WebSockets in front of a realm.
//!
//! One task per connection, one task for the clock, and one lock around
//! the realm and the table of who is connected. Nothing here knows any
//! rule of the game: it reads [`ClientMessage`]s, hands lines to the
//! [`Realm`], and sends what comes back.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use eq_game::CharacterId;
use eq_protocol::{ClientMessage, ServerMessage, MAX_LINE, VERSION};
use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio_tungstenite::tungstenite::Message;

use crate::realm::{Delivery, Realm};

/// A connected player's outbox, and which connection it belongs to.
struct Seat {
    connection: u64,
    outbox: UnboundedSender<ServerMessage>,
}

/// The realm and everyone connected to it, behind one lock.
struct Shared {
    realm: Realm,
    seats: HashMap<CharacterId, Seat>,
    next_connection: u64,
}

impl Shared {
    /// Sends each delivery to its player, if they are connected.
    fn route(&self, deliveries: Vec<Delivery>) {
        for delivery in deliveries {
            if let Some(seat) = self.seats.get(&delivery.to) {
                // A closed outbox means the connection is already going away.
                let _ = seat.outbox.send(ServerMessage::Lines { lines: delivery.lines });
            }
        }
    }
}

/// A running server.
pub struct Server {
    shared: Arc<Mutex<Shared>>,
    address: SocketAddr,
    listener: TcpListener,
    tick_every: Duration,
}

impl Server {
    /// Binds `address` (use port 0 for any free port) in front of `realm`.
    /// One tick of world time passes every `tick_every`.
    pub async fn bind(realm: Realm, address: &str, tick_every: Duration) -> std::io::Result<Self> {
        let listener = TcpListener::bind(address).await?;
        let address = listener.local_addr()?;
        let shared = Arc::new(Mutex::new(Shared { realm, seats: HashMap::new(), next_connection: 0 }));
        Ok(Self { shared, address, listener, tick_every })
    }

    /// The address the server is listening on.
    pub const fn address(&self) -> SocketAddr {
        self.address
    }

    /// Serves until `stop` resolves, then saves the realm.
    pub async fn run(self, stop: impl std::future::Future<Output = ()>) {
        let clock = tokio::spawn(keep_time(Arc::clone(&self.shared), self.tick_every));
        let doors = tokio::spawn(accept(self.listener, Arc::clone(&self.shared)));
        stop.await;
        clock.abort();
        doors.abort();
        lock(&self.shared).realm.close();
    }
}

/// The lock is only ever held for one short, non-blocking step, so a
/// poisoned lock means a bug already panicked a task; carry on with
/// the data as it is so the realm can still be saved.
fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

async fn keep_time(shared: Arc<Mutex<Shared>>, tick_every: Duration) {
    let mut clock = tokio::time::interval(tick_every);
    clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        clock.tick().await;
        let mut shared = lock(&shared);
        let deliveries = shared.realm.tick();
        shared.route(deliveries);
    }
}

async fn accept(listener: TcpListener, shared: Arc<Mutex<Shared>>) {
    loop {
        if let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(serve(stream, Arc::clone(&shared)));
        }
    }
}

/// One connection, from hello to goodbye.
async fn serve(stream: TcpStream, shared: Arc<Mutex<Shared>>) {
    let Ok(socket) = tokio_tungstenite::accept_async(stream).await else { return };
    let (mut to_client, mut from_client) = socket.split();
    let (outbox, mut inbox) = unbounded_channel();

    let hello = match from_client.next().await {
        Some(Ok(Message::Text(text))) => ClientMessage::from_json(&text).ok(),
        _ => None,
    };
    let (who, connection) = match admit(&shared, hello, outbox) {
        Ok(admitted) => admitted,
        Err(reason) => {
            let _ = to_client.send(Message::text(ServerMessage::Refused { reason }.to_json())).await;
            return;
        }
    };

    loop {
        tokio::select! {
            outgoing = inbox.recv() => {
                let Some(message) = outgoing else { break };
                let last = matches!(message, ServerMessage::Refused { .. });
                if to_client.send(Message::text(message.to_json())).await.is_err() || last {
                    break;
                }
            }
            incoming = from_client.next() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    if let Ok(ClientMessage::Line { text }) = ClientMessage::from_json(&text) {
                        let mut shared = lock(&shared);
                        let deliveries = shared.realm.line(who, clip(&text));
                        shared.route(deliveries);
                    }
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
        }
    }

    // Only the connection that still holds the seat gives it up: a player
    // who connected again from elsewhere has already taken it over.
    let mut shared = lock(&shared);
    if shared.seats.get(&who).is_some_and(|seat| seat.connection == connection) {
        shared.seats.remove(&who);
        shared.realm.leave(who);
    }
}

/// Lets a connection in, if its first message was a fair hello.
fn admit(shared: &Mutex<Shared>, hello: Option<ClientMessage>, outbox: UnboundedSender<ServerMessage>) -> Result<(CharacterId, u64), String> {
    let Some(ClientMessage::Hello { version, name, key }) = hello else {
        return Err("The first message must be a hello.".into());
    };
    if version != VERSION {
        return Err(format!("This server speaks protocol version {VERSION}; you sent {version}. Update your client."));
    }
    let mut shared = lock(shared);
    let entered = shared.realm.enter(&name, &key).map_err(|refusal| refusal.reason().to_owned())?;
    let connection = shared.next_connection;
    shared.next_connection += 1;
    let _ = outbox.send(ServerMessage::Welcome { version: VERSION, name: entered.name, new: entered.new });
    if let Some(old) = shared.seats.insert(entered.who, Seat { connection, outbox }) {
        let _ = old.outbox.send(ServerMessage::Refused { reason: "You have connected from somewhere else.".into() });
    }
    shared.route(entered.deliveries);
    Ok((entered.who, connection))
}

/// At most [`MAX_LINE`] bytes of `text`, cut at a character boundary.
fn clip(text: &str) -> &str {
    let mut end = text.len().min(MAX_LINE);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_lines_are_cut_without_splitting_a_character() {
        assert_eq!(clip("north"), "north");
        assert_eq!(clip(&"a".repeat(500)).len(), MAX_LINE);
        let accented = "é".repeat(150);
        assert_eq!(clip(&accented).len(), MAX_LINE);
        assert!(clip(&format!("a{accented}")).len() < MAX_LINE);
    }
}
