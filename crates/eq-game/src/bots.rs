//! Simple characters that play by themselves.
//!
//! A bot follows a [`Plan`]: a route to walk and a [`Policy`] for when to
//! gather. Bots act through ordinary commands, so everything they do is
//! in the log and replays like any player's moves.
//!
//! They exist for two jobs: testing what crowds do to a zone, and giving
//! a lone player some company to share it with.

use alloc::vec::Vec;

use eq_clock::{Duration, Tick};
use eq_events::{Events, OutOfOrder, Session};
use eq_nodes::Quality;

use crate::content::{CIRCUIT, DEPOSIT_ROUTES, MIN_WORKABLE_STOCK};
use crate::{CharacterId, Command, Event, Game, Outcome};

/// How long a bot stands idle when there is nothing it wants to do.
const IDLE: Duration = Duration::seconds(5);

/// How long a bot waits before trying a flooded way back again.
const TIDE_WAIT: Duration = Duration::minutes(1);

/// The least time between a bot's turns, so a route of dead ends cannot
/// spin without the clock moving.
const PAUSE: Duration = Duration::seconds(1);

/// When a bot gathers from the deposit in front of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Policy {
    /// Takes whatever can be taken.
    Greedy,
    /// Takes only from deposits that are pristine and at least half full.
    Careful,
}

impl Policy {
    /// Whether a bot with this policy would gather where `who` stands.
    pub fn wants_to_gather(self, game: &Game, who: CharacterId) -> bool {
        let Some(node) = game.place_of(who).and_then(|place| place.deposit).map(|deposit| deposit.node) else {
            return false;
        };
        match self {
            Self::Greedy => node.stock() >= MIN_WORKABLE_STOCK,
            Self::Careful => node.quality() == Quality::Pristine && node.stock() * 2 >= node.capacity(),
        }
    }
}

/// One step of a route: an exit to take, and what to do if the tide has
/// closed it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Leg {
    /// The exit's label.
    pub exit: &'static str,
    /// Wait for the exit to open (it is the only way back) instead of
    /// skipping this step.
    pub wait_if_closed: bool,
}

impl Leg {
    /// Take `exit`, or skip this step if it is closed.
    pub const fn go(exit: &'static str) -> Self {
        Self { exit, wait_if_closed: false }
    }

    /// Take `exit`, waiting for the tide if it is closed.
    pub const fn wait_for(exit: &'static str) -> Self {
        Self { exit, wait_if_closed: true }
    }
}

/// How one bot behaves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    /// When it gathers.
    pub policy: Policy,
    /// Exits walked once, to get into position.
    pub approach: &'static [Leg],
    /// Exits walked round and round afterwards. Empty means stay put.
    pub circuit: &'static [Leg],
}

impl Plan {
    /// Walks the whole shoreline, gathering wherever `policy` allows.
    pub const fn roam(policy: Policy) -> Self {
        Self { policy, approach: &[], circuit: CIRCUIT }
    }

    /// Walks to one place and stays, gathering whenever `policy` allows.
    pub const fn camp(policy: Policy, approach: &'static [Leg]) -> Self {
        Self { policy, approach, circuit: &[] }
    }

    /// A small mixed cast for keeping a lone player company: mostly
    /// careful, with one greedy roamer among every four.
    pub fn company(size: usize) -> Vec<Self> {
        const CAST: [Plan; 4] = [
            Plan::roam(Policy::Careful),
            Plan::roam(Policy::Greedy),
            Plan::camp(Policy::Careful, DEPOSIT_ROUTES[3]),
            Plan::camp(Policy::Careful, DEPOSIT_ROUTES[8]),
        ];
        CAST.into_iter().cycle().take(size).collect()
    }

