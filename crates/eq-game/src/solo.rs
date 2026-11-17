//! One player at the controls: a session, a character, and lines of text
//! in and out.
//!
//! Every client that lets one person play (the terminal, the browser)
//! needs the same loop: read a line, act, let the action's time pass, say
//! what happened. It lives here so the clients only differ in how they
//! show it.
//!
//! The player can have company: bots that share the shoreline and act
//! whenever time passes.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use eq_clock::{Duration, Tick};
use eq_events::{replay, Events, OutOfOrder, Session};

use crate::bots::{Bots, Plan};
use crate::seat::{interpret, Request};
use crate::{narrate, Action, CharacterId, Command, Event, Game, Outcome};

/// What came back from one line of input.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reply {
    /// What to show the player, in order.
    pub lines: Vec<String>,
    /// Whether the player asked to stop.
    pub quit: bool,
}

/// Names for the bots that keep a player company, in order.
const COMPANIONS: [&str; 8] = ["Ash", "Bryn", "Cole", "Dara", "Eli", "Fen", "Gil", "Hale"];

/// A single-player game in progress.
#[derive(Clone, Debug)]
pub struct Solo {
    seed: u64,
    company: usize,
    session: Session<Game>,
    bots: Bots,
}

impl Solo {
    /// The player's character.
    pub const YOU: CharacterId = CharacterId::new(0);

    /// Starts a game with the shoreline to yourself. The same seed
    /// always plays out the same way.
    pub fn new(seed: u64) -> Self {
        Self::with_company(seed, 0)
    }

    /// Starts a game with `company` bots sharing the shoreline: mostly
    /// careful, with a greedy one among every four.
    pub fn with_company(seed: u64, company: usize) -> Self {
        let crew = Plan::company(company).into_iter().enumerate().map(|(index, plan)| (companion_id(index), plan));
        Self { seed, company, session: Session::new(fresh_world(seed, company)), bots: Bots::new(crew) }
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
        let lines = match interpret(self.game(), Self::YOU, self.now(), line) {
            Request::Act(action) => self.act(action),
            Request::Wait(time) => self.wait(time),
            Request::Sleep(time) => self.sleep(time),
            Request::Say(lines) => lines,
            Request::Quit => return Reply { lines: Vec::new(), quit: true },
        };
        Reply { lines, quit: false }
    }

    /// Replays the command log from scratch and says whether it rebuilds
    /// exactly this world.
    pub fn replays_identically(&self) -> bool {
        let (rebuilt, _) = replay(fresh_world(self.seed, self.company), self.session.log(), self.now());
        &rebuilt == self.game()
    }

    /// Has the player's character act, then lets the action's time pass.
    fn act(&mut self, action: Action) -> Vec<String> {
        let command = Command { who: Self::YOU, action };
        let mut events = in_order(self.session.submit(self.now(), command));
        let moved = events.iter().any(|stamped| {
            matches!(stamped.value, Event::Character { who: Self::YOU, outcome: Outcome::Moved { .. } })
        });
        events.extend(self.pass_time_until(self.free_at()));

        let mut lines = self.tell(&events);
        if moved {
            lines.push(self.look());
        }
        lines
    }

    fn wait(&mut self, time: Duration) -> Vec<String> {
        let events = self.pass_time_until(self.now() + time);
        let mut lines = alloc::vec![String::from("Time passes.")];
        lines.extend(self.tell(&events));
        lines
    }

    /// Lies down, sleeps for `time` and wakes, unless something wakes the
    /// player first.
    fn sleep(&mut self, time: Duration) -> Vec<String> {
        let mut lines = self.act(Action::Rest);
        let lying_down = |solo: &Self| solo.game().character(Self::YOU).is_some_and(|you| you.is_lying_down());
        if !lying_down(self) {
            return lines;
        }
        let events = self.pass_time_until(self.now() + time);
        lines.extend(self.tell(&events));
        // Whoever sleeps the long sleep through wakes as a spirit.
        let dormant = self.game().character(Self::YOU).is_some_and(crate::Character::is_dormant);
        if lying_down(self) || dormant {
            lines.extend(self.act(Action::Wake));
        }
        lines
    }

