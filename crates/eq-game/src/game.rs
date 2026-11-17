//! The rules: how commands and the passing of time change the world.

use alloc::vec::Vec;

use eq_clock::Tick;
use eq_events::World;
use eq_rng::Rng;
use eq_world_graph::{Graph, PlaceId};

use crate::character::{Character, CharacterId};
use crate::combat::{may_attack, Blow, Refusal};
use crate::command::{Action, Command};
use crate::content::{
    self, ATTACK_TIME, BASE_EFFORT, DAMAGE_SPREAD, DOWNED_TIME, EFFORT_SPREAD, GATHER_TIME, HAVEN_MENDING,
    HEALTH_MENDS_EVERY, MENDING, MIN_WORKABLE_STOCK, TAINT_FADES_EVERY, TAKE_TIME, TRAVEL_TIME,
};
use crate::event::{Event, Outcome};
use crate::haul::{pressure_percent, taint_for, Haul};
use crate::occupancy::Occupancy;
use crate::types::{Attribute, Health, Place, Safety, Standing};
use crate::zone::Zone;

/// The whole world: one zone and the characters in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Game {
    rng: Rng,
    zone: Zone,
    characters: Vec<Character>,
    occupancy: Occupancy,
    /// Everyone who is down, so the tick need not search for them.
    fallen: Vec<CharacterId>,
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
        Self { rng: Rng::from_seed(seed), zone: Zone::new(map), characters, occupancy, fallen: Vec::new() }
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

    /// Whether `attacker` may trade blows with `target` right now.
    pub fn can_attack(&self, attacker: CharacterId, target: CharacterId) -> Result<(), Refusal> {
        if attacker == target {
            return Err(Refusal::Yourself);
        }
        let (Some(aggressor), Some(victim)) = (self.character(attacker), self.character(target)) else {
            return Err(Refusal::NotHere);
        };
        if aggressor.place() != victim.place() {
            return Err(Refusal::NotHere);
        }
        if victim.is_down() {
            return Err(Refusal::AlreadyDown);
        }
        let safety = self.place_of(attacker).map_or(Safety::Haven, |place| place.safety);
        may_attack(safety, victim.standing())
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
    /// what is visible: coming, going, gathering, being marked, blows
    /// landing, falling, rising and picking things up.
    fn audience_of_outcome(&self, who: CharacterId, outcome: &Outcome) -> Vec<CharacterId> {
        let mut audience = alloc::vec![who];
        let here = self.character(who).map(Character::place);
        match outcome {
            Outcome::Moved { from, to } => {
                audience.extend_from_slice(self.occupants(*from));
                audience.extend_from_slice(self.occupants(*to));
            }
            // The one who was struck hears of it as their own `Hurt`.
            Outcome::Struck { target, .. } => {
                let bystanders = here.map_or(&[][..], |place| self.occupants(place));
                audience.extend(bystanders.iter().filter(|&&other| other != *target));
            }
            Outcome::Gathered { .. }
            | Outcome::StandingChanged { to: Standing::Marked, .. }
            | Outcome::Downed { .. }
            | Outcome::Rose
            | Outcome::Took { .. } => {
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
        if let Some(until) = self.characters[index].down_until() {
            outcomes.push(Outcome::Down { until });
            return;
        }
        let busy_until = self.characters[index].busy_until();
        if now < busy_until {
            outcomes.push(Outcome::Busy { until: busy_until });
            return;
        }
        match action {
            Action::Go(label) => self.go(index, label, now, outcomes),
            Action::Gather => self.gather(index, now, outcomes, events),
            Action::Attack(target) => self.attack(index, *target, now, outcomes, events),
            Action::Take => self.take(index, now, outcomes),
        }
    }

    /// One exchange: the attacker strikes, and the target, if still
    /// standing, strikes back.
    fn attack(&mut self, index: usize, target: CharacterId, now: Tick, outcomes: &mut Vec<Outcome>, events: &mut Vec<Event>) {
        let attacker = id_at(index);
        if let Err(why) = self.can_attack(attacker, target) {
            outcomes.push(Outcome::CannotAttack { target, why });
            return;
        }
        let Some(victim) = self.index_of(target) else { return };
        self.characters[index].busy_until = now + ATTACK_TIME;

        let (damage, health) = self.strike(index, victim);
        outcomes.push(Outcome::Struck { target, damage });
        events.push(Event::Character { who: target, outcome: Outcome::Hurt { by: attacker, damage, health } });
        if health == 0 {
            self.fell(victim, attacker, now, events);
            return;
        }

        let (damage, health) = self.strike(victim, index);
        events.push(Event::Character { who: target, outcome: Outcome::Struck { target: attacker, damage } });
        outcomes.push(Outcome::Hurt { by: target, damage, health });
        if health == 0 {
            self.fell(index, target, now, events);
        }
    }

    /// Lands one blow and returns the damage done and the health left.
    fn strike(&mut self, striker: usize, struck: usize) -> (u32, u32) {
        // DAMAGE_SPREAD is tiny, so the cast cannot truncate.
        let roll = self.rng.below(DAMAGE_SPREAD) as u32;
        let blow = Blow {
            roll,
            vigor: self.characters[striker].attribute(Attribute::Vigor).level(),
            taint: self.characters[striker].taint(),
            resilience: self.characters[struck].attribute(Attribute::Resilience).level(),
        };
        let damage = blow.damage();
        (damage, self.characters[struck].hurt(damage))
    }

    /// Puts a character down: what they carried spills where they stand.
    fn fell(&mut self, index: usize, by: CharacterId, now: Tick, events: &mut Vec<Event>) {
        let who = id_at(index);
        let character = &mut self.characters[index];
        let (dropped, cleansed) = character.fall(now + DOWNED_TIME);
        if let Some(place) = self.zone.map_mut().place_mut(character.place()) {
            for (lying, more) in place.spoils.iter_mut().zip(dropped) {
                *lying += more;
            }
        }
        self.fallen.push(who);
        events.push(Event::Character { who, outcome: Outcome::Downed { by } });
        events.extend(cleansed.map(|outcome| Event::Character { who, outcome }));
    }

    fn take(&mut self, index: usize, now: Tick, outcomes: &mut Vec<Outcome>) {
        let character = &mut self.characters[index];
        let spoils = self.zone.map_mut().place_mut(character.place()).map(|place| core::mem::take(&mut place.spoils));
        match spoils {
            Some(spoils) if spoils.iter().any(|&amount| amount > 0) => {
                character.stow_all(spoils);
                character.busy_until = now + TAKE_TIME;
                outcomes.push(Outcome::Took { spoils });
            }
            _ => outcomes.push(Outcome::NothingToTake),
        }
    }

    /// Health mends slowly everywhere and quickly in a haven. Those who
    /// are down mend only by getting up.
    fn mend(&mut self, now: Tick) {
        if !now.get().is_multiple_of(HEALTH_MENDS_EVERY.as_ticks()) {
            return;
        }
        let map = self.zone.map();
        for character in self.characters.iter_mut().filter(|character| !character.is_down()) {
            let in_haven = map.place(character.place()).is_some_and(|place| place.safety == Safety::Haven);
            character.mend(if in_haven { HAVEN_MENDING } else { MENDING });
        }
    }

    /// Gets everyone up whose time on the ground is over.
    fn raise_the_fallen(&mut self, now: Tick, events: &mut Vec<Event>) {
        let characters = &mut self.characters;
        self.fallen.retain(|&who| {
            let Some(character) = usize::try_from(who.get()).ok().and_then(|index| characters.get_mut(index)) else {
                return false;
            };
            let still_down = character.down_until().is_some_and(|until| now < until);
            if !still_down {
                character.rise();
                events.push(Event::Character { who, outcome: Outcome::Rose });
            }
            still_down
        });
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
        self.mend(now);
        self.raise_the_fallen(now, events);
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

    /// Ash and Bryn on the Ore Ledge, with Bryn having stripped it and
    /// been marked. Returns the world and the tick it has reached.
    fn bryn_marked_on_the_ledge() -> (Game, u64) {
        let mut game = Game::new(1, &["Ash", "Bryn"]);
        let mut events = Vec::new();
        game.apply(&Command::go(ASH, "north"), Tick::new(0), &mut events);
        game.apply(&Command::go(BRYN, "north"), Tick::new(0), &mut events);
        let mut now = 30;
        while game.character(BRYN).unwrap().standing() == Standing::Clean {
            game.apply(&Command::gather(BRYN), Tick::new(now), &mut events);
            now += GATHER_TIME.as_ticks();
        }
        (game, now)
    }

    /// Ash attacks Bryn until one of them is down. Returns every event.
    fn fight_to_the_end(game: &mut Game, mut now: u64) -> (Vec<Event>, u64) {
        let mut events = Vec::new();
        while !game.character(ASH).unwrap().is_down() && !game.character(BRYN).unwrap().is_down() {
            game.apply(&Command::attack(ASH, BRYN), Tick::new(now), &mut events);
            now += ATTACK_TIME.as_ticks();
        }
        (events, now)
    }

    #[test]
    fn the_clean_cannot_be_attacked_in_the_wilderness_and_nobody_can_in_a_haven() {
        let mut game = Game::new(1, &["Ash", "Bryn"]);
        assert_eq!(game.can_attack(ASH, BRYN), Err(Refusal::Haven));
        assert_eq!(game.can_attack(ASH, ASH), Err(Refusal::Yourself));

        let mut events = Vec::new();
        game.apply(&Command::go(ASH, "north"), Tick::new(0), &mut events);
        assert_eq!(game.can_attack(ASH, BRYN), Err(Refusal::NotHere));
        game.apply(&Command::go(BRYN, "north"), Tick::new(0), &mut events);
        assert_eq!(game.can_attack(ASH, BRYN), Err(Refusal::Unmarked));

        game.apply(&Command::attack(ASH, BRYN), Tick::new(30), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::CannotAttack { target: BRYN, why: Refusal::Unmarked }));
        assert_eq!(game.character(BRYN).unwrap().health(), 100);
    }

    #[test]
    fn anyone_can_be_attacked_on_contested_ground() {
        let mut game = Game::new(1, &["Ash", "Bryn"]);
        let mut events = Vec::new();
        for (tick, exit) in [(0, "east"), (30, "cave")] {
            game.apply(&Command::go(ASH, exit), Tick::new(tick), &mut events);
            game.apply(&Command::go(BRYN, exit), Tick::new(tick), &mut events);
        }
        assert_eq!(game.can_attack(ASH, BRYN), Ok(()));
    }

    #[test]
    fn an_exchange_hurts_both_and_the_marked_one_hits_softer() {
        let (mut game, now) = bryn_marked_on_the_ledge();
        let mut events = Vec::new();
        game.apply(&Command::attack(ASH, BRYN), Tick::new(now), &mut events);

        let (ash, bryn) = (game.character(ASH).unwrap(), game.character(BRYN).unwrap());
        assert!(bryn.health() < ash.health(), "Ash {} vs Bryn {}", ash.health(), bryn.health());
        assert!(ash.health() < 100, "Bryn should have struck back");
        assert_eq!(ash.busy_until(), Tick::new(now) + ATTACK_TIME);
        let kinds: Vec<&Event> = events.iter().collect();
        assert!(matches!(kinds[0], Event::Character { who: ASH, outcome: Outcome::Struck { target: BRYN, .. } }));
        assert!(matches!(kinds[1], Event::Character { who: ASH, outcome: Outcome::Hurt { by: BRYN, .. } }));
        assert!(matches!(kinds[2], Event::Character { who: BRYN, outcome: Outcome::Hurt { by: ASH, .. } }));
        assert!(matches!(kinds[3], Event::Character { who: BRYN, outcome: Outcome::Struck { target: ASH, .. } }));
    }

    #[test]
    fn the_loser_goes_down_drops_everything_and_pays_off_the_mark() {
        let (mut game, now) = bryn_marked_on_the_ledge();
        let carried = game.character(BRYN).unwrap().carried(Resource::Ore);
        assert!(carried > 0);

        let (events, now) = fight_to_the_end(&mut game, now);

        let bryn = game.character(BRYN).unwrap();
        assert!(bryn.is_down() && !game.character(ASH).unwrap().is_down(), "the clean challenger should win");
        assert_eq!((bryn.carried_total(), bryn.taint(), bryn.standing()), (0, 0, Standing::Clean));
        assert_eq!(game.place_of(BRYN).unwrap().spoils[Resource::Ore.index()], carried);
        assert!(events.contains(&Event::Character { who: BRYN, outcome: Outcome::Downed { by: ASH } }));
        let cleansed = Outcome::StandingChanged { from: Standing::Marked, to: Standing::Clean };
        assert!(events.contains(&Event::Character { who: BRYN, outcome: cleansed }));

        // Down means out of it: no acting, and no kicking them while down.
        let mut events = Vec::new();
        game.apply(&Command::gather(BRYN), Tick::new(now), &mut events);
        assert!(matches!(outcome(&events), Some(Outcome::Down { .. })));
        assert_eq!(game.can_attack(ASH, BRYN), Err(Refusal::AlreadyDown));
    }

    #[test]
    fn the_winner_can_take_the_spoils_once() {
        let (mut game, now) = bryn_marked_on_the_ledge();
        let carried = game.character(BRYN).unwrap().carried(Resource::Ore);
        let (_, now) = fight_to_the_end(&mut game, now);

        let mut events = Vec::new();
        game.apply(&Command::take(ASH), Tick::new(now), &mut events);
        assert_eq!(game.character(ASH).unwrap().carried(Resource::Ore), carried);
        assert_eq!(game.place_of(ASH).unwrap().spoils, [0; Resource::ALL.len()]);

        game.apply(&Command::take(ASH), Tick::new(now + TAKE_TIME.as_ticks()), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::NothingToTake));
    }

    #[test]
    fn the_fallen_get_up_after_a_while_weak_but_standing() {
        let (mut game, now) = bryn_marked_on_the_ledge();
        let (_, now) = fight_to_the_end(&mut game, now);
        let until = game.character(BRYN).unwrap().down_until().unwrap().get();

        let mut events = Vec::new();
        for tick in now..until {
            game.tick(Tick::new(tick), &mut events);
        }
        assert!(game.character(BRYN).unwrap().is_down());
        game.tick(Tick::new(until), &mut events);

        let bryn = game.character(BRYN).unwrap();
        assert!(!bryn.is_down());
        assert_eq!(i64::from(bryn.health()), crate::content::HEALTH_ON_RISING);
        assert_eq!(events.iter().filter(|event| matches!(event, Event::Character { outcome: Outcome::Rose, .. })).count(), 1);
    }

    #[test]
    fn health_mends_slowly_in_the_wilds_and_quickly_in_a_haven() {
        let (mut game, now) = bryn_marked_on_the_ledge();
        let mut events = Vec::new();
        game.apply(&Command::attack(ASH, BRYN), Tick::new(now), &mut events);
        let hurt = game.character(ASH).unwrap().health();

        for tick in now + 1..=now + 100 {
            game.tick(Tick::new(tick), &mut events);
        }
        let in_the_wilds = game.character(ASH).unwrap().health() - hurt;
        assert!((1..=10).contains(&in_the_wilds), "mended {in_the_wilds}");

        game.apply(&Command::go(ASH, "south"), Tick::new(now + 100), &mut events);
        for tick in now + 101..=now + 200 {
            game.tick(Tick::new(tick), &mut events);
        }
        assert_eq!(game.character(ASH).unwrap().health(), 100);
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
