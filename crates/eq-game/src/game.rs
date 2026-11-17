//! The rules.

use alloc::string::String;
use alloc::vec::Vec;

use eq_clock::Tick;
use eq_curve::stepped_bonus;
use eq_events::World;
use eq_meter::Meter;
use eq_nodes::{Node, Quality};
use eq_rng::Rng;
use eq_skills::Proficiency;
use eq_world_graph::{Graph, Passage, PlaceId};

use crate::content::{
    self, ATTRIBUTE_RULES, ATTRIBUTE_XP_PER_UNIT, BASE_EFFORT, EFFORT_SPREAD, GATHER_TIME, MIN_WORKABLE_STOCK,
    SKILL_RULES, SKILL_XP_PER_UNIT, STANDING_BANDS, TAINT_AT_FULL_PRESSURE, TAINT_FADES_EVERY, TIERS_PER_WHOLE,
    TRAVEL_TIME, VITALITY_BANDS, YIELD_LOST_AT_FULL_TAINT, YIELD_PERCENT_PER_LEVEL,
};
use crate::types::{Attribute, Health, Place, Resource, SkillKind, Standing};

eq_ids::define_id!(
    /// Identifies a character within one [`Game`].
    CharacterId
);

/// Something a character can do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Leave by the exit with this label.
    Go(String),
    /// Gather from the deposit here.
    Gather,
}

/// An action and the character asked to take it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    /// Who acts.
    pub who: CharacterId,
    /// What they do.
    pub action: Action,
}

impl Command {
    /// `who` leaves by the exit labelled `label`.
    pub fn go(who: CharacterId, label: &str) -> Self {
        Self { who, action: Action::Go(label.into()) }
    }

    /// `who` gathers from the deposit where they stand.
    pub const fn gather(who: CharacterId) -> Self {
        Self { who, action: Action::Gather }
    }
}

/// What came of one character's action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// They walked from one place to another.
    Moved {
        /// Where they left.
        from: PlaceId,
        /// Where they arrived.
        to: PlaceId,
    },
    /// There is no exit with that label where they stand.
    NoSuchExit {
        /// What was asked for.
        label: String,
    },
    /// The exit exists but cannot be used right now.
    ExitClosed {
        /// The exit's label.
        label: String,
    },
    /// They are still finishing their last action.
    Busy {
        /// When they will be free.
        until: Tick,
    },
    /// There is no deposit where they stand.
    NothingToGather,
    /// The deposit has too little stock to be worth working.
    TooSparse {
        /// What would have been gathered.
        resource: Resource,
    },
    /// They gathered something.
    Gathered {
        /// What was gathered.
        resource: Resource,
        /// How much went into the pack.
        amount: u64,
        /// The deposit's quality when the work began.
        quality: Quality,
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
    /// Their taint carried them from one standing to another.
    StandingChanged {
        /// Their standing before.
        from: Standing,
        /// Their standing now.
        to: Standing,
    },
}

/// Something that happened in the world.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// Something one character did or experienced.
    Character {
        /// Who it happened to.
        who: CharacterId,
        /// What happened.
        outcome: Outcome,
    },
    /// A command named a character who does not exist.
    UnknownCharacter {
        /// The id that was asked for.
        who: CharacterId,
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

/// One character in the world.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Character {
    name: String,
    place: PlaceId,
    busy_until: Tick,
    skills: [Proficiency; SkillKind::ALL.len()],
    attributes: [Proficiency; Attribute::ALL.len()],
    pack: [u64; Resource::ALL.len()],
    taint: Meter,
}

impl Character {
    fn new(name: &str, place: PlaceId) -> Self {
        Self {
            name: name.into(),
            place,
            busy_until: Tick::ZERO,
            skills: [Proficiency::new(); SkillKind::ALL.len()],
            attributes: [Proficiency::new(); Attribute::ALL.len()],
            pack: [0; Resource::ALL.len()],
            taint: Meter::empty(0, 100),
        }
    }

