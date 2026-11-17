//! The rules: how commands and the passing of time change the world.

use alloc::vec::Vec;

use eq_clock::{Duration, Tick};
use eq_events::World;
use eq_rng::Rng;
use eq_world_graph::{Graph, PlaceId};

use alloc::string::String;

use crate::board::{Board, ContractId, Lot, TradeRefusal};
use crate::character::{Character, CharacterId, Death, Rebirth};
use crate::combat::{may_attack, may_rob, Blow, NameRefusal, Refusal, ReturnRefusal, RiteRefusal, RobRefusal, TendRefusal};
use crate::command::{Action, Command};
use crate::creature::{Being, Creature, CreatureId, CreatureKind, Relic, RelicTier};
use crate::content::{
    self, ATTACK_TIME, BASE_EFFORT, BOARD_TIME, MOST_VIGIL, RITE_CLEANSES, RITE_MENDS_PERCENT, RITE_TIME, TEND_TIME,
    VIGIL_PER_TENDING, WARD_TIME, CORPSE_TIME, CORRUPTED_WAIT, CORRUPTION_PER_DEATH, CORRUPTION_PER_SLEEPER_DEATH,
    CREATURE_BLOW_EVERY, GUARD_LASTS, LURKER_RETURNS, DAMAGE_SPREAD, DEGRADATION_FADES_EVERY, DEGRADATION_WAIT, DRAW_TIME, EFFORT_SPREAD,
    FADED_CARRYOVER_PERCENT, GATHER_TIME,
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

/// How often the world clears away what has had its time: crumbled
/// relics, spent Body Guards, and lairs due a new lurker.
const UPKEEP_EVERY: Duration = Duration::seconds(10);

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
    /// Every creature now in the world, oldest first.
    creatures: Vec<Creature>,
    /// The id the next creature will get.
    next_creature: u64,
    /// Lairs waiting for a new lurker, and when it comes.
    empty_lairs: Vec<(Tick, PlaceId)>,
    /// What the dead dropped, oldest first.
    relics: Vec<Relic>,
    /// The Contract Board, which stands at the anchor.
    board: Board,
    /// Where severed spirits are pulled to and new lives begin.
    anchor: PlaceId,
}

impl Game {
    /// A fresh world with one character per name, all at the starting
    /// place. The same seed and names always play out the same way.
    pub fn new(seed: u64, names: &[&str]) -> Self {
        let (map, anchor) = content::rocky_shoreline();
        Self::in_territory(seed, names, crate::territory::Territory { map, anchor })
    }

    /// A fresh world in `territory`, with one character per name, all
    /// at its anchor.
    pub fn in_territory(seed: u64, names: &[&str], territory: crate::territory::Territory) -> Self {
        let (map, start) = (territory.map, territory.anchor);
        let mut occupancy = Occupancy::new(map.len());
        let characters: Vec<Character> = names.iter().map(|name| Character::new(name, start)).collect();
        for index in 0..characters.len() {
            occupancy.enter(start, id_at(index));
        }
        let lairs: Vec<PlaceId> = map.places().filter(|(_, place)| place.lair.is_some()).map(|(id, _)| id).collect();
        let mut game = Self {
            rng: Rng::from_seed(seed),
            zone: Zone::new(map),
            characters,
            occupancy,
            dead: Vec::new(),
            forgotten: Vec::new(),
            shrine: Vec::new(),
            creatures: Vec::new(),
            next_creature: 0,
            empty_lairs: Vec::new(),
            relics: Vec::new(),
            board: Board::new(),
            anchor: start,
        };
        for lair in lairs {
            let id = game.new_creature_id();
            game.creatures.push(Creature::lurker(id, lair, Tick::ZERO));
        }
        game
    }

    /// A character, if the id belongs to this world.
    pub fn character(&self, who: CharacterId) -> Option<&Character> {
        self.characters.get(self.index_of(who)?)
    }

    /// Every character with its id, in the order they were named.
    pub fn characters(&self) -> impl Iterator<Item = (CharacterId, &Character)> {
        self.characters.iter().enumerate().map(|(index, character)| (id_at(index), character))
    }

    /// The id the next character to join will have.
    pub fn next_id(&self) -> CharacterId {
        id_at(self.characters.len())
    }