    /// When the player's character will be free to act again.
    fn free_at(&self) -> Tick {
        let busy_until = self.game().character(Self::YOU).map_or(self.now(), |you| you.busy_until());
        busy_until.max(self.now())
    }

    /// Runs the world to `target`, with the company acting as it goes.
    fn pass_time_until(&mut self, target: Tick) -> Events<Game> {
        in_order(self.bots.run(&mut self.session, target))
    }

    /// What the player would notice among `events`.
    fn tell(&self, events: &Events<Game>) -> Vec<String> {
        let lines = events.iter().filter_map(|stamped| narrate::event(&stamped.value, self.game(), Self::YOU));
        fold_repeats(lines)
    }
}

/// The id of the `index`th companion. The player is always first.
const fn companion_id(index: usize) -> CharacterId {
    CharacterId::new(index as u64 + 1)
}

/// The world a game starts from: the player, then their company.
fn fresh_world(seed: u64, company: usize) -> Game {
    let strangers: Vec<String> = (COMPANIONS.len()..company).map(|index| format!("Stranger {}", index + 1)).collect();
    let names: Vec<&str> = core::iter::once("You")
        .chain(COMPANIONS.into_iter().take(company))
        .chain(strangers.iter().map(String::as_str))
        .collect();
    Game::new(seed, &names)
}