    /// What the character is called.
    pub fn name(&self) -> &str {
        &self.name
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

    /// Everything the character carries, added together.
    pub fn carried_total(&self) -> u64 {
        self.pack.iter().sum()
    }

    /// How tainted the character is by over-harvesting, from 0 to 100.
    pub const fn taint(&self) -> u32 {
        // The meter is bounded to 0..=100.
        self.taint.value() as u32
    }

    /// How the land regards the character.
    pub fn standing(&self) -> Standing {
        STANDING_BANDS.band_of(self.taint.value())
    }

    /// Moves taint by `delta` and reports a change of standing, if any.
    fn shift_taint(&mut self, delta: i64) -> Option<Outcome> {
        let change = self.taint.adjust(delta);
        STANDING_BANDS.crossing(change).map(|(from, to)| Outcome::StandingChanged { from, to })
    }

    /// Trains the skill and attributes that gathering `resource` uses.
    fn train(&mut self, resource: Resource, units: u32, outcomes: &mut Vec<Outcome>) {
        let units = u64::from(units);
        let skill = resource.skill();
        let progress = &mut self.skills[skill.index()];
        if progress.practice(units * SKILL_XP_PER_UNIT, &SKILL_RULES).levels_gained > 0 {
            outcomes.push(Outcome::SkillRose { skill, level: progress.level() });
        }

        let xp = stepped_bonus(units * ATTRIBUTE_XP_PER_UNIT, resource.tier(), TIERS_PER_WHOLE);
        for &attribute in resource.attributes() {
            let progress = &mut self.attributes[attribute.index()];
            if progress.practice(xp, &ATTRIBUTE_RULES).levels_gained > 0 {
                outcomes.push(Outcome::AttributeRose { attribute, level: progress.level() });
            }
        }
    }
}

/// Everything that decides how much of a harvest reaches the pack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Haul {
    /// Units removed from the deposit.
    pub taken: u32,
    /// The deposit's quality when the work began.
    pub quality: Quality,
    /// The zone's health.
    pub health: Health,
    /// The gatherer's level in the attribute that governs this resource.
    pub attribute_level: u32,
    /// The gatherer's taint, from 0 to 100.
    pub taint: u32,
}

impl Haul {
    /// How much reaches the pack:
    ///
    /// `taken × quality × zone health × (1 + attribute level / 100) × (1 − taint loss)`
    ///
    /// rounded down once at the end.
    pub fn amount(&self) -> u64 {
        const WHOLE: u64 = 100;
        let attribute_percent = WHOLE + u64::from(self.attribute_level) * YIELD_PERCENT_PER_LEVEL;
        let taint_percent = WHOLE - u64::from(self.taint.min(100) * YIELD_LOST_AT_FULL_TAINT) / WHOLE;
        let parts = u64::from(self.taken)
            * u64::from(self.quality.yield_percent())
            * u64::from(self.health.yield_percent())
            * attribute_percent
            * taint_percent;
        parts / WHOLE.pow(4)
    }
}

/// Taint earned by one haul from a deposit under `pressure` percent.
pub fn taint_for(pressure: u32) -> i64 {
    i64::from(pressure.min(100) * TAINT_AT_FULL_PRESSURE / 100)
}

/// How hard a deposit is being pushed, from 0 to 100: the greater of its
/// wear and its bareness.
///
/// Wear comes from taking fast. Bareness comes from never letting the
/// stock recover, however little each visitor takes. Either one means the
/// deposit is over-harvested.
pub fn pressure_percent(node: &Node) -> u32 {
    node.wear_percent().max(node.bareness_percent())
}

/// The whole world: one zone and the characters in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Game {
    rng: Rng,
    map: Graph<Place>,
    characters: Vec<Character>,
    vitality: Meter,
}

impl Game {
    /// A fresh world with one character per name, all at the starting
    /// place. The same seed and names always play out the same way.
    pub fn new(seed: u64, names: &[&str]) -> Self {
        let (map, start) = content::rocky_shoreline();
        let characters = names.iter().map(|name| Character::new(name, start)).collect();
        Self { rng: Rng::from_seed(seed), map, characters, vitality: Meter::full(0, 100) }
    }

    /// A character, if the id belongs to this world.
    pub fn character(&self, who: CharacterId) -> Option<&Character> {
        self.characters.get(usize::try_from(who.get()).ok()?)
    }

    /// Every character with its id, in the order they were named.
    pub fn characters(&self) -> impl Iterator<Item = (CharacterId, &Character)> {
        self.characters.iter().enumerate().map(|(index, character)| (CharacterId::new(index as u64), character))
    }

    /// The place a character is in.
    pub fn place_of(&self, who: CharacterId) -> Option<&Place> {
        self.map.place(self.character(who)?.place)
    }