    /// Brings a newcomer into the world at the anchor, if the command
    /// was for the next free id.
    fn join(&mut self, who: CharacterId, name: &str, events: &mut Vec<Event>) {
        if who != self.next_id() {
            return events.push(Event::UnknownCharacter { who });
        }
        self.characters.push(Character::new(name, self.anchor));
        self.occupancy.enter(self.anchor, who);
        events.push(Event::Character { who, outcome: Outcome::Joined });
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

    /// The Contract Board. It stands at the anchor.
    pub const fn board(&self) -> &Board {
        &self.board
    }

    /// Whether `who` may tend `target` at `now`.
    pub fn can_tend(&self, who: CharacterId, target: CharacterId, now: Tick) -> Result<(), TendRefusal> {
        if who == target {
            return Err(TendRefusal::Yourself);
        }
        let (Some(tender), Some(sleeper)) = (self.character(who), self.character(target)) else {
            return Err(TendRefusal::NotHere);
        };
        if tender.place() != sleeper.place() || sleeper.is_spirit() {
            return Err(TendRefusal::NotHere);
        }
        if !sleeper.is_asleep(now) {
            return Err(TendRefusal::Awake);
        }
        if sleeper.vigil() >= MOST_VIGIL {
            return Err(TendRefusal::Enough);
        }
        Ok(())
    }

    /// The creatures in `place`, oldest first.
    pub fn creatures_at(&self, place: PlaceId) -> impl Iterator<Item = &Creature> {
        self.creatures.iter().filter(move |creature| creature.place == place)
    }

    /// The relics lying in `place`, oldest first.
    pub fn relics_at(&self, place: PlaceId) -> impl Iterator<Item = &Relic> {
        self.relics.iter().filter(move |relic| relic.place == place)
    }

    /// Whether `who` could open `relic`. Faded and resonant relics open
    /// for anyone. An ancient one opens only for its owner while the
    /// owner's body lies beside it or stands guard over it.
    pub fn can_open(&self, who: CharacterId, relic: &Relic) -> bool {
        if relic.tier() < RelicTier::Ancient || relic.owner == who {
            return true;
        }
        let body_lies = self.dead.contains(&relic.owner);
        let guarded = self.creatures_at(relic.place).any(|creature| creature.being.of == Some(relic.owner));
        !body_lies && !guarded
    }

    /// Whether there is anything where `who` stands that they could take.
    pub fn can_take(&self, who: CharacterId) -> bool {
        self.character(who).is_some_and(|character| {
            !character.is_spirit() && self.relics_at(character.place()).any(|relic| self.can_open(who, relic))
        })
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
            Event::DepositChanged { place, .. }
            | Event::PassageChanged { place, .. }
            | Event::CreatureAppeared { place, .. }
            | Event::CreatureGone { place, .. } => self.occupants(*place).to_vec(),
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
            // The one who began the rite hears of it as their own `RiteAnswered`.
            Outcome::RiteDone { with } => {
                audience.extend(here.into_iter().flat_map(|place| self.witnesses(place, who)).filter(|other| other != with));
            }
            Outcome::Gathered { .. }
            | Outcome::Joined
            | Outcome::StandingChanged { to: Standing::Marked, .. }
            | Outcome::Took { .. }
            | Outcome::LayDown { .. }
            | Outcome::Stole { .. }
            | Outcome::Returned
            | Outcome::RiteBegun
            | Outcome::Tended { .. }
            | Outcome::Posted { .. }
            | Outcome::Accepted { .. }
            | Outcome::HitCreature { .. }
            | Outcome::Slew { .. }
            | Outcome::Mauled { .. }
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
            Action::Fight(creature) => self.fight(index, *creature, now, outcomes, events),
            Action::Take => self.take(index, now, outcomes),
            Action::Rest => self.rest(index, now, outcomes),
            Action::Rob(target) => self.rob(index, *target, now, outcomes, events),
            Action::Name(target) => self.name(index, *target, now, outcomes, events),
            Action::Rite => self.rite(index, now, outcomes, events),
            Action::Tend(target) => self.tend(index, *target, now, outcomes),
            Action::Post { give, want } => self.post(index, *give, *want, now, outcomes),
            Action::Accept(id) => self.accept(index, *id, now, outcomes, events),
            Action::Withdraw(id) => self.withdraw(index, *id, now, outcomes),
            Action::Collect => self.collect(index, now, outcomes),
            Action::Wake | Action::Return | Action::Relinquish | Action::Seek | Action::Join(_) => {}
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
            Action::Gather => self.draw(index, now, outcomes),
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
            Action::Gather => self.draw(index, now, outcomes),
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

    /// A spirit draws ectoplasm from any place the living can work.
    fn draw(&mut self, index: usize, now: Tick, outcomes: &mut Vec<Outcome>) {
        let character = &mut self.characters[index];
        let workable = self.zone.map().place(character.place()).is_some_and(|place| place.deposit.is_some());
        if !workable {
            return outcomes.push(Outcome::NothingToGather);
        }
        let mut risen = Vec::new();
        let amount = character.draw(&mut risen);
        character.busy_until = now + DRAW_TIME;
        outcomes.push(Outcome::Drew { amount, held: character.ectoplasm() });
        outcomes.append(&mut risen);
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
            let life = self.begin_life(index, now, REBIRTH_CARRYOVER_PERCENT, true);
            outcomes.push(Outcome::Redeemed { by: None, life });
        }
    }

    /// Takes part in a Restoration Rite. The first to do so begins it;
    /// a second, in time, completes it: the deposit is mended, a ward
    /// is laid on it, and some of the zone's corruption lifts.
    fn rite(&mut self, index: usize, now: Tick, outcomes: &mut Vec<Outcome>, events: &mut Vec<Event>) {
        let who = id_at(index);
        let character = &mut self.characters[index];
        let place = character.place();
        let marked = character.standing() == Standing::Marked;
        let refusal = match self.zone.map().place(place).and_then(|place| place.deposit) {
            None => Some(RiteRefusal::NothingToMend),
            Some(_) if marked => Some(RiteRefusal::Marked),
            Some(deposit) if deposit.is_warded(now) => Some(RiteRefusal::Warded),
            Some(deposit) if !deposit.needs_rite(now) => Some(RiteRefusal::NoNeed),
            Some(_) => None,
        };
        if let Some(why) = refusal {
            return outcomes.push(Outcome::CannotRite { why });
        }
        character.busy_until = now + RITE_TIME;
        let Some(deposit) = self.zone.map_mut().place_mut(place).and_then(|place| place.deposit.as_mut()) else { return };
        let Some(beginner) = deposit.rite_open(now).filter(|beginner| *beginner != who) else {
            deposit.rite_begun = Some((who, now));
            return outcomes.push(Outcome::RiteBegun);
        };
        let before = deposit.node.quality();
        deposit.node.mend(RITE_MENDS_PERCENT);
        deposit.warded_until = now + WARD_TIME;
        deposit.rite_begun = None;
        let (resource, after) = (deposit.resource, deposit.node.quality());
        self.zone.cleanse(RITE_CLEANSES);
        outcomes.push(Outcome::RiteDone { with: beginner });
        events.push(Event::Character { who: beginner, outcome: Outcome::RiteAnswered { by: who } });
        if after != before {
            events.push(Event::DepositChanged { place, resource, from: before, to: after });
        }
    }

    /// Tends a sleeper, which pushes back the long sleep for them.
    fn tend(&mut self, index: usize, target: CharacterId, now: Tick, outcomes: &mut Vec<Outcome>) {
        if let Err(why) = self.can_tend(id_at(index), target, now) {
            return outcomes.push(Outcome::CannotTend { target, why });
        }
        let Some(sleeper) = self.index_of(target) else { return };
        let vigil = self.characters[sleeper].tend(VIGIL_PER_TENDING, MOST_VIGIL);
        self.characters[index].busy_until = now + TEND_TIME;
        outcomes.push(Outcome::Tended { target, vigil });
    }

    /// Whether the character at `index` is standing at the board.
    fn at_the_board(&self, index: usize) -> Result<(), TradeRefusal> {
        if self.characters[index].place() == self.anchor {
            Ok(())
        } else {
            Err(TradeRefusal::NotAtBoard)
        }
    }

    /// Pins up an offer. What is offered leaves the pack at once.
    fn post(&mut self, index: usize, give: Lot, want: Lot, now: Tick, outcomes: &mut Vec<Outcome>) {
        let who = id_at(index);
        let allowed = self.at_the_board(index).and_then(|()| self.board.may_post(who, give, want));
        if let Err(why) = allowed {
            return outcomes.push(Outcome::CannotTrade { why });
        }
        let poster = &mut self.characters[index];
        if !poster.spend(give.resource, give.amount) {
            return outcomes.push(Outcome::CannotTrade { why: TradeRefusal::CannotPay });
        }
        poster.busy_until = now + BOARD_TIME;
        let id = self.board.post(who, give, want);
        outcomes.push(Outcome::Posted { id, give, want });
    }

    /// Pays a contract's price and takes what it held. The price waits
    /// at the board for whoever posted it.
    fn accept(&mut self, index: usize, id: ContractId, now: Tick, outcomes: &mut Vec<Outcome>, events: &mut Vec<Event>) {
        let who = id_at(index);
        let found = self.at_the_board(index).and_then(|()| match self.board.contract(id) {
            None => Err(TradeRefusal::NoSuchContract),
            Some(contract) if contract.by == who => Err(TradeRefusal::YourOwn),
            Some(contract) => Ok(*contract),
        });
        let contract = match found {
            Ok(contract) => contract,
            Err(why) => return outcomes.push(Outcome::CannotTrade { why }),
        };
        let buyer = &mut self.characters[index];
        if !buyer.spend(contract.want.resource, contract.want.amount) {
            return outcomes.push(Outcome::CannotTrade { why: TradeRefusal::CannotPay });
        }
        buyer.stow(contract.give.resource, contract.give.amount);
        buyer.busy_until = now + BOARD_TIME;
        self.board.take_down(id);
        self.board.hold_for(contract.by, contract.want);
        outcomes.push(Outcome::Accepted { id, paid: contract.want, got: contract.give });
        events.push(Event::Character { who: contract.by, outcome: Outcome::Filled { id, by: who, earned: contract.want } });
    }

    /// Takes down a contract of one's own and has back what it held.
    fn withdraw(&mut self, index: usize, id: ContractId, now: Tick, outcomes: &mut Vec<Outcome>) {
        let who = id_at(index);
        let found = self.at_the_board(index).and_then(|()| match self.board.contract(id) {
            None => Err(TradeRefusal::NoSuchContract),
            Some(contract) if contract.by != who => Err(TradeRefusal::NotYours),
            Some(contract) => Ok(*contract),
        });
        match found {
            Ok(contract) => {
                self.board.take_down(id);
                let owner = &mut self.characters[index];
                owner.stow(contract.give.resource, contract.give.amount);
                owner.busy_until = now + BOARD_TIME;
                outcomes.push(Outcome::Withdrew { id, back: contract.give });
            }
            Err(why) => outcomes.push(Outcome::CannotTrade { why }),
        }
    }

    /// Takes everything the board holds for this character.
    fn collect(&mut self, index: usize, now: Tick, outcomes: &mut Vec<Outcome>) {
        if let Err(why) = self.at_the_board(index) {
            return outcomes.push(Outcome::CannotTrade { why });
        }
        let spoils = self.board.collect(id_at(index));
        if spoils.iter().all(|&amount| amount == 0) {
            return outcomes.push(Outcome::CannotTrade { why: TradeRefusal::NothingOwed });
        }
        let owner = &mut self.characters[index];
        owner.stow_all(spoils);
        owner.busy_until = now + BOARD_TIME;
        outcomes.push(Outcome::Collected { spoils });
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
        let life = self.begin_life(named, now, REBIRTH_CARRYOVER_PERCENT, true);
        events.push(Event::Character { who: target, outcome: Outcome::Redeemed { by: Some(id_at(index)), life } });
    }

    /// Puts a spirit back in its body, if it is there and allowed to yet.
    fn return_to_body(&mut self, index: usize, now: Tick) -> Outcome {
        let character = &mut self.characters[index];
        let Some(death) = character.death().copied() else { return Outcome::NotDead };
        if character.place() != death.body {
            return Outcome::CannotReturn { why: ReturnRefusal::NotAtBody };
        }
        if death.risen {
            return Outcome::CannotReturn { why: ReturnRefusal::Risen };
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
        Outcome::Reborn { life: self.begin_life(index, now, REBIRTH_CARRYOVER_PERCENT, true) }
    }

    /// Starts a spirit's next life at the anchor. It keeps `base`
    /// percent of what it knew, more with Lineage and, if it may spend
    /// it, ectoplasm. Returns which life this is.
    fn begin_life(&mut self, index: usize, now: Tick, base: u32, spend_ectoplasm: bool) -> u32 {
        let who = id_at(index);
        // Whoever died where something can be worked remembers that work best.
        let favoured = self.characters[index]
            .death()
            .and_then(|death| self.zone.map().place(death.body))
            .and_then(|place| place.deposit)
            .map(|deposit| deposit.resource.attributes()[0]);
        let character = &mut self.characters[index];
        self.occupancy.leave(character.place(), who);
        self.occupancy.enter(self.anchor, who);
        character.be_reborn(self.anchor, now, Rebirth { base, favoured, spend_ectoplasm });
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
            self.fell(victim, Cause::Character(attacker), corruption, now, events);
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
            self.fell(index, Cause::Character(target), CORRUPTION_PER_DEATH, now, events);
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
    fn fell(&mut self, index: usize, by: Cause, corruption: i64, now: Tick, events: &mut Vec<Event>) {
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
        let (dropped, cleansed) = character.die(Death { body, passing, ready_at, lost_at, risen: false });
        self.relics.extend(Relic::new(who, body, dropped, now));
        // Only a tethered spirit stays by its body.
        if passing != Passing::Tethered {
            character.place = self.anchor;
            self.occupancy.leave(body, who);
            self.occupancy.enter(self.anchor, who);
        }
        self.zone.mourn(corruption);
        self.dead.push(who);
        events.push(Event::Character { who, outcome: Outcome::Died { by, place: body, passing, lost_at } });
        events.extend(cleansed.map(|outcome| Event::Character { who, outcome }));
    }

    /// How a spirit will stand to its body: corrupted in a collapsed
    /// zone, tethered if anyone but the killer stood by, severed if not.
    fn passing(&self, who: CharacterId, by: Cause, place: PlaceId) -> Passing {
        if self.zone.health() == Health::Collapsed {
            return Passing::Corrupted;
        }
        let someone_stood_by = self.occupants(place).iter().any(|&other| {
            other != who && Cause::Character(other) != by && self.character(other).is_some_and(|character| !character.is_spirit())
        });
        if someone_stood_by {
            Passing::Tethered
        } else {
            Passing::Severed
        }
    }

    /// Opens every relic here that will open for this character and
    /// takes what is in them. Trying a sealed one troubles its guard.
    fn take(&mut self, index: usize, now: Tick, outcomes: &mut Vec<Outcome>) {
        let who = id_at(index);
        let here = self.characters[index].place();
        let (open, sealed): (Vec<Relic>, Vec<Relic>) =
            self.relics_at(here).copied().partition(|relic| self.can_open(who, relic));
        if open.is_empty() {
            for relic in &sealed {
                let guards = self.creatures.iter_mut().filter(|guard| guard.place == here && guard.being.of == Some(relic.owner));
                guards.for_each(|guard| guard.provoke(who));
            }
            outcomes.push(if sealed.is_empty() { Outcome::NothingToTake } else { Outcome::RelicSealed });
            return;
        }
        let mut spoils = [0; crate::types::Resource::ALL.len()];
        let mut time = Duration::ZERO;
        for relic in &open {
            time = time.max(relic.tier().open_time());
            for (taken, held) in spoils.iter_mut().zip(relic.contents) {
                *taken += held;
            }
        }
        self.relics.retain(|relic| !open.contains(relic));
        let character = &mut self.characters[index];
        character.stow_all(spoils);
        character.busy_until = now + time;
        outcomes.push(Outcome::Took { spoils });
    }

    /// Strikes a creature. It will come for whoever struck it.
    fn fight(&mut self, index: usize, target: CreatureId, now: Tick, outcomes: &mut Vec<Outcome>, events: &mut Vec<Event>) {
        let who = id_at(index);
        let here = self.characters[index].place();
        let Some(slot) = self.creatures.iter().position(|creature| creature.id == target && creature.place == here) else {
            return outcomes.push(Outcome::NoSuchCreature);
        };
        // DAMAGE_SPREAD is tiny, so the cast cannot truncate.
        let roll = self.rng.below(DAMAGE_SPREAD) as u32;
        let striker = &mut self.characters[index];
        striker.busy_until = now + ATTACK_TIME;
        let creature = &mut self.creatures[slot];
        let blow = Blow {
            roll,
            vigor: striker.attribute(Attribute::Vigor).level(),
            taint: striker.taint(),
            resilience: creature.resilience,
        };
        let damage = blow.damage();
        let left = creature.hurt(damage);
        creature.provoke(who);
        let being = creature.being;
        outcomes.push(Outcome::HitCreature { being, damage, left });
        if left == 0 {
            outcomes.push(Outcome::Slew { being });
            self.remove_creature(slot, now, events);
        }
    }

    fn new_creature_id(&mut self) -> CreatureId {
        let id = CreatureId::new(self.next_creature);
        self.next_creature += 1;
        id
    }

    /// Takes a creature out of the world. A lurker's lair fills again
    /// in time. A Body Guard's going frees the spirit whose body it was,
    /// which must then be reborn.
    fn remove_creature(&mut self, slot: usize, now: Tick, events: &mut Vec<Event>) {
        let creature = self.creatures.remove(slot);
        match creature.being {
            Being { kind: CreatureKind::Lurker, .. } => self.empty_lairs.push((now + LURKER_RETURNS, creature.place)),
            Being { kind: CreatureKind::BodyGuard, of } => {
                let still_guarded = self.creatures.iter().any(|other| other.being.of == of);
                let bound = of.and_then(|owner| self.index_of(owner)).filter(|&owner| {
                    !still_guarded && self.characters[owner].death().is_some_and(|death| death.risen)
                });
                if let Some(owner) = bound {
                    let outcome = self.rebirth(owner, now);
                    events.push(Event::Character { who: id_at(owner), outcome });
                }
            }
        }
    }

    /// Every creature with someone to go for strikes them, at its own pace.
    fn creatures_strike(&mut self, now: Tick, events: &mut Vec<Event>) {
        for slot in 0..self.creatures.len() {
            if now < self.creatures[slot].next_blow {
                continue;
            }
            self.creatures[slot].next_blow = now + CREATURE_BLOW_EVERY;
            let Some(victim) = self.quarry(&self.creatures[slot]) else { continue };
            // DAMAGE_SPREAD is tiny, so the cast cannot truncate.
            let roll = self.rng.below(DAMAGE_SPREAD) as u32;
            let creature = &self.creatures[slot];
            let struck = &mut self.characters[victim];
            let blow = Blow {
                roll,
                vigor: creature.vigor,
                taint: 0,
                resilience: struck.attribute(Attribute::Resilience).level(),
            };
            let (being, damage) = (creature.being, blow.damage());
            let caught_lying = struck.is_lying_down();
            let health = struck.hurt(damage);
            events.push(Event::Character { who: id_at(victim), outcome: Outcome::Mauled { by: being, damage, health } });
            self.rouse(victim, now, events);
            if health == 0 {
                let corruption = if caught_lying { CORRUPTION_PER_SLEEPER_DEATH } else { CORRUPTION_PER_DEATH };
                self.fell(victim, Cause::Creature(being), corruption, now, events);
            }
        }
    }

    /// Who a creature would strike: the first of its foes still living
    /// in its place, or, if it goes for anyone, whoever living is there.
    fn quarry(&self, creature: &Creature) -> Option<usize> {
        let living = |who: &CharacterId| self.character(*who).is_some_and(|character| !character.is_spirit());
        let here = self.occupants(creature.place);
        let foe = creature.foes.iter().find(|&foe| here.contains(foe) && living(foe));
        let anyone = || here.iter().find(|&who| creature.being.kind.is_aggressive() && living(who));
        foe.or_else(anyone).and_then(|&who| self.index_of(who))
    }

    /// Clears away what has had its time: relics crumble, spent Body
    /// Guards fall, and empty lairs fill.
    fn upkeep(&mut self, now: Tick, events: &mut Vec<Event>) {
        if !now.get().is_multiple_of(UPKEEP_EVERY.as_ticks()) {
            return;
        }
        self.relics.retain(|relic| now < relic.expires_at);
        while let Some(slot) = self.creatures.iter().position(|creature| creature.gone_at.is_some_and(|at| at <= now)) {
            let (place, being) = (self.creatures[slot].place, self.creatures[slot].being);
            events.push(Event::CreatureGone { place, being });
            self.remove_creature(slot, now, events);
        }
        let due: Vec<PlaceId> = self.empty_lairs.iter().filter(|(at, _)| *at <= now).map(|&(_, place)| place).collect();
        self.empty_lairs.retain(|(at, _)| *at > now);
        for place in due {
            let id = self.new_creature_id();
            let lurker = Creature::lurker(id, place, now);
            events.push(Event::CreatureAppeared { place, being: lurker.being });
            self.creatures.push(lurker);
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
            // A sleeper someone has tended lasts that much longer.
            let allowed = LONG_SLEEP + character.vigil();
            let overslept = character.asleep_from().is_some_and(|from| from <= now && now.since(from) >= allowed);
            if !overslept {
                continue;
            }
            let (who, place) = (id_at(index), character.place());
            let (dropped, cleansed) = character.sleep_to_death(now);
            self.relics.extend(Relic::new(who, place, dropped, now));
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
            let life = self.begin_life(index, now, FADED_CARRYOVER_PERCENT, false);
            events.push(Event::Character { who: id_at(index), outcome: Outcome::Faded { life } });
        }
    }

    /// A body left too long rises as a Body Guard over what its owner
    /// dropped. The spirit cannot go back into it: it can give the body
    /// up, or wait for the guard to fall.
    fn lose_bodies(&mut self, now: Tick, events: &mut Vec<Event>) {
        let overdue: Vec<usize> = self
            .dead
            .iter()
            .filter_map(|&who| self.index_of(who))
            .filter(|&index| self.characters[index].death().is_some_and(|death| death.lost_at <= now))
            .collect();
        for index in overdue {
            let who = id_at(index);
            let Some(death) = self.characters[index].death().copied() else { continue };
            self.characters[index].lose_body();
            self.dead.retain(|&spirit| spirit != who);
            let id = self.new_creature_id();
            let guard = Creature::body_guard(id, who, &self.characters[index], death.body, now, now + GUARD_LASTS);
            events.push(Event::Character { who, outcome: Outcome::BodyRose });
            events.push(Event::CreatureAppeared { place: death.body, being: guard.being });
            self.creatures.push(guard);
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
        let arrives_living = !character.is_spirit();
        // A spirit past its first tier crosses the Spirit Layer faster.
        let pace = if arrives_living { 1 } else { character.spirit_tier().pace() };
        character.place = exit.to;
        character.busy_until = now + Duration::ticks(TRAVEL_TIME.as_ticks() / pace);
        // Whatever lives there needs a moment to notice a newcomer.
        if arrives_living {
            let noticed_at = now + TRAVEL_TIME + CREATURE_BLOW_EVERY;
            for creature in self.creatures.iter_mut().filter(|creature| creature.place == exit.to) {
                creature.next_blow = creature.next_blow.max(noticed_at);
            }
        }
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
        // A ward lets a deposit be worked only while it is thriving.
        if deposit.is_warded(now) && !deposit.is_thriving() {
            outcomes.push(Outcome::Warded { resource });
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
        self.creatures_strike(now, events);
        self.upkeep(now, events);
    }

    fn apply(&mut self, command: &Command, now: Tick, events: &mut Vec<Event>) {
        let who = command.who;
        if let Action::Join(name) = &command.action {
            return self.join(who, name, events);
        }
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
    use crate::content::RITE_WINDOW;
    use crate::haul::pressure_percent;
    use crate::types::{Deposit, Resource, SkillKind};
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
        assert_eq!(game.relics_at(LEDGE).next().unwrap().contents[Resource::Ore.index()], carried);
        assert!(died(&events, BRYN).is_some());
        let cleansed = Outcome::StandingChanged { from: Standing::Marked, to: Standing::Clean };
        assert!(events.contains(&Event::Character { who: BRYN, outcome: cleansed }));
        assert_eq!(game.bodies_at(LEDGE).map(|(id, _)| id).collect::<Vec<_>>(), [BRYN]);

        // A spirit has no hands, and the living cannot touch what they cannot see.
        let mut events = Vec::new();
        game.apply(&Command::take(BRYN), Tick::new(now), &mut events);
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
    fn a_body_left_too_long_rises_as_a_guard_and_the_spirit_can_only_give_it_up() {
        let (mut game, now) = bryn_marked_on_the_ledge();
        let mining_before = game.character(BRYN).unwrap().skill(crate::types::SkillKind::Mining).level();
        assert!(mining_before >= 2, "Bryn should have learned something first: {mining_before}");
        let (events, now) = fight_to_the_end(&mut game, now);
        let lost_at = died(&events, BRYN).unwrap().1.get();

        let before = ticks(&mut game, now, lost_at - 1);
        assert!(game.character(BRYN).unwrap().is_spirit());
        assert!(!before.iter().any(|event| matches!(event, Event::Character { outcome: Outcome::Reborn { .. }, .. })));

        let after = ticks(&mut game, lost_at, lost_at);
        assert!(after.contains(&Event::Character { who: BRYN, outcome: Outcome::BodyRose }));
        let being = Being { kind: CreatureKind::BodyGuard, of: Some(BRYN) };
        assert!(after.contains(&Event::CreatureAppeared { place: LEDGE, being }));
        let guard = game.creatures_at(LEDGE).next().unwrap();
        assert_eq!((guard.being, guard.health()), (being, 120));
        assert!(game.character(BRYN).unwrap().is_spirit());
        assert_eq!(game.bodies_at(LEDGE).count(), 0);

        // The body is the guard now: no going back into it.
        let mut events = Vec::new();
        game.apply(&Command::go(BRYN, "north"), Tick::new(lost_at), &mut events);
        game.apply(&Command::return_to_body(BRYN), Tick::new(lost_at + 30), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::CannotReturn { why: ReturnRefusal::Risen }));
        game.apply(&Command::relinquish(BRYN), Tick::new(lost_at + 30), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::Reborn { life: 2 }));
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
        assert_eq!(game.relics_at(game.character(ASH).unwrap().place()).count(), 0);

        // Ore makes it a resonant relic, which takes a little longer to open.
        game.apply(&Command::take(ASH), Tick::new(now + TAKE_TIME.as_ticks()), &mut events);
        assert!(matches!(outcome(&events), Some(Outcome::Busy { .. })));
        game.apply(&Command::take(ASH), Tick::new(now + RelicTier::Resonant.open_time().as_ticks()), &mut events);
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
        let relic = game.relics_at(ash.place()).next().expect("what they carried lies where they lay");
        assert!(relic.contents[Resource::Ore.index()] > 0);
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

    const GROTTO: PlaceId = PlaceId::new(7);

    /// Walks `who` from camp down into the grotto at low tide. Returns
    /// the tick at which they are free to act there.
    fn into_the_grotto(game: &mut Game, who: CharacterId) -> u64 {
        let mut events = Vec::new();
        for (step, exit) in ["east", "cave", "down"].into_iter().enumerate() {
            game.apply(&Command::go(who, exit), Tick::new(step as u64 * 30), &mut events);
        }
        assert_eq!(game.character(who).unwrap().place(), GROTTO);
        90
    }

    fn lurker(game: &Game) -> Option<&Creature> {
        game.creatures_at(GROTTO).find(|creature| creature.being.kind == CreatureKind::Lurker)
    }

    #[test]
    fn a_lurker_keeps_the_grotto_and_goes_for_whoever_comes_in() {
        let mut game = Game::new(1, &["Ash"]);
        assert_eq!(lurker(&game).map(Creature::health), Some(60));
        let arrived = into_the_grotto(&mut game, ASH);

        // It needs a moment to notice a newcomer.
        let quiet = ticks(&mut game, 61, arrived + CREATURE_BLOW_EVERY.as_ticks() - 1);
        assert!(!quiet.iter().any(|event| matches!(event, Event::Character { outcome: Outcome::Mauled { .. }, .. })));
        let struck = ticks(&mut game, arrived + CREATURE_BLOW_EVERY.as_ticks(), arrived + 30);
        let blows = struck.iter().filter(|event| matches!(event, Event::Character { who: ASH, outcome: Outcome::Mauled { .. } }));
        assert_eq!(blows.count(), 5, "one blow every six seconds");
        assert!(game.character(ASH).unwrap().health() < 100);
    }

    #[test]
    fn a_lurker_can_be_brought_down_and_another_comes_in_time() {
        let mut game = Game::new(1, &["Ash"]);
        let mut now = into_the_grotto(&mut game, ASH);
        let id = lurker(&game).unwrap().id;
        let mut events = Vec::new();
        while lurker(&game).is_some() {
            game.tick(Tick::new(now), &mut events);
            if now.is_multiple_of(ATTACK_TIME.as_ticks()) {
                game.apply(&Command::fight(ASH, id), Tick::new(now), &mut events);
            }
            now += 1;
        }
        let being = Being { kind: CreatureKind::Lurker, of: None };
        assert!(events.contains(&Event::Character { who: ASH, outcome: Outcome::Slew { being } }));
        assert!(game.character(ASH).unwrap().health() >= 60, "a fit character wins comfortably");
        game.apply(&Command::fight(ASH, id), Tick::new(now + 3), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::NoSuchCreature));

        let later = ticks(&mut game, now, now + LURKER_RETURNS.as_ticks() + 10);
        assert!(later.contains(&Event::CreatureAppeared { place: GROTTO, being }));
        assert_ne!(lurker(&game).unwrap().id, id, "ids are never reused");
    }

    #[test]
    fn a_lurker_kills_the_unwary_and_their_tide_glass_is_sealed_in_an_ancient_relic() {
        let mut game = Game::new(1, &["Ash", "Bryn"]);
        let mut now = into_the_grotto(&mut game, ASH);
        let mut events = Vec::new();
        while !game.character(ASH).unwrap().is_spirit() {
            game.tick(Tick::new(now), &mut events);
            game.apply(&Command::gather(ASH), Tick::new(now), &mut events);
            now += 1;
        }
        let being = Being { kind: CreatureKind::Lurker, of: None };
        assert!(events.iter().any(|event| matches!(
            event,
            Event::Character { who: ASH, outcome: Outcome::Died { by: Cause::Creature(by), passing: Passing::Severed, .. } } if *by == being
        )));
        let relic = *game.relics_at(GROTTO).next().unwrap();
        assert_eq!((relic.owner, relic.tier()), (ASH, RelicTier::Ancient));
        assert!(game.can_open(ASH, &relic));
        assert!(!game.can_open(BRYN, &relic), "sealed while the body lies beside it");
    }

    #[test]
    fn a_body_guard_keeps_an_ancient_relic_until_it_is_brought_down() {
        let mut game = Game::new(1, &["Ash", "Bryn"]);
        let mut now = into_the_grotto(&mut game, ASH);
        let mut events = Vec::new();
        while !game.character(ASH).unwrap().is_spirit() {
            game.tick(Tick::new(now), &mut events);
            game.apply(&Command::gather(ASH), Tick::new(now), &mut events);
            now += 1;
        }
        // Ten minutes on, the body rises. Bryn comes for what it kept.
        let risen = now + CORPSE_TIME.as_ticks();
        ticks(&mut game, now, risen);
        let guard = game.creatures_at(GROTTO).find(|creature| creature.being.of == Some(ASH)).unwrap().id;
        let low_tide = (risen / 1_200 + 1) * 1_200;
        ticks(&mut game, risen + 1, low_tide);
        for (step, exit) in ["east", "cave", "down"].into_iter().enumerate() {
            game.apply(&Command::go(BRYN, exit), Tick::new(low_tide + step as u64 * 30), &mut events);
        }
        let mut now = low_tide + 90;
        ticks(&mut game, low_tide + 1, now);

        // The lurker is there too. Bryn deals with it first.
        let beast = lurker(&game).unwrap().id;
        while lurker(&game).is_some() {
            game.apply(&Command::fight(BRYN, beast), Tick::new(now), &mut events);
            now += ATTACK_TIME.as_ticks();
            ticks(&mut game, now - 2, now);
        }
        game.apply(&Command::take(BRYN), Tick::new(now), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::RelicSealed));
        assert_eq!(game.character(BRYN).unwrap().carried_total(), 0);

        // Trying the seal troubled the guard: it strikes back now.
        let health = game.character(BRYN).unwrap().health();
        ticks(&mut game, now + 1, now + CREATURE_BLOW_EVERY.as_ticks());
        assert!(game.character(BRYN).unwrap().health() < health);
        assert!(game.character(ASH).unwrap().is_spirit(), "Ash is bound until the guard falls");

        assert!(game.creatures_at(GROTTO).any(|creature| creature.id == guard));
    }

    #[test]
    fn bringing_down_a_guard_opens_what_it_kept() {
        let mut game = Game::new(1, &["Ash", "Bryn"]);
        let mut now = into_the_grotto(&mut game, ASH);
        let mut events = Vec::new();
        while !game.character(ASH).unwrap().is_spirit() {
            game.tick(Tick::new(now), &mut events);
            game.apply(&Command::gather(ASH), Tick::new(now), &mut events);
            now += 1;
        }
        let glass = game.relics_at(GROTTO).next().unwrap().contents[Resource::Tideglass.index()];
        ticks(&mut game, now, now + CORPSE_TIME.as_ticks());
        let guard = game.creatures_at(GROTTO).find(|creature| creature.being.of == Some(ASH)).unwrap().id;

        // Bryn is put beside it, fresh, without the walk.
        let mut at = now + CORPSE_TIME.as_ticks() + 1;
        for exit in ["east", "cave", "down"] {
            game.characters[1].busy_until = Tick::ZERO;
            let open = game.map().exit(game.character(BRYN).unwrap().place(), exit).unwrap().to;
            game.occupancy.leave(game.characters[1].place, BRYN);
            game.characters[1].place = open;
            game.occupancy.enter(open, BRYN);
        }
        while game.creatures_at(GROTTO).any(|creature| creature.id == guard) {
            game.apply(&Command::fight(BRYN, guard), Tick::new(at), &mut events);
            at += ATTACK_TIME.as_ticks();
        }
        assert!(events.contains(&Event::Character { who: ASH, outcome: Outcome::Reborn { life: 2 } }));
        game.apply(&Command::take(BRYN), Tick::new(at), &mut events);
        assert_eq!(game.character(BRYN).unwrap().carried(Resource::Tideglass), glass);
    }

    #[test]
    fn a_guard_that_falls_or_runs_out_of_time_frees_its_spirit_to_be_reborn() {
        let (mut game, now) = bryn_marked_on_the_ledge();
        let (events, now) = fight_to_the_end(&mut game, now);
        let lost_at = died(&events, BRYN).unwrap().1.get();
        ticks(&mut game, now, lost_at);
        let guard = game.creatures_at(LEDGE).next().unwrap().id;

        // Ash never troubled it, so it leaves Ash alone.
        let health = game.character(ASH).unwrap().health();
        ticks(&mut game, lost_at + 1, lost_at + 60);
        assert!(game.character(ASH).unwrap().health() >= health);

        let mut world = game.clone();
        let mut events = Vec::new();
        let mut at = lost_at + 60;
        while world.creatures_at(LEDGE).next().is_some() {
            world.apply(&Command::fight(ASH, guard), Tick::new(at), &mut events);
            at += ATTACK_TIME.as_ticks();
        }
        assert!(events.contains(&Event::Character { who: BRYN, outcome: Outcome::Reborn { life: 2 } }));
        assert!(!world.character(BRYN).unwrap().is_spirit());

        // Left alone, it crumbles in the end, with the same result.
        let later = ticks(&mut game, lost_at + 61, lost_at + GUARD_LASTS.as_ticks() + 10);
        let being = Being { kind: CreatureKind::BodyGuard, of: Some(BRYN) };
        assert!(later.contains(&Event::CreatureGone { place: LEDGE, being }));
        assert!(later.contains(&Event::Character { who: BRYN, outcome: Outcome::Reborn { life: 2 } }));
        assert_eq!(game.creatures_at(LEDGE).count(), 0);
    }

    #[test]
    fn relics_crumble_when_their_time_is_up() {
        let (mut game, now) = bryn_marked_on_the_ledge();
        let (_, now) = fight_to_the_end(&mut game, now);
        let relic = *game.relics_at(LEDGE).next().unwrap();
        assert_eq!(relic.tier(), RelicTier::Resonant);
        ticks(&mut game, now, relic.expires_at.get() - 1);
        assert_eq!(game.relics_at(LEDGE).count(), 1);
        ticks(&mut game, relic.expires_at.get(), relic.expires_at.get() + 10);
        assert_eq!(game.relics_at(LEDGE).count(), 0);
    }

    /// Bryn dead on the Ore Ledge, Ash having killed them, with the
    /// spirit walked back from camp to stand over its body.
    fn bryn_a_spirit_on_the_ledge() -> (Game, u64) {
        let (mut game, now) = bryn_marked_on_the_ledge();
        let (_, now) = fight_to_the_end(&mut game, now);
        let mut events = Vec::new();
        game.apply(&Command::go(BRYN, "north"), Tick::new(now), &mut events);
        (game, now + TRAVEL_TIME.as_ticks())
    }

    #[test]
    fn a_spirit_draws_ectoplasm_where_the_living_work_and_keeps_it_through_rebirth() {
        let (mut game, mut now) = bryn_a_spirit_on_the_ledge();
        let mut events = Vec::new();
        for _ in 0..45 {
            game.apply(&Command::gather(BRYN), Tick::new(now), &mut events);
            now += DRAW_TIME.as_ticks();
        }
        assert_eq!(outcome(&events), Some(&Outcome::Drew { amount: 1, held: 45 }));
        let mining = game.character(BRYN).unwrap().skill(SkillKind::Mining).level();

        game.apply(&Command::relinquish(BRYN), Tick::new(now), &mut events);
        let bryn = game.character(BRYN).unwrap();
        // 45 ectoplasm buys two percent on top of the fifteen; five are left over.
        assert_eq!((bryn.kept(), bryn.ectoplasm()), (17, 5));
        assert_eq!(bryn.skill(SkillKind::Mining).level(), mining * 17 / 100);
    }

    #[test]
    fn there_is_no_ectoplasm_at_camp_and_none_for_the_living() {
        let (mut game, now) = bryn_marked_on_the_ledge();
        let (_, now) = fight_to_the_end(&mut game, now);
        let mut events = Vec::new();
        game.apply(&Command::gather(BRYN), Tick::new(now), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::NothingToGather), "severed spirits start at camp");
        assert_eq!(game.character(ASH).unwrap().ectoplasm(), 0);
    }

    #[test]
    fn enough_ectoplasm_makes_a_wandering_phantom_who_travels_twice_as_fast() {
        let (mut game, mut now) = bryn_a_spirit_on_the_ledge();
        let mut events = Vec::new();
        for _ in 0..100 {
            game.apply(&Command::gather(BRYN), Tick::new(now), &mut events);
            now += DRAW_TIME.as_ticks();
        }
        let risen = Outcome::SpiritRose { tier: crate::spirit::SpiritTier::WanderingPhantom };
        assert!(events.contains(&Event::Character { who: BRYN, outcome: risen }));
        game.apply(&Command::go(BRYN, "north"), Tick::new(now), &mut events);
        assert_eq!(game.character(BRYN).unwrap().busy_until(), Tick::new(now + TRAVEL_TIME.as_ticks() / 2));
    }

    #[test]
    fn every_life_lived_adds_to_lineage() {
        let (mut game, now) = bryn_a_spirit_on_the_ledge();
        assert_eq!(game.character(BRYN).unwrap().attribute(Attribute::Lineage).level(), 0);
        let mut events = Vec::new();
        game.apply(&Command::relinquish(BRYN), Tick::new(now), &mut events);
        assert_eq!(game.character(BRYN).unwrap().attribute(Attribute::Lineage).level(), 2);
    }

    #[test]
    fn the_spirit_grows_as_the_body_wastes() {
        let mut game = Game::new(1, &["Ash"]);
        let mut events = Vec::new();
        game.apply(&Command::go(ASH, "north"), Tick::new(0), &mut events);
        let mut now = 30;
        while game.character(ASH).unwrap().skill(SkillKind::Mining).level() < 6 {
            game.tick(Tick::new(now), &mut events);
            game.apply(&Command::gather(ASH), Tick::new(now), &mut events);
            now += 1;
        }
        now += GATHER_TIME.as_ticks();
        assert_eq!(game.character(ASH).unwrap().attribute(Attribute::Attunement).level(), 0);
        game.apply(&Command::rest(ASH), Tick::new(now), &mut events);
        game.apply(&Command::wake(ASH), Tick::new(now + Duration::days(10).as_ticks()), &mut events);
        assert!(woke(&events).unwrap().2 > 0, "the body should have wasted");
        assert!(game.character(ASH).unwrap().attribute(Attribute::Attunement).level() > 0);
    }

    #[test]
    fn newcomers_join_at_the_anchor_in_order_and_are_seen_arriving() {
        let mut game = Game::new(1, &[]);
        assert_eq!((game.population(), game.next_id()), (0, ASH));
        let mut events = Vec::new();
        game.apply(&Command::join(ASH, "Ash"), Tick::new(5), &mut events);
        game.apply(&Command::join(BRYN, "Bryn"), Tick::new(9), &mut events);
        assert_eq!(events[1], Event::Character { who: BRYN, outcome: Outcome::Joined });
        assert_eq!(game.audience(&events[1]), [ASH, BRYN]);
        assert_eq!(game.occupants(game.anchor()), [ASH, BRYN]);
        assert_eq!(game.character(BRYN).unwrap().name(), "Bryn");

        // Only the next free id can join; anything else changes nothing.
        let before = game.clone();
        game.apply(&Command::join(ASH, "Cole"), Tick::new(10), &mut events);
        game.apply(&Command::join(CharacterId::new(7), "Cole"), Tick::new(10), &mut events);
        assert_eq!(game, before);
        assert_eq!(outcome_or_unknown(&events), Some(CharacterId::new(7)));
    }

    fn outcome_or_unknown(events: &[Event]) -> Option<CharacterId> {
        match events.last() {
            Some(Event::UnknownCharacter { who }) => Some(*who),
            _ => None,
        }
    }

    const fn lot(resource: Resource, amount: u64) -> Lot {
        Lot { resource, amount }
    }

    /// Ash back at camp with some ore, and Bryn there with some driftwood.
    fn two_with_goods_at_camp() -> (Game, u64) {
        let mut game = Game::new(1, &["Ash", "Bryn"]);
        let mut events = Vec::new();
        game.apply(&Command::go(ASH, "north"), Tick::new(0), &mut events);
        game.apply(&Command::go(BRYN, "east"), Tick::new(0), &mut events);
        for haul in 0..3 {
            let at = Tick::new(30 + haul * GATHER_TIME.as_ticks());
            game.apply(&Command::gather(ASH), at, &mut events);
            game.apply(&Command::gather(BRYN), at, &mut events);
        }
        game.apply(&Command::go(ASH, "south"), Tick::new(60), &mut events);
        game.apply(&Command::go(BRYN, "west"), Tick::new(60), &mut events);
        (game, 90)
    }

    #[test]
    fn a_contract_holds_what_is_offered_and_pays_the_poster_when_accepted() {
        let (mut game, now) = two_with_goods_at_camp();
        let ore = game.character(ASH).unwrap().carried(Resource::Ore);
        let wood = game.character(BRYN).unwrap().carried(Resource::Driftwood);
        assert!(ore >= 20 && wood >= 20, "not enough to trade with: {ore} ore, {wood} driftwood");
        let (give, want) = (lot(Resource::Ore, 20), lot(Resource::Driftwood, 15));

        let mut events = Vec::new();
        game.apply(&Command::post(ASH, give, want), Tick::new(now), &mut events);
        let id = ContractId::new(0);
        assert_eq!(outcome(&events), Some(&Outcome::Posted { id, give, want }));
        assert_eq!(game.character(ASH).unwrap().carried(Resource::Ore), ore - 20, "the offer is held by the board");

        // Ash can go: the trade does not need them there.
        game.apply(&Command::go(ASH, "north"), Tick::new(now + BOARD_TIME.as_ticks()), &mut events);
        events.clear();
        game.apply(&Command::accept(BRYN, id), Tick::new(now + 60), &mut events);
        assert_eq!(events[0], Event::Character { who: BRYN, outcome: Outcome::Accepted { id, paid: want, got: give } });
        assert_eq!(events[1], Event::Character { who: ASH, outcome: Outcome::Filled { id, by: BRYN, earned: want } });
        let bryn = game.character(BRYN).unwrap();
        assert_eq!((bryn.carried(Resource::Ore), bryn.carried(Resource::Driftwood)), (20, wood - 15));
        assert!(game.board().contracts().is_empty());
        assert_eq!(game.board().owed_to(ASH)[Resource::Driftwood.index()], 15);

        // The price waits at the board until Ash comes for it.
        game.apply(&Command::collect(ASH), Tick::new(now + 90), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::CannotTrade { why: TradeRefusal::NotAtBoard }));
        game.apply(&Command::go(ASH, "south"), Tick::new(now + 90), &mut events);
        game.apply(&Command::collect(ASH), Tick::new(now + 120), &mut events);
        assert_eq!(game.character(ASH).unwrap().carried(Resource::Driftwood), 15);
        game.apply(&Command::collect(ASH), Tick::new(now + 140), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::CannotTrade { why: TradeRefusal::NothingOwed }));
    }