/// Folds a run of identical lines into one, so a companion gathering
/// beside you for ten minutes reads as one line, not a hundred.
fn fold_repeats(lines: impl Iterator<Item = String>) -> Vec<String> {
    let mut runs: Vec<(String, usize)> = Vec::new();
    for line in lines {
        match runs.last_mut() {
            Some((last, count)) if *last == line => *count += 1,
            _ => runs.push((line, 1)),
        }
    }
    runs.into_iter().map(|(line, count)| if count == 1 { line } else { format!("{line} (x{count})") }).collect()
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
    fn repeats_are_folded_and_order_is_kept() {
        let lines = ["a", "b", "b", "b", "a", "a"].map(String::from).into_iter();
        assert_eq!(fold_repeats(lines), ["a", "b (x3)", "a (x2)"]);
        assert!(fold_repeats(core::iter::empty()).is_empty());
    }

    /// Plays until Bryn, the greedy one, is marked, then walks the player
    /// to wherever Bryn is standing. Gives up after a while.
    fn find_bryn_marked(solo: &mut Solo) -> bool {
        const BRYN: CharacterId = CharacterId::new(2);
        for _ in 0..600 {
            let bryn = solo.game().character(BRYN).unwrap();
            let you = solo.game().character(Solo::YOU).unwrap();
            if bryn.standing() == crate::Standing::Marked && bryn.place() == you.place() {
                return solo.game().can_attack(Solo::YOU, BRYN).is_ok();
            }
            // Follow the same circuit the bots walk; sooner or later you meet.
            let step = solo.commands() % crate::content::CIRCUIT.len();
            solo.handle(crate::content::CIRCUIT[step].exit);
            solo.handle("wait 20");
        }
        false
    }

    #[test]
    fn you_can_challenge_a_marked_companion_by_name_and_take_what_they_drop() {
        let mut solo = Solo::with_company(1, 3);
        assert_eq!(solo.handle("attack bryn").lines, ["Nobody can be harmed here."]);
        assert_eq!(solo.handle("attack nobody").lines, ["There is nobody called \"nobody\" here."]);

        assert!(find_bryn_marked(&mut solo), "never caught Bryn marked in the open");
        let mut said = Vec::new();
        for _ in 0..12 {
            said.extend(solo.handle("ATTACK Bryn").lines);
            if said.iter().any(|line| line.starts_with("Bryn dies.")) {
                break;
            }
        }
        assert!(said.iter().any(|line| line.starts_with("You strike Bryn for ")), "{said:?}");
        assert!(said.iter().any(|line| line.starts_with("Bryn strikes you for ")), "{said:?}");
        assert!(said.iter().any(|line| line.starts_with("Bryn dies.")), "{said:?}");
        assert!(solo.look().contains("Bryn's body lies here."));
        assert!(!solo.look().contains("Also here: Bryn"), "the living do not see spirits");
        assert!(solo.look().contains("relic lies here, Bryn's: "), "{}", solo.look());

        let took = solo.handle("take").lines;
        assert!(took[0].starts_with("You take "), "{took:?}");
        assert!(solo.replays_identically());
    }

    #[test]
    fn sleeping_is_one_line_in_and_a_rested_waking_out() {
        let mut solo = Solo::new(1);
        solo.handle("wait 10h");
        let rested = |solo: &Solo| solo.game().character(Solo::YOU).unwrap().rested();
        assert!(rested(&solo) < 40);

        let said = solo.handle("sleep").lines;
        assert_eq!(said, ["You settle down to sleep.", "You wake after 8 hours, well rested."]);
        assert_eq!(rested(&solo), 100);
        assert_eq!(solo.now(), Tick::new(10 * 3_600 + 30 + 8 * 3_600));
        assert!(solo.handle("stats").lines[0].contains("Rested       100"));
        assert!(solo.replays_identically());
    }

    #[test]
    fn sleeping_in_the_wilds_beside_a_greedy_companion_costs_you_your_pack() {
        let mut solo = Solo::with_company(1, 3);
        solo.handle("north");
        solo.handle("gather");
        let said = solo.handle("sleep 6h").lines;
        assert!(said.iter().any(|line| line.contains("Your pack is lighter")), "{said:?}");
        assert_eq!(solo.game().character(Solo::YOU).unwrap().carried_total(), 0);
    }

    #[test]
    fn sleeping_three_weeks_wakes_you_a_forgotten_spirit_who_can_find_the_way_back() {
        let mut solo = Solo::new(1);
        let lines = solo.handle("sleep 21d").lines.join("\n");
        assert!(lines.contains("You slept too long."), "{lines}");
        assert!(lines.contains("You stir, a spirit without a body. You have 10 hours awake"), "{lines}");
        assert!(solo.look().contains("You are forgotten: 0 of 3 memories found"));

        assert!(solo.handle("seek").lines[0].contains("nothing of you to find here"));
        for way in ["north", "north", "east"] {
            solo.handle(way);
            if way == "east" {
                // Back through camp to the beach.
                solo.handle("south");
                solo.handle("south");
                solo.handle("east");
            }
            let found = solo.handle("seek").lines.join("\n");
            assert!(found.contains("memories"), "{found}");
        }
        let you = solo.game().character(Solo::YOU).unwrap();
        assert!(!you.is_spirit());
        assert_eq!(you.life(), 2);
        assert!(solo.replays_identically());
    }

    #[test]
    fn sleeping_at_camp_is_safe() {
        let mut solo = Solo::with_company(1, 3);
        solo.handle("north");
        solo.handle("gather");
        solo.handle("south");
        let carried = solo.game().character(Solo::YOU).unwrap().carried_total();
        solo.handle("sleep 6h");
        assert_eq!(solo.game().character(Solo::YOU).unwrap().carried_total(), carried);
    }

    #[test]
    fn company_shares_the_shoreline_and_acts_while_you_wait() {
        let mut solo = Solo::with_company(1, 3);
        assert_eq!(solo.game().population(), 4);
        assert!(solo.look().contains("Also here: Ash, Bryn, Cole."));

        let said = solo.handle("wait 10m").lines;
        assert!(said.iter().any(|line| line.contains("leaves for")), "{said:?}");
        assert!(!solo.look().contains("Also here: Ash, Bryn, Cole."));
        assert!(solo.commands() > 10, "the company should have been busy");
        let carried: u64 = solo.game().characters().skip(1).map(|(_, bot)| bot.carried_total()).sum();
        assert!(carried > 0);
    }

    #[test]
    fn a_game_with_company_replays_identically() {
        let mut solo = Solo::with_company(5, 4);
        for line in ["east", "gather", "wait 20m", "east", "gather", "wait 1h"] {
            solo.handle(line);
        }
        assert!(solo.replays_identically());
    }

    #[test]
    fn more_company_than_names_still_gets_named() {
        let solo = Solo::with_company(1, 10);
        let names: Vec<&str> = solo.game().characters().map(|(_, character)| character.name()).collect();
        assert_eq!(names[..3], ["You", "Ash", "Bryn"]);
        assert_eq!(names[9..], ["Stranger 9", "Stranger 10"]);
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
