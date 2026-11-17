//! The rules: how commands and the passing of time change the world.

use alloc::vec::Vec;

use eq_clock::{Duration, Tick};
use eq_events::World;
use eq_rng::Rng;
use eq_world_graph::{Graph, PlaceId};

use alloc::string::String;

use crate::character::{Character, CharacterId, Death};
use crate::combat::{may_attack, may_rob, Blow, NameRefusal, Refusal, ReturnRefusal, RobRefusal};
use crate::command::{Action, Command};
use crate::content::{
    self, ATTACK_TIME, BASE_EFFORT, CORPSE_TIME, CORRUPTED_WAIT, CORRUPTION_PER_DEATH, CORRUPTION_PER_SLEEPER_DEATH,
    DAMAGE_SPREAD, DEGRADATION_FADES_EVERY, DEGRADATION_WAIT, EFFORT_SPREAD, FADED_CARRYOVER_PERCENT, GATHER_TIME,
    HAVEN_MENDING, HEALTH_MENDS_EVERY, LONG_SLEEP, MEMORIES_TO_REDEEM, MENDING, MIN_WORKABLE_STOCK, NAME_TIME,
    REBIRTH_CARRYOVER_PERCENT, REDEMPTION_CLOCK, RESTED_LASTS, SEEK_TIME, SETTLE_TIME, TAINT_FADES_EVERY,
    TAINT_FOR_ROBBING, TAKE_TIME, TETHERED_WAIT, TRAVEL_TIME,
};
use crate::rest::FULLY_RESTED;
use crate::event::{Event, Outcome};
use crate::haul::{pressure_percent, taint_for, Haul};
use crate::occupancy::Occupancy;
use crate::types::{Attribute, Cause, Health, Passing, Place, Safety, Standing};
use crate::zone::Zone;

/// A name cut in the shrine's stones: someone who faded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Epitaph {
    /// Who they were.
    pub name: String,
    /// Which of their lives ended this way.
    pub life: u32,
    /// When they faded.
    pub at: Tick,
}

/// How often the world looks for sleepers whose time has run out.
const LONG_SLEEP_CHECK: Duration = Duration::minutes(1);

