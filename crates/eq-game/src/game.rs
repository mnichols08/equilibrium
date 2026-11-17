//! The rules.

use alloc::string::String;
use alloc::vec::Vec;

use eq_clock::Tick;
use eq_curve::stepped_bonus;
use eq_events::World;
use eq_meter::Meter;
use eq_nodes::Quality;
use eq_rng::Rng;
use eq_skills::Proficiency;
use eq_world_graph::{Graph, Passage, PlaceId};

use crate::content::{
    self, ATTRIBUTE_RULES, ATTRIBUTE_XP_PER_UNIT, BASE_EFFORT, EFFORT_SPREAD, GATHER_TIME, MIN_WORKABLE_STOCK,
    SKILL_RULES, SKILL_XP_PER_UNIT, TIERS_PER_WHOLE, TRAVEL_TIME, VITALITY_BANDS, YIELD_PERCENT_PER_LEVEL,
};
use crate::types::{Attribute, Health, Place, Resource, SkillKind};

/// Something the player asks their character to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Leave by the exit with this label.
    Go(String),
    /// Gather from the deposit here.
    Gather,
}

/// Something that happened in the world.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// The character walked from one place to another.
    Moved {
        /// Where they left.
        from: PlaceId,
        /// Where they arrived.
        to: PlaceId,
    },
    /// There is no exit with that label here.
    NoSuchExit {
        /// What was asked for.
        label: String,
    },
    /// The exit exists but cannot be used right now.
    ExitClosed {
        /// The exit's label.
        label: String,
    },
    /// The character is still finishing their last action.
    Busy {
        /// When they will be free.
        until: Tick,
    },
    /// There is no deposit here.
    NothingToGather,
    /// The deposit here has too little stock to be worth working.
    TooSparse {
        /// What would have been gathered.
        resource: Resource,
    },
    /// The character gathered something.
    Gathered {
        /// What was gathered.
        resource: Resource,
        /// How much went into the pack.
        amount: u64,
        /// The deposit's quality when the work began.
        quality: Quality,
    },
    /// A deposit became more or less worn.
    DepositChanged {
        /// Where the deposit is.
        place: PlaceId,
        /// What it gives.
        resource: Resource,
        /// Its quality before.
        from: Quality,
        /// Its quality now.
        to: Quality,
    },
    /// A skill reached a new level.
    SkillRose {
        /// Which skill.
        skill: SkillKind,
        /// The new level.
        level: u32,
    },
    /// An attribute reached a new level.
    AttributeRose {
        /// Which attribute.
        attribute: Attribute,
        /// The new level.
        level: u32,
    },
    /// The zone moved from one vitality band to another.
    VitalityChanged {
        /// The band before.
        from: Health,
        /// The band now.
        to: Health,
    },
    /// A cyclic exit opened or closed.
    PassageChanged {
        /// The place the exit leaves.
        place: PlaceId,
        /// The exit's label.
        label: String,
        /// Whether it is now open.
        open: bool,
    },
}

/// The player's character.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Character {
    place: PlaceId,
    busy_until: Tick,
    skills: [Proficiency; SkillKind::ALL.len()],
    attributes: [Proficiency; Attribute::ALL.len()],
    pack: [u64; Resource::ALL.len()],
}

impl Character {
    fn new(place: PlaceId) -> Self {
        Self {
            place,
            busy_until: Tick::ZERO,
            skills: [Proficiency::new(); SkillKind::ALL.len()],
            attributes: [Proficiency::new(); Attribute::ALL.len()],
            pack: [0; Resource::ALL.len()],
        }
    }

    /// Where the character is.
    pub const fn place(&self) -> PlaceId {
        self.place
    }

    /// When the character's current action ends.
    pub const fn busy_until(&self) -> Tick {
        self.busy_until
    }

    /// A skill's progress.
    pub const fn skill(&self, skill: SkillKind) -> &Proficiency {
        &self.skills[skill.index()]
    }

    /// An attribute's progress.
    pub const fn attribute(&self, attribute: Attribute) -> &Proficiency {
        &self.attributes[attribute.index()]
    }

    /// How much of a resource the character carries.
    pub const fn carried(&self, resource: Resource) -> u64 {
        self.pack[resource.index()]
    }

