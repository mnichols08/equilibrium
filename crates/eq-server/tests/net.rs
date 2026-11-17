//! The server over real WebSockets on this machine.

mod common;

use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use common::{unsaved, Scratch};
use eq_protocol::{ClientMessage, ServerMessage, VERSION};
use eq_server::net::Server;
use eq_server::Realm;
use tokio::sync::oneshot;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

/// A server on a free port, ticking every five milliseconds, on its
/// own thread. Stops and saves when dropped.
struct Running {
    address: SocketAddr,
    stop: Option<oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Running {
    fn start(realm: Realm) -> Self {
        let (ready, address) = std::sync::mpsc::channel();
        let (stop, stopped) = oneshot::channel();
        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
            runtime.block_on(async {
                let server = Server::bind(realm, "127.0.0.1:0", Duration::from_millis(5)).await.unwrap();
                ready.send(server.address()).unwrap();
                server
                    .run(async {
                        let _ = stopped.await;
                    })
                    .await;
            });
        });
        Self { address: address.recv().unwrap(), stop: Some(stop), thread: Some(thread) }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.stop.take().map(|stop| stop.send(()));
        let _ = self.thread.take().map(std::thread::JoinHandle::join);
    }
}

/// A player's end of a connection.
struct Client(WebSocket<MaybeTlsStream<TcpStream>>);

impl Client {
    fn connect(server: &Running) -> Self {
        let (socket, _) = tungstenite::connect(format!("ws://{}", server.address)).unwrap();
        if let MaybeTlsStream::Plain(stream) = socket.get_ref() {
            stream.set_read_timeout(Some(Duration::from_millis(20))).unwrap();
        }
        Self(socket)
    }

    fn send(&mut self, message: &ClientMessage) {
        self.0.send(Message::text(message.to_json())).unwrap();
    }

    /// The next message, or `None` if nothing comes within the timeout.
    fn next(&mut self) -> Option<ServerMessage> {
        match self.0.read() {
            Ok(Message::Text(text)) => Some(ServerMessage::from_json(&text).unwrap()),
            _ => None,
        }
    }

    /// Reads until some line contains `wanted`. Returns everything read.
    fn until(&mut self, wanted: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut seen = String::new();
        while Instant::now() < deadline {
            if let Some(ServerMessage::Lines { lines }) = self.next() {
                seen.push_str(&lines.join("\n"));
                seen.push('\n');
                if seen.contains(wanted) {
                    return seen;
                }
            }
        }
        panic!("never saw {wanted:?}; saw:\n{seen}");
    }

    fn joined(server: &Running, name: &str, key: &str) -> Self {
        let mut client = Self::connect(server);
        client.send(&ClientMessage::hello(name, key));
        assert!(matches!(client.next(), Some(ServerMessage::Welcome { version: VERSION, .. })), "{name} was not welcomed");
        client
    }
}

#[test]
fn two_players_share_a_world_over_websockets() {
    let server = Running::start(unsaved(1, 0));
    let mut ada = Client::joined(&server, "Ada", "a");
    ada.until("Tidewatch Camp [haven]");
    let mut ben = Client::joined(&server, "Ben", "b");
    ada.until("Ben walks out of the tide");
    assert!(ben.until("Exits:").contains("Also here: Ada."));

    ada.send(&ClientMessage::line("north"));
    assert!(ada.until("Exits:").contains("Ore Ledge [wilderness]"));
    ben.until("Ada leaves for Ore Ledge.");

    // World time runs by itself: thirty ticks later Ada can work.
    std::thread::sleep(Duration::from_millis(400));
    ada.send(&ClientMessage::line("gather"));
    ada.until("You gather ");
    ada.send(&ClientMessage::line("pack"));
    ada.until("You carry: ");
}

#[test]
fn the_door_refuses_old_clients_wrong_keys_and_anything_that_is_not_a_hello() {
    let server = Running::start(unsaved(1, 0));
    let mut ada = Client::joined(&server, "Ada", "right");

    let mut stale = Client::connect(&server);
    stale.send(&ClientMessage::Hello { version: VERSION + 1, name: "Old".into(), key: "k".into(), scenes: false });
    assert!(matches!(stale.next(), Some(ServerMessage::Refused { reason }) if reason.contains("protocol version")));

    let mut thief = Client::connect(&server);
    thief.send(&ClientMessage::hello("Ada", "wrong"));
    assert!(matches!(thief.next(), Some(ServerMessage::Refused { reason }) if reason.contains("passphrase")));

    let mut rude = Client::connect(&server);
    rude.send(&ClientMessage::line("look"));
    assert!(matches!(rude.next(), Some(ServerMessage::Refused { reason }) if reason.contains("hello")));

    // The right key from a second place takes the seat over.
    let mut elsewhere = Client::joined(&server, "Ada", "right");
    elsewhere.until("Tidewatch Camp");
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut told = false;
    while !told && Instant::now() < deadline {
        told = matches!(ada.next(), Some(ServerMessage::Refused { reason }) if reason.contains("somewhere else"));
    }
    assert!(told, "the first connection should be told it was replaced");
}

#[test]
fn a_stopped_server_comes_back_with_everyone_where_they_were() {
    let scratch = Scratch::new("net-restart");
    let server = Running::start(scratch.realm(3, 2));
    let mut ada = Client::joined(&server, "Ada", "a");
    ada.send(&ClientMessage::line("east"));
    ada.until("Shingle Beach [wilderness]");
    drop(ada);
    drop(server);

    let server = Running::start(scratch.realm(3, 2));
    let mut ada = Client::joined(&server, "Ada", "a");
    assert!(ada.until("Exits:").contains("Shingle Beach [wilderness]"));
}