    #[test]
    fn the_board_refuses_what_cannot_be_paid_for_and_gives_back_what_is_withdrawn() {
        let (mut game, now) = two_with_goods_at_camp();
        let ore = game.character(ASH).unwrap().carried(Resource::Ore);
        let refused = |events: &[Event]| match outcome(events) {
            Some(Outcome::CannotTrade { why }) => Some(*why),
            _ => None,
        };
        let mut events = Vec::new();
        let mut at = now;
        let mut step = |game: &mut Game, command: Command, events: &mut Vec<Event>| {
            game.apply(&command, Tick::new(at), events);
            at += BOARD_TIME.as_ticks();
        };

        step(&mut game, Command::post(ASH, lot(Resource::Ore, ore + 1), lot(Resource::Kelp, 1)), &mut events);
        assert_eq!(refused(&events), Some(TradeRefusal::CannotPay));
        step(&mut game, Command::post(ASH, lot(Resource::Ore, 0), lot(Resource::Kelp, 1)), &mut events);
        assert_eq!(refused(&events), Some(TradeRefusal::Empty));

        step(&mut game, Command::post(ASH, lot(Resource::Ore, 5), lot(Resource::Tideglass, 99)), &mut events);
        let id = ContractId::new(0);
        step(&mut game, Command::accept(ASH, id), &mut events);
        assert_eq!(refused(&events), Some(TradeRefusal::YourOwn));
        step(&mut game, Command::accept(BRYN, id), &mut events);
        assert_eq!(refused(&events), Some(TradeRefusal::CannotPay));
        step(&mut game, Command::withdraw(BRYN, id), &mut events);
        assert_eq!(refused(&events), Some(TradeRefusal::NotYours));
        step(&mut game, Command::accept(BRYN, ContractId::new(9)), &mut events);
        assert_eq!(refused(&events), Some(TradeRefusal::NoSuchContract));

        step(&mut game, Command::withdraw(ASH, id), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::Withdrew { id, back: lot(Resource::Ore, 5) }));
        assert_eq!(game.character(ASH).unwrap().carried(Resource::Ore), ore);
    }

    #[test]
    fn a_tended_sleeper_outlasts_the_long_sleep_by_a_day_for_each_tending() {
        let mut game = Game::new(1, &["Ash", "Bryn"]);
        let mut events = Vec::new();
        game.apply(&Command::tend(BRYN, ASH), Tick::new(0), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::CannotTend { target: ASH, why: TendRefusal::Awake }));
        game.apply(&Command::tend(ASH, ASH), Tick::new(0), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::CannotTend { target: ASH, why: TendRefusal::Yourself }));

        game.apply(&Command::rest(ASH), Tick::new(0), &mut events);
        let asleep = SETTLE_TIME.as_ticks();
        game.apply(&Command::tend(BRYN, ASH), Tick::new(asleep), &mut events);
        game.apply(&Command::tend(BRYN, ASH), Tick::new(asleep + TEND_TIME.as_ticks()), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::Tended { target: ASH, vigil: Duration::days(2) }));

        // Fourteen days would have killed Ash. Sixteen do.
        let untended = asleep + LONG_SLEEP.as_ticks() + 120;
        ticks(&mut game, 1, untended);
        assert!(game.character(ASH).unwrap().is_asleep(Tick::new(untended)));
        ticks(&mut game, untended + 1, untended + Duration::days(2).as_ticks());
        assert!(game.character(ASH).unwrap().forgotten().is_some());
    }

    #[test]
    fn tending_only_helps_so_far_and_is_gone_on_waking() {
        let mut game = Game::new(1, &["Ash", "Bryn"]);
        let mut events = Vec::new();
        game.apply(&Command::rest(ASH), Tick::new(0), &mut events);
        let mut now = SETTLE_TIME.as_ticks();
        for _ in 0..14 {
            game.apply(&Command::tend(BRYN, ASH), Tick::new(now), &mut events);
            now += TEND_TIME.as_ticks();
        }
        assert_eq!(game.character(ASH).unwrap().vigil(), MOST_VIGIL);
        game.apply(&Command::tend(BRYN, ASH), Tick::new(now), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::CannotTend { target: ASH, why: TendRefusal::Enough }));

        game.apply(&Command::wake(ASH), Tick::new(now), &mut events);
        assert_eq!(game.character(ASH).unwrap().vigil(), Duration::ZERO);
    }

    /// Ash, Bryn and Cole on the Ore Ledge, which Cole has just stripped.
    /// Returns the world and the tick at which all three are free.
    fn a_stripped_ledge() -> (Game, u64) {
        const COLE: CharacterId = CharacterId::new(2);
        let mut game = Game::new(1, &["Ash", "Bryn", "Cole"]);
        let mut events = Vec::new();
        for who in [ASH, BRYN, COLE] {
            game.apply(&Command::go(who, "north"), Tick::new(0), &mut events);
        }
        let mut now = 30;
        while pressure_percent(&game.map().place(LEDGE).unwrap().deposit.unwrap().node) < 90 {
            game.apply(&Command::gather(COLE), Tick::new(now), &mut events);
            now += GATHER_TIME.as_ticks();
        }
        (game, now)
    }

    fn ledge(game: &Game) -> Deposit {
        game.map().place(LEDGE).unwrap().deposit.unwrap()
    }

    #[test]
    fn two_voices_mend_a_stripped_deposit_and_ward_it() {
        let (mut game, now) = a_stripped_ledge();
        let before = ledge(&game);
        let mut events = Vec::new();

        game.apply(&Command::rite(ASH), Tick::new(now), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::RiteBegun));
        assert_eq!(ledge(&game).node, before.node, "one voice mends nothing");
        // The same voice twice is still one voice.
        game.apply(&Command::rite(ASH), Tick::new(now + RITE_TIME.as_ticks()), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::RiteBegun));

        events.clear();
        game.apply(&Command::rite(BRYN), Tick::new(now + 40), &mut events);
        assert_eq!(events[0], Event::Character { who: BRYN, outcome: Outcome::RiteDone { with: ASH } });
        assert_eq!(events[1], Event::Character { who: ASH, outcome: Outcome::RiteAnswered { by: BRYN } });
        let after = ledge(&game);
        assert_eq!(after.node.stock(), after.node.capacity() / 2, "brought up to half, no further");
        assert!(after.node.strain_percent() + 50 <= before.node.strain_percent().max(50));
        assert!(after.is_warded(Tick::new(now + 40)));
        assert_eq!(after.warded_until, Tick::new(now + 40) + WARD_TIME);
    }

    #[test]
    fn a_ward_lets_a_deposit_be_worked_only_while_it_thrives() {
        let (mut game, now) = a_stripped_ledge();
        let mut events = Vec::new();
        game.apply(&Command::rite(ASH), Tick::new(now), &mut events);
        game.apply(&Command::rite(BRYN), Tick::new(now), &mut events);
        let free = now + RITE_TIME.as_ticks();

        // Mended but still worn: it will not yield, to anyone.
        assert!(!ledge(&game).is_thriving());
        game.apply(&Command::gather(ASH), Tick::new(free), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::Warded { resource: Resource::Ore }));
        game.apply(&Command::rite(ASH), Tick::new(free), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::CannotRite { why: RiteRefusal::Warded }));

        // When the ward has run its time the deposit can be worked again.
        let lifted = free + WARD_TIME.as_ticks();
        ticks(&mut game, free + 1, lifted);
        game.apply(&Command::gather(ASH), Tick::new(lifted), &mut events);
        assert!(matches!(outcome(&events), Some(Outcome::Gathered { .. }) | Some(Outcome::AttributeRose { .. }) | Some(Outcome::SkillRose { .. })));
    }

    #[test]
    fn a_rite_is_not_for_the_marked_nor_for_ground_that_is_well_nor_for_camp() {
        const COLE: CharacterId = CharacterId::new(2);
        let (mut game, now) = a_stripped_ledge();
        let refused = |events: &[Event]| match outcome(events) {
            Some(Outcome::CannotRite { why }) => Some(*why),
            _ => None,
        };
        let mut events = Vec::new();
        assert_eq!(game.character(COLE).unwrap().standing(), Standing::Marked);
        game.apply(&Command::rite(COLE), Tick::new(now), &mut events);
        assert_eq!(refused(&events), Some(RiteRefusal::Marked));

        game.apply(&Command::go(ASH, "north"), Tick::new(now), &mut events);
        game.apply(&Command::rite(ASH), Tick::new(now + 30), &mut events);
        assert_eq!(refused(&events), Some(RiteRefusal::NoNeed), "the High Shelf is untouched");
        game.apply(&Command::go(BRYN, "south"), Tick::new(now), &mut events);
        game.apply(&Command::rite(BRYN), Tick::new(now + 30), &mut events);
        assert_eq!(refused(&events), Some(RiteRefusal::NothingToMend));
    }

    #[test]
    fn a_rite_left_unanswered_lapses() {
        let (mut game, now) = a_stripped_ledge();
        let mut events = Vec::new();
        game.apply(&Command::rite(ASH), Tick::new(now), &mut events);
        let late = now + RITE_WINDOW.as_ticks() + 1;
        game.apply(&Command::rite(BRYN), Tick::new(late), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::RiteBegun), "too late to answer: Bryn begins anew");
        game.apply(&Command::rite(ASH), Tick::new(late + 5), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::RiteAnswered { by: ASH }), "Ash answers the new one");
    }
}