    /// Trains the skill and attributes that gathering `resource` uses.
    fn train(&mut self, resource: Resource, units: u32, events: &mut Vec<Event>) {
        let units = u64::from(units);
        let skill = resource.skill();
        let progress = &mut self.skills[skill.index()];
        if progress.practice(units * SKILL_XP_PER_UNIT, &SKILL_RULES).levels_gained > 0 {
            events.push(Event::SkillRose { skill, level: progress.level() });
        }

        let xp = stepped_bonus(units * ATTRIBUTE_XP_PER_UNIT, resource.tier(), TIERS_PER_WHOLE);
        for &attribute in resource.attributes() {
            let progress = &mut self.attributes[attribute.index()];
            if progress.practice(xp, &ATTRIBUTE_RULES).levels_gained > 0 {
                events.push(Event::AttributeRose { attribute, level: progress.level() });
            }
        }
    }
}

/// How much of a harvest reaches the pack.
///
/// `taken × quality × zone health × (1 + attribute level / 100)`, rounded
/// down once at the end.
pub fn yield_amount(taken: u32, quality: Quality, health: Health, attribute_level: u32) -> u64 {
    let skill_percent = 100 + u64::from(attribute_level) * YIELD_PERCENT_PER_LEVEL;
    let parts = u64::from(taken)
        * u64::from(quality.yield_percent())
        * u64::from(health.yield_percent())
        * skill_percent;
    parts / 1_000_000
}

/// The whole world: one zone, one character.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Game {
    rng: Rng,
    map: Graph<Place>,
    hero: Character,
    vitality: Meter,
}

impl Game {
    /// A fresh world. The same seed always plays out the same way.
    pub fn new(seed: u64) -> Self {
        let (map, start) = content::rocky_shoreline();
        Self { rng: Rng::from_seed(seed), map, hero: Character::new(start), vitality: Meter::full(0, 100) }
    }

    /// The player's character.
    pub const fn hero(&self) -> &Character {
        &self.hero
    }

    /// The map.
    pub const fn map(&self) -> &Graph<Place> {
        &self.map
    }

    /// The place the character is in.
    pub fn here(&self) -> &Place {
        self.map.place(self.hero.place).expect("the character is always somewhere on the map")
    }

    /// Zone vitality, from 0 to 100.
    pub const fn vitality(&self) -> i64 {
        self.vitality.value()
    }

    /// The zone's vitality band.
    pub fn health(&self) -> Health {
        VITALITY_BANDS.band_of(self.vitality.value())
    }

    fn go(&mut self, label: &str, now: Tick, events: &mut Vec<Event>) {
        let from = self.hero.place;
        let Some(exit) = self.map.exit(from, label) else {
            events.push(Event::NoSuchExit { label: label.into() });
            return;
        };
        if !exit.passage.is_open(now) {
            events.push(Event::ExitClosed { label: label.into() });
            return;
        }
        self.hero.place = exit.to;
        self.hero.busy_until = now + TRAVEL_TIME;
        events.push(Event::Moved { from, to: exit.to });
    }

    fn gather(&mut self, now: Tick, events: &mut Vec<Event>) {
        let place = self.hero.place;
        let health = self.health();
        let Some(deposit) = self.map.place_mut(place).and_then(|place| place.deposit.as_mut()) else {
            events.push(Event::NothingToGather);
            return;
        };
        let resource = deposit.resource;
        if deposit.node.stock() < MIN_WORKABLE_STOCK {
            events.push(Event::TooSparse { resource });
            return;
        }

        // EFFORT_SPREAD is tiny, so the cast cannot truncate.
        let effort = BASE_EFFORT + self.rng.below(EFFORT_SPREAD) as u32;
        let harvest = deposit.node.harvest(effort);
        let quality_after = deposit.node.quality();

        let governing = resource.attributes()[0];
        let level = self.hero.attribute(governing).level();
        let amount = yield_amount(harvest.taken, harvest.quality, health, level);
        self.hero.pack[resource.index()] += amount;
        self.hero.busy_until = now + GATHER_TIME;

        events.push(Event::Gathered { resource, amount, quality: harvest.quality });
        if quality_after != harvest.quality {
            events.push(Event::DepositChanged { place, resource, from: harvest.quality, to: quality_after });
        }
        self.hero.train(resource, harvest.taken, events);
    }

    fn grow_deposits(&mut self, events: &mut Vec<Event>) {
        let growth = self.health().growth();
        for (place, data) in self.map.places_mut() {
            let Some(deposit) = data.deposit.as_mut() else { continue };
            let from = deposit.node.quality();
            deposit.node.tick(growth);
            let to = deposit.node.quality();
            if from != to {
                events.push(Event::DepositChanged { place, resource: deposit.resource, from, to });
            }
        }
    }