    fn leg(&self, step: usize) -> Option<Leg> {
        match self.approach.get(step) {
            Some(leg) => Some(*leg),
            None if self.circuit.is_empty() => None,
            None => Some(self.circuit[(step - self.approach.len()) % self.circuit.len()]),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Bot {
    who: CharacterId,
    plan: Plan,
    step: usize,
    free_at: Tick,
}

/// A set of bots and where each has got to in its plan.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bots {
    bots: Vec<Bot>,
}

impl Bots {
    /// Puts each named character under a plan.
    pub fn new(crew: impl IntoIterator<Item = (CharacterId, Plan)>) -> Self {
        let bots = crew.into_iter().map(|(who, plan)| Bot { who, plan, step: 0, free_at: Tick::ZERO }).collect();
        Self { bots }
    }

    /// How many bots there are.
    pub fn len(&self) -> usize {
        self.bots.len()
    }

    /// Whether there are no bots.
    pub fn is_empty(&self) -> bool {
        self.bots.is_empty()
    }

    /// Runs the world to `until`, letting each bot act whenever it is
    /// free, soonest first. Returns everything that happened.
    pub fn run(&mut self, session: &mut Session<Game>, until: Tick) -> Result<Events<Game>, OutOfOrder> {
        let mut events = Vec::new();
        while let Some(index) = self.next_free_before(until) {
            self.take_turn(index, session, &mut events)?;
        }
        events.extend(session.advance_to(until)?);
        Ok(events)
    }

    /// Has every bot stand still until `now`, as if it had been resting.
    pub fn stand_still_until(&mut self, now: Tick) {
        for bot in &mut self.bots {
            bot.free_at = bot.free_at.max(now);
        }
    }

    fn next_free_before(&self, until: Tick) -> Option<usize> {
        (0..self.bots.len()).min_by_key(|&index| self.bots[index].free_at).filter(|&index| self.bots[index].free_at < until)
    }

    /// One bot gathers if its policy says so, and otherwise walks the
    /// next leg of its plan.
    fn take_turn(&mut self, index: usize, session: &mut Session<Game>, events: &mut Events<Game>) -> Result<(), OutOfOrder> {
        let bot = &mut self.bots[index];
        let now = bot.free_at.max(session.now());
        let busy_until = |session: &Session<Game>| session.world().character(bot.who).map_or(now, |character| character.busy_until());

        if bot.plan.policy.wants_to_gather(session.world(), bot.who) {
            events.extend(session.submit(now, Command::gather(bot.who))?);
            bot.free_at = busy_until(session).max(now + IDLE);
            return Ok(());
        }
        let Some(leg) = bot.plan.leg(bot.step) else {
            bot.free_at = now + IDLE;
            return Ok(());
        };
        let tried = session.submit(now, Command::go(bot.who, leg.exit))?;
        let closed = tried.iter().any(|stamped| {
            matches!(stamped.value, Event::Character { outcome: Outcome::ExitClosed { .. }, .. })
        });
        events.extend(tried);
        if closed && leg.wait_if_closed {
            bot.free_at = now + TIDE_WAIT;
        } else {
            // A step that led nowhere (somewhere flooded) is skipped.
            bot.step += 1;
            bot.free_at = busy_until(session).max(now + PAUSE);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASH: CharacterId = CharacterId::new(0);
    const BRYN: CharacterId = CharacterId::new(1);

    fn session(names: &[&str]) -> Session<Game> {
        Session::new(Game::new(1, names))
    }

    #[test]
    fn a_camper_walks_to_its_place_and_stays() {
        let mut session = session(&["Ash"]);
        let mut bots = Bots::new([(ASH, Plan::camp(Policy::Greedy, DEPOSIT_ROUTES[1]))]);
        bots.run(&mut session, Tick::new(600)).unwrap();
        assert_eq!(session.world().place_of(ASH).unwrap().name, "High Shelf");
        assert!(session.world().character(ASH).unwrap().carried_total() > 0);
    }

    #[test]
    fn a_roamer_gets_round_the_whole_shoreline() {
        let mut session = session(&["Ash"]);
        let mut bots = Bots::new([(ASH, Plan::roam(Policy::Careful))]);
        let events = bots.run(&mut session, Tick::new(6_000)).unwrap();
        let mut visited: Vec<u64> = events
            .iter()
            .filter_map(|stamped| match stamped.value {
                Event::Character { outcome: Outcome::Moved { to, .. }, .. } => Some(to.get()),
                _ => None,
            })
            .collect();
        visited.sort_unstable();
        visited.dedup();
        assert_eq!(visited.len(), session.world().map().len(), "visited {visited:?}");
    }

    #[test]
    fn careful_bots_stay_clean_and_greedy_ones_get_marked() {
        let mut session = session(&["Ash", "Bryn"]);
        let mut bots = Bots::new([(ASH, Plan::roam(Policy::Careful)), (BRYN, Plan::roam(Policy::Greedy))]);
        bots.run(&mut session, Tick::new(3_000)).unwrap();
        assert_eq!(session.world().character(ASH).unwrap().taint(), 0);
        assert!(session.world().character(BRYN).unwrap().taint() >= 50);
    }

    #[test]
    fn standing_still_stops_the_bots_without_stopping_the_world() {
        let mut session = session(&["Ash"]);
        let mut bots = Bots::new([(ASH, Plan::camp(Policy::Greedy, DEPOSIT_ROUTES[0]))]);
        bots.run(&mut session, Tick::new(300)).unwrap();
        let commands = session.log().len();

        session.advance_to(Tick::new(900)).unwrap();
        bots.stand_still_until(session.now());
        assert_eq!(session.log().len(), commands);
        bots.run(&mut session, Tick::new(910)).unwrap();
        assert!(session.log().len() > commands);
    }

    #[test]
    fn the_company_is_mostly_careful() {
        let cast = Plan::company(8);
        let greedy = cast.iter().filter(|plan| plan.policy == Policy::Greedy).count();
        assert_eq!((cast.len(), greedy), (8, 2));
        assert!(Plan::company(0).is_empty());
        assert!(Bots::default().is_empty());
        assert_eq!(Bots::new([(ASH, cast[0])]).len(), 1);
    }
}