/// The whole world: one zone and the characters in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Game {
    rng: Rng,
    zone: Zone,
    characters: Vec<Character>,
    occupancy: Occupancy,
    /// Every spirit with a body still lying somewhere, so the tick need
    /// not search for them.
    dead: Vec<CharacterId>,
    /// Every forgotten spirit, dormant or not.
    forgotten: Vec<CharacterId>,
    /// Those who faded, oldest first.
    shrine: Vec<Epitaph>,
    /// Where severed spirits are pulled to and new lives begin.
    anchor: PlaceId,
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
        Self {
            rng: Rng::from_seed(seed),
            zone: Zone::new(map),
            characters,
            occupancy,
            dead: Vec::new(),
            forgotten: Vec::new(),
            shrine: Vec::new(),
            anchor: start,
        }
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

    /// The zone's spirit anchor: where severed spirits are pulled to and
    /// new lives begin.
    pub const fn anchor(&self) -> PlaceId {
        self.anchor
    }

    /// The names of those who faded, oldest first. They are cut in the
    /// stones at the anchor.
    pub fn shrine(&self) -> &[Epitaph] {
        &self.shrine
    }

    /// Everyone whose body lies in `place`, in the order they died.
    pub fn bodies_at(&self, place: PlaceId) -> impl Iterator<Item = (CharacterId, &Character)> {
        self.dead
            .iter()
            .filter_map(|&who| Some((who, self.character(who)?)))
            .filter(move |(_, character)| character.death().is_some_and(|death| death.body == place))
    }

    /// Whether `viewer` can see `other`. The living cannot see spirits;
    /// spirits see everyone.
    pub fn can_see(&self, viewer: CharacterId, other: CharacterId) -> bool {
        let is_spirit = |who| self.character(who).is_some_and(Character::is_spirit);
        viewer == other || !is_spirit(other) || is_spirit(viewer)
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
        if aggressor.place() != victim.place() || victim.is_spirit() {
            return Err(Refusal::NotHere);
        }
        let safety = self.place_of(attacker).map_or(Safety::Haven, |place| place.safety);
        may_attack(safety, victim.standing())
    }

    /// Whether `robber` may go through `target`'s pack at `now`.
    pub fn can_rob(&self, robber: CharacterId, target: CharacterId, now: Tick) -> Result<(), RobRefusal> {
        if robber == target {
            return Err(RobRefusal::Yourself);
        }
        let (Some(thief), Some(victim)) = (self.character(robber), self.character(target)) else {
            return Err(RobRefusal::NotHere);
        };
        if thief.place() != victim.place() || victim.is_spirit() {
            return Err(RobRefusal::NotHere);
        }
        if !victim.is_asleep(now) {
            return Err(RobRefusal::Awake);
        }
        let safety = self.place_of(robber).map_or(Safety::Haven, |place| place.safety);
        may_rob(safety)?;
        if victim.carried_total() == 0 {
            return Err(RobRefusal::NothingToSteal);
        }
        Ok(())
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

    /// The actor always learns their own outcome. Bystanders who can
    /// see the actor learn of what is visible: coming, going, gathering,
    /// being marked, blows landing, picking things up, lying down,
    /// waking, going through a sleeper's pack, returning to a body and
    /// being reborn. A death is seen by everyone where the body falls.
    fn audience_of_outcome(&self, who: CharacterId, outcome: &Outcome) -> Vec<CharacterId> {
        let mut audience = alloc::vec![who];
        let here = self.character(who).map(Character::place);
        match outcome {
            Outcome::Moved { from, to } => {
                audience.extend(self.witnesses(*from, who));
                audience.extend(self.witnesses(*to, who));
            }
            Outcome::Died { place, .. } => audience.extend_from_slice(self.occupants(*place)),
            // The one who was struck hears of it as their own `Hurt`.
            Outcome::Struck { target, .. } => {
                audience.extend(here.into_iter().flat_map(|place| self.witnesses(place, who)).filter(|other| other != target));
            }
            Outcome::Gathered { .. }
            | Outcome::StandingChanged { to: Standing::Marked, .. }
            | Outcome::Took { .. }
            | Outcome::LayDown { .. }
            | Outcome::Stole { .. }
            | Outcome::Returned
            | Outcome::Named { .. }
            | Outcome::Redeemed { .. }
            | Outcome::Faded { .. }
            | Outcome::Reborn { .. } => audience.extend(here.into_iter().flat_map(|place| self.witnesses(place, who))),
            // Getting up before sleep came is nobody else's business.
            Outcome::Woke { slept, .. } if slept.as_ticks() > 0 => {
                audience.extend(here.into_iter().flat_map(|place| self.witnesses(place, who)));
            }
            _ => {}
        }
        audience
    }

    /// Everyone in `place` who can see `actor`.
    fn witnesses(&self, place: PlaceId, actor: CharacterId) -> impl Iterator<Item = CharacterId> + '_ {
        self.occupants(place).iter().copied().filter(move |&witness| self.can_see(witness, actor))
    }

    fn index_of(&self, who: CharacterId) -> Option<usize> {
        usize::try_from(who.get()).ok().filter(|&index| index < self.characters.len())
    }

    fn act(&mut self, index: usize, action: &Action, now: Tick, outcomes: &mut Vec<Outcome>, events: &mut Vec<Event>) {
        if self.characters[index].forgotten().is_some() {
            self.act_as_forgotten(index, action, now, outcomes);
            return;
        }
        if self.characters[index].is_spirit() {
            self.act_as_spirit(index, action, now, outcomes);
            return;
        }
        match action {
            Action::Return | Action::Relinquish => return outcomes.push(Outcome::NotDead),
            Action::Seek => return outcomes.push(Outcome::NotForgotten),
            _ => {}
        }
        // Waking is the one thing a sleeper, or someone settling, can do.
        if matches!(action, Action::Wake) {
            outcomes.push(self.characters[index].wake(now).unwrap_or(Outcome::NotAsleep));
            return;
        }
        if self.characters[index].is_asleep(now) {
            outcomes.push(Outcome::Asleep);
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
            Action::Rest => self.rest(index, now, outcomes),
            Action::Rob(target) => self.rob(index, *target, now, outcomes, events),
            Action::Name(target) => self.name(index, *target, now, outcomes, events),
            Action::Wake | Action::Return | Action::Relinquish | Action::Seek => {}
        }
    }

    /// A spirit can walk, go back into its body, or give the body up.
    /// It has no hands for anything else.
    fn act_as_spirit(&mut self, index: usize, action: &Action, now: Tick, outcomes: &mut Vec<Outcome>) {
        let busy_until = self.characters[index].busy_until();
        if now < busy_until {
            outcomes.push(Outcome::Busy { until: busy_until });
            return;
        }
        match action {
            Action::Go(label) => self.go(index, label, now, outcomes),
            Action::Return => outcomes.push(self.return_to_body(index, now)),
            Action::Relinquish => outcomes.push(self.rebirth(index, now)),
            _ => outcomes.push(Outcome::Bodiless),
        }
    }

    /// A forgotten spirit lies dormant until it is woken. Awake, it can
    /// walk, search for memories of itself, or go dormant again. It has
    /// no body to return to or give up.
    fn act_as_forgotten(&mut self, index: usize, action: &Action, now: Tick, outcomes: &mut Vec<Outcome>) {
        let character = &mut self.characters[index];
        if character.is_dormant() {
            outcomes.push(match action {
                Action::Wake => {
                    character.forgotten_mut().expect("a dormant spirit is forgotten").dormant = false;
                    character.busy_until = now;
                    Outcome::Stirred
                }
                _ => Outcome::Asleep,
            });
            return;
        }
        let busy_until = character.busy_until();
        if now < busy_until {
            outcomes.push(Outcome::Busy { until: busy_until });
            return;
        }
        match action {
            Action::Go(label) => self.go(index, label, now, outcomes),
            Action::Seek => self.seek(index, now, outcomes),
            Action::Rest => {
                character.forgotten_mut().expect("checked above").dormant = true;
                outcomes.push(Outcome::WentDormant);
            }
            Action::Wake => outcomes.push(Outcome::NotAsleep),
            Action::Return | Action::Relinquish => outcomes.push(Outcome::CannotReturn { why: ReturnRefusal::NoBody }),
            _ => outcomes.push(Outcome::Bodiless),
        }
    }

    /// Searches a place for a memory. Each place with a deposit holds
    /// one for each forgotten spirit; enough of them redeem it.
    fn seek(&mut self, index: usize, now: Tick, outcomes: &mut Vec<Outcome>) {
        let character = &mut self.characters[index];
        let here = character.place();
        let holds_one = self.zone.map().place(here).is_some_and(|place| place.deposit.is_some());
        let Some(forgotten) = character.forgotten_mut() else { return outcomes.push(Outcome::NotForgotten) };
        if !holds_one || forgotten.found_in.contains(&here) {
            return outcomes.push(Outcome::NothingToRemember);
        }
        forgotten.found_in.push(here);
        let (found, redeemed) = (forgotten.memories(), forgotten.is_redeemed());
        character.busy_until = now + SEEK_TIME;
        outcomes.push(Outcome::Remembered { found, needed: MEMORIES_TO_REDEEM });
        if redeemed {
            let life = self.begin_life(index, now, REBIRTH_CARRYOVER_PERCENT);
            outcomes.push(Outcome::Redeemed { by: None, life });
        }
    }

    /// Speaks a forgotten spirit's name at the shrine, which redeems it
    /// wherever it lies.
    fn name(&mut self, index: usize, target: CharacterId, now: Tick, outcomes: &mut Vec<Outcome>, events: &mut Vec<Event>) {
        let refusal = if self.characters[index].place() != self.anchor {
            Some(NameRefusal::NotAtShrine)
        } else if !self.forgotten.contains(&target) {
            Some(NameRefusal::NotForgotten)
        } else {
            None
        };
        if let Some(why) = refusal {
            return outcomes.push(Outcome::CannotName { target, why });
        }
        let Some(named) = self.index_of(target) else { return };
        self.characters[index].busy_until = now + NAME_TIME;
        outcomes.push(Outcome::Named { target });
        let life = self.begin_life(named, now, REBIRTH_CARRYOVER_PERCENT);
        events.push(Event::Character { who: target, outcome: Outcome::Redeemed { by: Some(id_at(index)), life } });
    }

    /// Puts a spirit back in its body, if it is there and allowed to yet.
    fn return_to_body(&mut self, index: usize, now: Tick) -> Outcome {
        let character = &mut self.characters[index];
        let Some(death) = character.death().copied() else { return Outcome::NotDead };
        if character.place() != death.body {
            return Outcome::CannotReturn { why: ReturnRefusal::NotAtBody };
        }
        if now < death.ready_at {
            return Outcome::CannotReturn { why: ReturnRefusal::TooSoon(death.ready_at) };
        }
        character.return_to_body();
        self.dead.retain(|&who| who != id_at(index));
        Outcome::Returned
    }

    /// Ends a spirit's tie to its body and starts a new life at the anchor.
    fn rebirth(&mut self, index: usize, now: Tick) -> Outcome {
        Outcome::Reborn { life: self.begin_life(index, now, REBIRTH_CARRYOVER_PERCENT) }
    }

    /// Starts a spirit's next life at the anchor, keeping `carryover`
    /// percent of what it knew. Returns which life this is.
    fn begin_life(&mut self, index: usize, now: Tick, carryover: u32) -> u32 {
        let who = id_at(index);
        let character = &mut self.characters[index];
        self.occupancy.leave(character.place(), who);
        self.occupancy.enter(self.anchor, who);
        character.be_reborn(self.anchor, now, carryover);
        self.dead.retain(|&spirit| spirit != who);
        self.forgotten.retain(|&spirit| spirit != who);
        character.life()
    }

    /// Settles down. Sleep begins after a short while if nothing disturbs it.
    fn rest(&mut self, index: usize, now: Tick, outcomes: &mut Vec<Outcome>) {
        let asleep_at = now + SETTLE_TIME;
        self.characters[index].lie_down(asleep_at);
        outcomes.push(Outcome::LayDown { asleep_at });
    }

    /// Goes through a sleeper's pack. Robbing someone the land has not
    /// marked taints the robber enough to be marked for it.
    fn rob(&mut self, index: usize, target: CharacterId, now: Tick, outcomes: &mut Vec<Outcome>, events: &mut Vec<Event>) {
        if let Err(why) = self.can_rob(id_at(index), target, now) {
            outcomes.push(Outcome::CannotRob { target, why });
            return;
        }
        let Some(victim) = self.index_of(target) else { return };
        let wronged = self.characters[victim].standing() == Standing::Clean;
        let spoils = self.characters[victim].empty_pack();
        events.push(Event::Character { who: target, outcome: Outcome::Robbed });

        let robber = &mut self.characters[index];
        robber.stow_all(spoils);
        robber.busy_until = now + TAKE_TIME;
        outcomes.push(Outcome::Stole { target, spoils });
        if wronged {
            outcomes.extend(robber.shift_taint(TAINT_FOR_ROBBING));
        }
    }

    /// Wakes a character who has been struck. A blow ends sleep at once,
    /// and stops someone who was only settling from getting there.
    fn rouse(&mut self, index: usize, now: Tick, events: &mut Vec<Event>) {
        if let Some(outcome) = self.characters[index].wake(now) {
            events.push(Event::Character { who: id_at(index), outcome });
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

        let caught_lying = self.characters[victim].is_lying_down();
        let (damage, health) = self.strike(index, victim);
        outcomes.push(Outcome::Struck { target, damage });
        events.push(Event::Character { who: target, outcome: Outcome::Hurt { by: attacker, damage, health } });
        self.rouse(victim, now, events);
        if health == 0 {
            let corruption = if caught_lying { CORRUPTION_PER_SLEEPER_DEATH } else { CORRUPTION_PER_DEATH };
            self.fell(victim, attacker, corruption, now, events);
            return;
        }
        // Someone caught lying down is on their feet now, but too late to
        // answer this blow.
        if caught_lying {
            return;
        }

        let (damage, health) = self.strike(victim, index);
        events.push(Event::Character { who: target, outcome: Outcome::Struck { target: attacker, damage } });
        outcomes.push(Outcome::Hurt { by: target, damage, health });
        if health == 0 {
            self.fell(index, target, CORRUPTION_PER_DEATH, now, events);
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

    /// Kills a character. The body stays where it fell with everything
    /// they carried beside it, and the spirit is left to find its way back.
    /// The zone pays `corruption` for the death.
    fn fell(&mut self, index: usize, by: CharacterId, corruption: i64, now: Tick, events: &mut Vec<Event>) {
        let who = id_at(index);
        let body = self.characters[index].place();
        let passing = self.passing(who, by, body);
        // A spirit worn by recent deaths is held back longer each time.
        let worn = Duration::ticks(u64::from(self.characters[index].degradation()) * DEGRADATION_WAIT.as_ticks());
        let held = match passing {
            Passing::Tethered => TETHERED_WAIT,
            Passing::Severed | Passing::Forgotten => Duration::ZERO,
            Passing::Corrupted => CORRUPTED_WAIT,
        };
        let ready_at = now + held + worn;
        let lost_at = now + CORPSE_TIME;

        let character = &mut self.characters[index];
        let (dropped, cleansed) = character.die(Death { body, passing, ready_at, lost_at });
        if let Some(place) = self.zone.map_mut().place_mut(body) {
            for (lying, more) in place.spoils.iter_mut().zip(dropped) {
                *lying += more;
            }
        }
        // Only a tethered spirit stays by its body.
        if passing != Passing::Tethered {
            character.place = self.anchor;
            self.occupancy.leave(body, who);
            self.occupancy.enter(self.anchor, who);
        }
        self.zone.mourn(corruption);
        self.dead.push(who);
        let by = Cause::Character(by);
        events.push(Event::Character { who, outcome: Outcome::Died { by, place: body, passing, lost_at } });
        events.extend(cleansed.map(|outcome| Event::Character { who, outcome }));
    }

    /// How a spirit will stand to its body: corrupted in a collapsed
    /// zone, tethered if anyone but the killer stood by, severed if not.
    fn passing(&self, who: CharacterId, by: CharacterId, place: PlaceId) -> Passing {
        if self.zone.health() == Health::Collapsed {
            return Passing::Corrupted;
        }
        let someone_stood_by = self.occupants(place).iter().any(|&other| {
            other != who && other != by && self.character(other).is_some_and(|character| !character.is_spirit())
        });
        if someone_stood_by {
            Passing::Tethered
        } else {
            Passing::Severed
        }
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

    /// The rested bonus wears off while a character is up and about.
    fn tire(&mut self, now: Tick) {
        let every = RESTED_LASTS.as_ticks() / u64::from(FULLY_RESTED);
        if !now.get().is_multiple_of(every) {
            return;
        }
        let up_and_about = |character: &&mut Character| !character.is_lying_down() && !character.is_spirit();
        for character in self.characters.iter_mut().filter(up_and_about) {
            character.tire();
        }
    }

    /// Health mends slowly everywhere, and quickly in a haven or asleep.
    /// Spirits have no body to mend.
    fn mend(&mut self, now: Tick) {
        if !now.get().is_multiple_of(HEALTH_MENDS_EVERY.as_ticks()) {
            return;
        }
        let map = self.zone.map();
        for character in self.characters.iter_mut().filter(|character| !character.is_spirit()) {
            let in_haven = map.place(character.place()).is_some_and(|place| place.safety == Safety::Haven);
            let mending_well = in_haven || character.is_asleep(now);
            character.mend(if mending_well { HAVEN_MENDING } else { MENDING });
        }
    }

    /// The wear of dying fades from every spirit, living or not.
    fn recover_spirits(&mut self, now: Tick) {
        if !now.get().is_multiple_of(DEGRADATION_FADES_EVERY.as_ticks()) {
            return;
        }
        for character in &mut self.characters {
            character.recover_spirit();
        }
    }

    /// Anyone who has slept the long sleep through dies where they lie.
    /// Nothing is left to return to: what they carried spills where
    /// they lay, and the spirit lies dormant until it is woken.
    fn end_long_sleeps(&mut self, now: Tick, events: &mut Vec<Event>) {
        if !now.get().is_multiple_of(LONG_SLEEP_CHECK.as_ticks()) {
            return;
        }
        for index in 0..self.characters.len() {
            let character = &mut self.characters[index];
            let overslept = character.asleep_from().is_some_and(|from| from <= now && now.since(from) >= LONG_SLEEP);
            if !overslept {
                continue;
            }
            let (who, place) = (id_at(index), character.place());
            let (dropped, cleansed) = character.sleep_to_death(now);
            if let Some(ground) = self.zone.map_mut().place_mut(place) {
                for (lying, more) in ground.spoils.iter_mut().zip(dropped) {
                    *lying += more;
                }
            }
            self.zone.mourn(CORRUPTION_PER_SLEEPER_DEATH);
            self.forgotten.push(who);
            let outcome = Outcome::Died { by: Cause::LongSleep, place, passing: Passing::Forgotten, lost_at: now };
            events.push(Event::Character { who, outcome });
            events.extend(cleansed.map(|outcome| Event::Character { who, outcome }));
        }
    }

    /// Every waking forgotten spirit's time runs. When it runs out the
    /// character fades: their name goes on the shrine, and a new life
    /// begins with almost nothing of the old.
    fn run_redemption_clocks(&mut self, now: Tick, events: &mut Vec<Event>) {
        let mut faded = Vec::new();
        for &who in &self.forgotten {
            let Some(index) = self.index_of(who) else { continue };
            let Some(forgotten) = self.characters[index].forgotten_mut() else { continue };
            if forgotten.dormant {
                continue;
            }
            forgotten.played = forgotten.played + Duration::ticks(1);
            if forgotten.played >= REDEMPTION_CLOCK {
                faded.push(index);
            }
        }
        for index in faded {
            let character = &self.characters[index];
            self.shrine.push(Epitaph { name: character.name().into(), life: character.life(), at: now });
            let life = self.begin_life(index, now, FADED_CARRYOVER_PERCENT);
            events.push(Event::Character { who: id_at(index), outcome: Outcome::Faded { life } });
        }
    }

    /// A body left too long is lost, and its spirit begins a new life.
    fn lose_bodies(&mut self, now: Tick, events: &mut Vec<Event>) {
        let overdue: Vec<usize> = self
            .dead
            .iter()
            .filter_map(|&who| self.index_of(who))
            .filter(|&index| self.characters[index].death().is_some_and(|death| death.lost_at <= now))
            .collect();
        for index in overdue {
            let outcome = self.rebirth(index, now);
            events.push(Event::Character { who: id_at(index), outcome });
        }
    }

    fn go(&mut self, index: usize, label: &str, now: Tick, outcomes: &mut Vec<Outcome>) {
        let character = &mut self.characters[index];
        let from = character.place();
        let Some(exit) = self.zone.map().exit(from, label) else {
            outcomes.push(Outcome::NoSuchExit { label: label.into() });
            return;
        };
        // The tide is nothing to a spirit: water can be walked.
        if !character.is_spirit() && !exit.passage.is_open(now) {
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
        self.tire(now);
        self.mend(now);
        self.recover_spirits(now);
        self.end_long_sleeps(now, events);
        self.run_redemption_clocks(now, events);
        self.lose_bodies(now, events);
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
    use crate::types::{Resource, SkillKind};
    use eq_world_graph::PlaceId;

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

    /// Ash attacks Bryn until one of them is dead. Returns every event.
    fn fight_to_the_end(game: &mut Game, mut now: u64) -> (Vec<Event>, u64) {
        let mut events = Vec::new();
        while !game.character(ASH).unwrap().is_spirit() && !game.character(BRYN).unwrap().is_spirit() {
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

    const LEDGE: PlaceId = PlaceId::new(1);
    const CAMP: PlaceId = PlaceId::new(0);

    fn died(events: &[Event], who: CharacterId) -> Option<(Passing, Tick)> {
        events.iter().find_map(|event| match event {
            Event::Character { who: dead, outcome: Outcome::Died { passing, lost_at, .. } } if *dead == who => {
                Some((*passing, *lost_at))
            }
            _ => None,
        })
    }

    #[test]
    fn the_loser_dies_drops_everything_and_pays_off_the_mark() {
        let (mut game, now) = bryn_marked_on_the_ledge();
        let carried = game.character(BRYN).unwrap().carried(Resource::Ore);
        assert!(carried > 0);

        let (events, now) = fight_to_the_end(&mut game, now);

        let bryn = game.character(BRYN).unwrap();
        assert!(bryn.is_spirit() && !game.character(ASH).unwrap().is_spirit(), "the clean challenger should win");
        assert_eq!((bryn.carried_total(), bryn.taint(), bryn.standing()), (0, 0, Standing::Clean));
        assert_eq!(game.map().place(LEDGE).unwrap().spoils[Resource::Ore.index()], carried);
        assert!(died(&events, BRYN).is_some());
        let cleansed = Outcome::StandingChanged { from: Standing::Marked, to: Standing::Clean };
        assert!(events.contains(&Event::Character { who: BRYN, outcome: cleansed }));
        assert_eq!(game.bodies_at(LEDGE).map(|(id, _)| id).collect::<Vec<_>>(), [BRYN]);

        // A spirit has no hands, and the living cannot touch what they cannot see.
        let mut events = Vec::new();
        game.apply(&Command::gather(BRYN), Tick::new(now), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::Bodiless));
        assert_eq!(game.can_attack(ASH, BRYN), Err(Refusal::NotHere));
        assert!(!game.can_see(ASH, BRYN) && game.can_see(BRYN, ASH) && game.can_see(BRYN, BRYN));
    }

    #[test]
    fn dying_alone_pulls_the_spirit_to_the_anchor_to_walk_back_and_return() {
        let (mut game, now) = bryn_marked_on_the_ledge();
        let (events, now) = fight_to_the_end(&mut game, now);
        assert_eq!(died(&events, BRYN).map(|(passing, _)| passing), Some(Passing::Severed));
        assert_eq!((game.anchor(), game.character(BRYN).unwrap().place()), (CAMP, CAMP));
        assert_eq!(game.occupants(CAMP), [BRYN]);

        let mut events = Vec::new();
        game.apply(&Command::return_to_body(BRYN), Tick::new(now), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::CannotReturn { why: ReturnRefusal::NotAtBody }));

        game.apply(&Command::go(BRYN, "north"), Tick::new(now), &mut events);
        let arrived = now + TRAVEL_TIME.as_ticks();
        game.apply(&Command::return_to_body(BRYN), Tick::new(arrived), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::Returned));

        let bryn = game.character(BRYN).unwrap();
        assert!(!bryn.is_spirit());
        assert_eq!((bryn.place(), i64::from(bryn.health()), bryn.life()), (LEDGE, crate::content::HEALTH_ON_RETURN, 1));
        assert_eq!(game.bodies_at(LEDGE).count(), 0);
    }

    #[test]
    fn dying_with_someone_standing_by_keeps_the_spirit_close() {
        let mut game = Game::new(1, &["Ash", "Bryn", "Cole"]);
        let cole = CharacterId::new(2);
        let mut events = Vec::new();
        for who in [ASH, BRYN, cole] {
            game.apply(&Command::go(who, "north"), Tick::new(0), &mut events);
        }
        let mut now = 30;
        while game.character(BRYN).unwrap().standing() == Standing::Clean {
            game.apply(&Command::gather(BRYN), Tick::new(now), &mut events);
            now += GATHER_TIME.as_ticks();
        }
        let (events, now) = fight_to_the_end(&mut game, now);
        assert_eq!(died(&events, BRYN).map(|(passing, _)| passing), Some(Passing::Tethered));
        assert_eq!(game.character(BRYN).unwrap().place(), LEDGE);

        let mut events = Vec::new();
        game.apply(&Command::return_to_body(BRYN), Tick::new(now), &mut events);
        assert!(matches!(outcome(&events), Some(Outcome::CannotReturn { why: ReturnRefusal::TooSoon(_) })));
        game.apply(&Command::return_to_body(BRYN), Tick::new(now + TETHERED_WAIT.as_ticks()), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::Returned));
    }

    #[test]
    fn a_body_left_too_long_is_lost_and_the_spirit_is_reborn_with_a_share_of_what_it_knew() {
        let (mut game, now) = bryn_marked_on_the_ledge();
        let mining_before = game.character(BRYN).unwrap().skill(crate::types::SkillKind::Mining).level();
        assert!(mining_before >= 2, "Bryn should have learned something first: {mining_before}");
        let (events, now) = fight_to_the_end(&mut game, now);
        let lost_at = died(&events, BRYN).unwrap().1.get();

        let before = ticks(&mut game, now, lost_at - 1);
        assert!(game.character(BRYN).unwrap().is_spirit());
        assert!(!before.iter().any(|event| matches!(event, Event::Character { outcome: Outcome::Reborn { .. }, .. })));

        let after = ticks(&mut game, lost_at, lost_at);
        assert!(after.contains(&Event::Character { who: BRYN, outcome: Outcome::Reborn { life: 2 } }));
        let bryn = game.character(BRYN).unwrap();
        assert!(!bryn.is_spirit());
        assert_eq!((bryn.place(), bryn.health(), bryn.rested(), bryn.life()), (CAMP, 100, 100, 2));
        let mining = bryn.skill(crate::types::SkillKind::Mining);
        assert_eq!((mining.level(), mining.peak()), (mining_before * 15 / 100, mining_before * 15 / 100));
        assert_eq!(game.bodies_at(LEDGE).count(), 0);
    }

    #[test]
    fn dying_again_soon_holds_the_spirit_back_longer() {
        let (mut game, now) = bryn_marked_on_the_ledge();
        let (_, now) = fight_to_the_end(&mut game, now);
        assert_eq!(game.character(BRYN).unwrap().degradation(), 1);
        let mut events = Vec::new();
        game.apply(&Command::go(BRYN, "north"), Tick::new(now), &mut events);
        let back = now + TRAVEL_TIME.as_ticks();
        game.apply(&Command::return_to_body(BRYN), Tick::new(back), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::Returned));

        // The cave is contested ground: Ash needs no mark to strike there.
        for (tick, exit) in [(back, "south"), (back + 30, "east"), (back + 60, "cave")] {
            game.apply(&Command::go(ASH, exit), Tick::new(tick), &mut events);
            game.apply(&Command::go(BRYN, exit), Tick::new(tick), &mut events);
        }
        let (events, _) = fight_to_the_end(&mut game, back + 90);
        let died_at = events.iter().find_map(|event| match event {
            Event::Character { who: BRYN, outcome: Outcome::Died { lost_at, .. } } => Some(*lost_at),
            _ => None,
        });
        let death = *game.character(BRYN).unwrap().death().unwrap();
        assert_eq!(Some(death.lost_at), died_at);
        // One earlier death: held back three minutes from the moment of dying.
        assert_eq!(death.ready_at + CORPSE_TIME, death.lost_at + DEGRADATION_WAIT);
        assert_eq!(game.character(BRYN).unwrap().degradation(), 2);
    }

    #[test]
    fn the_wear_of_dying_fades() {
        let (mut game, now) = bryn_marked_on_the_ledge();
        let (_, now) = fight_to_the_end(&mut game, now);
        ticks(&mut game, now, now + DEGRADATION_FADES_EVERY.as_ticks());
        assert_eq!(game.character(BRYN).unwrap().degradation(), 0);
    }

    #[test]
    fn a_spirit_can_give_up_its_body_and_the_living_cannot() {
        let (mut game, now) = bryn_marked_on_the_ledge();
        let (_, now) = fight_to_the_end(&mut game, now);
        let mut events = Vec::new();
        game.apply(&Command::relinquish(ASH), Tick::new(now), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::NotDead));
        game.apply(&Command::relinquish(BRYN), Tick::new(now), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::Reborn { life: 2 }));
        assert!(!game.character(BRYN).unwrap().is_spirit());
    }

    #[test]
    fn the_tide_is_nothing_to_a_spirit() {
        let (mut game, now) = bryn_marked_on_the_ledge();
        let (_, _) = fight_to_the_end(&mut game, now);
        let mut events = Vec::new();
        // High tide: the cave mouth is under water for the living.
        game.apply(&Command::go(BRYN, "east"), Tick::new(700), &mut events);
        game.apply(&Command::go(BRYN, "cave"), Tick::new(730), &mut events);
        assert_eq!(game.place_of(BRYN).unwrap().name, "Tidal Cave");
    }

    #[test]
    fn each_death_costs_the_zone_and_the_cost_fades() {
        let (mut game, now) = bryn_marked_on_the_ledge();
        ticks(&mut game, now, now);
        let before = game.vitality();
        let (_, now) = fight_to_the_end(&mut game, now + 1);
        ticks(&mut game, now, now);
        assert_eq!(game.zone().corruption(), crate::content::CORRUPTION_PER_DEATH);
        assert!(game.vitality() <= before - crate::content::CORRUPTION_PER_DEATH);

        ticks(&mut game, now + 1, now + 6 * crate::content::CORRUPTION_FADES_EVERY.as_ticks());
        assert_eq!(game.zone().corruption(), 0);
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

    fn ticks(game: &mut Game, from: u64, to: u64) -> Vec<Event> {
        let mut events = Vec::new();
        for tick in from..=to {
            game.tick(Tick::new(tick), &mut events);
        }
        events
    }

    fn woke(events: &[Event]) -> Option<(Duration, u32, u32)> {
        events.iter().rev().find_map(|event| match event {
            Event::Character { outcome: Outcome::Woke { slept, rested, wasted }, .. } => Some((*slept, *rested, *wasted)),
            _ => None,
        })
    }

    #[test]
    fn sleep_comes_after_settling_and_a_sleeper_can_only_wake() {
        let mut game = Game::new(1, &["Ash"]);
        let mut events = Vec::new();
        game.apply(&Command::rest(ASH), Tick::new(0), &mut events);
        let asleep_at = Tick::ZERO + SETTLE_TIME;
        assert_eq!(outcome(&events), Some(&Outcome::LayDown { asleep_at }));
        assert!(game.character(ASH).unwrap().is_lying_down());
        assert!(!game.character(ASH).unwrap().is_asleep(Tick::new(10)));

        game.apply(&Command::gather(ASH), Tick::new(10), &mut events);
        assert!(matches!(outcome(&events), Some(Outcome::Busy { .. })), "still settling");
        game.apply(&Command::go(ASH, "north"), asleep_at, &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::Asleep));
        assert_eq!(game.place_of(ASH).unwrap().name, "Tidewatch Camp");

        game.apply(&Command::wake(ASH), Tick::new(100), &mut events);
        assert!(matches!(outcome(&events), Some(Outcome::Woke { .. })));
        game.apply(&Command::wake(ASH), Tick::new(101), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::NotAsleep));
    }

    #[test]
    fn being_up_spends_rest_and_a_nights_sleep_restores_it() {
        let mut game = Game::new(1, &["Ash"]);
        let day = RESTED_LASTS.as_ticks();
        ticks(&mut game, 1, day / 2);
        assert_eq!(game.character(ASH).unwrap().rested(), 50);
        ticks(&mut game, day / 2 + 1, day);
        assert_eq!(game.character(ASH).unwrap().rested(), 0);

        let mut events = Vec::new();
        game.apply(&Command::rest(ASH), Tick::new(day), &mut events);
        let asleep_at = day + SETTLE_TIME.as_ticks();
        let night = crate::content::FULL_REST.as_ticks();
        ticks(&mut game, day + 1, asleep_at + night);
        assert_eq!(game.character(ASH).unwrap().rested(), 0, "rest is counted on waking, and sleepers do not tire");

        game.apply(&Command::wake(ASH), Tick::new(asleep_at + night), &mut events);
        assert_eq!(woke(&events), Some((crate::content::FULL_REST, 100, 0)));
    }

    #[test]
    fn the_rested_learn_faster() {
        let haul = |tired: bool| {
            let mut game = Game::new(1, &["Ash"]);
            let mut events = Vec::new();
            let start = if tired { RESTED_LASTS.as_ticks() } else { 0 };
            ticks(&mut game, 1, start);
            game.apply(&Command::go(ASH, "north"), Tick::new(start), &mut events);
            game.apply(&Command::gather(ASH), Tick::new(start + 30), &mut events);
            game.character(ASH).unwrap().skill(crate::types::SkillKind::Mining).progress()
        };
        let (fresh, tired) = (haul(false), haul(true));
        assert!(fresh > tired, "fresh {fresh} vs tired {tired}");
    }

    #[test]
    fn lying_too_long_wastes_the_body_to_a_floor_and_it_comes_back_fast() {
        use crate::types::SkillKind;
        let mut game = Game::new(1, &["Ash"]);
        let mut events = Vec::new();
        game.apply(&Command::go(ASH, "north"), Tick::new(0), &mut events);
        let mut now = 30;
        let mut hauls = 0;
        while game.character(ASH).unwrap().skill(SkillKind::Mining).level() < 6 {
            game.apply(&Command::gather(ASH), Tick::new(now), &mut events);
            game.tick(Tick::new(now + 1), &mut events);
            now += 600; // leave the ledge time to regrow
            for tick in now - 598..=now {
                game.tick(Tick::new(tick), &mut events);
            }
            hauls += 1;
        }
        let peak = game.character(ASH).unwrap().skill(SkillKind::Mining).level();

        // Inside the forgiving stretch nothing is lost.
        game.apply(&Command::rest(ASH), Tick::new(now), &mut events);
        let three_days = now + Duration::days(3).as_ticks();
        game.apply(&Command::wake(ASH), Tick::new(three_days), &mut events);
        assert_eq!(woke(&events).unwrap().2, 0);

        // Ten days is far past it: down to the floor, and no further.
        game.apply(&Command::rest(ASH), Tick::new(three_days), &mut events);
        let much_later = three_days + Duration::days(10).as_ticks();
        game.apply(&Command::wake(ASH), Tick::new(much_later), &mut events);
        let mining = *game.character(ASH).unwrap().skill(SkillKind::Mining);
        assert_eq!((mining.level(), mining.peak()), (peak / 2, peak));
        assert!(woke(&events).unwrap().2 >= peak - peak / 2);

        // Muscle memory: back to the old peak in far fewer hauls.
        let mut now = much_later;
        let mut hauls_back = 0;
        while game.character(ASH).unwrap().skill(SkillKind::Mining).level() < peak {
            game.apply(&Command::gather(ASH), Tick::new(now), &mut events);
            now += GATHER_TIME.as_ticks();
            hauls_back += 1;
        }
        assert!(hauls_back * 3 < hauls, "{hauls_back} hauls to get back what took {hauls}");
    }

    #[test]
    fn a_blow_wakes_a_sleeper_who_cannot_answer_it() {
        let (mut game, now) = bryn_marked_on_the_ledge();
        let mut events = Vec::new();
        game.apply(&Command::rest(BRYN), Tick::new(now), &mut events);
        let later = now + 1_000;

        events.clear();
        game.apply(&Command::attack(ASH, BRYN), Tick::new(later), &mut events);
        assert_eq!(game.character(ASH).unwrap().health(), 100, "a sleeper should not strike back");
        assert!(!game.character(BRYN).unwrap().is_lying_down());
        assert!(woke(&events).is_some());

        // Awake now, Bryn answers the next blow.
        game.apply(&Command::attack(ASH, BRYN), Tick::new(later + ATTACK_TIME.as_ticks()), &mut events);
        assert!(game.character(ASH).unwrap().health() < 100);
    }

    #[test]
    fn a_blow_while_settling_leaves_you_standing() {
        let (mut game, now) = bryn_marked_on_the_ledge();
        let mut events = Vec::new();
        game.apply(&Command::rest(BRYN), Tick::new(now), &mut events);
        game.apply(&Command::attack(ASH, BRYN), Tick::new(now + 5), &mut events);
        assert_eq!(woke(&events).map(|(slept, ..)| slept), Some(Duration::ZERO));
        assert!(!game.character(BRYN).unwrap().is_lying_down());
    }

    #[test]
    fn robbing_needs_a_sleeper_outside_a_haven_with_something_to_take() {
        let mut game = Game::new(1, &["Ash", "Bryn"]);
        let mut events = Vec::new();
        assert_eq!(game.can_rob(ASH, ASH, Tick::ZERO), Err(RobRefusal::Yourself));
        assert_eq!(game.can_rob(ASH, BRYN, Tick::ZERO), Err(RobRefusal::Awake));

        game.apply(&Command::rest(BRYN), Tick::new(0), &mut events);
        assert_eq!(game.can_rob(ASH, BRYN, Tick::new(10)), Err(RobRefusal::Awake), "still settling");
        assert_eq!(game.can_rob(ASH, BRYN, Tick::new(30)), Err(RobRefusal::Haven));
        game.apply(&Command::wake(BRYN), Tick::new(40), &mut events);

        for who in [ASH, BRYN] {
            game.apply(&Command::go(who, "north"), Tick::new(40), &mut events);
        }
        game.apply(&Command::rest(BRYN), Tick::new(70), &mut events);
        assert_eq!(game.can_rob(ASH, BRYN, Tick::new(100)), Err(RobRefusal::NothingToSteal));
        game.apply(&Command::go(ASH, "south"), Tick::new(100), &mut events);
        assert_eq!(game.can_rob(ASH, BRYN, Tick::new(130)), Err(RobRefusal::NotHere));
    }

    #[test]
    fn robbing_the_clean_marks_you_and_robbing_the_marked_does_not() {
        // Bryn is marked and carries ore. Ash robs Bryn asleep: no taint.
        let (mut game, now) = bryn_marked_on_the_ledge();
        let carried = game.character(BRYN).unwrap().carried_total();
        let mut events = Vec::new();
        game.apply(&Command::rest(BRYN), Tick::new(now), &mut events);
        let asleep = now + SETTLE_TIME.as_ticks();
        game.apply(&Command::rob(ASH, BRYN), Tick::new(asleep), &mut events);
        let ash = game.character(ASH).unwrap();
        assert_eq!((ash.carried_total(), ash.taint()), (carried, 0));
        assert!(events.contains(&Event::Character { who: BRYN, outcome: Outcome::Robbed }));
        assert!(game.character(BRYN).unwrap().is_asleep(Tick::new(asleep)), "robbery should not wake the sleeper");

        // Now Ash, clean and carrying it all, sleeps; Bryn wakes and robs it back.
        game.apply(&Command::rest(ASH), Tick::new(asleep + 5), &mut events);
        let much_later = asleep + 3_000; // long enough for Bryn's mark to fade
        ticks(&mut game, asleep + 6, much_later);
        game.apply(&Command::wake(BRYN), Tick::new(much_later), &mut events);
        assert_eq!(game.character(BRYN).unwrap().standing(), Standing::Clean);
        game.apply(&Command::rob(BRYN, ASH), Tick::new(much_later), &mut events);
        assert_eq!(game.character(BRYN).unwrap().standing(), Standing::Marked);
    }

    #[test]
    fn sleepers_mend_as_fast_as_in_a_haven() {
        let (mut game, now) = bryn_marked_on_the_ledge();
        let mut events = Vec::new();
        game.apply(&Command::attack(ASH, BRYN), Tick::new(now), &mut events);
        let hurt = game.character(ASH).unwrap().health();
        game.apply(&Command::rest(ASH), Tick::new(now + 3), &mut events);
        let asleep = now + 3 + SETTLE_TIME.as_ticks();
        ticks(&mut game, now + 4, asleep + 20);
        assert!(game.character(ASH).unwrap().health() >= (hurt + 10).min(100));
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

    /// Ash, with ore in the pack, asleep on the Ore Ledge until the body
    /// gives out. Returns the world, the tick it has reached and every
    /// event of the last minute.
    fn ash_forgotten(names: &[&str]) -> (Game, u64, Vec<Event>) {
        let mut game = Game::new(1, names);
        let mut events = Vec::new();
        game.apply(&Command::go(ASH, "north"), Tick::new(0), &mut events);
        game.apply(&Command::gather(ASH), Tick::new(30), &mut events);
        game.apply(&Command::rest(ASH), Tick::new(40), &mut events);
        let end = 40 + SETTLE_TIME.as_ticks() + LONG_SLEEP.as_ticks() + 60;
        ticks(&mut game, 41, end - 120);
        let last = ticks(&mut game, end - 119, end);
        (game, end, last)
    }

    #[test]
    fn two_weeks_asleep_kills_the_body_and_leaves_a_dormant_spirit() {
        let (game, _, events) = ash_forgotten(&["Ash", "Bryn"]);
        let ash = game.character(ASH).unwrap();
        assert!(ash.is_spirit() && ash.is_dormant() && !ash.is_lying_down());
        assert_eq!(ash.death(), None);
        assert_eq!(ash.carried_total(), 0);
        let ledge = game.place_of(ASH).unwrap();
        assert!(ledge.spoils[Resource::Ore.index()] > 0, "what they carried lies where they lay");
        assert_eq!(game.bodies_at(ash.place()).count(), 0, "there is no body to go back to");
        assert!(events.iter().any(|event| matches!(
            event,
            Event::Character { who: ASH, outcome: Outcome::Died { by: Cause::LongSleep, passing: Passing::Forgotten, .. } }
        )));
        assert_eq!(game.zone().corruption(), CORRUPTION_PER_SLEEPER_DEATH);
    }

    #[test]
    fn a_dormant_spirit_can_only_wake_and_its_clock_runs_only_while_it_is_awake() {
        let (mut game, now, _) = ash_forgotten(&["Ash"]);
        let mut events = Vec::new();
        game.apply(&Command::go(ASH, "south"), Tick::new(now), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::Asleep));

        // A year dormant costs nothing.
        let later = now + Duration::days(30).as_ticks();
        ticks(&mut game, now + 1, later);
        assert_eq!(game.character(ASH).unwrap().forgotten().unwrap().played, Duration::ZERO);

        game.apply(&Command::wake(ASH), Tick::new(later), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::Stirred));
        ticks(&mut game, later + 1, later + 600);
        assert_eq!(game.character(ASH).unwrap().forgotten().unwrap().played, Duration::minutes(10));

        game.apply(&Command::rest(ASH), Tick::new(later + 600), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::WentDormant));
        ticks(&mut game, later + 601, later + 6_000);
        assert_eq!(game.character(ASH).unwrap().forgotten().unwrap().played, Duration::minutes(10));

        for command in [Command::return_to_body(ASH), Command::relinquish(ASH)] {
            game.apply(&Command::wake(ASH), Tick::new(later + 6_000), &mut events);
            game.apply(&command, Tick::new(later + 6_000), &mut events);
            assert_eq!(outcome(&events), Some(&Outcome::CannotReturn { why: ReturnRefusal::NoBody }));
        }
    }

    #[test]
    fn three_memories_from_three_places_redeem_a_forgotten_spirit() {
        let (mut game, mut now, _) = ash_forgotten(&["Ash"]);
        let mut events = Vec::new();
        game.apply(&Command::wake(ASH), Tick::new(now), &mut events);

        game.apply(&Command::seek(ASH), Tick::new(now), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::Remembered { found: 1, needed: MEMORIES_TO_REDEEM }));
        now += SEEK_TIME.as_ticks();
        game.apply(&Command::seek(ASH), Tick::new(now), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::NothingToRemember), "one memory to a place");

        game.apply(&Command::go(ASH, "north"), Tick::new(now), &mut events);
        now += TRAVEL_TIME.as_ticks();
        game.apply(&Command::seek(ASH), Tick::new(now), &mut events);
        now += SEEK_TIME.as_ticks();

        // The camp has nothing to work, so nothing to find.
        for _ in 0..2 {
            game.apply(&Command::go(ASH, "south"), Tick::new(now), &mut events);
            now += TRAVEL_TIME.as_ticks();
        }
        game.apply(&Command::seek(ASH), Tick::new(now), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::NothingToRemember));

        game.apply(&Command::go(ASH, "east"), Tick::new(now), &mut events);
        now += TRAVEL_TIME.as_ticks();
        game.apply(&Command::seek(ASH), Tick::new(now), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::Redeemed { by: None, life: 2 }));

        let ash = game.character(ASH).unwrap();
        assert!(!ash.is_spirit());
        assert_eq!((ash.place(), ash.life(), ash.health()), (game.anchor(), 2, 100));
        assert!(game.shrine().is_empty());
        game.apply(&Command::seek(ASH), Tick::new(now), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::NotForgotten));
    }

    #[test]
    fn the_living_can_redeem_the_forgotten_by_name_at_the_shrine() {
        let (mut game, now, _) = ash_forgotten(&["Ash", "Bryn", "Cole"]);
        const COLE: CharacterId = CharacterId::new(2);
        let mut events = Vec::new();

        game.apply(&Command::name(BRYN, COLE), Tick::new(now), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::CannotName { target: COLE, why: NameRefusal::NotForgotten }));
        game.apply(&Command::go(COLE, "south"), Tick::new(now), &mut events);
        game.apply(&Command::name(COLE, ASH), Tick::new(now + 30), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::CannotName { target: ASH, why: NameRefusal::NotAtShrine }));

        events.clear();
        game.apply(&Command::name(BRYN, ASH), Tick::new(now + 30), &mut events);
        assert_eq!(events[0], Event::Character { who: BRYN, outcome: Outcome::Named { target: ASH } });
        assert_eq!(events[1], Event::Character { who: ASH, outcome: Outcome::Redeemed { by: Some(BRYN), life: 2 } });
        let ash = game.character(ASH).unwrap();
        assert!(!ash.is_spirit() && !ash.is_dormant());
        assert_eq!(ash.place(), game.anchor());
        assert_eq!(game.occupants(game.anchor()), [ASH, BRYN]);
    }

    #[test]
    fn a_forgotten_spirit_that_runs_out_of_time_fades_and_is_remembered_in_stone() {
        let mut game = Game::new(1, &["Ash"]);
        let mut events = Vec::new();
        game.apply(&Command::go(ASH, "north"), Tick::new(0), &mut events);
        let mut now = 30;
        while game.character(ASH).unwrap().skill(SkillKind::Mining).level() < 5 {
            game.tick(Tick::new(now), &mut events);
            game.apply(&Command::gather(ASH), Tick::new(now), &mut events);
            now += 1;
        }
        now += GATHER_TIME.as_ticks();
        game.apply(&Command::rest(ASH), Tick::new(now), &mut events);
        let dead = now + SETTLE_TIME.as_ticks() + LONG_SLEEP.as_ticks() + 60;
        ticks(&mut game, now + 1, dead);
        game.apply(&Command::wake(ASH), Tick::new(dead), &mut events);

        let faded = ticks(&mut game, dead + 1, dead + REDEMPTION_CLOCK.as_ticks());
        assert!(faded.contains(&Event::Character { who: ASH, outcome: Outcome::Faded { life: 2 } }));
        let ash = game.character(ASH).unwrap();
        assert!(!ash.is_spirit());
        assert_eq!((ash.place(), ash.life()), (game.anchor(), 2));
        assert_eq!(ash.skill(SkillKind::Mining).level(), 0, "five percent of a wasted handful is nothing");
        assert_eq!(game.shrine().len(), 1);
        assert_eq!((game.shrine()[0].name.as_str(), game.shrine()[0].life), ("Ash", 1));
    }
}