    /// Vitality is 100 minus the average wear across the zone's deposits.
    /// Wear is zero while a deposit is pristine, so careful use costs the
    /// zone nothing; only over-harvesting does.
    fn update_vitality(&mut self, events: &mut Vec<Event>) {
        let (wear, deposits) = self
            .map
            .places()
            .filter_map(|(_, place)| place.deposit.as_ref())
            .fold((0i64, 0i64), |(wear, count), deposit| {
                (wear + i64::from(deposit.node.wear_percent()), count + 1)
            });
        let penalty = if deposits == 0 { 0 } else { wear / deposits };
        let change = self.vitality.set(100 - penalty);
        if let Some((from, to)) = VITALITY_BANDS.crossing(change) {
            events.push(Event::VitalityChanged { from, to });
        }
    }

    fn report_passages(&self, now: Tick, events: &mut Vec<Event>) {
        let Some(previous) = now.get().checked_sub(1).map(Tick::new) else { return };
        for (place, exit) in self.map.all_exits() {
            if matches!(exit.passage, Passage::Open) {
                continue;
            }
            let open = exit.passage.is_open(now);
            if open != exit.passage.is_open(previous) {
                events.push(Event::PassageChanged { place, label: exit.label.clone(), open });
            }
        }
    }
}

impl World for Game {
    type Command = Command;
    type Event = Event;

    fn tick(&mut self, now: Tick, events: &mut Vec<Event>) {
        self.grow_deposits(events);
        self.update_vitality(events);
        self.report_passages(now, events);
    }

    fn apply(&mut self, command: &Command, now: Tick, events: &mut Vec<Event>) {
        if now < self.hero.busy_until {
            events.push(Event::Busy { until: self.hero.busy_until });
            return;
        }
        match command {
            Command::Go(label) => self.go(label, now, events),
            Command::Gather => self.gather(now, events),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yield_follows_quality_health_and_attribute() {
        assert_eq!(yield_amount(10, Quality::Pristine, Health::Balanced, 0), 10);
        assert_eq!(yield_amount(10, Quality::Pristine, Health::Peak, 0), 12);
        assert_eq!(yield_amount(10, Quality::Depleted, Health::Balanced, 0), 2);
        assert_eq!(yield_amount(10, Quality::Depleted, Health::Collapsed, 0), 1);
        assert_eq!(yield_amount(10, Quality::Pristine, Health::Balanced, 50), 15);
    }

    #[test]
    fn yield_never_falls_as_the_attribute_rises() {
        let mut previous = 0;
        for level in 0..=100 {
            let amount = yield_amount(12, Quality::Healthy, Health::Balanced, level);
            assert!(amount >= previous);
            previous = amount;
        }
    }

    #[test]
    fn a_fresh_world_is_at_peak_health_at_camp() {
        let game = Game::new(1);
        assert_eq!((game.vitality(), game.health()), (100, Health::Peak));
        assert_eq!(game.here().name, "Tidewatch Camp");
    }

    #[test]
    fn commands_are_refused_while_busy() {
        let mut game = Game::new(1);
        let mut events = Vec::new();
        game.apply(&Command::Go("north".into()), Tick::new(0), &mut events);
        game.apply(&Command::Gather, Tick::new(1), &mut events);
        assert_eq!(events.last(), Some(&Event::Busy { until: Tick::ZERO + TRAVEL_TIME }));
    }

    #[test]
    fn the_cave_floods_at_high_tide() {
        let mut game = Game::new(1);
        let mut events = Vec::new();
        game.apply(&Command::Go("east".into()), Tick::new(0), &mut events);
        game.apply(&Command::Go("cave".into()), Tick::new(700), &mut events);
        assert_eq!(events.last(), Some(&Event::ExitClosed { label: "cave".into() }));
        game.apply(&Command::Go("cave".into()), Tick::new(1_200), &mut events);
        assert_eq!(game.here().name, "Tidal Cave");
    }

    #[test]
    fn the_tide_turning_is_reported_once_per_side() {
        let mut game = Game::new(1);
        let mut events = Vec::new();
        for tick in 1..=600 {
            game.tick(Tick::new(tick), &mut events);
        }
        let closings = events.iter().filter(|event| matches!(event, Event::PassageChanged { open: false, .. })).count();
        assert_eq!(closings, 2);
    }
}