    /// The map.
    pub const fn map(&self) -> &Graph<Place> {
        &self.map
    }

    /// Zone vitality, from 0 to 100.
    pub const fn vitality(&self) -> i64 {
        self.vitality.value()
    }

    /// The zone's vitality band.
    pub fn health(&self) -> Health {
        VITALITY_BANDS.band_of(self.vitality.value())
    }

    fn act(&mut self, index: usize, action: &Action, now: Tick, outcomes: &mut Vec<Outcome>, events: &mut Vec<Event>) {
        let busy_until = self.characters[index].busy_until;
        if now < busy_until {
            outcomes.push(Outcome::Busy { until: busy_until });
            return;
        }
        match action {
            Action::Go(label) => self.go(index, label, now, outcomes),
            Action::Gather => self.gather(index, now, outcomes, events),
        }
    }

    fn go(&mut self, index: usize, label: &str, now: Tick, outcomes: &mut Vec<Outcome>) {
        let character = &mut self.characters[index];
        let from = character.place;
        let Some(exit) = self.map.exit(from, label) else {
            outcomes.push(Outcome::NoSuchExit { label: label.into() });
            return;
        };
        if !exit.passage.is_open(now) {
            outcomes.push(Outcome::ExitClosed { label: label.into() });
            return;
        }
        character.place = exit.to;
        character.busy_until = now + TRAVEL_TIME;
        outcomes.push(Outcome::Moved { from, to: exit.to });
    }

    fn gather(&mut self, index: usize, now: Tick, outcomes: &mut Vec<Outcome>, events: &mut Vec<Event>) {
        let health = self.health();
        let character = &mut self.characters[index];
        let place = character.place;
        let Some(deposit) = self.map.place_mut(place).and_then(|place| place.deposit.as_mut()) else {
            outcomes.push(Outcome::NothingToGather);
            return;
        };
        let resource = deposit.resource;
        if deposit.node.stock() < MIN_WORKABLE_STOCK {
            outcomes.push(Outcome::TooSparse { resource });
            return;
        }

        // EFFORT_SPREAD is tiny, so the cast cannot truncate.
        let effort = BASE_EFFORT + self.rng.below(EFFORT_SPREAD) as u32;
        let pressure_before = pressure_percent(&deposit.node);
        let harvest = deposit.node.harvest(effort);
        let quality_after = deposit.node.quality();

        let haul = Haul {
            taken: harvest.taken,
            quality: harvest.quality,
            health,
            attribute_level: character.attribute(resource.attributes()[0]).level(),
            taint: character.taint(),
        };
        let amount = haul.amount();
        character.pack[resource.index()] += amount;
        character.busy_until = now + GATHER_TIME;

        outcomes.push(Outcome::Gathered { resource, amount, quality: harvest.quality });
        if quality_after != harvest.quality {
            events.push(Event::DepositChanged { place, resource, from: harvest.quality, to: quality_after });
        }
        character.train(resource, harvest.taken, outcomes);
        // Working a deposit that was already under pressure taints the one
        // who does it. The haul that first pushes it over does not.
        outcomes.extend(character.shift_taint(taint_for(pressure_before)));
    }

