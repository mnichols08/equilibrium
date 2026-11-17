//! Play Equilibrium in a terminal.
//!
//! ```text
//! equilibrium [--seed N] [--verify]
//! ```
//!
//! `--verify` replays the session's command log when you quit and checks
//! that it rebuilds exactly the same world.

use std::io::{self, BufRead, IsTerminal, Write};
use std::process::ExitCode;

use eq_clock::Duration;
use eq_events::{replay, Events, OutOfOrder, Session};
use eq_game::parser::{parse, Input};
use eq_game::{narrate, Command, Event, Game};

const USAGE: &str = "usage: equilibrium [--seed N] [--verify]";

const INTRO: &str = "\
EQUILIBRIUM
The shoreline gives what you take from it, and remembers how you took it.
Type \"help\" for commands.";

struct Options {
    seed: u64,
    verify: bool,
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut options = Options { seed: 1, verify: false };
    let mut args = args.skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--verify" => options.verify = true,
            "--seed" => {
                let value = args.next().ok_or("--seed needs a number")?;
                options.seed = value.parse().map_err(|_| format!("\"{value}\" is not a valid seed"))?;
            }
            other => return Err(format!("unknown option \"{other}\"")),
        }
    }
    Ok(options)
}

/// One game in progress, and where its text goes.
struct Play<W: Write> {
    session: Session<Game>,
    out: W,
}

impl<W: Write> Play<W> {
    fn new(seed: u64, out: W) -> Self {
        Self { session: Session::new(Game::new(seed)), out }
    }

    fn game(&self) -> &Game {
        self.session.world()
    }

    fn say(&mut self, text: &str) -> io::Result<()> {
        writeln!(self.out, "{text}")
    }

    fn look(&mut self) -> io::Result<()> {
        let text = narrate::place(self.game(), self.session.now());
        self.say(&text)
    }

    /// Prints what the player would notice among `events`.
    fn tell(&mut self, events: &Events<Game>) -> io::Result<()> {
        for stamped in events {
            if let Some(text) = narrate::event(&stamped.value, self.game()) {
                self.say(&text)?;
            }
        }
        Ok(())
    }

    /// Applies a command, then lets its time cost pass.
    fn act(&mut self, command: Command) -> io::Result<()> {
        let mut events = unwrap_ordered(self.session.submit(self.session.now(), command));
        let moved = events.iter().any(|stamped| matches!(stamped.value, Event::Moved { .. }));
        let free_at = self.game().hero().busy_until().max(self.session.now());
        events.extend(unwrap_ordered(self.session.advance_to(free_at)));
        self.tell(&events)?;
        if moved {
            self.look()?;
        }
        Ok(())
    }

    fn wait(&mut self, time: Duration) -> io::Result<()> {
        let events = unwrap_ordered(self.session.advance_to(self.session.now() + time));
        self.say("Time passes.")?;
        self.tell(&events)
    }

    /// Handles one line. Returns `false` when the player wants to stop.
    fn handle(&mut self, line: &str) -> io::Result<bool> {
        match parse(line) {
            Input::Do(command) => self.act(command)?,
            Input::Wait(time) => self.wait(time)?,
            Input::Look => self.look()?,
            Input::Inspect => {
                let text = narrate::inspect(self.game());
                self.say(&text)?;
            }
            Input::Stats => {
                let text = narrate::stats(self.game());
                self.say(&text)?;
            }
            Input::Pack => {
                let text = narrate::pack(self.game());
                self.say(&text)?;
            }
            Input::Help => self.say(narrate::HELP)?,
            Input::Unknown(reason) => self.say(&reason)?,
            Input::Empty => {}
            Input::Quit => return Ok(false),
        }
        Ok(true)
    }

    fn prompt(&mut self) -> io::Result<()> {
        write!(self.out, "\n[{}] > ", narrate::clock(self.session.now()))?;
        self.out.flush()
    }

