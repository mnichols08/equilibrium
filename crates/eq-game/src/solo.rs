//! One player at the controls: a session, a character, and lines of text
//! in and out.
//!
//! Every client that lets one person play (the terminal, the browser)
//! needs the same loop: read a line, act, let the action's time pass, say
//! what happened. It lives here so the clients only differ in how they
//! show it.

use alloc::string::String;
use alloc::vec::Vec;

use eq_clock::{Duration, Tick};
use eq_events::{replay, Events, OutOfOrder, Session};

use crate::parser::{parse, Input};
use crate::{narrate, Action, CharacterId, Command, Event, Game, Outcome};

/// What came back from one line of input.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reply {
    /// What to show the player, in order.
    pub lines: Vec<String>,
    /// Whether the player asked to stop.
    pub quit: bool,
}

/// A single-player game in progress.
#[derive(Clone, Debug)]
pub struct Solo {
    seed: u64,
    session: Session<Game>,
}

impl Solo {
    /// The player's character.
    pub const YOU: CharacterId = CharacterId::new(0);

    /// Starts a game. The same seed always plays out the same way.
    pub fn new(seed: u64) -> Self {
        Self { seed, session: Session::new(Self::fresh_world(seed)) }
    }

    fn fresh_world(seed: u64) -> Game {
        Game::new(seed, &["You"])
    }

    /// The world as it stands.
    pub const fn game(&self) -> &Game {
        self.session.world()
    }

    /// The current tick.
    pub const fn now(&self) -> Tick {
        self.session.now()
    }

    /// How many commands have been logged.
    pub fn commands(&self) -> usize {
        self.session.log().len()
    }

    /// Describes where the player is.
    pub fn look(&self) -> String {
        narrate::place(self.game(), Self::YOU, self.now())
    }

    /// Handles one line of input.
    pub fn handle(&mut self, line: &str) -> Reply {
        let lines = match parse(line) {
            Input::Do(action) => self.act(action),
            Input::Wait(time) => self.wait(time),
            Input::Look => alloc::vec![self.look()],
            Input::Inspect => alloc::vec![narrate::inspect(self.game(), Self::YOU)],
            Input::Stats => alloc::vec![narrate::stats(self.game(), Self::YOU)],
            Input::Pack => alloc::vec![narrate::pack(self.game(), Self::YOU)],
            Input::Help => alloc::vec![narrate::HELP.into()],
            Input::Unknown(reason) => alloc::vec![reason],
            Input::Empty => Vec::new(),
            Input::Quit => return Reply { lines: Vec::new(), quit: true },
        };
        Reply { lines, quit: false }
    }

    /// Replays the command log from scratch and says whether it rebuilds
    /// exactly this world.
    pub fn replays_identically(&self) -> bool {
        let (rebuilt, _) = replay(Self::fresh_world(self.seed), self.session.log(), self.now());
        &rebuilt == self.game()
    }

    /// Has the player's character act, then lets the action's time pass.
    fn act(&mut self, action: Action) -> Vec<String> {
        let command = Command { who: Self::YOU, action };
        let mut events = in_order(self.session.submit(self.now(), command));
        let moved = events.iter().any(|stamped| {
            matches!(stamped.value, Event::Character { who: Self::YOU, outcome: Outcome::Moved { .. } })
        });
        events.extend(in_order(self.session.advance_to(self.free_at())));

        let mut lines = self.tell(&events);
        if moved {
            lines.push(self.look());
        }
        lines
    }

    fn wait(&mut self, time: Duration) -> Vec<String> {
        let events = in_order(self.session.advance_to(self.now() + time));
        let mut lines = alloc::vec![String::from("Time passes.")];
        lines.extend(self.tell(&events));
        lines
    }

    /// When the player's character will be free to act again.
    fn free_at(&self) -> Tick {
        let busy_until = self.game().character(Self::YOU).map_or(self.now(), |you| you.busy_until());
        busy_until.max(self.now())
    }

    /// What the player would notice among `events`.
    fn tell(&self, events: &Events<Game>) -> Vec<String> {
        events.iter().filter_map(|stamped| narrate::event(&stamped.value, self.game(), Self::YOU)).collect()
    }
}

/// `Solo` only ever moves forward from its own clock, so an out-of-order
/// tick would be a bug here, not bad input.
fn in_order(result: Result<Events<Game>, OutOfOrder>) -> Events<Game> {
    result.expect("a solo game never submits a tick from the past")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(lines: &[&str]) -> (Solo, Vec<String>) {
        let mut solo = Solo::new(1);
        let said = lines.iter().flat_map(|line| solo.handle(line).lines).collect();
        (solo, said)
    }

    #[test]
    fn walking_describes_the_new_place() {
        let (_, said) = play(&["north"]);
        assert_eq!(said[0], "You make your way to Ore Ledge.");
        assert!(said[1].starts_with("Ore Ledge [wilderness]"));
    }

    #[test]
    fn actions_take_time() {
        let mut solo = Solo::new(1);
        solo.handle("north");
        assert_eq!(solo.now(), Tick::new(30));
        solo.handle("gather");
        assert_eq!(solo.now(), Tick::new(35));
        solo.handle("wait 10m");
        assert_eq!(solo.now(), Tick::new(635));
    }

    #[test]
    fn views_do_not_enter_the_log_or_take_time() {
        let mut solo = Solo::new(1);
        for line in ["look", "stats", "pack", "inspect", "help", "nonsense", ""] {
            assert!(!solo.handle(line).quit);
        }
        assert_eq!((solo.commands(), solo.now()), (0, Tick::ZERO));
    }

    #[test]
    fn quit_is_reported_and_changes_nothing() {
        let mut solo = Solo::new(1);
        assert_eq!(solo.handle("quit"), Reply { lines: Vec::new(), quit: true });
        assert_eq!(solo.commands(), 0);
    }

    #[test]
    fn a_played_game_replays_identically() {
        let mut solo = Solo::new(7);
        for line in ["north", "gather", "gather", "south", "east", "cave", "gather", "wait 15m", "out", "gather"] {
            solo.handle(line);
        }
        assert!(solo.replays_identically());
        assert_eq!(solo.commands(), 9);
    }
}
