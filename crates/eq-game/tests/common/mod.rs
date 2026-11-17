//! A shoreline with bots on it, sampled as it runs, for testing what
//! crowds do to a zone. The bots themselves are `eq_game::bots`.
#![allow(dead_code)] // each test file uses a different part of this

use eq_clock::Duration;
use eq_events::Session;
use eq_game::bots::{Bots, Plan};
use eq_game::{CharacterId, Event, Game, Outcome};

pub const SEED: u64 = 2026;

/// How often the zone is sampled during a run.
pub const SAMPLE_EVERY: Duration = Duration::seconds(100);

/// One reading of the zone during a run.
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub vitality: i64,
    /// Everything gathered by everyone since the previous sample.
    pub gathered: u64,
}

/// The names a crowd of `heads` bots is given.
pub fn names(heads: usize) -> Vec<String> {
    (0..heads).map(|index| format!("Bot {index}")).collect()
}

/// A fresh world holding a crowd of `heads` bots, as `Crowd::new` builds
/// it. Replays start from this.
pub fn fresh_world(heads: usize) -> Game {
    let names = names(heads);
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    Game::new(SEED, &names)
}

/// A shoreline with bots on it.
pub struct Crowd {
    pub session: Session<Game>,
    pub samples: Vec<Sample>,
    bots: Bots,
}

impl Crowd {
    pub fn new(plans: &[Plan]) -> Self {
        let crew = plans.iter().enumerate().map(|(index, &plan)| (CharacterId::new(index as u64), plan));
        Self { session: Session::new(fresh_world(plans.len())), samples: Vec::new(), bots: Bots::new(crew) }
    }

    pub fn game(&self) -> &Game {
        self.session.world()
    }

    /// Runs every bot for `time`, sampling the zone as it goes.
    pub fn run(&mut self, time: Duration) {
        let end = self.session.now() + time;
        while self.session.now() < end {
            let until = (self.session.now() + SAMPLE_EVERY).min(end);
            let events = self.bots.run(&mut self.session, until).unwrap();
            let gathered = events.iter().filter_map(|stamped| haul(&stamped.value)).sum();
            self.samples.push(Sample { vitality: self.game().vitality(), gathered });
        }
    }

    /// Lets time pass with every bot standing still.
    pub fn rest(&mut self, time: Duration) {
        self.session.advance_to(self.session.now() + time).unwrap();
        self.bots.stand_still_until(self.session.now());
    }

    /// What one bot carries.
    pub fn carried(&self, index: usize) -> u64 {
        self.game().character(CharacterId::new(index as u64)).unwrap().carried_total()
    }

    /// What all bots carry between them.
    pub fn carried_by_all(&self) -> u64 {
        self.game().characters().map(|(_, character)| character.carried_total()).sum()
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
}

/// The amount gathered, if `event` is a haul.
pub fn haul(event: &Event) -> Option<u64> {
    match event {
        Event::Character { outcome: Outcome::Gathered { amount, .. }, .. } => Some(*amount),
        _ => None,
    }
}

/// The amount gathered, if `events` include a haul.
pub fn gathered(events: &[Event]) -> Option<u64> {
    events.iter().find_map(haul)
}