    fn fade_taint(&mut self, now: Tick, events: &mut Vec<Event>) {
        if !now.get().is_multiple_of(TAINT_FADES_EVERY.as_ticks()) {
            return;
        }
        for (index, character) in self.characters.iter_mut().enumerate() {
            if let Some(outcome) = character.shift_taint(-1) {
                events.push(Event::Character { who: CharacterId::new(index as u64), outcome });
            }
        }
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

    /// Vitality is 100 minus the average pressure on the zone's deposits.
    /// A deposit that is pristine and at least half full is under no
    /// pressure, so careful use costs the zone nothing.
    fn update_vitality(&mut self, events: &mut Vec<Event>) {
        let (pressure, deposits) = self
            .map
            .places()
            .filter_map(|(_, place)| place.deposit.as_ref())
            .fold((0i64, 0i64), |(pressure, count), deposit| {
                (pressure + i64::from(pressure_percent(&deposit.node)), count + 1)
            });
        let penalty = if deposits == 0 { 0 } else { pressure / deposits };
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
        self.fade_taint(now, events);
    }

    fn apply(&mut self, command: &Command, now: Tick, events: &mut Vec<Event>) {
        let who = command.who;
        let Some(index) = usize::try_from(who.get()).ok().filter(|&index| index < self.characters.len()) else {
            events.push(Event::UnknownCharacter { who });
            return;
        };
        // What the character did comes first, then what it did to the world.
        let mut outcomes = Vec::new();
        let mut world_events = Vec::new();
        self.act(index, &command.action, now, &mut outcomes, &mut world_events);
        events.extend(outcomes.into_iter().map(|outcome| Event::Character { who, outcome }));
        events.append(&mut world_events);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASH: CharacterId = CharacterId::new(0);
    const BRYN: CharacterId = CharacterId::new(1);

    fn outcome(events: &[Event]) -> Option<&Outcome> {
        events.iter().rev().find_map(|event| match event {
            Event::Character { outcome, .. } => Some(outcome),
            _ => None,
        })
    }

    const fn haul(quality: Quality, health: Health, attribute_level: u32, taint: u32) -> Haul {
        Haul { taken: 10, quality, health, attribute_level, taint }
    }

    #[test]
    fn yield_follows_quality_health_attribute_and_taint() {
        assert_eq!(haul(Quality::Pristine, Health::Balanced, 0, 0).amount(), 10);
        assert_eq!(haul(Quality::Pristine, Health::Peak, 0, 0).amount(), 12);
        assert_eq!(haul(Quality::Depleted, Health::Balanced, 0, 0).amount(), 2);
        assert_eq!(haul(Quality::Depleted, Health::Collapsed, 0, 0).amount(), 1);
        assert_eq!(haul(Quality::Pristine, Health::Balanced, 50, 0).amount(), 15);
        assert_eq!(haul(Quality::Pristine, Health::Balanced, 0, 40).amount(), 7);
        assert_eq!(haul(Quality::Pristine, Health::Balanced, 0, 100).amount(), 2);
    }

    #[test]
    fn yield_rises_with_the_attribute_and_falls_with_taint() {
        let by_level = (0..=100).map(|level| haul(Quality::Healthy, Health::Balanced, level, 0).amount());
        assert!(by_level.collect::<Vec<_>>().windows(2).all(|pair| pair[0] <= pair[1]));
        let by_taint = (0..=100).map(|taint| haul(Quality::Healthy, Health::Balanced, 0, taint).amount());
        assert!(by_taint.collect::<Vec<_>>().windows(2).all(|pair| pair[0] >= pair[1]));
    }

    #[test]
    fn taint_scales_with_pressure_and_careful_hauls_earn_none() {
        assert_eq!(taint_for(0), 0);
        assert_eq!(taint_for(50), 5);
        assert_eq!(taint_for(100), 10);
        assert_eq!(taint_for(999), 10);
    }

    #[test]
    fn stripping_a_deposit_marks_you_and_the_mark_fades() {
        let mut game = Game::new(1, &["Ash", "Bryn"]);
        let mut events = Vec::new();
        game.apply(&Command::go(ASH, "north"), Tick::new(0), &mut events);
        let mut now = 30;
        while game.place_of(ASH).unwrap().deposit.unwrap().node.stock() >= MIN_WORKABLE_STOCK {
            game.apply(&Command::gather(ASH), Tick::new(now), &mut events);
            now += GATHER_TIME.as_ticks();
        }
        let marked = Outcome::StandingChanged { from: Standing::Clean, to: Standing::Marked };
        assert!(events.contains(&Event::Character { who: ASH, outcome: marked }));
        assert_eq!(game.character(ASH).unwrap().standing(), Standing::Marked);
        assert_eq!(game.character(BRYN).unwrap().taint(), 0);

        events.clear();
        for tick in now..now + 100 * TAINT_FADES_EVERY.as_ticks() {
            game.tick(Tick::new(tick), &mut events);
        }
        let cleansed = Outcome::StandingChanged { from: Standing::Marked, to: Standing::Clean };
        assert!(events.contains(&Event::Character { who: ASH, outcome: cleansed }));
        assert_eq!(game.character(ASH).unwrap().taint(), 0);
    }

    #[test]
    fn a_fresh_world_is_at_peak_health_with_everyone_at_camp() {
        let game = Game::new(1, &["Ash", "Bryn"]);
        assert_eq!((game.vitality(), game.health()), (100, Health::Peak));
        assert_eq!(game.characters().count(), 2);
        assert_eq!(game.place_of(BRYN).unwrap().name, "Tidewatch Camp");
        assert_eq!(game.character(BRYN).unwrap().name(), "Bryn");
    }

    #[test]
    fn commands_are_refused_while_busy() {
        let mut game = Game::new(1, &["Ash"]);
        let mut events = Vec::new();
        game.apply(&Command::go(ASH, "north"), Tick::new(0), &mut events);
        game.apply(&Command::gather(ASH), Tick::new(1), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::Busy { until: Tick::ZERO + TRAVEL_TIME }));
    }

    #[test]
    fn one_character_being_busy_does_not_hold_up_another() {
        let mut game = Game::new(1, &["Ash", "Bryn"]);
        let mut events = Vec::new();
        game.apply(&Command::go(ASH, "north"), Tick::new(0), &mut events);
        game.apply(&Command::go(BRYN, "east"), Tick::new(1), &mut events);
        assert_eq!(game.place_of(ASH).unwrap().name, "Ore Ledge");
        assert_eq!(game.place_of(BRYN).unwrap().name, "Shingle Beach");
    }

    #[test]
    fn two_characters_draw_on_the_same_deposit() {
        let mut game = Game::new(1, &["Ash", "Bryn"]);
        let mut events = Vec::new();
        game.apply(&Command::go(ASH, "north"), Tick::new(0), &mut events);
        game.apply(&Command::go(BRYN, "north"), Tick::new(0), &mut events);
        let before = game.place_of(ASH).unwrap().deposit.unwrap().node.stock();
        game.apply(&Command::gather(ASH), Tick::new(30), &mut events);
        game.apply(&Command::gather(BRYN), Tick::new(30), &mut events);
        let after = game.place_of(ASH).unwrap().deposit.unwrap().node.stock();

        assert!(before - after >= 2 * BASE_EFFORT);
        assert!(game.character(ASH).unwrap().carried(Resource::Ore) > 0);
        assert!(game.character(BRYN).unwrap().carried(Resource::Ore) > 0);
    }

    #[test]
    fn commands_for_unknown_characters_change_nothing() {
        let mut game = Game::new(1, &["Ash"]);
        let before = game.clone();
        let mut events = Vec::new();
        let ghost = CharacterId::new(9);
        game.apply(&Command::gather(ghost), Tick::new(0), &mut events);
        assert_eq!(events, [Event::UnknownCharacter { who: ghost }]);
        assert_eq!(game, before);
    }

    #[test]
    fn the_cave_floods_at_high_tide() {
        let mut game = Game::new(1, &["Ash"]);
        let mut events = Vec::new();
        game.apply(&Command::go(ASH, "east"), Tick::new(0), &mut events);
        game.apply(&Command::go(ASH, "cave"), Tick::new(700), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::ExitClosed { label: "cave".into() }));
        game.apply(&Command::go(ASH, "cave"), Tick::new(1_200), &mut events);
        assert_eq!(game.place_of(ASH).unwrap().name, "Tidal Cave");
    }

