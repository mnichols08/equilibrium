//! Playing on a server.
//!
//! The server holds the game. This end only sends the lines the player
//! types and prints the lines that come back, whenever they come: other
//! players act while you are reading.

use std::io::{self, BufRead, Write};
use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use eq_protocol::{ClientMessage, ServerMessage};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

/// How long one wait for the server lasts before the keyboard gets a turn.
const POLL: Duration = Duration::from_millis(50);

/// How long to go on listening after the input has ended, so the
/// answers to the last lines are not lost.
const LINGER: Duration = Duration::from_millis(600);

/// Who is playing, and where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Login {
    /// The server's address, such as `ws://127.0.0.1:4000`.
    pub url: String,
    /// The character's name.
    pub name: String,
    /// The passphrase that keeps the name yours.
    pub key: String,
}

type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

fn failed(problem: impl std::fmt::Display) -> io::Error {
    io::Error::other(problem.to_string())
}

/// Plays on a server until the input ends, the player quits, or the
/// server closes the connection.
pub fn play(login: &Login, input: impl BufRead + Send + 'static, mut out: impl Write) -> io::Result<()> {
    let (mut socket, _) = tungstenite::connect(&login.url).map_err(|error| failed(format!("could not reach {}: {error}", login.url)))?;
    if let MaybeTlsStream::Plain(stream) = socket.get_ref() {
        stream.set_read_timeout(Some(POLL))?;
    }
    socket.send(Message::text(ClientMessage::hello(&login.name, &login.key).to_json())).map_err(failed)?;

    let typed = lines_from(input);
    let mut input_ended_at = None;
    loop {
        match receive(&mut socket)? {
            Some(ServerMessage::Welcome { name, new, .. }) => {
                writeln!(out, "{}", if new { format!("Welcome, {name}.") } else { format!("Welcome back, {name}.") })?;
            }
            Some(ServerMessage::Lines { lines }) => {
                for line in lines {
                    writeln!(out, "{line}")?;
                }
                // Something is still coming back: keep listening a little longer.
                input_ended_at = input_ended_at.map(|_| Instant::now());
            }
            Some(ServerMessage::Refused { reason }) => {
                writeln!(out, "{reason}")?;
                return Ok(());
            }
            None => {}
        }
        out.flush()?;

        loop {
            match typed.try_recv() {
                Ok(line) if matches!(line.trim(), "quit" | "exit" | "q") => return leave(&mut socket, &mut out),
                Ok(line) => socket.send(Message::text(ClientMessage::line(&line).to_json())).map_err(failed)?,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    input_ended_at.get_or_insert_with(Instant::now);
                    break;
                }
            }
        }
        if input_ended_at.is_some_and(|ended| ended.elapsed() >= LINGER) {
            return leave(&mut socket, &mut out);
        }
    }
}

/// Reads the input on its own thread, so waiting for the keyboard never
/// holds up what the server has to say.
fn lines_from(input: impl BufRead + Send + 'static) -> Receiver<String> {
    let (send, typed) = mpsc::channel();
    std::thread::spawn(move || {
        for line in input.lines().map_while(Result::ok) {
            if send.send(line).is_err() {
                break;
            }
        }
    });
    typed
}

/// The next message from the server, or `None` if none came in time.
fn receive(socket: &mut Socket) -> io::Result<Option<ServerMessage>> {
    match socket.read() {
        Ok(Message::Text(text)) => ServerMessage::from_json(&text).map(Some).map_err(failed),
        Ok(Message::Close(_)) => Err(failed("the server closed the connection")),
        Ok(_) => Ok(None),
        Err(tungstenite::Error::Io(error)) if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => Ok(None),
        Err(error) => Err(failed(error)),
    }
}

fn leave(socket: &mut Socket, out: &mut impl Write) -> io::Result<()> {
    let _ = socket.close(None);
    writeln!(out, "You step away. Your character stays in the world, and will lie down to sleep in two minutes.")
}
