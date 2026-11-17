//! Bots for testing the living loop: one or many, sharing one shoreline.
#![allow(dead_code)] // each test file uses a different part of this

use eq_clock::{Duration, Tick};
use eq_events::Session;
use eq_game::content::MIN_WORKABLE_STOCK;
use eq_game::{CharacterId, Command, Event, Game, Outcome};
use eq_nodes::Quality;

pub const SEED: u64 = 2026;

/// How often the zone is sampled during a run.
pub const SAMPLE_EVERY: Duration = Duration::seconds(100);

/// How long a bot stands idle when there is nothing it wants to do.
const IDLE: Duration = Duration::seconds(5);

/// How long a bot waits before trying a flooded way back again.
const TIDE_WAIT: Duration = Duration::minutes(1);

/// One step of a route: an exit to take, and whether to wait for it if
/// the tide has closed it (the only way back from somewhere) or skip it.
#[derive(Clone, Copy)]
pub struct Leg {
    pub exit: &'static str,
    pub wait_if_closed: bool,
}

pub const fn go(exit: &'static str) -> Leg {
    Leg { exit, wait_if_closed: false }
}

pub const fn wait_for(exit: &'static str) -> Leg {
    Leg { exit, wait_if_closed: true }
}

/// The roaming circuit, visiting every deposit and returning to camp:
/// both ore shelves, the beach, the cave and grotto, the shallows and
/// reef, then the wrack line and the spit.
pub const CIRCUIT: &[Leg] = &[
    go("north"),
    go("north"),
    go("south"),
    go("south"),
    go("east"),
    go("cave"),
    go("down"),
    go("up"),
    wait_for("out"),
    go("east"),
    go("east"),
    wait_for("west"),
    go("west"),
    go("west"),
    go("south"),
    go("south"),
    go("north"),
    go("north"),
];

/// How to walk from camp to each of the nine deposits.
pub const DEPOSITS: [&[Leg]; 9] = [
    &[go("north")],
    &[go("north"), go("north")],
    &[go("east")],
    &[go("east"), go("east")],
    &[go("east"), go("east"), go("east")],
    &[go("east"), go("cave")],
    &[go("east"), go("cave"), go("down")],
    &[go("south")],
    &[go("south"), go("south")],
];

/// Decides whether a bot gathers from the deposit in front of it.
pub type Policy = fn(&Game, CharacterId) -> bool;

/// Takes whatever can be taken.
pub fn greedy(game: &Game, who: CharacterId) -> bool {
    deposit(game, who).is_some_and(|node| node.stock() >= MIN_WORKABLE_STOCK)
}

/// Takes only from deposits that are pristine and at least half full.
pub fn careful(game: &Game, who: CharacterId) -> bool {
    deposit(game, who).is_some_and(|node| node.quality() == Quality::Pristine && node.stock() * 2 >= node.capacity())
}

fn deposit(game: &Game, who: CharacterId) -> Option<eq_nodes::Node> {
    game.place_of(who)?.deposit.map(|deposit| deposit.node)
}

/// How one bot behaves.
#[derive(Clone, Copy)]
pub struct Plan {
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

    fn next_leg(&self, step: usize) -> Option<Leg> {
        match self.approach.get(step) {
            Some(leg) => Some(*leg),
            None if self.circuit.is_empty() => None,
            None => Some(self.circuit[(step - self.approach.len()) % self.circuit.len()]),
        }
    }
}

struct Bot {
    who: CharacterId,
    plan: Plan,
    step: usize,
    free_at: Tick,
}

/// One reading of the zone during a run.
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub vitality: i64,
    /// Everything gathered by everyone since the previous sample.
    pub gathered: u64,
}

/// A shoreline with bots on it.
pub struct Crowd {
    pub session: Session<Game>,
    pub samples: Vec<Sample>,
    bots: Vec<Bot>,
    gathered_since_sample: u64,
}