    #[test]
    fn the_tide_turning_is_reported_once_per_side_of_each_tidal_way() {
        let mut game = Game::new(1, &["Ash"]);
        let mut events = Vec::new();
        for tick in 1..=600 {
            game.tick(Tick::new(tick), &mut events);
        }
        let closings = events.iter().filter(|event| matches!(event, Event::PassageChanged { open: false, .. })).count();
        // The cave mouth and the reef crossing, each seen from both ends.
        assert_eq!(closings, 4);
    }

    #[test]
    fn pressure_is_the_worse_of_wear_and_bareness() {
        let config = eq_nodes::NodeConfig { capacity: 100, regrow_cost: 20, strain_per_unit: 1, recovery_every: 2 };
        // Emptied gently: no wear, fully bare.
        let mut bare = Node::new(config);
        bare.harvest(100);
        assert_eq!((bare.wear_percent(), bare.bareness_percent(), pressure_percent(&bare)), (0, 100, 100));
        // Hit hard but still mostly full: worn, not bare.
        let mut worn = Node::new(eq_nodes::NodeConfig { strain_per_unit: 20, ..config });
        worn.harvest(40);
        assert_eq!((worn.wear_percent(), worn.bareness_percent(), pressure_percent(&worn)), (73, 0, 73));
    }
}
