//! The rules: how commands and the passing of time change the world.

use alloc::vec::Vec;

use eq_clock::Tick;
use eq_events::World;
use eq_rng::Rng;
use eq_world_graph::{Graph, PlaceId};

use crate::character::{Character, CharacterId};
use crate::command::{Action, Command};
use crate::content::{self, BASE_EFFORT, EFFORT_SPREAD, GATHER_TIME, MIN_WORKABLE_STOCK, TAINT_FADES_EVERY, TRAVEL_TIME};
use crate::event::{Event, Outcome};
use crate::haul::{pressure_percent, taint_for, Haul};
use crate::occupancy::Occupancy;
use crate::types::{Health, Place, Standing};
use crate::zone::Zone;

/// The whole world: one zone and the characters in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Game {
    rng: Rng,
    zone: Zone,
    characters: Vec<Character>,
    occupancy: Occupancy,
}

impl Game {
    /// A fresh world with one character per name, all at the starting
    /// place. The same seed and names always play out the same way.
    pub fn new(seed: u64, names: &[&str]) -> Self {
        let (map, start) = content::rocky_shoreline();
        let mut occupancy = Occupancy::new(map.len());
        let characters: Vec<Character> = names.iter().map(|name| Character::new(name, start)).collect();
        for index in 0..characters.len() {
            occupancy.enter(start, id_at(index));
        }
        Self { rng: Rng::from_seed(seed), zone: Zone::new(map), characters, occupancy }
    }

    /// A character, if the id belongs to this world.
    pub fn character(&self, who: CharacterId) -> Option<&Character> {
        self.characters.get(self.index_of(who)?)
    }

    /// Every character with its id, in the order they were named.
    pub fn characters(&self) -> impl Iterator<Item = (CharacterId, &Character)> {
        self.characters.iter().enumerate().map(|(index, character)| (id_at(index), character))
    }

    /// How many characters the world holds.
    pub fn population(&self) -> usize {
        self.characters.len()
    }

    /// The place a character is in.
    pub fn place_of(&self, who: CharacterId) -> Option<&Place> {
        self.zone.map().place(self.character(who)?.place())
    }

    /// Everyone in `place`, in id order.
    pub fn occupants(&self, place: PlaceId) -> &[CharacterId] {
        self.occupancy.at(place)
    }

    /// The zone.
    pub const fn zone(&self) -> &Zone {
        &self.zone
    }

    /// The map.
    pub const fn map(&self) -> &Graph<Place> {
        self.zone.map()
    }

    /// Zone vitality, from 0 to 100.
    pub const fn vitality(&self) -> i64 {
        self.zone.vitality()
    }

    /// The zone's vitality band.
    pub fn health(&self) -> Health {
        self.zone.health()
    }

    /// Everyone who should be told about `event`, in id order.
    ///
    /// This is who is close enough to notice, not who the event is about.
    /// A server sends each event only to its audience, so the cost of an
    /// event grows with the crowd around it, not with the whole world.
    pub fn audience(&self, event: &Event) -> Vec<CharacterId> {
        let mut audience: Vec<CharacterId> = match event {
            Event::Character { who, outcome } => self.audience_of_outcome(*who, outcome),
            Event::UnknownCharacter { .. } => Vec::new(),
            Event::DepositChanged { place, .. } | Event::PassageChanged { place, .. } => self.occupants(*place).to_vec(),
            Event::VitalityChanged { .. } => self.characters().map(|(id, _)| id).collect(),
        };
        audience.sort_unstable();
        audience.dedup();
        audience
    }

    /// The actor always learns their own outcome. Bystanders see only
    /// what is visible: coming, going, gathering and being marked.
    fn audience_of_outcome(&self, who: CharacterId, outcome: &Outcome) -> Vec<CharacterId> {
        let mut audience = alloc::vec![who];
        let here = self.character(who).map(Character::place);
        match outcome {
            Outcome::Moved { from, to } => {
                audience.extend_from_slice(self.occupants(*from));
                audience.extend_from_slice(self.occupants(*to));
            }
            Outcome::Gathered { .. } | Outcome::StandingChanged { to: Standing::Marked, .. } => {
                audience.extend_from_slice(here.map_or(&[], |place| self.occupants(place)));
            }
            _ => {}
        }
        audience
    }

    fn index_of(&self, who: CharacterId) -> Option<usize> {
        usize::try_from(who.get()).ok().filter(|&index| index < self.characters.len())
    }

    fn act(&mut self, index: usize, action: &Action, now: Tick, outcomes: &mut Vec<Outcome>, events: &mut Vec<Event>) {
        let busy_until = self.characters[index].busy_until();
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
        let from = character.place();
        let Some(exit) = self.zone.map().exit(from, label) else {
            outcomes.push(Outcome::NoSuchExit { label: label.into() });
            return;
        };
        if !exit.passage.is_open(now) {
            outcomes.push(Outcome::ExitClosed { label: label.into() });
            return;
        }
        character.place = exit.to;
        character.busy_until = now + TRAVEL_TIME;
        self.occupancy.leave(from, id_at(index));
        self.occupancy.enter(exit.to, id_at(index));
        outcomes.push(Outcome::Moved { from, to: exit.to });
    }

    fn gather(&mut self, index: usize, now: Tick, outcomes: &mut Vec<Outcome>, events: &mut Vec<Event>) {
        let health = self.zone.health();
        let character = &mut self.characters[index];
        let place = character.place();
        let Some(deposit) = self.zone.map_mut().place_mut(place).and_then(|place| place.deposit.as_mut()) else {
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
        character.stow(resource, amount);
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
                events.push(Event::Character { who: id_at(index), outcome });
            }
        }
    }
}

/// The id of the character stored at `index`.
const fn id_at(index: usize) -> CharacterId {
    CharacterId::new(index as u64)
}

impl World for Game {
    type Command = Command;
    type Event = Event;

    fn tick(&mut self, now: Tick, events: &mut Vec<Event>) {
        self.zone.tick(now, events);
        self.fade_taint(now, events);
    }

    fn apply(&mut self, command: &Command, now: Tick, events: &mut Vec<Event>) {
        let who = command.who;
        let Some(index) = self.index_of(who) else {
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
    use crate::types::Resource;

    const ASH: CharacterId = CharacterId::new(0);
    const BRYN: CharacterId = CharacterId::new(1);

    fn outcome(events: &[Event]) -> Option<&Outcome> {
        events.iter().rev().find_map(|event| match event {
            Event::Character { outcome, .. } => Some(outcome),
            _ => None,
        })
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
}