    /// Replays the log from scratch and compares the result.
    fn verify(&mut self, seed: u64) -> io::Result<bool> {
        let now = self.session.now();
        let (rebuilt, _) = replay(Game::new(seed), self.session.log(), now);
        let same = &rebuilt == self.game();
        let commands = self.session.log().len();
        let text = if same {
            format!("Replay check passed: {commands} commands over {} ticks rebuilt the same world.", now.get())
        } else {
            format!("Replay check FAILED: {commands} commands over {} ticks rebuilt a different world.", now.get())
        };
        self.say(&text)?;
        Ok(same)
    }
}

/// The CLI only ever moves forward from the session's own clock, so an
/// out-of-order tick would be a bug here, not bad input.
fn unwrap_ordered(result: Result<Events<Game>, OutOfOrder>) -> Events<Game> {
    result.expect("the terminal client never submits a tick from the past")
}

fn run(options: &Options) -> io::Result<bool> {
    let stdin = io::stdin();
    let echo = !stdin.is_terminal();
    let mut play = Play::new(options.seed, io::stdout().lock());

    play.say(INTRO)?;
    play.say("")?;
    play.look()?;

    let mut lines = stdin.lock().lines();
    loop {
        play.prompt()?;
        let Some(line) = lines.next().transpose()? else { break };
        if echo {
            play.say(&line)?;
        }
        if !play.handle(&line)? {
            break;
        }
    }
    play.say("")?;
    if options.verify {
        return play.verify(options.seed);
    }
    Ok(true)
}

fn main() -> ExitCode {
    let options = match parse_args(std::env::args()) {
        Ok(options) => options,
        Err(problem) => {
            eprintln!("{problem}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(&options) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("equilibrium: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eq_clock::Tick;

    fn play(lines: &[&str]) -> String {
        let mut play = Play::new(1, Vec::new());
        for line in lines {
            play.handle(line).unwrap();
        }
        String::from_utf8(play.out).unwrap()
    }

    #[test]
    fn walking_describes_the_new_place() {
        let text = play(&["north"]);
        assert!(text.contains("You make your way to Ore Ledge."));
        assert!(text.contains("Ore Ledge [wilderness]"));
    }

    #[test]
    fn actions_take_time() {
        let mut play = Play::new(1, Vec::new());
        play.handle("north").unwrap();
        assert_eq!(play.session.now(), Tick::new(30));
        play.handle("gather").unwrap();
        assert_eq!(play.session.now(), Tick::new(35));
        play.handle("wait 10m").unwrap();
        assert_eq!(play.session.now(), Tick::new(635));
    }

    #[test]
    fn views_do_not_enter_the_log() {
        let mut play = Play::new(1, Vec::new());
        for line in ["look", "stats", "pack", "inspect", "help", "nonsense", ""] {
            assert!(play.handle(line).unwrap());
        }
        assert!(play.session.log().is_empty());
        assert_eq!(play.session.now(), Tick::ZERO);
    }

    #[test]
    fn quit_stops_the_loop() {
        let mut play = Play::new(1, Vec::new());
        assert!(!play.handle("quit").unwrap());
    }

    #[test]
    fn a_played_session_passes_the_replay_check() {
        let mut play = Play::new(7, Vec::new());
        for line in ["north", "gather", "gather", "south", "east", "cave", "gather", "wait 15m", "out", "gather"] {
            play.handle(line).unwrap();
        }
        assert!(play.verify(7).unwrap());
        assert!(!play.verify(8).unwrap());
    }

    #[test]
    fn arguments_are_parsed_and_checked() {
        let args = |list: &[&str]| parse_args(list.iter().map(|arg| arg.to_string()));
        let options = args(&["eq", "--seed", "42", "--verify"]).unwrap();
        assert_eq!((options.seed, options.verify), (42, true));
        assert!(args(&["eq", "--seed"]).is_err());
        assert!(args(&["eq", "--seed", "x"]).is_err());
        assert!(args(&["eq", "--fast"]).is_err());
    }
}
