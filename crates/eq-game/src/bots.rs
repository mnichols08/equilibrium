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
use crate::{CharacterId, Command, Event, Game, Outcome, Standing};

/// A warden with less health than this leaves fighting to others.
const HEALTH_TO_FIGHT: u32 = 40;

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
    /// Whether it challenges marked characters it meets, and picks up
    /// what they drop.
    pub warden: bool,
}

impl Plan {
    /// Walks the whole shoreline, gathering wherever `policy` allows.
    pub const fn roam(policy: Policy) -> Self {
        Self { policy, approach: &[], circuit: CIRCUIT, warden: false }
    }

    /// Walks to one place and stays, gathering whenever `policy` allows.
    pub const fn camp(policy: Policy, approach: &'static [Leg]) -> Self {
        Self { policy, approach, circuit: &[], warden: false }
    }

    /// The same plan, but challenging any marked character it meets.
    pub const fn as_warden(self) -> Self {
        Self { warden: true, ..self }
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

    /// One bot's turn. In order of preference: a warden deals with a
    /// marked character or with spoils on the ground; any bot gathers if
    /// its policy says so; otherwise it walks the next leg of its plan.
    fn take_turn(&mut self, index: usize, session: &mut Session<Game>, events: &mut Events<Game>) -> Result<(), OutOfOrder> {
        let bot = &mut self.bots[index];
        let now = bot.free_at.max(session.now());
        // Bring the world up to the bot's turn first, so it decides on
        // what is there now and not on what was there when someone last
        // acted.
        events.extend(session.advance_to(now)?);
        let busy_until = |session: &Session<Game>| session.world().character(bot.who).map_or(now, |character| character.busy_until());

        if let Some(until) = session.world().character(bot.who).and_then(|character| character.down_until()) {
            bot.free_at = until.max(now + PAUSE);
            return Ok(());
        }
        if let Some(duty) = warden_duty(session.world(), bot) {
            events.extend(session.submit(now, duty)?);
            bot.free_at = busy_until(session).max(now + PAUSE);
            return Ok(());
        }
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

/// What a warden would do before anything else: challenge a marked
/// character standing with it, or pick up what lies on the ground.
fn warden_duty(game: &Game, bot: &Bot) -> Option<Command> {
    if !bot.plan.warden {
        return None;
    }
    let me = game.character(bot.who)?;
    let fit_to_fight = me.health() >= HEALTH_TO_FIGHT;
    let marked = game.occupants(me.place()).iter().copied().find(|&other| {
        game.character(other).is_some_and(|character| character.standing() == Standing::Marked)
            && game.can_attack(bot.who, other).is_ok()
    });
    if let (true, Some(target)) = (fit_to_fight, marked) {
        return Some(Command::attack(bot.who, target));
    }
    let spoils = game.place_of(bot.who)?.spoils.iter().any(|&amount| amount > 0);
    spoils.then(|| Command::take(bot.who))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_warden_puts_down_a_marked_bot_and_takes_what_it_drops() {
        let mut session = session(&["Ash", "Bryn"]);
        let ledge = DEPOSIT_ROUTES[0];
        let mut bots = Bots::new([
            (ASH, Plan::camp(Policy::Careful, ledge).as_warden()),
            (BRYN, Plan::camp(Policy::Greedy, ledge)),
        ]);
        let events = bots.run(&mut session, Tick::new(3_000)).unwrap();
        let downed = Event::Character { who: BRYN, outcome: Outcome::Downed { by: ASH } };
        assert!(events.iter().any(|stamped| stamped.value == downed), "Bryn was never put down");
        assert!(events.iter().any(|stamped| {
            matches!(stamped.value, Event::Character { who: ASH, outcome: Outcome::Took { .. } })
        }));
        assert!(!events.iter().any(|stamped| {
            matches!(stamped.value, Event::Character { who: ASH, outcome: Outcome::Downed { .. } })
        }));
    }

    #[test]
    fn a_waiting_bot_sees_the_world_change_around_it() {
        // A careful camper alone: once the ledge is no longer pristine it
        // must wait, with nobody else acting, and notice when it heals.
        let mut session = session(&["Ash"]);
        let mut bots = Bots::new([(ASH, Plan::camp(Policy::Careful, DEPOSIT_ROUTES[0]))]);
        bots.run(&mut session, Tick::new(1_000)).unwrap();
        let early = session.world().character(ASH).unwrap().carried_total();
        bots.run(&mut session, Tick::new(10_000)).unwrap();
        let late = session.world().character(ASH).unwrap().carried_total();
        assert!(late > early * 3, "it stopped noticing: {early} then {late}");
    }

    #[test]
    fn bots_that_are_not_wardens_never_fight() {
        let mut session = session(&["Ash", "Bryn"]);
        let ledge = DEPOSIT_ROUTES[0];
        let mut bots =
            Bots::new([(ASH, Plan::camp(Policy::Careful, ledge)), (BRYN, Plan::camp(Policy::Greedy, ledge))]);
        let events = bots.run(&mut session, Tick::new(3_000)).unwrap();
        assert!(!events.iter().any(|stamped| {
            matches!(stamped.value, Event::Character { outcome: Outcome::Struck { .. }, .. })
        }));
    }

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