impl Crowd {
    pub fn new(plans: &[Plan]) -> Self {
        let names: Vec<String> = (0..plans.len()).map(|index| format!("Bot {index}")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let bots = plans
            .iter()
            .enumerate()
            .map(|(index, &plan)| Bot { who: CharacterId::new(index as u64), plan, step: 0, free_at: Tick::ZERO })
            .collect();
        Self { session: Session::new(Game::new(SEED, &names)), samples: Vec::new(), bots, gathered_since_sample: 0 }
    }

    pub fn game(&self) -> &Game {
        self.session.world()
    }

    /// Runs every bot for `time`, sampling the zone as it goes.
    pub fn run(&mut self, time: Duration) {
        let end = self.session.now() + time;
        while self.session.now() < end {
            let until = (self.session.now() + SAMPLE_EVERY).min(end);
            self.run_until(until);
            self.samples.push(Sample { vitality: self.game().vitality(), gathered: self.gathered_since_sample });
            self.gathered_since_sample = 0;
        }
    }

    /// Lets time pass with every bot standing still.
    pub fn rest(&mut self, time: Duration) {
        self.session.advance_to(self.session.now() + time).unwrap();
        let now = self.session.now();
        for bot in &mut self.bots {
            bot.free_at = bot.free_at.max(now);
        }
    }

    /// What one bot carries.
    pub fn carried(&self, index: usize) -> u64 {
        self.game().character(self.bots[index].who).unwrap().carried_total()
    }

    /// What all bots carry between them.
    pub fn carried_by_all(&self) -> u64 {
        (0..self.bots.len()).map(|index| self.carried(index)).sum()
    }

    pub fn lowest_vitality(&self) -> i64 {
        self.samples.iter().map(|sample| sample.vitality).min().unwrap_or(100)
    }

    /// Gathered by everyone in each quarter of the run so far.
    pub fn quarters(&self) -> [u64; 4] {
        let mut quarters = [0; 4];
        for (index, sample) in self.samples.iter().enumerate() {
            quarters[index * 4 / self.samples.len()] += sample.gathered;
        }
        quarters
    }

    /// Acts for whichever bot is free soonest, until none is free before
    /// `until`.
    fn run_until(&mut self, until: Tick) {
        while let Some(index) = self.next_free_before(until) {
            self.take_turn(index);
        }
        self.session.advance_to(until).unwrap();
    }

    fn next_free_before(&self, until: Tick) -> Option<usize> {
        (0..self.bots.len()).min_by_key(|&index| self.bots[index].free_at).filter(|&index| self.bots[index].free_at < until)
    }

    fn take_turn(&mut self, index: usize) {
        let (who, plan, step) = (self.bots[index].who, self.bots[index].plan, self.bots[index].step);
        let now = self.bots[index].free_at.max(self.session.now());

        if (plan.policy)(self.game(), who) {
            let events = self.submit(now, Command::gather(who));
            self.gathered_since_sample += gathered(&events).unwrap_or(0);
            self.bots[index].free_at = self.busy_until(who).max(now + IDLE);
            return;
        }
        let Some(leg) = plan.next_leg(step) else {
            self.bots[index].free_at = now + IDLE;
            return;
        };
        let events = self.submit(now, Command::go(who, leg.exit));
        let closed = outcomes(&events).any(|outcome| matches!(outcome, Outcome::ExitClosed { .. }));
        if closed && leg.wait_if_closed {
            self.bots[index].free_at = now + TIDE_WAIT;
        } else {
            // A step that led nowhere (somewhere flooded) is skipped.
            self.bots[index].step += 1;
            self.bots[index].free_at = self.busy_until(who).max(now + Duration::seconds(1));
        }
    }

    fn submit(&mut self, now: Tick, command: Command) -> Vec<Event> {
        self.session.submit(now, command).unwrap().into_iter().map(|stamped| stamped.value).collect()
    }

    fn busy_until(&self, who: CharacterId) -> Tick {
        self.game().character(who).unwrap().busy_until()
    }
}

/// The outcomes among `events`, whoever they happened to.
pub fn outcomes(events: &[Event]) -> impl Iterator<Item = &Outcome> {
    events.iter().filter_map(|event| match event {
        Event::Character { outcome, .. } => Some(outcome),
        _ => None,
    })
}

/// The amount gathered, if `events` include a haul.
pub fn gathered(events: &[Event]) -> Option<u64> {
    outcomes(events).find_map(|outcome| match outcome {
        Outcome::Gathered { amount, .. } => Some(*amount),
        _ => None,
    })
}
