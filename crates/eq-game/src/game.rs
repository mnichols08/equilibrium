//! The rules: how commands and the passing of time change the world.

use alloc::string::String;
use alloc::vec::Vec;

use eq_clock::{Duration, Tick};
use eq_events::World;
use eq_rng::Rng;
use eq_world_graph::{Graph, Passage, PlaceId};


use crate::bloodline::Bloodline;
use crate::board::{Board, Bounty, ContractId, Lot, TradeRefusal};
use crate::character::{Character, CharacterId, Death, Rebirth};
use crate::combat::{may_attack, may_rob, Blow, FoundRefusal, NameRefusal, Refusal, ReturnRefusal, RiteRefusal, RobRefusal, TendRefusal};
use crate::command::{Action, Command};
use crate::creature::{Being, Creature, CreatureId, CreatureKind, Grave, Relic, RelicTier};
use crate::content::{
    self, ATTACK_TIME, BASE_EFFORT, BOARD_TIME, BURY_TIME, MOST_VIGIL, RITE_MENDS_PERCENT, RITE_TIME, TEND_TIME,
    VIGIL_PER_TENDING, WARD_TIME, CORPSE_TIME, CORRUPTION_PER_DEATH, CORRUPTION_PER_SLEEPER_DEATH,
    CREATURE_BLOW_EVERY, GUARD_LASTS, LURKER_RETURNS, DAMAGE_SPREAD, DRAW_TIME, AFTERLIFE_XP_PER_DRAW, FIGHTING_XP_PER_BLOW, EFFORT_SPREAD,
    FADED_CARRYOVER_PERCENT, GATHER_TIME,
    HAVEN_MENDING, HEALTH_MENDS_EVERY, LONG_SLEEP, MEMORIES_TO_REDEEM, MENDING, MIN_WORKABLE_STOCK, NAME_TIME,
    REBIRTH_CARRYOVER_PERCENT, REDEMPTION_CLOCK, RESTED_LASTS, SEEK_TIME, SETTLE_TIME, TAINT_FADES_EVERY,
    TAINT_FOR_ROBBING, TAKE_TIME, TRAVEL_TIME,
};
use crate::rest::FULLY_RESTED;
use crate::event::{Event, Outcome};
use crate::haul::{pressure_percent, taint_for, Haul};
use crate::occupancy::Occupancy;
use crate::pact::{Dealing, Dealt, Pact, PactRefusal, Pacts};
use crate::types::{Attribute, Cause, Health, Passing, Place, Safety, SkillKind, Standing};
use crate::balance::Balance;
use crate::era::Era;
use crate::escort::{Escort, Escorts};
use crate::zone::{Epitaph, Weather, Zone};

/// How often the world looks for sleepers whose time has run out.
const LONG_SLEEP_CHECK: Duration = Duration::minutes(1);

/// How often the world clears away what has had its time: crumbled
/// relics, spent Body Guards, and lairs due a new lurker.
const UPKEEP_EVERY: Duration = Duration::seconds(10);

/// The whole world: its territories and the characters in them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Game {
    rng: Rng,
    /// The seed the world was made from. Territories are made from it.
    seed: u64,
    /// How many territories may still be made for founders.
    founders_left: usize,
    /// Where the passes out of the founders' lands all lead, if this is
    /// a world for founders.
    meeting_ground: Option<PlaceId>,
    /// Where souls enter the afterlife, in a world with founders.
    river: Option<PlaceId>,
    /// How the lands stand to each other.
    pacts: Pacts,
    /// How far all that peace and war has tipped the world.
    balance: Balance,
    /// The era the world is in.
    era: Era,
    /// What founders have called the bloodlines, in bloodline order.
    bloodline_names: [Option<String>; Bloodline::ALL.len()],
    /// Every grave in the world, oldest first.
    graves: Vec<Grave>,
    /// Who has paid to be seen safely somewhere, and by whom.
    escorts: Escorts,
    /// Every place in the world. Each zone is a run of them.
    map: Graph<Place>,
    /// The world's territories, oldest first. There is always one.
    zones: Vec<Zone>,
    characters: Vec<Character>,
    occupancy: Occupancy,
    /// Every spirit with a body still lying somewhere, so the tick need
    /// not search for them.
    dead: Vec<CharacterId>,
    /// Every forgotten spirit, dormant or not.
    forgotten: Vec<CharacterId>,
    /// Every creature now in the world, oldest first.
    creatures: Vec<Creature>,
    /// The id the next creature will get.
    next_creature: u64,
    /// Lairs waiting for a new lurker, and when it comes.
    empty_lairs: Vec<(Tick, PlaceId)>,
    /// What the dead dropped, oldest first.
    relics: Vec<Relic>,
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
            seed,
            founders_left: 0,
            meeting_ground: None,
            river: None,
            pacts: Pacts::new(),
            balance: Balance::new(),
            era: Era::Rebuilding,
            bloodline_names: [const { None }; Bloodline::ALL.len()],
            graves: Vec::new(),
            escorts: Escorts::new(),
            zones: alloc::vec![Zone::new(PlaceId::new(0), map.len(), start, None)],
            map,
            characters,
            occupancy,
            dead: Vec::new(),
            forgotten: Vec::new(),
            creatures: Vec::new(),
            next_creature: 0,
            empty_lairs: Vec::new(),
            relics: Vec::new(),
        };
        for lair in lairs {
            game.settle_lair(lair);
        }
        game
    }

    /// An empty world for founders: the Rocky Shoreline, and room for
    /// `founders` more territories. Each of the first `founders` people
    /// to join uninvited gets a territory of their own, made from the
    /// seed. Those a founder has invited arrive in that founder's land.
    /// Once every territory is founded, the uninvited arrive wherever
    /// there are fewest people.
    ///
    /// `settlers` are already living on the shoreline when the world
    /// begins. They found nothing.
    pub fn new_world(seed: u64, founders: usize, settlers: &[&str]) -> Self {
        let mut game = Self::new(seed, settlers);
        game.founders_left = founders;
        if founders > 0 {
            // Where the lands meet. It is a zone of its own, with one place.
            let ground = game.map.add_place(content::meeting_ground());
            game.occupancy.grow(game.map.len());
            game.zones.push(Zone::new(ground, 1, ground, None));
            game.meeting_ground = Some(ground);
            // Where souls enter the afterlife. Also a zone of its own.
            let river = game.map.add_place(content::river_of_souls());
            game.occupancy.grow(game.map.len());
            game.zones.push(Zone::new(river, 1, river, None));
            game.river = Some(river);
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
        // Founders and those they invite arrive alive. So does everyone
        // in a world that was never meant to have founders. Anyone else
        // begins as a spirit and has to earn a first life.
        let invited = self.zones.iter().any(|zone| zone.invited.contains(&name.to_lowercase()));
        let unborn = self.is_founded() && !invited && self.founders_left == 0;
        let (zone, founded) = self.zone_for_newcomer(who, name);
        // The unborn begin where every soul enters the afterlife.
        let anchor = self.river.filter(|_| unborn).unwrap_or_else(|| self.zones[zone].anchor());
        let mut newcomer = Character::new(name, anchor);
        newcomer.bloodline = self.zones[zone].bloodline();
        if unborn {
            newcomer.be_unborn();
            self.forgotten.push(who);
        }
        self.characters.push(newcomer);
        self.occupancy.enter(anchor, who);
        events.push(Event::Character { who, outcome: Outcome::Joined });
        if founded {
            events.push(Event::Character { who, outcome: Outcome::Founded });
        }
        if unborn {
            events.push(Event::Character { who, outcome: Outcome::Unborn });
        }
    }

    /// Which territory a newcomer arrives in, and whether it was made
    /// for them: the land of whoever invited them; or, while there is
    /// room for founders, a new land of their own; or wherever there
    /// are fewest people.
    fn zone_for_newcomer(&mut self, who: CharacterId, name: &str) -> (usize, bool) {
        let handle = name.to_lowercase();
        if let Some(zone) = self.zones.iter().position(|zone| zone.invited.contains(&handle)) {
            self.zones[zone].invited.retain(|invited| *invited != handle);
            return (zone, false);
        }
        if self.founders_left > 0 {
            self.founders_left -= 1;
            return (self.found_territory(who), true);
        }
        (self.emptiest_land(None), false)
    }

    /// Where souls enter the afterlife, if this world has such a place.
    pub const fn river(&self) -> Option<PlaceId> {
        self.river
    }

    /// Carries a soul to the River of Souls. Returns whether the world
    /// has one to carry it to.
    fn carry_to_river(&mut self, index: usize) -> bool {
        let Some(river) = self.river else { return false };
        let who = id_at(index);
        let character = &mut self.characters[index];
        self.occupancy.leave(character.place(), who);
        character.place = river;
        self.occupancy.enter(river, who);
        true
    }

    /// Whether anyone has founded a land here yet.
    fn is_founded(&self) -> bool {
        self.zones.iter().any(|zone| zone.founder().is_some())
    }

    /// The land with the fewest people who call it home, not counting
    /// `apart_from`. The oldest land wins a tie. Nobody lives on the
    /// Meeting Ground, and the unborn and the forgotten are nobody's
    /// people yet.
    fn emptiest_land(&self, apart_from: Option<CharacterId>) -> usize {
        let people = |zone: &Zone| {
            self.characters()
                .filter(|(id, character)| Some(*id) != apart_from && character.forgotten().is_none() && zone.contains(character.home()))
                .count()
        };
        // Once there are founders, people arise in founded lands only:
        // the old shoreline is joined to nothing.
        let founded = self.is_founded();
        let lived_in = |zone: &Zone| {
            self.meeting_ground.is_none_or(|ground| !zone.contains(ground)) && (!founded || zone.founder().is_some())
        };
        (0..self.zones.len())
            .filter(|&index| lived_in(&self.zones[index]))
            .min_by_key(|&index| (people(&self.zones[index]), index))
            .unwrap_or(0)
    }

    /// Buries a body where it lies. It will not rise, what its owner
    /// dropped goes into the ground with it, and the owner's next life
    /// will remember this one.
    fn bury(&mut self, index: usize, target: CharacterId, now: Tick, outcomes: &mut Vec<Outcome>, events: &mut Vec<Event>) {
        let who = id_at(index);
        let here = self.characters[index].place();
        let lies_here = self.dead.contains(&target) && self.character(target).and_then(Character::death).is_some_and(|death| death.body == here && !death.risen);
        let Some(body) = self.index_of(target).filter(|_| lies_here) else {
            return outcomes.push(Outcome::CannotBury { target });
        };
        self.characters[body].be_buried();
        self.dead.retain(|&spirit| spirit != target);
        // What they dropped goes into the ground with them, and stays.
        let mut contents = [0; crate::types::Resource::ALL.len()];
        for relic in self.relics.iter().filter(|relic| relic.owner == target && relic.place == here) {
            for (buried, dropped) in contents.iter_mut().zip(relic.contents) {
                *buried += dropped;
            }
        }
        self.relics.retain(|relic| !(relic.owner == target && relic.place == here));
        self.graves.push(Grave { owner: target, place: here, contents });
        self.characters[index].busy_until = now + BURY_TIME;
        outcomes.push(Outcome::Buried { target });
        events.push(Event::Character { who: target, outcome: Outcome::WasBuried { by: who } });
    }

    /// The graves in a place, oldest first.
    pub fn graves_at(&self, place: PlaceId) -> impl Iterator<Item = &Grave> {
        self.graves.iter().filter(move |grave| grave.place == place)
    }

    /// Digs up a grave where the digger stands: their own if one of
    /// theirs is here, or else the oldest. They have what was buried
    /// in it. The price is the tie it kept: its owner no longer learns
    /// again fast what that life knew, whether the owner is the digger,
    /// or someone else, living or still in the afterlife; and it changes
    /// the owner, body for spirit (see `Character::be_unearthed`), which
    /// costs something however much of that life they had won back.
    /// Digging up someone else's grave is robbery, and the land marks it.
    fn dig(&mut self, index: usize, now: Tick, outcomes: &mut Vec<Outcome>, events: &mut Vec<Event>) {
        let who = id_at(index);
        let here = self.characters[index].place();
        let slot = |owned: bool| self.graves.iter().position(|grave| grave.place == here && (grave.owner == who) == owned);
        let Some(slot) = slot(true).or_else(|| slot(false)) else {
            return outcomes.push(Outcome::NoGrave);
        };
        let grave = self.graves.remove(slot);
        let digger = &mut self.characters[index];
        digger.stow_all(grave.contents);
        digger.busy_until = now + BURY_TIME;
        outcomes.push(Outcome::DugUp { owner: grave.owner, took: grave.contents });
        if grave.owner == who {
            let (fell, levels) = digger.be_unearthed();
            return outcomes.push(Outcome::Unearthed { fell, levels });
        }
        outcomes.extend(digger.shift_taint(TAINT_FOR_ROBBING));
        if let Some(owner) = self.index_of(grave.owner) {
            let owner = &mut self.characters[owner];
            // If they are still in the afterlife from that death, the
            // life it was will not be remembered when they come out.
            if owner.death().is_some_and(|death| death.buried && death.body == here) {
                owner.be_disturbed();
            }
            let (fell, levels) = owner.be_unearthed();
            events.push(Event::Character { who: grave.owner, outcome: Outcome::GraveRobbed { by: who } });
            events.push(Event::Character { who: grave.owner, outcome: Outcome::Unearthed { fell, levels } });
        }
    }

    /// Makes a new territory from the world's seed and adds it to the
    /// map, joined to nothing. Returns which zone it is.
    fn found_territory(&mut self, founder: CharacterId) -> usize {
        // Each territory gets its own seed, so the world's own dice are
        // left alone and the lands come out the same on every replay.
        let number = self.zones.len() as u64;
        let seed = Rng::from_seed(self.seed).split(number).below(u64::MAX);
        let mut territory = crate::territory::generate(seed);
        let pass = territory.add_pass();
        let places = territory.map.len();
        let first = self.map.absorb(territory.map);
        let anchor = PlaceId::new(first.get() + territory.anchor.get());
        // The pass leads out to where every land's pass leads.
        if let Some(ground) = self.meeting_ground {
            let pass = PlaceId::new(first.get() + pass.get());
            let founded = self.zones.iter().filter(|zone| zone.founder.is_some()).count();
            let way_back = content::pass_label(founded);
            self.map.connect_both(pass, "beyond", ground, &way_back, Passage::Open).expect("both places are on the map");
            // And a way leads into it from the River of Souls, for spirits only:
            // nothing leads back.
            if let Some(river) = self.river {
                self.map.connect(river, &way_back, anchor, Passage::Open).expect("both places are on the map");
            }
        }
        self.occupancy.grow(self.map.len());
        let mut zone = Zone::new(first, places, anchor, Some(founder));
        // The lands take the six bloodlines in turn.
        zone.bloodline = Bloodline::of_land(self.zones.iter().filter(|zone| zone.founder.is_some()).count());
        let lairs: Vec<PlaceId> =
            zone.place_ids().filter(|&place| self.map.place(place).is_some_and(|data| data.lair.is_some())).collect();
        for lair in lairs {
            self.settle_lair(lair);
        }
        self.zones.push(zone);
        self.zones.len() - 1
    }

    /// Puts into `lair` the creature that makes its home there.
    fn settle_lair(&mut self, lair: PlaceId) {
        let Some(kind) = self.map.place(lair).and_then(|data| data.lair) else { return };
        let id = self.new_creature_id();
        self.creatures.push(Creature::native(kind, id, lair, Tick::ZERO));
    }

    /// The territory the character at `index` founded, if they stand as
    /// its founder.
    fn founded_by(&self, index: usize) -> Result<usize, FoundRefusal> {
        let zone = self.zone_index(self.characters[index].home());
        if self.zones[zone].founder == Some(id_at(index)) {
            Ok(zone)
        } else {
            Err(FoundRefusal::NotFounder)
        }
    }

    /// What a bloodline is called: what a founder named it, or what
    /// the old stories call it.
    pub fn bloodline_name(&self, bloodline: Bloodline) -> &str {
        self.bloodline_names[bloodline.index()].as_deref().unwrap_or_else(|| bloodline.old_name())
    }

    /// A founder names the bloodline of their land. The first founder
    /// of that blood to do so names it for good, in every land.
    fn name_bloodline(&mut self, index: usize, raw: &str, outcomes: &mut Vec<Outcome>) {
        let checked = self.founded_by(index).and_then(|zone| {
            let bloodline = self.zones[zone].bloodline();
            if self.bloodline_names[bloodline.index()].is_some() {
                return Err(FoundRefusal::AlreadyNamed);
            }
            let taken = self.bloodline_names.iter().flatten().map(String::as_str);
            let name = eq_naming::check(raw, &content::LAND_NAME_RULES, taken, &content::BLOCKED_NAMES);
            name.map(|name| (bloodline, name)).map_err(FoundRefusal::Name)
        });
        match checked {
            Ok((bloodline, name)) => {
                self.bloodline_names[bloodline.index()] = Some(name.clone());
                outcomes.push(Outcome::BloodlineNamed { name });
            }
            Err(why) => outcomes.push(Outcome::CannotFound { why }),
        }
    }

    /// A founder names their land, or their house. Either is for good.
    fn name_founding(&mut self, index: usize, raw: &str, house: bool, outcomes: &mut Vec<Outcome>) {
        let checked = self.founded_by(index).and_then(|zone| {
            let named = if house { &self.zones[zone].house } else { &self.zones[zone].name };
            if named.is_some() {
                return Err(FoundRefusal::AlreadyNamed);
            }
            let taken = self.zones.iter().filter_map(|other| if house { other.house() } else { other.name() });
            let name = eq_naming::check(raw, &content::LAND_NAME_RULES, taken, &content::BLOCKED_NAMES);
            name.map(|name| (zone, name)).map_err(FoundRefusal::Name)
        });
        match checked {
            Ok((zone, name)) if house => {
                self.zones[zone].house = Some(name.clone());
                outcomes.push(Outcome::HouseNamed { name });
            }
            Ok((zone, name)) => {
                self.zones[zone].name = Some(name.clone());
                outcomes.push(Outcome::LandNamed { name });
            }
            Err(why) => outcomes.push(Outcome::CannotFound { why }),
        }
    }

    /// A founder raises their land's house where they stand: once, in
    /// their own land, out of what they carry.
    fn raise(&mut self, index: usize, now: Tick, outcomes: &mut Vec<Outcome>) {
        let here = self.characters[index].place();
        let checked = self.founded_by(index).and_then(|zone| {
            let land = &self.zones[zone];
            let fit = self.map.place(here).is_some_and(|place| place.safety != Safety::Haven && place.lair.is_none());
            if land.hall().is_some() {
                Err(FoundRefusal::HouseStands)
            } else if !land.contains(here) || !fit {
                Err(FoundRefusal::CannotStandHere)
            } else if self.house_cost().iter().any(|(resource, amount)| self.characters[index].carried(*resource) < *amount) {
                Err(FoundRefusal::CannotAfford)
            } else {
                Ok(zone)
            }
        });
        match checked {
            Ok(zone) => {
                let cost = self.house_cost();
                let founder = &mut self.characters[index];
                for (resource, amount) in cost {
                    founder.spend(resource, amount);
                }
                founder.busy_until = now + content::RAISE_TIME;
                self.zones[zone].hall = Some(here);
                // A house stands on ground that could have been worked:
                // whatever this place gave, it gives no more.
                if let Some(site) = self.map.place_mut(here) {
                    site.deposit = None;
                }
                outcomes.push(Outcome::HouseRaised);
            }
            Err(why) => outcomes.push(Outcome::CannotFound { why }),
        }
    }

    /// Whether `character` is under their own roof: in the house of the
    /// land they belong to, where nobody can strike or rob them.
    pub fn is_sheltered(&self, character: &Character) -> bool {
        self.zones[self.land_of(character)].hall() == Some(character.place())
    }

    /// The era the world is in.
    pub const fn era(&self) -> Era {
        self.era
    }

    /// What raising a house costs in this era.
    pub fn house_cost(&self) -> [(crate::types::Resource, u64); 2] {
        content::HOUSE_COST.map(|(resource, amount)| (resource, amount * self.era.building_percent() / 100))
    }

    /// Works out the era from how the lived-in lands are doing and
    /// whether any are at war, and tells everyone if it has changed.
    fn turn_era(&mut self, events: &mut Vec<Event>) {
        let founded = self.is_founded();
        let lands: Vec<&Zone> = self.zones.iter().filter(|zone| zone.founder().is_some() || !founded).collect();
        // A world has only a handful of lands.
        let vitality = lands.iter().map(|zone| zone.vitality()).sum::<i64>() / lands.len().max(1) as i64;
        let to = Era::of(vitality, self.pacts.count(Pact::War));
        if to != self.era {
            events.push(Event::EraChanged { from: self.era, to });
            self.era = to;
        }
    }

    /// How far the world leans, and what that is like to live in.
    pub const fn balance(&self) -> Balance {
        self.balance
    }

    /// Lets the pacts between lands push the world, and works out what
    /// the world does to every deposit this tick. Wars push it toward
    /// fury and alliances toward stillness; with as many of one as the
    /// other, or none at all, it eases back toward balance.
    fn weather(&mut self, now: Tick, events: &mut Vec<Event>) -> Weather {
        if now.get().is_multiple_of(content::BALANCE_EVERY.as_ticks()) {
            // A world has only a handful of lands, so these are small.
            let pull = self.pacts.count(Pact::War) as i64 - self.pacts.count(Pact::Alliance) as i64;
            let change = if pull == 0 { self.balance.settle() } else { self.balance.tip(pull * content::LEAN_PER_PACT) };
            events.extend(change.map(|(from, to)| Event::MoodChanged { from, to }));
        }
        if now.get().is_multiple_of(content::BALANCE_EVERY.as_ticks()) {
            self.turn_era(events);
        }
        let mood = self.balance.mood();
        let storm = mood.storm_percent() > 0 && now.get().is_multiple_of(content::STORM_EVERY.as_ticks());
        if storm {
            events.push(Event::Storm);
            self.batter(mood.storm_harm(), events);
            // The land itself is the worse for it, as it is for a death.
            for zone in &mut self.zones {
                zone.mourn(mood.storm_wear());
            }
        }
        if !mood.sustains_animals() {
            self.starve_animals(now, events);
        }
        Weather { growth_percent: mood.growth_percent(), storm_percent: if storm { mood.storm_percent() } else { 0 } }
    }

    /// A storm takes `harm` health from everyone living who is caught
    /// outside a haven and their own house. It never takes the last of it.
    fn batter(&mut self, harm: u32, events: &mut Vec<Event>) {
        for index in 0..self.characters.len() {
            let character = &self.characters[index];
            let sheltered = character.is_spirit()
                || self.is_sheltered(character)
                || self.map.place(character.place()).is_none_or(|place| place.safety == Safety::Haven);
            let damage = harm.min(character.health().saturating_sub(1));
            if sheltered || damage == 0 {
                continue;
            }
            let health = self.characters[index].hurt(damage);
            events.push(Event::Character { who: id_at(index), outcome: Outcome::Battered { damage, health } });
        }
    }

    /// In a parched world the creatures of the wild die where they are.
    /// Their lairs stay empty until the world can keep them again.
    fn starve_animals(&mut self, now: Tick, events: &mut Vec<Event>) {
        while let Some(slot) = self.creatures.iter().position(|creature| creature.being.kind == CreatureKind::Lurker) {
            let (place, being) = (self.creatures[slot].place, self.creatures[slot].being);
            events.push(Event::CreatureGone { place, being });
            self.remove_creature(slot, now);
        }
    }

    /// The land a character belongs to: the one their lives begin in.
    fn land_of(&self, character: &Character) -> usize {
        self.zone_index(character.home())
    }

    /// How the lands of two characters stand to each other.
    fn pact_between(&self, a: &Character, b: &Character) -> Option<Pact> {
        self.pacts.between(self.land_of(a), self.land_of(b))
    }

    /// How the lands with havens at `a` and `b` stand to each other.
    pub fn pact(&self, a: PlaceId, b: PlaceId) -> Option<Pact> {
        self.pacts.between(self.zone_index(a), self.zone_index(b))
    }

    /// Whether the land with its haven at `from` has made the one at
    /// `to` this offer and still waits for an answer.
    pub fn has_offered(&self, from: PlaceId, to: PlaceId, dealing: Dealing) -> bool {
        self.pacts.has_offered(self.zone_index(from), self.zone_index(to), dealing)
    }

    /// The founded land known as `name`: by its own name, or by its
    /// founder's.
    fn land_called(&self, name: &str) -> Option<usize> {
        let founder_is = |zone: &Zone| {
            zone.founder().and_then(|founder| self.character(founder)).is_some_and(|founder| founder.name().eq_ignore_ascii_case(name))
        };
        self.zones.iter().position(|zone| zone.name().is_some_and(|called| called.eq_ignore_ascii_case(name)) || founder_is(zone))
    }

    /// A founder, standing on the Meeting Ground, deals with another
    /// land. What the two lands are to each other afterwards is news
    /// for everyone who belongs to either.
    fn deal(&mut self, index: usize, with: &str, dealing: Dealing, outcomes: &mut Vec<Outcome>, events: &mut Vec<Event>) {
        let here = self.characters[index].place();
        let dealt = self
            .founded_by(index)
            .map_err(|_| PactRefusal::NotFounder)
            .and_then(|mine| if Some(here) == self.meeting_ground { Ok(mine) } else { Err(PactRefusal::NotOnMeetingGround) })
            .and_then(|mine| self.land_called(with.trim()).map(|theirs| (mine, theirs)).ok_or(PactRefusal::NoSuchLand))
            .and_then(|(mine, theirs)| self.pacts.deal(mine, theirs, dealing).map(|dealt| (mine, theirs, dealt)));
        match dealt {
            Err(why) => outcomes.push(Outcome::CannotDeal { why }),
            Ok((mine, theirs, Dealt::Offered)) => {
                outcomes.push(Outcome::Offered { land: self.zones[theirs].anchor(), dealing });
                let offer = Outcome::WasOffered { land: self.zones[mine].anchor(), dealing };
                events.extend(self.zones[theirs].founder().map(|who| Event::Character { who, outcome: offer }));
            }
            Ok((mine, theirs, dealt)) => {
                // Every pact made or broken is felt by the world at once.
                let push = match dealt {
                    Dealt::AtWar => content::LEAN_PER_DEALING,
                    Dealt::Allied | Dealt::Peace => -content::LEAN_PER_DEALING,
                    Dealt::Parted | Dealt::Offered => 0,
                };
                let change = self.balance.tip(push);
                events.extend(change.map(|(from, to)| Event::MoodChanged { from, to }));
                events.push(Event::Pact { by: self.zones[mine].anchor(), with: self.zones[theirs].anchor(), dealt });
            }
        }
    }

    /// A founder invites someone by name. Whoever joins the world under
    /// that name arrives in the founder's land.
    fn invite(&mut self, index: usize, raw: &str, outcomes: &mut Vec<Outcome>) {
        let checked = self.founded_by(index).and_then(|zone| {
            let name = eq_naming::check(raw, &content::PERSON_NAME_RULES, [], &content::BLOCKED_NAMES).map_err(FoundRefusal::Name)?;
            if self.characters.iter().any(|character| character.name().eq_ignore_ascii_case(&name)) {
                return Err(FoundRefusal::AlreadyHere);
            }
            if self.zones[zone].invited.len() >= content::MOST_INVITES {
                return Err(FoundRefusal::TooManyInvites);
            }
            Ok((zone, name))
        });
        match checked {
            Ok((zone, name)) => {
                let handle = name.to_lowercase();
                // An invitation goes to one land only: the latest to offer it.
                for other in &mut self.zones {
                    other.invited.retain(|invited| *invited != handle);
                }
                self.zones[zone].invited.push(handle);
                outcomes.push(Outcome::Invited { name });
            }
            Err(why) => outcomes.push(Outcome::CannotFound { why }),
        }
    }

    /// How many characters the world holds.
    pub fn population(&self) -> usize {
        self.characters.len()
    }

    /// The place a character is in.
    pub fn place_of(&self, who: CharacterId) -> Option<&Place> {
        self.map.place(self.character(who)?.place())
    }

    /// Everyone in `place`, in id order.
    pub fn occupants(&self, place: PlaceId) -> &[CharacterId] {
        self.occupancy.at(place)
    }

    /// The first territory's anchor. In a world of one territory this
    /// is the anchor; in a larger one, ask the zone ([`Game::zone_at`]).
    pub fn anchor(&self) -> PlaceId {
        self.zones[0].anchor()
    }

    /// The first territory's shrine: the names of those who faded.
    pub fn shrine(&self) -> &[Epitaph] {
        self.zones[0].shrine()
    }

    /// The first territory's Contract Board.
    pub fn board(&self) -> &Board {
        self.zones[0].board()
    }

    /// Where the passes out of the founders' lands all lead, if this is
    /// a world for founders.
    pub const fn meeting_ground(&self) -> Option<PlaceId> {
        self.meeting_ground
    }

    /// Every territory, oldest first.
    pub fn zones(&self) -> &[Zone] {
        &self.zones
    }

    /// The territory `place` is part of.
    pub fn zone_at(&self, place: PlaceId) -> &Zone {
        &self.zones[self.zone_index(place)]
    }

    /// The territory `who` is standing in.
    pub fn zone_of(&self, who: CharacterId) -> Option<&Zone> {
        self.character(who).map(|character| self.zone_at(character.place()))
    }

    /// Which territory `place` is part of. A place that is on no map
    /// counts as the first territory's.
    fn zone_index(&self, place: PlaceId) -> usize {
        self.zones.iter().position(|zone| zone.contains(place)).unwrap_or(0)
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

    /// The first territory.
    pub fn zone(&self) -> &Zone {
        &self.zones[0]
    }

    /// The map.
    pub const fn map(&self) -> &Graph<Place> {
        &self.map
    }

    /// The first territory's vitality, from 0 to 100.
    pub fn vitality(&self) -> i64 {
        self.zones[0].vitality()
    }

    /// The first territory's vitality band.
    pub fn health(&self) -> Health {
        self.zones[0].health()
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
        if self.is_sheltered(victim) {
            return Err(Refusal::Haven);
        }
        let safety = self.place_of(attacker).map_or(Safety::Haven, |place| place.safety);
        // War opens the wilderness: an enemy needs no mark to be struck.
        // So does a price on someone's head.
        if safety != Safety::Haven && (self.pact_between(aggressor, victim) == Some(Pact::War) || self.is_hunted(target)) {
            return Ok(());
        }
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
        if self.is_sheltered(victim) {
            return Err(RobRefusal::Haven);
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
            // The whole world feels it.
            Event::MoodChanged { .. } | Event::EraChanged { .. } | Event::Storm => self.characters().map(|(id, _)| id).collect(),
            // Everyone who belongs to either land hears of it.
            Event::Pact { by, with, .. } => {
                let lands = [self.zone_index(*by), self.zone_index(*with)];
                self.characters().filter(|(_, character)| lands.contains(&self.land_of(character))).map(|(id, _)| id).collect()
            }
            // Everyone standing in that territory feels it.
            Event::VitalityChanged { anchor, .. } => {
                let zone = self.zone_at(*anchor);
                self.characters().filter(|(_, character)| zone.contains(character.place())).map(|(id, _)| id).collect()
            }
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
            | Outcome::Buried { .. }
            | Outcome::LandNamed { .. }
            | Outcome::HouseNamed { .. }
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
            Action::Spar => return outcomes.push(Outcome::NoShade),
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
            Action::Fight(creature) => self.fight(index, *creature, now, outcomes),
            Action::Take => self.take(index, now, outcomes),
            Action::Rest => self.rest(index, now, outcomes),
            Action::Rob(target) => self.rob(index, *target, now, outcomes, events),
            Action::Name(target) => self.name(index, *target, now, outcomes, events),
            Action::NameLand(name) => self.name_founding(index, name, false, outcomes),
            Action::NameHouse(name) => self.name_founding(index, name, true, outcomes),
            Action::Invite(name) => self.invite(index, name, outcomes),
            Action::NameBloodline(name) => self.name_bloodline(index, name, outcomes),
            Action::Raise => self.raise(index, now, outcomes),
            Action::Dig => self.dig(index, now, outcomes, events),
            Action::Hire { reward, to } => self.hire(index, *reward, to, now, outcomes),
            Action::Guard(client) => self.guard(index, *client, outcomes, events),
            Action::Dismiss => self.dismiss(index, outcomes, events),
            Action::Bounty { on, reward } => self.bounty(index, *on, *reward, now, outcomes, events),
            Action::Deal { with, dealing } => self.deal(index, with, *dealing, outcomes, events),
            Action::Bury(target) => self.bury(index, *target, now, outcomes, events),
            Action::Rite => self.rite(index, now, outcomes, events),
            Action::Tend(target) => self.tend(index, *target, now, outcomes),
            Action::Post { give, want } => self.post(index, *give, *want, now, outcomes),
            Action::Accept(id) => self.accept(index, *id, now, outcomes, events),
            Action::Withdraw(id) => self.withdraw(index, *id, now, outcomes),
            Action::Collect => self.collect(index, now, outcomes),
            Action::Wake | Action::Return | Action::Relinquish | Action::Seek | Action::Spar | Action::Join(_) => {}
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
            Action::Seek => self.seek(index, now, outcomes),
            Action::Spar => self.spar(index, now, outcomes),
            Action::Return => outcomes.push(Outcome::CannotReturn { why: ReturnRefusal::NoWayBack }),
            // There is no giving up and starting again. The way back is the afterlife.
            Action::Relinquish => outcomes.push(Outcome::CannotReturn { why: ReturnRefusal::Unredeemed }),
            _ => outcomes.push(Outcome::Bodiless),
        }
    }

    /// A forgotten spirit lies dormant until it is woken. Awake, it can
    /// walk, search for memories of itself, or go dormant again. It has
    /// no body to return to or give up.
    fn act_as_forgotten(&mut self, index: usize, action: &Action, now: Tick, outcomes: &mut Vec<Outcome>) {
        let character = &mut self.characters[index];
        if character.is_dormant() {
            if *action != Action::Wake {
                return outcomes.push(Outcome::Asleep);
            }
            let forgotten = character.forgotten_mut().expect("a dormant spirit is forgotten");
            forgotten.dormant = false;
            // Waking for the first time, it comes ashore where every soul does.
            let first_waking = forgotten.played == Duration::ZERO;
            character.busy_until = now;
            outcomes.push(Outcome::Stirred);
            if first_waking && self.carry_to_river(index) {
                outcomes.push(Outcome::CameToRiver);
            }
            return;
        }
        let character = &mut self.characters[index];
        let busy_until = character.busy_until();
        if now < busy_until {
            outcomes.push(Outcome::Busy { until: busy_until });
            return;
        }
        match action {
            Action::Go(label) => self.go(index, label, now, outcomes),
            Action::Gather => self.draw(index, now, outcomes),
            Action::Seek => self.seek(index, now, outcomes),
            Action::Spar => self.spar(index, now, outcomes),
            Action::Rest => {
                character.forgotten_mut().expect("checked above").dormant = true;
                outcomes.push(Outcome::WentDormant);
            }
            Action::Wake => outcomes.push(Outcome::NotAsleep),
            Action::Return => outcomes.push(Outcome::CannotReturn { why: ReturnRefusal::NoWayBack }),
            Action::Relinquish => outcomes.push(Outcome::CannotReturn { why: ReturnRefusal::Unredeemed }),
            _ => outcomes.push(Outcome::Bodiless),
        }
    }

    /// A spirit draws ectoplasm from any place the living can work.
    fn draw(&mut self, index: usize, now: Tick, outcomes: &mut Vec<Outcome>) {
        let character = &mut self.characters[index];
        let Some(deposit) = self.map.place(character.place()).and_then(|place| place.deposit) else {
            return outcomes.push(Outcome::NothingToGather);
        };
        let mut risen = Vec::new();
        let amount = character.draw(&mut risen);
        character.busy_until = now + DRAW_TIME;
        outcomes.push(Outcome::Drew { amount, held: character.ectoplasm() });
        outcomes.append(&mut risen);
        // The work is practice too, and enough practice is the way back.
        if character.practise(deposit.resource.skill(), AFTERLIFE_XP_PER_DRAW, outcomes) >= character.afterlife_goal() {
            self.arise(index, now, outcomes);
        }
    }

    /// A spirit strikes a shade. Shades drift wherever the living do
    /// not work; they do not strike back, and there is always another.
    /// It is fighting learned from nothing, with nothing at risk.
    fn spar(&mut self, index: usize, now: Tick, outcomes: &mut Vec<Outcome>) {
        let character = &self.characters[index];
        if self.map.place(character.place()).is_none_or(|place| place.deposit.is_some()) {
            return outcomes.push(Outcome::NoShade);
        }
        // DAMAGE_SPREAD is tiny, so the cast cannot truncate.
        let roll = self.rng.below(DAMAGE_SPREAD) as u32;
        // A spirit has only what it has practised here to strike with.
        let fighting = character.practised(SkillKind::Fighting).level();
        let blow = Blow { roll, vigor: 0, fighting, taint: 0, resilience: 0 };
        let character = &mut self.characters[index];
        character.busy_until = now + ATTACK_TIME;
        let (damage, left) = (blow.damage(), character.hurt_shade(blow.damage()));
        outcomes.push(Outcome::Sparred { damage, left });
        if character.practise(SkillKind::Fighting, FIGHTING_XP_PER_BLOW, outcomes) >= character.afterlife_goal() {
            self.arise(index, now, outcomes);
        }
    }

    /// Ends a spirit's time in the afterlife: it lives again, wherever
    /// the world is emptiest.
    fn arise(&mut self, index: usize, now: Tick, outcomes: &mut Vec<Outcome>) {
        let was_forgotten = self.characters[index].forgotten().is_some();
        let life = self.begin_life(index, now, REBIRTH_CARRYOVER_PERCENT, true);
        outcomes.push(if was_forgotten { Outcome::Redeemed { by: None, life } } else { Outcome::Reborn { life } });
    }

    /// Searches a place for a memory. Each place with a deposit holds
    /// one for each forgotten spirit; enough of them redeem it.
    fn seek(&mut self, index: usize, now: Tick, outcomes: &mut Vec<Outcome>) {
        let character = &mut self.characters[index];
        let here = character.place();
        let holds_one = self.map.place(here).is_some_and(|place| place.deposit.is_some());
        // Only those who lived and were forgotten have memories to find.
        let Some(forgotten) = character.forgotten_mut().filter(|forgotten| !forgotten.unborn) else {
            return outcomes.push(Outcome::NothingToRemember);
        };
        if !holds_one || forgotten.found_in.contains(&here) {
            return outcomes.push(Outcome::NothingToRemember);
        }
        forgotten.found_in.push(here);
        let found = forgotten.memories();
        character.busy_until = now + SEEK_TIME;
        outcomes.push(Outcome::Remembered { found, needed: MEMORIES_TO_REDEEM });
        if found < MEMORIES_TO_REDEEM {
            return;
        }
        self.arise(index, now, outcomes);
    }

    /// Takes part in a Restoration Rite. The first to do so begins it;
    /// a second, in time, completes it: the deposit is mended, a ward
    /// is laid on it, and some of the zone's corruption lifts.
    fn rite(&mut self, index: usize, now: Tick, outcomes: &mut Vec<Outcome>, events: &mut Vec<Event>) {
        let who = id_at(index);
        let character = &mut self.characters[index];
        let place = character.place();
        let marked = character.standing() == Standing::Marked;
        let refusal = match self.map.place(place).and_then(|place| place.deposit) {
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
        let Some(deposit) = self.map.place_mut(place).and_then(|place| place.deposit.as_mut()) else { return };
        let Some(beginner) = deposit.rite_open(now).filter(|beginner| *beginner != who) else {
            deposit.rite_begun = Some((who, now));
            return outcomes.push(Outcome::RiteBegun);
        };
        let before = deposit.node.quality();
        // Two bloodlines joined in a rite mend what one alone cannot.
        // (Ids are positions in the list of characters.)
        let blood = |who: CharacterId| self.characters.get(who.get() as usize).map(Character::bloodline);
        let mixed = blood(who) != blood(beginner);
        deposit.node.mend(if mixed { content::MIXED_RITE_MENDS_PERCENT } else { RITE_MENDS_PERCENT });
        deposit.warded_until = now + WARD_TIME;
        deposit.rite_begun = None;
        let (resource, after) = (deposit.resource, deposit.node.quality());
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

    /// The territory the character at `index` stands in.
    fn zone_of_index(&self, index: usize) -> &Zone {
        self.zone_at(self.characters[index].place())
    }

    fn zone_of_index_mut(&mut self, index: usize) -> &mut Zone {
        let zone = self.zone_index(self.characters[index].place());
        &mut self.zones[zone]
    }

    /// Whether the character at `index` is standing at the board.
    fn at_the_board(&self, index: usize) -> Result<(), TradeRefusal> {
        let here = self.characters[index].place();
        if here == self.zone_at(here).anchor() {
            Ok(())
        } else {
            Err(TradeRefusal::NotAtBoard)
        }
    }

    /// Pins up an offer. What is offered leaves the pack at once.
    fn post(&mut self, index: usize, give: Lot, want: Lot, now: Tick, outcomes: &mut Vec<Outcome>) {
        let who = id_at(index);
        let allowed = self.at_the_board(index).and_then(|()| self.zone_of_index(index).board.may_post(who, give, want));
        if let Err(why) = allowed {
            return outcomes.push(Outcome::CannotTrade { why });
        }
        let poster = &mut self.characters[index];
        if !poster.spend(give.resource, give.amount) {
            return outcomes.push(Outcome::CannotTrade { why: TradeRefusal::CannotPay });
        }
        poster.busy_until = now + BOARD_TIME;
        let id = self.zone_of_index_mut(index).board.post(who, give, want);
        outcomes.push(Outcome::Posted { id, give, want });
    }

    /// Every escort wanted or under way.
    pub fn escorts(&self) -> &[Escort] {
        self.escorts.all()
    }

    /// The place called `name`: "ground" for the Meeting Ground, or the
    /// haven of the land of that name or founder.
    fn place_called(&self, name: &str) -> Option<PlaceId> {
        match name.trim() {
            "ground" | "meeting ground" | "the meeting ground" => self.meeting_ground,
            land => self.land_called(land).map(|zone| self.zones[zone].anchor()),
        }
    }

    /// Puts up a reward at the board here for whoever sees this
    /// character safely to a place. The board holds the reward.
    fn hire(&mut self, index: usize, reward: Lot, to: &str, now: Tick, outcomes: &mut Vec<Outcome>) {
        let who = id_at(index);
        let here = self.characters[index].place();
        let checked = self.at_the_board(index).and_then(|()| match self.place_called(to) {
            _ if reward.amount == 0 => Err(TradeRefusal::Empty),
            _ if self.escorts.of(who).is_some() => Err(TradeRefusal::AlreadyHiring),
            None => Err(TradeRefusal::NoSuchPlace),
            Some(to) if to == here => Err(TradeRefusal::NoSuchPlace),
            Some(to) => Ok(to),
        });
        let to = match checked {
            Ok(to) => to,
            Err(why) => return outcomes.push(Outcome::CannotTrade { why }),
        };
        let client = &mut self.characters[index];
        if !client.spend(reward.resource, reward.amount) {
            return outcomes.push(Outcome::CannotTrade { why: TradeRefusal::CannotPay });
        }
        client.busy_until = now + BOARD_TIME;
        self.escorts.open(Escort { client: who, guard: None, reward, to, posted_at: here });
        outcomes.push(Outcome::Hiring { reward, to });
    }

    /// Takes the escort job of someone standing in the same place.
    fn guard(&mut self, index: usize, client: CharacterId, outcomes: &mut Vec<Outcome>, events: &mut Vec<Event>) {
        let who = id_at(index);
        let together = self.character(client).is_some_and(|other| other.place() == self.characters[index].place());
        if !together || !self.escorts.take(client, who) {
            return outcomes.push(Outcome::CannotTrade { why: TradeRefusal::NotHiring });
        }
        let Some(job) = self.escorts.of(client).copied() else { return };
        outcomes.push(Outcome::Guarding { client, reward: job.reward, to: job.to });
        events.push(Event::Character { who: client, outcome: Outcome::Guarded { by: who } });
    }

    /// Gives up on an escort. The reward goes back to the board it was
    /// put up on.
    fn dismiss(&mut self, index: usize, outcomes: &mut Vec<Outcome>, events: &mut Vec<Event>) {
        match self.escorts.close(id_at(index)) {
            Some(job) => {
                self.refund(job);
                outcomes.push(Outcome::EscortOver { paid: false });
                let lost = Outcome::EscortDone { client: job.client, reward: None };
                events.extend(job.guard.map(|who| Event::Character { who, outcome: lost }));
            }
            None => outcomes.push(Outcome::CannotTrade { why: TradeRefusal::NotHiring }),
        }
    }

    /// Sends an escort's reward back to the board it was put up on, for
    /// the client to collect.
    fn refund(&mut self, job: Escort) {
        let zone = self.zone_index(job.posted_at);
        self.zones[zone].board.hold_for(job.client, job.reward);
    }

    /// Settles every escort that has come to an end this tick: paid if
    /// client and guard stand together where the client was going;
    /// refunded if the client is dead; open again if the guard is.
    fn settle_escorts(&mut self, events: &mut Vec<Event>) {
        let living = |who: CharacterId| self.character(who).filter(|character| !character.is_spirit());
        let mut over = Vec::new();
        let mut fallen = Vec::new();
        for job in self.escorts.all() {
            let Some(client) = living(job.client) else {
                over.push((*job, false));
                continue;
            };
            match job.guard.map(|guard| (guard, living(guard))) {
                Some((guard, None)) => fallen.push(guard),
                Some((_, Some(guard))) if client.place() == job.to && guard.place() == job.to => over.push((*job, true)),
                _ => {}
            }
        }
        for guard in fallen {
            for client in self.escorts.release(guard) {
                events.push(Event::Character { who: client, outcome: Outcome::GuardLost });
            }
        }
        for (job, paid) in over {
            self.escorts.close(job.client);
            match (paid, job.guard.and_then(|guard| self.index_of(guard))) {
                (true, Some(guard)) => self.characters[guard].stow(job.reward.resource, job.reward.amount),
                _ => self.refund(job),
            }
            events.push(Event::Character { who: job.client, outcome: Outcome::EscortOver { paid } });
            let done = Outcome::EscortDone { client: job.client, reward: paid.then_some(job.reward) };
            events.extend(job.guard.map(|who| Event::Character { who, outcome: done }));
        }
    }

    /// Puts a price on someone's head at the board here. It changes
    /// nothing about who may be attacked where: it makes them worth
    /// the trouble.
    fn bounty(&mut self, index: usize, on: CharacterId, reward: Lot, now: Tick, outcomes: &mut Vec<Outcome>, events: &mut Vec<Event>) {
        let who = id_at(index);
        let allowed = self.at_the_board(index).and_then(|()| match self.index_of(on) {
            None => Err(TradeRefusal::NoSuchContract),
            Some(_) if on == who => Err(TradeRefusal::OnYourself),
            Some(_) if reward.amount == 0 => Err(TradeRefusal::Empty),
            // A newcomer's head costs more than it is worth.
            Some(target) => {
                let least =
                    if self.characters[target].is_newcomer() { content::LEAST_BOUNTY_ON_NEWCOMER } else { content::LEAST_BOUNTY };
                if reward.amount < least {
                    Err(TradeRefusal::TooLittle(least))
                } else {
                    Ok(())
                }
            }
        });
        if let Err(why) = allowed {
            return outcomes.push(Outcome::CannotTrade { why });
        }
        let poster = &mut self.characters[index];
        if !poster.spend(reward.resource, reward.amount) {
            return outcomes.push(Outcome::CannotTrade { why: TradeRefusal::CannotPay });
        }
        poster.busy_until = now + BOARD_TIME;
        self.zone_of_index_mut(index).board.post_bounty(Bounty { on, by: who, reward });
        outcomes.push(Outcome::BountyPosted { on, reward });
        events.push(Event::Character { who: on, outcome: Outcome::Hunted { reward } });
    }

    /// Whether there is a price on `who`'s head at any board.
    pub fn is_hunted(&self, who: CharacterId) -> bool {
        self.zones.iter().any(|zone| zone.board().bounties().iter().any(|bounty| bounty.on == who))
    }

    /// Pays whoever killed `victim` every bounty on their head, at
    /// whichever boards hold them. Taking a price for a newcomer the
    /// land had no quarrel with (`innocent`) marks the one who took it.
    fn pay_bounties(&mut self, victim: CharacterId, by: Cause, innocent: bool, events: &mut Vec<Event>) {
        let Cause::Character(killer) = by else { return };
        let mut paid = false;
        for zone in &mut self.zones {
            let at = zone.anchor();
            for reward in zone.board.pay_bounties(victim, killer) {
                paid = true;
                events.push(Event::Character { who: killer, outcome: Outcome::BountyEarned { on: victim, reward, at } });
            }
        }
        // Blood money stains whoever takes it, and stains them through
        // if the land had no quarrel with the dead.
        if let (true, Some(hunter)) = (paid, self.index_of(killer)) {
            let stain = if innocent { TAINT_FOR_ROBBING } else { content::TAINT_FOR_BLOOD_MONEY };
            let marked = self.characters[hunter].shift_taint(stain);
            events.extend(marked.map(|outcome| Event::Character { who: killer, outcome }));
        }
    }

    /// Pays a contract's price and takes what it held. The price waits
    /// at the board for whoever posted it.
    fn accept(&mut self, index: usize, id: ContractId, now: Tick, outcomes: &mut Vec<Outcome>, events: &mut Vec<Event>) {
        let who = id_at(index);
        let found = self.at_the_board(index).and_then(|()| match self.zone_of_index(index).board.contract(id) {
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
        let board = &mut self.zone_of_index_mut(index).board;
        board.take_down(id);
        board.hold_for(contract.by, contract.want);
        outcomes.push(Outcome::Accepted { id, paid: contract.want, got: contract.give });
        events.push(Event::Character { who: contract.by, outcome: Outcome::Filled { id, by: who, earned: contract.want } });
    }

    /// Takes down a contract of one's own and has back what it held.
    fn withdraw(&mut self, index: usize, id: ContractId, now: Tick, outcomes: &mut Vec<Outcome>) {
        let who = id_at(index);
        let found = self.at_the_board(index).and_then(|()| match self.zone_of_index(index).board.contract(id) {
            None => Err(TradeRefusal::NoSuchContract),
            Some(contract) if contract.by != who => Err(TradeRefusal::NotYours),
            Some(contract) => Ok(*contract),
        });
        match found {
            Ok(contract) => {
                self.zone_of_index_mut(index).board.take_down(id);
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
        let spoils = self.zone_of_index_mut(index).board.collect(id_at(index));
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
        let here = self.characters[index].place();
        let refusal = if here != self.zone_at(here).anchor() {
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

    /// Starts a spirit's next life. It arises at the anchor of whichever
    /// land has the fewest people, which becomes its home: nobody
    /// chooses where they are born. It keeps `base` percent of what it
    /// knew, more with Lineage and, if it may spend it, ectoplasm.
    /// Returns which life this is.
    fn begin_life(&mut self, index: usize, now: Tick, base: u32, spend_ectoplasm: bool) -> u32 {
        let who = id_at(index);
        // Whoever died where something can be worked remembers that work best.
        let favoured = self.characters[index]
            .death()
            .and_then(|death| self.map.place(death.body))
            .and_then(|place| place.deposit)
            .map(|deposit| deposit.resource.attributes()[0]);
        let land = &self.zones[self.emptiest_land(Some(who))];
        let (home, bloodline) = (land.anchor(), land.bloodline());
        let character = &mut self.characters[index];
        self.occupancy.leave(character.place(), who);
        self.occupancy.enter(home, who);
        character.be_reborn(home, now, Rebirth { base, favoured, spend_ectoplasm, bloodline });
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
        self.characters[index].learn_from_blow(self.era.fighting_xp_percent(), outcomes);
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
            fighting: self.characters[striker].skill(SkillKind::Fighting).level(),
            taint: self.characters[striker].taint(),
            resilience: self.characters[struck].attribute(Attribute::Resilience).level(),
        };
        let damage = blow.damage();
        (damage, self.characters[struck].hurt(damage))
    }

    /// Kills a character. The body stays where it fell with everything
    /// they carried beside it, and the spirit goes on to the afterlife.
    /// The zone pays `corruption` for the death.
    fn fell(&mut self, index: usize, by: Cause, corruption: i64, now: Tick, events: &mut Vec<Event>) {
        let who = id_at(index);
        let body = self.characters[index].place();
        let passing = self.passing(who, by, body);
        let lost_at = now + CORPSE_TIME;

        let character = &mut self.characters[index];
        let innocent = character.is_newcomer() && character.standing() == Standing::Clean;
        let (dropped, cleansed) = character.die(Death { body, by, passing, lost_at, risen: false, buried: false, disturbed: false });
        self.relics.extend(Relic::new(who, body, dropped, now));
        // The spirit does not stay. It goes where every soul goes, the
        // River of Souls, or in a world without one to the haven of the
        // land it died in. There is no way back into the body.
        let zone = self.zones.iter().position(|zone| zone.contains(body)).unwrap_or(0);
        let onward = self.river.unwrap_or_else(|| self.zones[zone].anchor());
        character.place = onward;
        self.occupancy.leave(body, who);
        self.occupancy.enter(onward, who);
        self.zones[zone].mourn(corruption);
        // A land at war feels every one of its dead, wherever they fall,
        // and so does every land it is at war with.
        let home = self.land_of(&self.characters[index]);
        let at_war: Vec<usize> = self.pacts.with(home, Pact::War).collect();
        if !at_war.is_empty() {
            // The land the body fell in has mourned already.
            for land in at_war.into_iter().chain([home]).filter(|&land| land != zone) {
                self.zones[land].mourn(corruption);
            }
        }
        self.dead.push(who);
        self.pay_bounties(who, by, innocent, events);
        events.push(Event::Character { who, outcome: Outcome::Died { by, place: body, passing, lost_at } });
        events.extend(cleansed.map(|outcome| Event::Character { who, outcome }));
    }

    /// How a spirit will stand to its body: corrupted in a collapsed
    /// zone, tethered if anyone but the killer stood by, severed if not.
    fn passing(&self, who: CharacterId, by: Cause, place: PlaceId) -> Passing {
        if self.zone_at(place).health() == Health::Collapsed {
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
        // A body whose things were taken by someone else has been disturbed.
        for owner in open.iter().map(|relic| relic.owner).filter(|owner| *owner != who) {
            if let Some(owner) = self.index_of(owner) {
                self.characters[owner].be_disturbed();
            }
        }
        let character = &mut self.characters[index];
        character.stow_all(spoils);
        character.busy_until = now + time;
        outcomes.push(Outcome::Took { spoils });
    }

    /// Strikes a creature. It will come for whoever struck it.
    fn fight(&mut self, index: usize, target: CreatureId, now: Tick, outcomes: &mut Vec<Outcome>) {
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
            fighting: striker.skill(SkillKind::Fighting).level(),
            taint: striker.taint(),
            resilience: creature.resilience,
        };
        striker.learn_from_blow(self.era.fighting_xp_percent(), outcomes);
        let (damage, left) = creature.hurt(blow.damage());
        creature.provoke(who);
        let being = creature.being;
        outcomes.push(Outcome::HitCreature { being, damage, left });
        if left == 0 {
            outcomes.push(Outcome::Slew { being });
            self.remove_creature(slot, now);
        }
    }

    fn new_creature_id(&mut self) -> CreatureId {
        let id = CreatureId::new(self.next_creature);
        self.next_creature += 1;
        id
    }

    /// Takes a creature out of the world. A lurker's lair fills again
    /// in time.
    fn remove_creature(&mut self, slot: usize, now: Tick) {
        let creature = self.creatures.remove(slot);
        match creature.being {
            // Whatever makes its home in a place comes back to it in time.
            Being { kind: CreatureKind::Lurker | CreatureKind::Sentinel, .. } => {
                // While the Void consumes the world, nothing stays down.
                let rest = if self.era.lairs_never_rest() { Duration::ZERO } else { LURKER_RETURNS };
                self.empty_lairs.push((now + rest, creature.place));
            }
            // The spirit whose body it was is no nearer the living for
            // its fall: that way is through the afterlife.
            Being { kind: CreatureKind::BodyGuard, .. } => {}
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
                // A roused world rouses what lives in it: the extra is
                // counted the way skill at arms is, as a share of the blow.
                fighting: (self.balance.mood().ferocity_percent() - 100) * content::FIGHTING_PER_PERCENT_DAMAGE as u32,
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
        // A sentinel lets its land's allies by.
        let land = self.zone_index(creature.place);
        let ally = |who: &CharacterId| {
            creature.being.kind == CreatureKind::Sentinel
                && self.character(*who).is_some_and(|character| self.pacts.between(land, self.land_of(character)) == Some(Pact::Alliance))
        };
        let anyone = || here.iter().find(|&who| creature.being.kind.is_aggressive() && living(who) && !ally(who));
        foe.or_else(anyone).and_then(|&who| self.index_of(who))
    }

    /// Reports every tidal exit that opened or closed this tick.
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
            self.remove_creature(slot, now);
        }
        // Nothing comes back to a lair in a world that cannot keep it;
        // what bars a pass is stone, and comes back regardless.
        let alive = self.balance.mood().sustains_animals();
        let lair = |place: PlaceId| self.map.place(place).and_then(|data| data.lair).unwrap_or(CreatureKind::Lurker);
        let returns = |&(at, place): &(Tick, PlaceId)| at <= now && (alive || lair(place) != CreatureKind::Lurker);
        let due: Vec<PlaceId> = self.empty_lairs.iter().filter(|entry| returns(entry)).map(|&(_, place)| place).collect();
        self.empty_lairs.retain(|entry| !returns(entry));
        for place in due {
            let id = self.new_creature_id();
            let kind = self.map.place(place).and_then(|data| data.lair).unwrap_or(CreatureKind::Lurker);
            let native = Creature::native(kind, id, place, now);
            events.push(Event::CreatureAppeared { place, being: native.being });
            self.creatures.push(native);
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
        let map = &self.map;
        for character in self.characters.iter_mut().filter(|character| !character.is_spirit()) {
            let in_haven = map.place(character.place()).is_some_and(|place| place.safety == Safety::Haven);
            let mending_well = in_haven || character.is_asleep(now);
            character.mend(if mending_well { HAVEN_MENDING } else { MENDING });
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
            let zone = self.zone_index(place);
            self.zones[zone].mourn(CORRUPTION_PER_SLEEPER_DEATH);
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
            // No clock runs on the dormant, nor on those who have never lived.
            if forgotten.dormant || forgotten.unborn {
                continue;
            }
            forgotten.played = forgotten.played + Duration::ticks(1);
            if forgotten.played >= REDEMPTION_CLOCK {
                faded.push(index);
            }
        }
        for index in faded {
            let character = &self.characters[index];
            let epitaph = Epitaph { name: character.name().into(), life: character.life(), at: now };
            let zone = self.zone_index(character.home());
            self.zones[zone].shrine.push(epitaph);
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
        let Some(exit) = self.map.exit(from, label) else {
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
        let health = self.zone_of_index(index).health();
        let character = &mut self.characters[index];
        let place = character.place();
        let Some(deposit) = self.map.place_mut(place).and_then(|place| place.deposit.as_mut()) else {
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
        let weather = self.weather(now, events);
        for zone in &mut self.zones {
            zone.tick(&mut self.map, now, weather, events);
        }
        self.report_passages(now, events);
        self.fade_taint(now, events);
        self.tire(now);
        self.mend(now);
        self.end_long_sleeps(now, events);
        self.run_redemption_clocks(now, events);
        self.lose_bodies(now, events);
        self.creatures_strike(now, events);
        self.upkeep(now, events);
        self.settle_escorts(events);
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
    use crate::balance::Mood;
    use crate::content::AFTERLIFE_LEVEL;

    /// What the afterlife asks of someone who has died once.
    const AFTER_ONE_DEATH: u32 = AFTERLIFE_LEVEL + crate::content::AFTERLIFE_LEVEL_PER_DEATH;
    use crate::era::Era;
    use crate::content::{GRAVE_SHIFT, BALANCE_EVERY, LEAN_PER_DEALING, LEAST_BOUNTY, LEAST_BOUNTY_ON_NEWCOMER, RITE_WINDOW, STORM_EVERY};
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
    fn the_dead_are_carried_off_and_there_is_no_way_back_into_the_body() {
        let (mut game, now) = bryn_marked_on_the_ledge();
        let (events, now) = fight_to_the_end(&mut game, now);
        assert!(died(&events, BRYN).is_some());
        // The spirit does not stay by the body, whoever was standing by.
        assert_eq!(game.character(BRYN).unwrap().place(), CAMP);
        assert_eq!(game.occupants(LEDGE), [ASH]);
        assert_eq!(game.bodies_at(LEDGE).count(), 1, "the body lies where it fell");

        // Walking back to it changes nothing: that life is over.
        let mut events = Vec::new();
        game.apply(&Command::return_to_body(BRYN), Tick::new(now), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::CannotReturn { why: ReturnRefusal::NoWayBack }));
        game.apply(&Command::go(BRYN, "north"), Tick::new(now), &mut events);
        game.apply(&Command::go(ASH, "north"), Tick::new(now), &mut events);
        game.apply(&Command::return_to_body(BRYN), Tick::new(now + TRAVEL_TIME.as_ticks()), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::CannotReturn { why: ReturnRefusal::NoWayBack }));
        assert!(game.character(BRYN).unwrap().is_spirit());
        // The living are told the same, in their own way.
        game.apply(&Command::return_to_body(ASH), Tick::new(now + 60), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::NotDead));
    }

    #[test]
    fn a_body_left_too_long_rises_as_a_guard_and_the_spirit_must_go_the_long_way() {
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

        // There is no giving the body up for a quick new life either:
        // the only way on is through the afterlife. Bryn goes up to the
        // ledge to practise the work there.
        let mut events = Vec::new();
        game.apply(&Command::go(BRYN, "north"), Tick::new(lost_at), &mut events);
        game.apply(&Command::relinquish(BRYN), Tick::new(lost_at + 30), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::CannotReturn { why: ReturnRefusal::Unredeemed }));
        let (events, _) = through_the_afterlife(&mut game, BRYN, lost_at + 31);
        assert!(events.contains(&Event::Character { who: BRYN, outcome: Outcome::Reborn { life: 2 } }));
        let bryn = game.character(BRYN).unwrap();
        assert!(!bryn.is_spirit());
        assert_eq!((bryn.place(), bryn.health(), bryn.rested(), bryn.life()), (CAMP, 100, 0, 2), "whole, but weary");
        // Death kept nothing. The ledge is where Bryn practised, so mining is what came back.
        let mining = bryn.skill(crate::types::SkillKind::Mining);
        assert_eq!((mining.level(), mining.peak()), (AFTER_ONE_DEATH, AFTER_ONE_DEATH.max(mining_before)));
        assert_eq!(game.bodies_at(LEDGE).count(), 0);
    }

    #[test]
    fn nobody_can_simply_give_a_body_up() {
        let (mut game, now) = bryn_marked_on_the_ledge();
        let (_, now) = fight_to_the_end(&mut game, now);
        let mut events = Vec::new();
        game.apply(&Command::relinquish(ASH), Tick::new(now), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::NotDead));
        game.apply(&Command::relinquish(BRYN), Tick::new(now), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::CannotReturn { why: ReturnRefusal::Unredeemed }));
        assert!(game.character(BRYN).unwrap().is_spirit());
    }

    #[test]
    fn a_buried_body_never_rises_and_its_owner_relearns_the_old_life_fast() {
        let (mut game, now) = bryn_marked_on_the_ledge();
        let mining = game.character(BRYN).unwrap().skill(SkillKind::Mining).level();
        let (_, now) = fight_to_the_end(&mut game, now);
        assert_eq!((game.bodies_at(LEDGE).count(), game.relics_at(LEDGE).count()), (1, 1));

        // Ash could take what Bryn dropped. Ash buries Bryn instead.
        let mut events = Vec::new();
        game.apply(&Command::bury(ASH, ASH), Tick::new(now), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::CannotBury { target: ASH }));
        events.clear();
        game.apply(&Command::bury(ASH, BRYN), Tick::new(now), &mut events);
        assert_eq!(events, [
            Event::Character { who: ASH, outcome: Outcome::Buried { target: BRYN } },
            Event::Character { who: BRYN, outcome: Outcome::WasBuried { by: ASH } },
        ]);
        assert_eq!(game.character(ASH).unwrap().busy_until(), Tick::new(now) + BURY_TIME);
        assert_eq!((game.bodies_at(LEDGE).count(), game.relics_at(LEDGE).count()), (0, 0), "all of it is in the ground");
        game.apply(&Command::bury(ASH, BRYN), Tick::new(now + 60), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::CannotBury { target: BRYN }), "once is enough");

        // No guard rises from a grave, and there is no climbing out of one.
        let later = ticks(&mut game, now + 61, now + CORPSE_TIME.as_ticks() * 2);
        assert!(!later.iter().any(|event| matches!(event, Event::CreatureAppeared { .. })));
        let mut events = Vec::new();
        let at = now + CORPSE_TIME.as_ticks() * 2;
        game.apply(&Command::go(BRYN, "north"), Tick::new(at), &mut events);

        // Bryn practises foraging on the beach. Mining starts from nothing
        // in the next life, but that life remembers how high it stood.
        let at = to_the_beach(&mut game, BRYN, at + 31);
        through_the_afterlife(&mut game, BRYN, at);
        let bryn = game.character(BRYN).unwrap();
        let (mining_now, foraging) = (bryn.skill(SkillKind::Mining), bryn.skill(SkillKind::Foraging));
        assert_eq!((mining_now.level(), mining_now.peak()), (0, mining));
        assert_eq!((foraging.level(), foraging.peak()), (AFTER_ONE_DEATH, AFTER_ONE_DEATH));
    }

    /// Walks a spirit on the Ore Ledge down to the Shingle Beach.
    fn to_the_beach(game: &mut Game, who: CharacterId, from: u64) -> u64 {
        let mut events = Vec::new();
        let mut now = from;
        for way in ["south", "east"] {
            game.apply(&Command::go(who, way), Tick::new(now), &mut events);
            now = game.character(who).unwrap().busy_until().get();
        }
        assert_eq!(game.place_of(who).unwrap().name, "Shingle Beach");
        now
    }

    #[test]
    fn a_price_on_someones_head_is_paid_to_whoever_kills_them() {
        let (mut game, now) = bryn_marked_on_the_ledge();
        let ore = game.character(BRYN).unwrap().carried(Resource::Ore);
        let reward = Lot { resource: Resource::Ore, amount: 20 };
        assert!(ore >= 30, "Bryn should have something to post with: {ore}");
        // Ash is no newcomer, so an ordinary price will do.
        while game.character(ASH).unwrap().is_newcomer() {
            game.characters[0].train(Resource::Ore, 12, &mut Vec::new());
        }
        let mut events = Vec::new();

        // Not out here: bounties go up at the board. So Bryn walks home,
        // and puts a price on the head of the one who is about to do the killing.
        game.apply(&Command::bounty(BRYN, ASH, reward), Tick::new(now), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::CannotTrade { why: TradeRefusal::NotAtBoard }));
        game.apply(&Command::go(BRYN, "south"), Tick::new(now), &mut events);
        let at_camp = now + TRAVEL_TIME.as_ticks();
        game.apply(&Command::bounty(BRYN, BRYN, reward), Tick::new(at_camp), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::CannotTrade { why: TradeRefusal::OnYourself }));
        game.apply(&Command::bounty(BRYN, ASH, Lot { resource: Resource::Tideglass, amount: 50 }), Tick::new(at_camp), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::CannotTrade { why: TradeRefusal::CannotPay }));
        events.clear();
        game.apply(&Command::bounty(BRYN, ASH, reward), Tick::new(at_camp), &mut events);
        assert_eq!(events, [
            Event::Character { who: BRYN, outcome: Outcome::BountyPosted { on: ASH, reward } },
            Event::Character { who: ASH, outcome: Outcome::Hunted { reward } },
        ]);
        assert_eq!(game.character(BRYN).unwrap().carried(Resource::Ore), ore - 20, "the board holds it");
        assert_eq!(game.board().bounties().len(), 1);
        let board = crate::narrate::board(&game, BRYN);
        assert!(board.contains("20 heavy ore for the death of Ash"), "{board}");

        // A price changes nothing about where a blow may land: camp is still a haven.
        game.apply(&Command::go(ASH, "south"), Tick::new(at_camp), &mut events);
        assert_eq!(game.can_attack(BRYN, ASH), Err(Refusal::Haven));

        // Back on the ledge Ash, the warden, kills marked Bryn. Nothing was on Bryn's head.
        let mut then = at_camp + TRAVEL_TIME.as_ticks();
        for who in [ASH, BRYN] {
            game.apply(&Command::go(who, "north"), Tick::new(then), &mut events);
        }
        then += TRAVEL_TIME.as_ticks();
        let (fight, then) = fight_to_the_end(&mut game, then);
        assert!(game.character(BRYN).unwrap().is_spirit());
        assert!(!fight.iter().any(|event| matches!(event, Event::Character { outcome: Outcome::BountyEarned { .. }, .. })));
        assert_eq!(game.board().bounties().len(), 1, "the price on Ash stands");

        // Had Bryn won, the board would have paid Bryn.
        let mut board = game.board().clone();
        assert_eq!(board.pay_bounties(ASH, BRYN), [reward]);
        assert_eq!(board.owed_to(BRYN)[Resource::Ore.index()], 20);
        assert!(board.bounties().is_empty());
        let _ = then;
    }

    #[test]
    fn a_price_makes_its_target_fair_game_and_a_newcomers_head_costs_more_and_marks_the_taker() {
        let mut game = Game::new(1, &["Ash", "Bryn", "Cole"]);
        const COLE: CharacterId = CharacterId::new(2);
        game.characters[2].stow(Resource::Ore, 200);
        let mut events = Vec::new();
        let ore = |amount| Lot { resource: Resource::Ore, amount };

        // Bryn has done nothing and is new to the world: a price there has to be steep.
        assert!(game.character(BRYN).unwrap().is_newcomer());
        game.apply(&Command::bounty(COLE, BRYN, ore(99)), Tick::new(0), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::CannotTrade { why: TradeRefusal::TooLittle(LEAST_BOUNTY_ON_NEWCOMER) }));
        game.apply(&Command::bounty(COLE, BRYN, ore(100)), Tick::new(0), &mut events);
        assert!(game.is_hunted(BRYN) && !game.is_hunted(ASH));

        // Out in the wilderness, clean Bryn could not be touched. Now anyone may try.
        for who in [ASH, BRYN] {
            game.apply(&Command::go(who, "north"), Tick::new(0), &mut events);
        }
        assert_eq!(game.can_attack(ASH, BRYN), Ok(()));
        assert_eq!(game.can_attack(BRYN, ASH), Err(Refusal::Unmarked), "the price is on one head only");

        // Ash, who is the stronger, takes it, and the land marks Ash for it.
        hardened(&mut game, ASH, 40);
        let (fight, _) = fight_to_the_end(&mut game, 30);
        assert!(game.character(BRYN).unwrap().is_spirit(), "Ash should have won");
        assert!(fight.contains(&Event::Character { who: ASH, outcome: Outcome::BountyEarned { on: BRYN, reward: ore(100), at: CAMP } }));
        assert_eq!(game.character(ASH).unwrap().standing(), Standing::Marked);
        assert!(!game.is_hunted(BRYN), "paid, and done with");
    }

    #[test]
    fn someone_who_has_made_their_way_can_be_hunted_for_less_and_without_shame() {
        let (mut game, now) = bryn_marked_on_the_ledge();
        // Bryn has worked the ledge to a mark. Time passes and the mark fades.
        let mut events = ticks(&mut game, now, now + 3_000);
        assert_eq!(game.character(BRYN).unwrap().standing(), Standing::Clean);
        while game.character(BRYN).unwrap().is_newcomer() {
            game.characters[1].train(Resource::Ore, 12, &mut Vec::new());
        }
        game.characters[0].stow(Resource::Kelp, 50);
        let then = now + 3_001;
        game.apply(&Command::go(ASH, "south"), Tick::new(then), &mut events);
        let kelp = |amount| Lot { resource: Resource::Kelp, amount };
        let at_camp = then + TRAVEL_TIME.as_ticks();
        game.apply(&Command::bounty(ASH, BRYN, kelp(9)), Tick::new(at_camp), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::CannotTrade { why: TradeRefusal::TooLittle(LEAST_BOUNTY) }));
        game.apply(&Command::bounty(ASH, BRYN, kelp(10)), Tick::new(at_camp), &mut events);
        assert!(game.is_hunted(BRYN));
        let free = game.character(ASH).unwrap().busy_until().get();
        game.apply(&Command::go(ASH, "north"), Tick::new(free), &mut events);
        assert_eq!(game.can_attack(ASH, BRYN), Ok(()));
        fight_to_the_end(&mut game, free + TRAVEL_TIME.as_ticks());
        if game.character(BRYN).unwrap().is_spirit() {
            assert_eq!(game.character(ASH).unwrap().standing(), Standing::Clean, "no mark for hunting the seasoned");
        }
    }

    #[test]
    fn killing_someone_with_a_price_on_their_head_pays_at_the_board_it_was_posted_on() {
        let (mut game, now) = bryn_marked_on_the_ledge();
        let reward = Lot { resource: Resource::Kelp, amount: 15 };
        // Someone, some time ago, put a price on Bryn.
        game.zones[0].board.post_bounty(Bounty { on: BRYN, by: ASH, reward });
        let (fight, now) = fight_to_the_end(&mut game, now);
        let earned = Event::Character { who: ASH, outcome: Outcome::BountyEarned { on: BRYN, reward, at: CAMP } };
        assert!(fight.contains(&earned));
        assert_eq!(game.audience(&earned), [ASH]);
        // Blood money stains: not a mark, for one killing the land wanted.
        let ash = game.character(ASH).unwrap();
        assert_eq!((i64::from(ash.taint()), ash.standing()), (crate::content::TAINT_FOR_BLOOD_MONEY, Standing::Clean));

        let mut events = Vec::new();
        game.apply(&Command::go(ASH, "south"), Tick::new(now), &mut events);
        game.apply(&Command::collect(ASH), Tick::new(now + TRAVEL_TIME.as_ticks()), &mut events);
        assert!(matches!(outcome(&events), Some(Outcome::Collected { .. })));
        assert_eq!(game.character(ASH).unwrap().carried(Resource::Kelp), 15);
    }

    #[test]
    fn a_spirit_can_fight_shades_where_the_living_do_not_work_and_comes_back_a_fighter() {
        let (mut game, now) = bryn_marked_on_the_ledge();
        let (_, now) = fight_to_the_end(&mut game, now);
        let mut events = Vec::new();

        // The living see no shades. Bryn, severed, is at camp, where nothing is worked.
        game.apply(&Command::spar(ASH), Tick::new(now), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::NoShade));
        game.apply(&Command::spar(BRYN), Tick::new(now), &mut events);
        let Some(&Outcome::Sparred { damage, left }) = outcome(&events) else { panic!("no blow landed: {events:?}") };
        assert_eq!(left, crate::content::SHADE_HEALTH - damage);
        assert_eq!(game.character(BRYN).unwrap().busy_until(), Tick::new(now) + ATTACK_TIME);
        assert_eq!(game.character(BRYN).unwrap().health(), 0, "a shade never strikes back");

        // Blow after blow, shade after shade, until there is enough to live on.
        let (events, then) = through_the_afterlife(&mut game, BRYN, now + ATTACK_TIME.as_ticks());
        assert!(events.iter().any(|event| matches!(event, Event::Character { outcome: Outcome::Sparred { left: 0, .. }, .. })));
        let minutes = (then - now) / 60;
        assert!((30..=40).contains(&minutes), "about as long as gathering takes: {minutes} minutes");
        let bryn = game.character(BRYN).unwrap();
        assert_eq!((bryn.life(), bryn.skill(SkillKind::Fighting).level()), (2, AFTER_ONE_DEATH));

        // Where the living work there is work to practise, not shades.
        let (mut game, now) = bryn_a_spirit_on_the_ledge();
        let mut events = Vec::new();
        game.apply(&Command::spar(BRYN), Tick::new(now), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::NoShade));
    }

    #[test]
    fn the_living_learn_to_fight_by_fighting_and_hit_harder_for_it() {
        let mut game = Game::new(1, &["Ash"]);
        let now = into_the_grotto(&mut game, ASH);
        let mut events = Vec::new();
        game.apply(&Command::fight(ASH, lurker(&game).unwrap().id), Tick::new(now), &mut events);
        // One blow is three points of the hundred a level takes, and more when rested.
        assert!(game.character(ASH).unwrap().skill(SkillKind::Fighting).progress() >= FIGHTING_XP_PER_BLOW);
    }

    #[test]
    fn every_death_asks_more_of_the_afterlife_and_sends_more_back() {
        let (mut game, now) = bryn_marked_on_the_ledge();
        assert_eq!(game.character(BRYN).unwrap().afterlife_goal(), AFTER_ONE_DEATH, "what a first death will ask");
        let (_, now) = fight_to_the_end(&mut game, now);
        let (_, first) = through_the_afterlife(&mut game, BRYN, now);
        let bryn = game.character(BRYN).unwrap();
        assert_eq!((bryn.life(), bryn.skill(SkillKind::Fighting).level(), bryn.afterlife_goal()), (2, 15, 20));

        // Killed again, Bryn is longer about it, and comes back knowing more.
        game.fell(1, Cause::Character(ASH), 0, Tick::new(first), &mut Vec::new());
        let (_, second) = through_the_afterlife(&mut game, BRYN, first);
        let bryn = game.character(BRYN).unwrap();
        assert_eq!((bryn.life(), bryn.skill(SkillKind::Fighting).level()), (3, 20));
        assert!(second - first > first - now, "{} against {}", second - first, first - now);

        // It only ever asks so much.
        let mut old = Character::new("Old", CAMP);
        for _ in 0..40 {
            old.die(Death { body: CAMP, by: Cause::LongSleep, passing: Passing::Severed, lost_at: Tick::ZERO, risen: false, buried: false, disturbed: false });
            old.be_reborn(CAMP, Tick::ZERO, Rebirth { base: 0, favoured: None, spend_ectoplasm: false, bloodline: Bloodline::Tidal });
        }
        assert_eq!(old.afterlife_goal(), crate::content::MOST_AFTERLIFE_LEVEL);
    }

    #[test]
    fn a_body_left_in_peace_is_remembered_and_a_looted_one_is_not() {
        let (game, now) = bryn_marked_on_the_ledge();
        let mining = game.character(BRYN).unwrap().skill(SkillKind::Mining).level();
        let next_life = |loot: bool| {
            let mut game = game.clone();
            let (_, now) = fight_to_the_end(&mut game, now);
            let mut events = Vec::new();
            if loot {
                game.apply(&Command::take(ASH), Tick::new(now), &mut events);
                assert!(matches!(outcome(&events), Some(Outcome::Took { .. })));
            }
            // Severed, Bryn is at camp. The beach is one step east.
            game.apply(&Command::go(BRYN, "east"), Tick::new(now), &mut events);
            through_the_afterlife(&mut game, BRYN, now + TRAVEL_TIME.as_ticks());
            let mining = *game.character(BRYN).unwrap().skill(SkillKind::Mining);
            (mining.level(), mining.peak())
        };
        assert_eq!(next_life(false), (0, mining), "nobody touched it: the old life can be won back fast");
        assert_eq!(next_life(true), (0, 0), "Ash took what lay beside it: the old life is gone");
    }

    #[test]
    fn a_friend_who_buries_the_body_keeps_it_from_the_robber_all_the_way_through_the_afterlife() {
        let (mut start, now) = bryn_marked_on_the_ledge();
        // Cole is Bryn's friend; Ash is the one who does the killing.
        let mut events = Vec::new();
        start.apply(&Command::join(COLE, "Cole"), Tick::new(now), &mut events);
        let mining = start.character(BRYN).unwrap().skill(SkillKind::Mining).level();
        let (_, died) = fight_to_the_end(&mut start, now);
        set_down(&mut start, COLE, LEDGE);

        // Bryn is a good way into the afterlife, on the beach, when Ash goes
        // through what Bryn dropped. Does Bryn's next life remember this one?
        let next_life = |friend_buries: bool| {
            let mut game = start.clone();
            let mut events = Vec::new();
            if friend_buries {
                game.apply(&Command::bury(COLE, BRYN), Tick::new(died), &mut events);
                assert!(events.contains(&Event::Character { who: COLE, outcome: Outcome::Buried { target: BRYN } }));
            }
            game.apply(&Command::go(BRYN, "east"), Tick::new(died), &mut events);
            let midway = died + 300;
            for tick in died + 1..=midway {
                game.tick(Tick::new(tick), &mut events);
                if game.character(BRYN).unwrap().busy_until() <= Tick::new(tick) {
                    game.apply(&Command::gather(BRYN), Tick::new(tick), &mut events);
                }
            }
            assert!(game.character(BRYN).unwrap().is_spirit(), "still in the afterlife");
            events.clear();
            game.apply(&Command::take(ASH), Tick::new(midway), &mut events);
            let robbed = matches!(outcome(&events), Some(Outcome::Took { .. }));
            through_the_afterlife(&mut game, BRYN, midway + 1);
            (robbed, game.character(BRYN).unwrap().skill(SkillKind::Mining).peak())
        };
        assert_eq!(next_life(false), (true, 0), "nobody guarded it: robbed midway, and the old life is forgotten");
        assert_eq!(next_life(true), (false, mining), "in the ground there was nothing to rob, and the old life comes back fast");

        // Once Bryn lives again, what happens at the old body is nothing to Bryn.
        let mut game = start.clone();
        through_the_afterlife(&mut game, BRYN, died);
        let before = *game.character(BRYN).unwrap().skill(SkillKind::Mining);
        let later = game.character(BRYN).unwrap().busy_until().get() + 1;
        game.apply(&Command::take(ASH), Tick::new(later), &mut Vec::new());
        assert_eq!(*game.character(BRYN).unwrap().skill(SkillKind::Mining), before);
        assert_eq!(before.peak(), mining.max(before.level()));
    }

    #[test]
    fn digging_up_a_grave_pays_what_is_in_it_and_costs_its_owner_the_life_it_kept() {
        let (mut start, now) = bryn_marked_on_the_ledge();
        let mut events = Vec::new();
        start.apply(&Command::join(COLE, "Cole"), Tick::new(now), &mut events);
        let mining = start.character(BRYN).unwrap().skill(SkillKind::Mining).level();
        let carried = start.character(BRYN).unwrap().carried_total();
        let (_, died) = fight_to_the_end(&mut start, now);
        set_down(&mut start, COLE, LEDGE);
        start.apply(&Command::bury(COLE, BRYN), Tick::new(died), &mut events);
        let grave = *start.graves_at(LEDGE).next().unwrap();
        assert_eq!((grave.owner, grave.contents.iter().sum::<u64>()), (BRYN, carried), "everything went into the ground");
        assert!(crate::narrate::place(&start, COLE, Tick::new(died)).contains("Bryn lies buried here."));
        // Bryn lives again, by way of the beach, and remembers.
        start.apply(&Command::go(BRYN, "east"), Tick::new(died), &mut events);
        let (_, born) = through_the_afterlife(&mut start, BRYN, died + TRAVEL_TIME.as_ticks());
        assert_eq!(start.character(BRYN).unwrap().skill(SkillKind::Mining).peak(), mining);
        // Time passes. Bryn works, and wins back all of that life and more.
        hardened(&mut start, BRYN, 30);
        while start.character(BRYN).unwrap().skill(SkillKind::Mining).level() < mining {
            start.characters[1].train(Resource::Ore, 12, &mut Vec::new());
        }
        let level = |game: &Game, attribute| game.character(BRYN).unwrap().attribute(attribute).level();
        let before = (level(&start, Attribute::Vigor), level(&start, Attribute::Attunement));
        start.characters.iter_mut().for_each(|character| character.busy_until = Tick::ZERO);
        let dig = |game: &mut Game, who: CharacterId| {
            let mut events = Vec::new();
            game.apply(&Command::dig(who), Tick::new(born), &mut events);
            events
        };

        // Nothing to dig at camp.
        assert_eq!(outcome(&dig(&mut start, BRYN)), Some(&Outcome::NoGrave));

        // Bryn remembers the ledge, goes back, and robs their own grave:
        // everything they carried is theirs again, and the old life is gone.
        let mut own = start.clone();
        set_down(&mut own, BRYN, LEDGE);
        assert!(crate::narrate::place(&own, BRYN, Tick::new(born)).contains("You lie buried here."));
        assert_eq!(dig(&mut own, BRYN), [
            Event::Character { who: BRYN, outcome: Outcome::DugUp { owner: BRYN, took: grave.contents } },
            Event::Character { who: BRYN, outcome: Outcome::Unearthed { fell: Attribute::Vigor, levels: GRAVE_SHIFT } },
        ]);
        // Having relearned everything does not make it free: the body
        // is the less for it, and the spirit the more.
        assert_eq!((level(&own, Attribute::Vigor), level(&own, Attribute::Attunement)), (before.0 - GRAVE_SHIFT, before.1 + GRAVE_SHIFT));
        let bryn = own.character(BRYN).unwrap();
        assert_eq!((bryn.carried_total(), bryn.skill(SkillKind::Mining).peak()), (carried, bryn.skill(SkillKind::Mining).level()));
        assert_eq!(bryn.attribute(Attribute::Vigor).peak(), before.0 - GRAVE_SHIFT, "and it will not come back any faster");
        assert_eq!(bryn.standing(), Standing::Clean, "it was theirs to take");
        assert_eq!(own.graves_at(LEDGE).count(), 0, "a grave is dug up once");

        // Ash gets there first instead. Ash has the goods and the land's
        // mark, and Bryn, wherever Bryn is, has lost that life.
        let mut robbed = start.clone();
        let stolen = dig(&mut robbed, ASH);
        assert!(stolen.contains(&Event::Character { who: ASH, outcome: Outcome::DugUp { owner: BRYN, took: grave.contents } }));
        assert!(stolen.contains(&Event::Character { who: BRYN, outcome: Outcome::GraveRobbed { by: ASH } }));
        assert_eq!(robbed.character(ASH).unwrap().standing(), Standing::Marked);
        assert!(stolen.contains(&Event::Character { who: BRYN, outcome: Outcome::Unearthed { fell: Attribute::Vigor, levels: GRAVE_SHIFT } }));
        let bryn = robbed.character(BRYN).unwrap();
        assert_eq!(bryn.skill(SkillKind::Mining).peak(), bryn.skill(SkillKind::Mining).level());
        // The same change comes over Bryn, who is nowhere near; Ash is as Ash was.
        assert_eq!((level(&robbed, Attribute::Vigor), level(&robbed, Attribute::Attunement)), (before.0 - GRAVE_SHIFT, before.1 + GRAVE_SHIFT));
        assert_eq!(robbed.character(ASH).unwrap().attribute(Attribute::Attunement).level(), 0);
    }

    /// Walks a spirit through the afterlife, memory by memory, until it
    /// lives again. Returns what happened and when it was over.
    fn through_the_afterlife(game: &mut Game, who: CharacterId, from: u64) -> (Vec<Event>, u64) {
        let mut events = Vec::new();
        let mut now = from;
        while game.character(who).unwrap().is_spirit() {
            assert!(now < from + 7_200, "the afterlife should not take two hours");
            game.tick(Tick::new(now), &mut events);
            if game.character(who).unwrap().busy_until() <= Tick::new(now) {
                let step = crate::bots::afterlife(game, who).expect("a land with nothing left to remember");
                game.apply(&step, Tick::new(now), &mut events);
            }
            now += 1;
        }
        (events, now)
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
            assert!(hauls_back < hauls, "it should never take as long again");
            game.apply(&Command::gather(ASH), Tick::new(now), &mut events);
            // Leave the ledge time to regrow, as before.
            for tick in now + 1..=now + 600 {
                game.tick(Tick::new(tick), &mut events);
            }
            events.clear();
            now += 600;
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

        for (command, why) in [(Command::return_to_body(ASH), ReturnRefusal::NoWayBack), (Command::relinquish(ASH), ReturnRefusal::Unredeemed)] {
            game.apply(&Command::wake(ASH), Tick::new(later + 6_000), &mut events);
            game.apply(&command, Tick::new(later + 6_000), &mut events);
            assert_eq!(outcome(&events), Some(&Outcome::CannotReturn { why }));
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
        assert!(game.character(ASH).unwrap().is_spirit(), "a fallen guard frees the relic, not the spirit");
        game.apply(&Command::take(BRYN), Tick::new(at), &mut events);
        assert_eq!(game.character(BRYN).unwrap().carried(Resource::Tideglass), glass);
    }

    #[test]
    fn a_guard_that_falls_or_runs_out_of_time_leaves_its_spirit_to_find_its_own_way() {
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
        assert!(world.character(BRYN).unwrap().is_spirit());

        // Left alone, it crumbles in the end, with the same result.
        let later = ticks(&mut game, lost_at + 61, lost_at + GUARD_LASTS.as_ticks() + 10);
        let being = Being { kind: CreatureKind::BodyGuard, of: Some(BRYN) };
        assert!(later.contains(&Event::CreatureGone { place: LEDGE, being }));
        assert!(game.character(BRYN).unwrap().is_spirit());
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

        // The body rises, and the long way round is all that is left.
        through_the_afterlife(&mut game, BRYN, now + CORPSE_TIME.as_ticks());
        let bryn = game.character(BRYN).unwrap();
        // It takes 179 draws to practise a skill to the 15 a first death
        // asks. A hundred and sixty of that ectoplasm buys eight percent
        // of the old life; the rest is kept.
        assert_eq!((bryn.kept(), bryn.ectoplasm()), (8, 19));
        assert_eq!(bryn.skill(SkillKind::Mining).level(), AFTER_ONE_DEATH.max(mining * 8 / 100));
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
        through_the_afterlife(&mut game, BRYN, now);
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
    fn two_bloodlines_joined_in_a_rite_mend_what_one_alone_cannot() {
        let (start, now) = a_stripped_ledge();
        let strain_after = |blood: Bloodline| {
            let mut game = start.clone();
            game.characters[1].bloodline = blood;
            let mut events = Vec::new();
            game.apply(&Command::rite(ASH), Tick::new(now), &mut events);
            game.apply(&Command::rite(BRYN), Tick::new(now + 40), &mut events);
            ledge(&game).node.strain_percent()
        };
        let (alone, joined) = (strain_after(Bloodline::Tidal), strain_after(Bloodline::Ash));
        assert!(alone > 0, "one bloodline lifts half: {alone}");
        assert_eq!(joined, 0, "two lift all of it");
    }

    #[test]
    fn everyone_is_born_into_the_bloodline_of_their_land_and_founders_name_it() {
        let mut game = founders_met();
        let blood = |game: &Game, who| game.character(who).unwrap().bloodline();
        assert_eq!((blood(&game, ASH), blood(&game, BRYN), blood(&game, COLE)), (Bloodline::Tidal, Bloodline::Covenant, Bloodline::Covenant));
        assert_eq!(game.bloodline_name(Bloodline::Covenant), "the Covenant line");

        let name = |game: &mut Game, who, name: &str| {
            let mut events = Vec::new();
            game.apply(&Command::name_bloodline(who, name), Tick::new(30), &mut events);
            outcome(&events).cloned()
        };
        let cannot = |why| Some(Outcome::CannotFound { why });
        assert_eq!(name(&mut game, COLE, "Ironkin"), cannot(FoundRefusal::NotFounder));
        assert_eq!(name(&mut game, BRYN, "Ironkin"), Some(Outcome::BloodlineNamed { name: "Ironkin".into() }));
        assert_eq!(name(&mut game, BRYN, "Stonekin"), cannot(FoundRefusal::AlreadyNamed));
        assert_eq!(name(&mut game, ASH, "ironkin"), cannot(FoundRefusal::Name(eq_naming::Problem::Taken)));
        assert_eq!(game.bloodline_name(blood(&game, COLE)), "Ironkin");
        assert!(crate::narrate::stats(&game, COLE).contains("Bloodline    Ironkin (learns Resilience and Willpower faster)"));

        // Blood tells: the same work teaches Bryn's line more Resilience than Ash's.
        let resilience = |game: &mut Game, index: usize| {
            for _ in 0..40 {
                game.characters[index].train(Resource::Ore, 12, &mut Vec::new());
            }
            let known = *game.characters[index].attribute(Attribute::Resilience);
            u64::from(known.level()) * 1_000 + known.progress()
        };
        assert!(resilience(&mut game, 1) > resilience(&mut game, 0));

        // Blood is kept: wherever Cole next arises, it is as one of the Covenant line.
        game.fell(2, Cause::Character(ASH), 0, Tick::new(40), &mut Vec::new());
        game.begin_life(2, Tick::new(50), 0, false);
        assert_eq!((game.character(COLE).unwrap().home(), blood(&game, COLE)), (game.character(ASH).unwrap().home(), Bloodline::Covenant));
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

    const COLE: CharacterId = CharacterId::new(2);
    const DARA: CharacterId = CharacterId::new(3);

    /// A world with room for two founders, and Ash and Bryn in it.
    fn two_founders() -> Game {
        let mut game = Game::new_world(1, 2, &[]);
        let mut events = Vec::new();
        game.apply(&Command::join(ASH, "Ash"), Tick::new(0), &mut events);
        game.apply(&Command::join(BRYN, "Bryn"), Tick::new(0), &mut events);
        assert_eq!(events[1], Event::Character { who: ASH, outcome: Outcome::Founded });
        game
    }

    #[test]
    fn each_founder_arrives_alone_in_a_land_of_their_own() {
        let game = two_founders();
        assert_eq!(game.zones().len(), 5, "the shoreline, the Meeting Ground, the River of Souls, and one land each");
        let (ash, bryn) = (game.character(ASH).unwrap(), game.character(BRYN).unwrap());
        let (hers, his) = (game.zone_at(ash.place()), game.zone_at(bryn.place()));
        assert_eq!((hers.founder(), his.founder()), (Some(ASH), Some(BRYN)));
        assert_eq!((ash.place(), ash.home()), (hers.anchor(), hers.anchor()));
        assert_ne!(hers.anchor(), his.anchor());
        assert_eq!(game.occupants(hers.anchor()), [ASH]);
        assert_eq!(game.occupants(game.anchor()), [], "nobody is on the old shoreline");

        // The lands are different places, each whole, and nothing joins them.
        let names = |zone: &Zone| zone.place_ids().map(|place| game.map().place(place).unwrap().name).collect::<Vec<_>>();
        assert_eq!((names(hers).len(), names(his).len()), (11, 11), "a haven, nine places to work and a pass");
        assert_ne!(names(hers), names(his));
        assert!(game.map().first_step(hers.anchor(), game.anchor(), |_| true).is_none(), "nothing leads to the old shoreline");
        assert_eq!(game.creatures_at(game.map().places().find(|(id, place)| hers.contains(*id) && place.lair.is_some()).unwrap().0).count(), 1);
    }

    #[test]
    fn the_same_seed_founds_the_same_lands_and_another_seed_others() {
        let places = |seed: u64| {
            let mut game = Game::new_world(seed, 1, &[]);
            game.apply(&Command::join(ASH, "Ash"), Tick::new(0), &mut Vec::new());
            game.zone_of(ASH).unwrap().place_ids().map(|place| game.map().place(place).unwrap().position).collect::<Vec<_>>()
        };
        assert_eq!(places(4), places(4));
        assert_ne!(places(4), places(5));
    }

    #[test]
    fn a_founder_names_land_and_house_once_and_no_two_lands_share_a_name() {
        let mut game = two_founders();
        let mut events = Vec::new();
        let refused = |events: &[Event]| match outcome(events) {
            Some(Outcome::CannotFound { why }) => Some(*why),
            _ => None,
        };
        game.apply(&Command::name_land(ASH, "  the salt   march "), Tick::new(1), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::LandNamed { name: "The Salt March".into() }));
        game.apply(&Command::name_house(ASH, "thorne"), Tick::new(1), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::HouseNamed { name: "Thorne".into() }));
        let land = game.zone_of(ASH).unwrap();
        assert_eq!((land.name(), land.house()), (Some("The Salt March"), Some("Thorne")));

        game.apply(&Command::name_land(ASH, "Second Thoughts"), Tick::new(2), &mut events);
        assert_eq!(refused(&events), Some(FoundRefusal::AlreadyNamed));
        game.apply(&Command::name_land(BRYN, "the saltmarch"), Tick::new(2), &mut events);
        assert_eq!(refused(&events), Some(FoundRefusal::Name(eq_naming::Problem::Taken)));
        game.apply(&Command::name_land(BRYN, "x"), Tick::new(2), &mut events);
        assert_eq!(refused(&events), Some(FoundRefusal::Name(eq_naming::Problem::TooShort)));
        game.apply(&Command::name_land(BRYN, "Fort 9"), Tick::new(2), &mut events);
        assert_eq!(refused(&events), Some(FoundRefusal::Name(eq_naming::Problem::BadCharacter('9'))));
        assert_eq!(game.zone_of(BRYN).unwrap().name(), None);
    }

    #[test]
    fn the_invited_arrive_in_their_founders_land_and_the_uninvited_where_there_are_fewest() {
        let mut game = two_founders();
        let mut events = Vec::new();
        game.apply(&Command::invite(BRYN, "cole"), Tick::new(1), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::Invited { name: "Cole".into() }));
        game.apply(&Command::invite(BRYN, "Ash"), Tick::new(1), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::CannotFound { why: FoundRefusal::AlreadyHere }));

        // Both lands are founded, so Cole would otherwise go where there are fewest.
        events.clear();
        game.apply(&Command::join(COLE, "Cole"), Tick::new(2), &mut events);
        assert_eq!(events, [Event::Character { who: COLE, outcome: Outcome::Joined }], "invited, not a founder");
        let brynsland = game.zone_of(BRYN).unwrap().anchor();
        assert_eq!((game.character(COLE).unwrap().place(), game.character(COLE).unwrap().home()), (brynsland, brynsland));
        assert_eq!(game.audience(&events[0]), [BRYN, COLE], "Bryn sees them arrive; Ash, elsewhere, does not");

        // The invitation is used up, and only a founder can give one.
        game.apply(&Command::invite(COLE, "Dara"), Tick::new(3), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::CannotFound { why: FoundRefusal::NotFounder }));
        events.clear();
        game.apply(&Command::join(DARA, "Dara"), Tick::new(4), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::Unborn), "nobody asked for Dara");
        let dara = game.character(DARA).unwrap();
        assert!(dara.is_spirit());
        assert_eq!(Some(dara.place()), game.river(), "every soul enters the afterlife at the river");
    }

    #[test]
    fn souls_come_ashore_at_the_river_and_ways_lead_from_it_into_every_land() {
        let mut game = two_founders();
        let river = game.river().unwrap();
        assert_eq!(game.map().place(river).map(|place| (place.name, place.deposit.is_none())), Some(("The River of Souls", true)));
        let (ashland, brynsland) = (game.character(ASH).unwrap().home(), game.character(BRYN).unwrap().home());
        assert_eq!(game.map().exit(river, "first").map(|exit| exit.to), Some(ashland));
        assert_eq!(game.map().exit(river, "second").map(|exit| exit.to), Some(brynsland));
        assert!(game.map().first_step(ashland, river, |_| true).is_none(), "nothing leads the living there");

        // The unborn begin on its bank, with a shade to fight, and can walk into a land.
        let mut events = Vec::new();
        game.apply(&Command::join(COLE, "Cole"), Tick::new(1), &mut events);
        assert_eq!(game.occupants(river), [COLE]);
        game.apply(&Command::spar(COLE), Tick::new(1), &mut events);
        assert!(matches!(outcome(&events), Some(Outcome::Sparred { .. })));
        game.apply(&Command::go(COLE, "second"), Tick::new(10), &mut events);
        assert_eq!(game.character(COLE).unwrap().place(), brynsland);

        // Bryn dies at home, and is at the river that instant: the dead
        // do not linger by their bodies.
        let lair = game.zone_at(brynsland).place_ids().find(|&place| game.map().place(place).unwrap().lair == Some(CreatureKind::Lurker));
        let mut now = walk_to(&mut game, BRYN, lair.unwrap(), 20);
        while !game.character(BRYN).unwrap().is_spirit() {
            ticks(&mut game, now, now);
            now += 1;
        }
        assert_eq!(game.occupants(river), [BRYN]);
        assert_eq!(game.bodies_at(lair.unwrap()).count(), 1, "the body stays where it fell, a long way off");
        // Nothing leads from the river back to it but a whole new life.
        let mut events = Vec::new();
        game.apply(&Command::return_to_body(BRYN), Tick::new(now), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::CannotReturn { why: ReturnRefusal::NoWayBack }));
    }

    #[test]
    fn a_world_of_one_shoreline_has_no_river_and_its_dead_stay_where_they_are() {
        let (mut game, now) = bryn_marked_on_the_ledge();
        assert_eq!(game.river(), None);
        let (_, now) = fight_to_the_end(&mut game, now);
        let events = ticks(&mut game, now, now + CORPSE_TIME.as_ticks());
        assert!(!events.iter().any(|event| matches!(event, Event::Character { outcome: Outcome::CameToRiver, .. })));
        assert_eq!(game.character(BRYN).unwrap().place(), CAMP);
    }

    const EDDA: CharacterId = CharacterId::new(4);
    const FENN: CharacterId = CharacterId::new(5);

    #[test]
    fn the_unborn_earn_a_first_life_and_arise_wherever_people_are_fewest() {
        let mut game = two_founders();
        let mut events = Vec::new();
        game.apply(&Command::invite(BRYN, "Cole"), Tick::new(1), &mut events);
        game.apply(&Command::join(COLE, "Cole"), Tick::new(2), &mut events);
        let (ashland, brynsland) = (game.zone_of(ASH).unwrap().anchor(), game.zone_of(BRYN).unwrap().anchor());

        // Bryn's land has two people and Ash's one. Three strangers come.
        let mut now = 3;
        let mut homes = Vec::new();
        for (who, name) in [(DARA, "Dara"), (EDDA, "Edda"), (FENN, "Fenn")] {
            events.clear();
            game.apply(&Command::join(who, name), Tick::new(now), &mut events);
            assert_eq!(outcome(&events), Some(&Outcome::Unborn));
            // The unborn can do nothing a living person can.
            game.apply(&Command::take(who), Tick::new(now), &mut events);
            assert_eq!(outcome(&events), Some(&Outcome::Bodiless));
            let (walked, then) = through_the_afterlife(&mut game, who, now);
            assert!(walked.contains(&Event::Character { who, outcome: Outcome::Redeemed { by: None, life: 1 } }));
            let born = game.character(who).unwrap();
            assert!(!born.is_spirit());
            assert_eq!((born.place(), born.life(), born.attribute(Attribute::Lineage).level()), (born.home(), 1, 0));
            assert_eq!(born.best_practised(), 0, "what was practised is theirs now, as a skill");
            assert!(SkillKind::ALL.iter().any(|&skill| born.skill(skill).level() == AFTERLIFE_LEVEL));
            assert_eq!(born.bloodline(), game.zone_at(born.home()).bloodline(), "the unborn take the blood of the land they arise in");
            homes.push(born.home());
            now = then;
        }
        // One to even the lands up, then the older land first, then the other.
        assert_eq!(homes, [ashland, ashland, brynsland]);
    }

    #[test]
    fn the_unborn_are_in_no_hurry() {
        let mut game = two_founders();
        let mut events = Vec::new();
        game.apply(&Command::join(COLE, "Cole"), Tick::new(1), &mut events);
        let long = crate::content::REDEMPTION_CLOCK.as_ticks() * 3;
        let waited = ticks(&mut game, 2, long);
        assert!(!waited.iter().any(|event| matches!(event, Event::Character { who: COLE, .. })), "nothing happens to them");
        assert!(game.character(COLE).unwrap().forgotten().is_some_and(|forgotten| forgotten.unborn));
    }

    #[test]
    fn the_dead_arise_where_people_are_fewest_and_not_where_they_lived() {
        let mut game = two_founders();
        let mut events = Vec::new();
        for (who, name) in [(COLE, "Cole"), (DARA, "Dara")] {
            game.apply(&Command::invite(BRYN, name), Tick::new(1), &mut events);
            game.apply(&Command::join(who, name), Tick::new(2), &mut events);
        }
        let ashland = game.zone_of(ASH).unwrap().anchor();
        // Cole, of Bryn's crowded land, walks into its lair and does not walk out.
        let land = game.zone_of(COLE).unwrap();
        let lair = game.map().places().find(|(id, place)| land.contains(*id) && place.lair.is_some()).unwrap().0;
        let mut now = 3;
        while !game.character(COLE).unwrap().is_spirit() {
            assert!(now < 7_200, "the lair never finished Cole");
            game.tick(Tick::new(now), &mut events);
            let cole = game.character(COLE).unwrap();
            if cole.busy_until() <= Tick::new(now) {
                let step = match game.map().first_step(cole.place(), lair, |_| true) {
                    Some(exit) => Command::go(COLE, &exit.label),
                    None => Command::gather(COLE),
                };
                game.apply(&step, Tick::new(now), &mut events);
            }
            now += 1;
        }
        through_the_afterlife(&mut game, COLE, now);
        let cole = game.character(COLE).unwrap();
        assert_eq!((cole.place(), cole.home(), cole.life()), (ashland, ashland, 2), "Ash was alone");
    }

    #[test]
    fn each_land_has_its_own_health_board_and_rebirth() {
        let mut game = two_founders();
        let mut events = Vec::new();
        let home = game.character(ASH).unwrap().home();
        // Ash strips whatever lies one step from her haven.
        let way = game.map().exits(home)[0].label.clone();
        game.apply(&Command::go(ASH, &way), Tick::new(0), &mut events);
        let mut now = 30;
        for _ in 0..400 {
            game.tick(Tick::new(now), &mut events);
            game.apply(&Command::gather(ASH), Tick::new(now), &mut events);
            now += GATHER_TIME.as_ticks();
        }
        ticks(&mut game, now, now + 5);
        let (hers, his) = (game.zone_of(ASH).unwrap(), game.zone_of(BRYN).unwrap());
        assert!(hers.vitality() < 100, "Ash's land should feel it: {}", hers.vitality());
        assert_eq!(his.vitality(), 100, "Bryn's land is another land");
        assert_eq!(game.vitality(), 100, "and so is the shoreline");

        // A contract pinned in one land is not on another's board.
        let back = game.map().exits(game.character(ASH).unwrap().place())[0].label.clone();
        game.apply(&Command::go(ASH, &back), Tick::new(now + 10), &mut events);
        let ore = Lot { resource: game.map().place(game.map().exits(home)[0].to).unwrap().deposit.unwrap().resource, amount: 1 };
        game.apply(&Command::post(ASH, ore, lot(Resource::Kelp, 1)), Tick::new(now + 40), &mut events);
        assert_eq!(game.zone_at(home).board().contracts().len(), 1);
        assert_eq!(game.zone_of(BRYN).unwrap().board().contracts().len(), 0);
        game.apply(&Command::accept(BRYN, ContractId::new(0)), Tick::new(now + 40), &mut events);
        assert_eq!(outcome(&events), Some(&Outcome::CannotTrade { why: TradeRefusal::NoSuchContract }));
    }

    /// Walks `who` from where they stand to `target` by the shortest
    /// way, a step every thirty ticks from `now`, with the world
    /// ticking. Returns the tick they arrive at, or stop at if they die.
    fn walk_to(game: &mut Game, who: CharacterId, target: PlaceId, mut now: u64) -> u64 {
        let mut events = Vec::new();
        while game.character(who).unwrap().place() != target && !game.character(who).unwrap().is_spirit() {
            let here = game.character(who).unwrap().place();
            let way = game.map().first_step(here, target, |_| true).expect("there is a way").label.clone();
            game.apply(&Command::go(who, &way), Tick::new(now), &mut events);
            let arrived = game.character(who).unwrap().place() != here;
            // A flooded way is waited out.
            let wait = if arrived { TRAVEL_TIME.as_ticks() } else { 60 };
            ticks(game, now + 1, now + wait);
            now += wait;
        }
        now
    }

    fn pass_of(game: &Game, who: CharacterId) -> PlaceId {
        let land = game.zone_at(game.character(who).unwrap().home());
        land.place_ids().find(|&place| game.map().place(place).unwrap().lair == Some(CreatureKind::Sentinel)).unwrap()
    }

    #[test]
    fn every_land_has_a_pass_and_every_pass_leads_to_the_meeting_ground() {
        let game = two_founders();
        let ground = game.meeting_ground().unwrap();
        assert_eq!(game.map().place(ground).map(|place| (place.name, place.safety)), Some(("The Meeting Ground", Safety::Contested)));
        for (who, back) in [(ASH, "first"), (BRYN, "second")] {
            let pass = pass_of(&game, who);
            assert_eq!(game.map().exit(pass, "beyond").map(|exit| exit.to), Some(ground));
            assert_eq!(game.map().exit(ground, back).map(|exit| exit.to), Some(pass));
            assert_eq!(game.creatures_at(pass).map(|creature| creature.being.kind).collect::<Vec<_>>(), [CreatureKind::Sentinel]);
        }
        // So there is a way between the two lands now, and it runs through both passes.
        let (hers, his) = (game.character(ASH).unwrap().home(), game.character(BRYN).unwrap().home());
        assert!(game.map().first_step(hers, his, |_| true).is_some());
        assert_eq!(game.zone_at(ground).founder(), None);
        assert_eq!(game.occupants(ground), []);
    }

    /// Sets someone down in a place without the walk.
    fn set_down(game: &mut Game, who: CharacterId, place: PlaceId) {
        let index = game.index_of(who).unwrap();
        game.occupancy.leave(game.characters[index].place, who);
        game.characters[index].place = place;
        game.characters[index].busy_until = Tick::ZERO;
        game.occupancy.enter(place, who);
    }

    /// Ash and Bryn, founders, on the Meeting Ground, with Cole at home
    /// in Bryn's land.
    fn founders_met() -> Game {
        let mut game = two_founders();
        let mut events = Vec::new();
        game.apply(&Command::invite(BRYN, "Cole"), Tick::new(1), &mut events);
        game.apply(&Command::join(COLE, "Cole"), Tick::new(2), &mut events);
        let ground = game.meeting_ground().unwrap();
        set_down(&mut game, ASH, ground);
        set_down(&mut game, BRYN, ground);
        game
    }

    fn deal(game: &mut Game, who: CharacterId, with: &str, dealing: Dealing) -> Vec<Event> {
        let mut events = Vec::new();
        game.apply(&Command::deal(who, with, dealing), Tick::new(10), &mut events);
        events
    }

    fn refused(events: &[Event]) -> Option<PactRefusal> {
        match outcome(events) {
            Some(Outcome::CannotDeal { why }) => Some(*why),
            _ => None,
        }
    }

    #[test]
    fn lands_deal_through_their_founders_on_the_meeting_ground() {
        let mut game = founders_met();
        let (ashland, brynsland) = (game.character(ASH).unwrap().home(), game.character(BRYN).unwrap().home());
        assert_eq!(game.pact(ashland, brynsland), None, "lands that never deal are nothing to each other");

        assert_eq!(refused(&deal(&mut game, COLE, "Ash", Dealing::War)), Some(PactRefusal::NotFounder));
        assert_eq!(refused(&deal(&mut game, ASH, "Nobody", Dealing::Ally)), Some(PactRefusal::NoSuchLand));
        assert_eq!(refused(&deal(&mut game, ASH, "ash", Dealing::Ally)), Some(PactRefusal::OwnLand));
        assert_eq!(refused(&deal(&mut game, ASH, "Bryn", Dealing::Truce)), Some(PactRefusal::NoPact));
        let mut home = game.clone();
        set_down(&mut home, ASH, ashland);
        assert_eq!(refused(&deal(&mut home, ASH, "Bryn", Dealing::War)), Some(PactRefusal::NotOnMeetingGround));

        // An alliance waits for the other founder to say the same.
        let offered = deal(&mut game, ASH, "Bryn", Dealing::Ally);
        assert_eq!(offered, [
            Event::Character { who: ASH, outcome: Outcome::Offered { land: brynsland, dealing: Dealing::Ally } },
            Event::Character { who: BRYN, outcome: Outcome::WasOffered { land: ashland, dealing: Dealing::Ally } },
        ]);
        assert_eq!(game.pact(ashland, brynsland), None);
        let sealed = deal(&mut game, BRYN, "ASH", Dealing::Ally);
        assert_eq!(sealed, [Event::Pact { by: brynsland, with: ashland, dealt: Dealt::Allied }]);
        assert_eq!(game.pact(ashland, brynsland), Some(Pact::Alliance));
        assert_eq!(game.audience(&sealed[0]), [ASH, BRYN, COLE], "everyone of either land hears, wherever they are");

        // A land can be named by its own name, and war takes only one.
        game.apply(&Command::name_land(ASH, "Saltmarch"), Tick::new(11), &mut Vec::new());
        let war = deal(&mut game, BRYN, "saltmarch", Dealing::War);
        assert_eq!(war, [Event::Pact { by: brynsland, with: ashland, dealt: Dealt::AtWar }]);
        assert_eq!(game.pact(brynsland, ashland), Some(Pact::War));
    }

    #[test]
    fn a_sentinel_lets_its_lands_allies_by() {
        let mut strangers = founders_met();
        let pass = pass_of(&strangers, ASH);
        let mut allies = strangers.clone();
        deal(&mut allies, ASH, "Bryn", Dealing::Ally);
        deal(&mut allies, BRYN, "Ash", Dealing::Ally);
        for game in [&mut strangers, &mut allies] {
            set_down(game, BRYN, pass);
            ticks(game, 11, 71);
        }
        assert!(strangers.character(BRYN).unwrap().health() < 100, "a stranger is set upon");
        assert_eq!(allies.character(BRYN).unwrap().health(), 100, "an ally walks through");
    }

    #[test]
    fn war_opens_the_wilderness_and_every_death_sickens_both_lands() {
        let mut peace = founders_met();
        let mut war = peace.clone();
        deal(&mut war, ASH, "Bryn", Dealing::War);
        let (ashland, brynsland) = (peace.character(ASH).unwrap().home(), peace.character(BRYN).unwrap().home());

        // Ash goes into Bryn's land after Cole, who has done nothing.
        let wilds = peace
            .zone_at(brynsland)
            .place_ids()
            .find(|&place| peace.map().place(place).is_some_and(|data| data.safety == Safety::Buffer && data.lair.is_none()))
            .unwrap();
        for game in [&mut peace, &mut war] {
            set_down(game, ASH, wilds);
            set_down(game, COLE, wilds);
        }
        assert_eq!(peace.can_attack(ASH, COLE), Err(Refusal::Unmarked));
        assert_eq!(war.can_attack(ASH, COLE), Ok(()));
        set_down(&mut war, ASH, brynsland);
        set_down(&mut war, COLE, brynsland);
        assert_eq!(war.can_attack(ASH, COLE), Err(Refusal::Haven), "a haven is a haven, even in war");
        set_down(&mut war, ASH, wilds);
        set_down(&mut war, COLE, wilds);

        let mut events = Vec::new();
        let mut now = 20;
        while !war.character(COLE).unwrap().is_spirit() && !war.character(ASH).unwrap().is_spirit() {
            war.apply(&Command::attack(ASH, COLE), Tick::new(now), &mut events);
            now += ATTACK_TIME.as_ticks();
        }
        // One of them died in Bryn's land. Ash's land, far away, feels it too.
        let felt = |game: &Game, land: PlaceId| game.zone_at(land).corruption();
        assert_eq!((felt(&war, brynsland), felt(&war, ashland)), (CORRUPTION_PER_DEATH, CORRUPTION_PER_DEATH));
    }

    #[test]
    fn a_founder_raises_a_house_and_the_lands_people_are_safe_under_its_roof() {
        let mut game = founders_met();
        let (ashland, brynsland) = (game.character(ASH).unwrap().home(), game.character(BRYN).unwrap().home());
        let wilds = |game: &Game, land: PlaceId| {
            let fit = |place: &PlaceId| game.map().place(*place).is_some_and(|data| data.safety == Safety::Buffer && data.lair.is_none());
            game.zone_at(land).place_ids().find(fit).unwrap()
        };
        let site = wilds(&game, brynsland);
        let raise = |game: &mut Game, who: CharacterId| {
            let mut events = Vec::new();
            game.apply(&Command::raise(who), Tick::new(20), &mut events);
            outcome(&events).cloned()
        };
        let cannot = |why| Some(Outcome::CannotFound { why });

        set_down(&mut game, COLE, site);
        assert_eq!(raise(&mut game, COLE), cannot(FoundRefusal::NotFounder));
        set_down(&mut game, BRYN, brynsland);
        assert_eq!(raise(&mut game, BRYN), cannot(FoundRefusal::CannotStandHere), "camp is a haven already");
        let abroad = wilds(&game, ashland);
        set_down(&mut game, BRYN, abroad);
        assert_eq!(raise(&mut game, BRYN), cannot(FoundRefusal::CannotStandHere), "not in someone else's land");
        set_down(&mut game, BRYN, site);
        assert_eq!(raise(&mut game, BRYN), cannot(FoundRefusal::CannotAfford));
        for (resource, amount) in crate::content::HOUSE_COST {
            game.characters[1].stow(resource, amount + 1);
        }
        let worked = |game: &Game| game.zone_at(brynsland).place_ids().filter(|&place| game.map().place(place).unwrap().deposit.is_some()).count();
        let before = worked(&game);
        assert_eq!(raise(&mut game, BRYN), Some(Outcome::HouseRaised));
        assert_eq!(game.zone_at(brynsland).hall(), Some(site));
        // The price that is never paid off: one place fewer to work, for good.
        assert!(game.map().place(site).unwrap().deposit.is_none());
        assert_eq!(worked(&game), before - 1);
        // 40 driftwood and 20 ore, a quarter off in the Era of Rebuilding: 30 and 15.
        assert_eq!(game.character(BRYN).unwrap().carried_total(), 2 + 10 + 5, "the house took what it costs");
        game.characters[1].busy_until = Tick::ZERO;
        assert_eq!(raise(&mut game, BRYN), cannot(FoundRefusal::HouseStands));

        // War, and Ash in Bryn's land. Cole is asleep under his own roof.
        deal(&mut game, ASH, "Bryn", Dealing::War);
        set_down(&mut game, ASH, site);
        let mut events = Vec::new();
        game.characters[2].stow(Resource::Kelp, 5);
        game.apply(&Command::rest(COLE), Tick::new(30), &mut events);
        let asleep = Tick::new(30) + SETTLE_TIME;
        assert_eq!(game.can_attack(ASH, COLE), Err(Refusal::Haven));
        assert_eq!(game.can_rob(ASH, COLE, asleep), Err(RobRefusal::Haven));
        // It is no roof of Ash's: she is as open here as anywhere at war.
        assert_eq!(game.can_attack(COLE, ASH), Ok(()));
        assert!(crate::narrate::place(&game, COLE, asleep).contains("nobody can strike or rob you"));
    }

    /// What the first workable place in `land` holds.
    fn stock_in(game: &Game, land: PlaceId) -> u32 {
        let zone = game.zone_at(land);
        zone.place_ids().find_map(|place| game.map().place(place).and_then(|data| data.deposit)).unwrap().node.stock()
    }

    /// Empties the first workable place in `land`.
    fn strip(game: &mut Game, land: PlaceId) {
        let place = game.zone_at(land).place_ids().find(|&place| game.map().place(place).is_some_and(|data| data.deposit.is_some()));
        let deposit = game.map.place_mut(place.unwrap()).and_then(|data| data.deposit.as_mut()).unwrap();
        deposit.node.scour(100);
    }

    #[test]
    fn lands_that_never_deal_leave_the_world_as_it_is() {
        let mut game = founders_met();
        let events = ticks(&mut game, 1, 7_200);
        assert_eq!((game.balance().lean(), game.balance().mood()), (0, Mood::Even));
        assert!(!events.iter().any(|event| matches!(event, Event::MoodChanged { .. } | Event::Storm)));
    }

    #[test]
    fn too_much_peace_dries_the_world_until_nothing_grows_back() {
        let mut game = founders_met();
        let home = game.character(ASH).unwrap().home();
        deal(&mut game, ASH, "Bryn", Dealing::Ally);
        deal(&mut game, BRYN, "Ash", Dealing::Ally);
        assert_eq!(game.balance().lean(), -LEAN_PER_DEALING, "an alliance is felt at once");

        // One alliance and no war: a step toward stillness every hour.
        let ten = BALANCE_EVERY.as_ticks() * 10;
        let events = ticks(&mut game, 11, ten);
        assert_eq!((game.balance().lean(), game.balance().mood()), (-20, Mood::Dry));
        let dried = Event::MoodChanged { from: Mood::Even, to: Mood::Dry };
        assert!(events.contains(&dried));
        assert_eq!(game.audience(&dried), [ASH, BRYN, COLE], "the whole world feels it");

        // Dry, a stripped place comes back at half the pace. Parched, not at all.
        let regrown = |game: &Game, lean: i64| {
            let mut game = game.clone();
            game.balance.tip(lean - game.balance.lean());
            strip(&mut game, home);
            ticks(&mut game, ten + 1, ten + 300);
            stock_in(&game, home)
        };
        let (even, dry, parched) = (regrown(&game, 0), regrown(&game, -20), regrown(&game, -60));
        assert!(even > 0 && dry * 2 <= even + 1 && dry > 0, "even {even}, dry {dry}");
        assert_eq!(parched, 0);
    }

    #[test]
    fn too_much_war_brings_storms_and_a_truce_lets_the_world_settle() {
        let mut game = founders_met();
        let home = game.character(ASH).unwrap().home();
        let mut untroubled = game.clone();
        let ten = BALANCE_EVERY.as_ticks() * 10;
        let stormy = ten + STORM_EVERY.as_ticks() * 2;
        ticks(&mut untroubled, 11, stormy);
        deal(&mut game, ASH, "Bryn", Dealing::War);
        assert_eq!(game.balance().lean(), LEAN_PER_DEALING);
        let events = ticks(&mut game, 11, ten);
        assert_eq!(game.balance().mood(), Mood::Restless);
        assert!(events.contains(&Event::MoodChanged { from: Mood::Even, to: Mood::Restless }));

        // The next storm takes a share of everything, in every land.
        let storm = ticks(&mut game, ten + 1, stormy);
        assert!(storm.contains(&Event::Storm));
        assert!(stock_in(&game, home) < stock_in(&untroubled, home), "a deposit nobody touched is the poorer for it");

        // Both founders ask for a truce. Peace is felt at once, and with
        // nothing pushing any more the world eases all the way back.
        deal(&mut game, ASH, "Bryn", Dealing::Truce);
        let truce = deal(&mut game, BRYN, "Ash", Dealing::Truce);
        assert!(truce.contains(&Event::MoodChanged { from: Mood::Restless, to: Mood::Even }));
        let from = stormy + 1;
        let after = ticks(&mut game, from, from + BALANCE_EVERY.as_ticks() * 20);
        assert_eq!(game.balance().lean(), 0);
        assert!(!after.contains(&Event::Storm));
    }

    #[test]
    fn a_roused_world_batters_whoever_is_caught_out_and_rouses_its_creatures() {
        let mut game = founders_met();
        let (ashland, brynsland) = (game.character(ASH).unwrap().home(), game.character(BRYN).unwrap().home());
        game.balance.tip(100);
        // Ash and Bryn stand on the Meeting Ground; Cole is at home in the haven.
        let at = STORM_EVERY.as_ticks();
        let storm = ticks(&mut game, at, at);
        let battered = |who| storm.iter().any(|event| matches!(event, Event::Character { who: hit, outcome: Outcome::Battered { .. } } if *hit == who));
        assert!(battered(ASH) && battered(BRYN) && !battered(COLE));
        assert!((85..=86).contains(&game.character(ASH).unwrap().health()), "fifteen, less a moment's mending");
        assert_eq!(game.character(COLE).unwrap().health(), 100);
        // However many storms, the last of someone's health is never the storm's.
        game.characters[0].hurt(80);
        ticks(&mut game, at * 2, at * 2);
        let ash = game.character(ASH).unwrap();
        assert!(!ash.is_spirit() && ash.health() <= 2, "{}", ash.health());

        // A lurker in a raging world hits half as hard again.
        let lair = |game: &Game| game.zone_at(brynsland).place_ids().find(|&place| game.map().place(place).unwrap().lair == Some(CreatureKind::Lurker)).unwrap();
        let first_blow = |lean: i64| {
            let mut game = founders_met();
            game.balance.tip(lean);
            let den = lair(&game);
            set_down(&mut game, COLE, den);
            let blows = ticks(&mut game, 1, 30);
            blows.iter().find_map(|event| match event {
                Event::Character { who: COLE, outcome: Outcome::Mauled { damage, .. } } => Some(*damage),
                _ => None,
            })
        };
        let (even, raging) = (first_blow(0).unwrap(), first_blow(100).unwrap());
        assert!(raging * 10 >= even * 14, "even {even}, raging {raging}");
        let _ = ashland;
    }

    #[test]
    fn a_parched_world_cannot_keep_its_creatures_and_stone_does_not_care() {
        let mut game = founders_met();
        let kinds = |game: &Game| {
            let count = |kind| game.creatures.iter().filter(|creature| creature.being.kind == kind).count();
            (count(CreatureKind::Lurker), count(CreatureKind::Sentinel))
        };
        assert_eq!(kinds(&game), (3, 2), "a lurker in the grotto and one in each land; a sentinel in each pass");
        game.balance.tip(-100);
        let died = ticks(&mut game, 1, 1);
        assert_eq!(kinds(&game), (0, 2));
        assert_eq!(died.iter().filter(|event| matches!(event, Event::CreatureGone { .. })).count(), 3);
        // None come back while it lasts.
        ticks(&mut game, 2, LURKER_RETURNS.as_ticks() * 2);
        assert_eq!(kinds(&game).0, 0);
        // When the world can keep them again, they do.
        game.balance.tip(60);
        let from = LURKER_RETURNS.as_ticks() * 2 + 1;
        ticks(&mut game, from, from + UPKEEP_EVERY.as_ticks());
        assert_eq!(kinds(&game), (3, 2));
    }

    #[test]
    fn a_guard_is_paid_when_client_and_guard_stand_where_the_client_was_going() {
        let mut game = founders_met();
        let (ground, home) = (game.meeting_ground().unwrap(), game.character(COLE).unwrap().home());
        let reward = Lot { resource: Resource::Ore, amount: 40 };
        game.characters[2].stow(Resource::Ore, 50);
        let act = |game: &mut Game, command: Command| {
            let mut events = Vec::new();
            game.apply(&command, Tick::new(20), &mut events);
            game.characters.iter_mut().for_each(|character| character.busy_until = Tick::ZERO);
            events
        };
        let refused = |events: &[Event]| match outcome(events) {
            Some(Outcome::CannotTrade { why }) => Some(*why),
            _ => None,
        };

        // Cole, at home, wants seeing to the Meeting Ground.
        assert_eq!(refused(&act(&mut game, Command::hire(COLE, reward, "nowhere"))), Some(TradeRefusal::NoSuchPlace));
        assert_eq!(refused(&act(&mut game, Command::hire(COLE, Lot { amount: 500, ..reward }, "ground"))), Some(TradeRefusal::CannotPay));
        assert_eq!(outcome(&act(&mut game, Command::hire(COLE, reward, "ground"))), Some(&Outcome::Hiring { reward, to: ground }));
        assert_eq!(refused(&act(&mut game, Command::hire(COLE, reward, "ash"))), Some(TradeRefusal::AlreadyHiring));
        assert_eq!(game.character(COLE).unwrap().carried(Resource::Ore), 10, "the board holds the reward");
        assert!(crate::narrate::board(&game, COLE).contains("Cole wants seeing safely to The Meeting Ground"));

        // Bryn is not there to take it, and Cole cannot guard Cole.
        assert_eq!(refused(&act(&mut game, Command::guard(BRYN, COLE))), Some(TradeRefusal::NotHiring));
        assert_eq!(refused(&act(&mut game, Command::guard(COLE, COLE))), Some(TradeRefusal::NotHiring));
        set_down(&mut game, BRYN, home);
        let taken = act(&mut game, Command::guard(BRYN, COLE));
        assert_eq!(taken, [
            Event::Character { who: BRYN, outcome: Outcome::Guarding { client: COLE, reward, to: ground } },
            Event::Character { who: COLE, outcome: Outcome::Guarded { by: BRYN } },
        ]);
        assert_eq!(refused(&act(&mut game, Command::guard(ASH, COLE))), Some(TradeRefusal::NotHiring), "it is taken");

        // One of them there is not enough. Both, and Bryn is paid on the spot.
        set_down(&mut game, COLE, ground);
        assert!(ticks(&mut game, 21, 21).is_empty());
        set_down(&mut game, BRYN, ground);
        let done = ticks(&mut game, 22, 22);
        assert!(done.contains(&Event::Character { who: COLE, outcome: Outcome::EscortOver { paid: true } }));
        assert!(done.contains(&Event::Character { who: BRYN, outcome: Outcome::EscortDone { client: COLE, reward: Some(reward) } }));
        assert_eq!(game.character(BRYN).unwrap().carried(Resource::Ore), 40);
        assert!(game.escorts().is_empty());
    }

    #[test]
    fn an_escort_that_comes_to_nothing_sends_the_reward_back_to_its_board() {
        let mut start = founders_met();
        let home = start.character(COLE).unwrap().home();
        let reward = Lot { resource: Resource::Kelp, amount: 25 };
        start.characters[2].stow(Resource::Kelp, 25);
        set_down(&mut start, BRYN, home);
        let mut events = Vec::new();
        start.apply(&Command::hire(COLE, reward, "ground"), Tick::new(20), &mut events);
        start.apply(&Command::guard(BRYN, COLE), Tick::new(20), &mut events);
        assert_eq!(start.escorts()[0].guard, Some(BRYN));
        start.characters.iter_mut().for_each(|character| character.busy_until = Tick::ZERO);
        let owed = |game: &Game| game.zone_at(home).board().owed_to(COLE)[Resource::Kelp.index()];

        // Cole thinks better of it.
        let mut game = start.clone();
        events.clear();
        game.apply(&Command::dismiss(COLE), Tick::new(21), &mut events);
        assert!(events.contains(&Event::Character { who: COLE, outcome: Outcome::EscortOver { paid: false } }));
        assert!(events.contains(&Event::Character { who: BRYN, outcome: Outcome::EscortDone { client: COLE, reward: None } }));
        assert_eq!((owed(&game), game.escorts().len()), (25, 0));

        // The guard dies: the job is open again, and Ash can take it.
        let mut game = start.clone();
        game.fell(1, Cause::Character(ASH), 0, Tick::new(21), &mut Vec::new());
        let lost = ticks(&mut game, 22, 22);
        assert!(lost.contains(&Event::Character { who: COLE, outcome: Outcome::GuardLost }));
        assert_eq!(game.escorts()[0].guard, None);

        // The client dies: nobody is paid, and the reward goes home.
        let mut game = start.clone();
        game.fell(2, Cause::Character(ASH), 0, Tick::new(21), &mut Vec::new());
        let over = ticks(&mut game, 22, 22);
        assert!(over.contains(&Event::Character { who: BRYN, outcome: Outcome::EscortDone { client: COLE, reward: None } }));
        assert_eq!((owed(&game), game.character(BRYN).unwrap().carried(Resource::Kelp)), (25, 0));
    }

    #[test]
    fn the_era_turns_with_war_and_with_the_state_of_the_lands() {
        let mut game = founders_met();
        assert_eq!((game.era(), game.house_cost()), (Era::Rebuilding, [(Resource::Driftwood, 30), (Resource::Ore, 15)]));

        // War makes it an era of conflict, known to everyone, and fighting teaches double.
        deal(&mut game, ASH, "Bryn", Dealing::War);
        let turned = ticks(&mut game, 11, BALANCE_EVERY.as_ticks());
        let change = Event::EraChanged { from: Era::Rebuilding, to: Era::Conflict };
        assert!(turned.contains(&change));
        assert_eq!(game.audience(&change), [ASH, BRYN, COLE]);
        assert_eq!(game.house_cost(), crate::content::HOUSE_COST);
        let learned = |game: &Game| {
            let mut game = game.clone();
            let before = game.character(ASH).unwrap().skill(SkillKind::Fighting).progress();
            game.apply(&Command::attack(ASH, BRYN), Tick::new(BALANCE_EVERY.as_ticks() + 1), &mut Vec::new());
            game.character(ASH).unwrap().skill(SkillKind::Fighting).progress() - before
        };
        let at_war = learned(&game);

        // A truce, and it is rebuilding again.
        deal(&mut game, ASH, "Bryn", Dealing::Truce);
        deal(&mut game, BRYN, "Ash", Dealing::Truce);
        let from = BALANCE_EVERY.as_ticks() + 1;
        let healed = ticks(&mut game, from, BALANCE_EVERY.as_ticks() * 2);
        assert!(healed.contains(&Event::EraChanged { from: Era::Conflict, to: Era::Rebuilding }));
        // The Meeting Ground is contested, so the same blow can still be struck.
        assert_eq!(at_war, learned(&game) * 2);

        // Lands failing everywhere: the Void. Nothing put down stays down.
        let from = BALANCE_EVERY.as_ticks() * 2 + 1;
        let mut fell = ticks(&mut game, from, BALANCE_EVERY.as_ticks() * 3 - 60);
        for zone in &mut game.zones {
            zone.mourn(100);
        }
        fell.extend(ticks(&mut game, BALANCE_EVERY.as_ticks() * 3 - 59, BALANCE_EVERY.as_ticks() * 3));
        assert!(fell.iter().any(|event| matches!(event, Event::EraChanged { to: Era::VoidConsumption, .. })), "{:?}", game.era());
    }

    #[test]
    fn a_world_without_founders_has_no_meeting_ground() {
        let game = Game::new(1, &["Ash"]);
        assert_eq!((game.meeting_ground(), game.zones().len()), (None, 1));
    }

    #[test]
    fn someone_new_cannot_get_past_a_sentinel() {
        let mut game = two_founders();
        let pass = pass_of(&game, ASH);
        let mut now = walk_to(&mut game, ASH, pass, 1);
        let sentinel = game.creatures_at(pass).next().map(|creature| creature.id);
        let mut events = Vec::new();
        // Whether or not the lurker on the way got her first, she fights
        // whatever bars the pass for as long as she lives.
        while !game.character(ASH).unwrap().is_spirit() && now < 3_000 {
            if let Some(sentinel) = sentinel {
                game.apply(&Command::fight(ASH, sentinel), Tick::new(now), &mut events);
            }
            ticks(&mut game, now + 1, now + ATTACK_TIME.as_ticks());
            now += ATTACK_TIME.as_ticks();
        }
        assert!(game.character(ASH).unwrap().is_spirit(), "nobody new walks out of their land");
        assert_eq!(game.occupants(game.meeting_ground().unwrap()), []);
    }

    /// Raises a character's Vigor and Resilience by working ore, far
    /// from anything that could interrupt.
    fn hardened(game: &mut Game, who: CharacterId, level: u32) {
        let index = game.index_of(who).unwrap();
        let mut outcomes = Vec::new();
        while game.characters[index].attribute(Attribute::Vigor).level() < level
            || game.characters[index].attribute(Attribute::Resilience).level() < level
        {
            game.characters[index].train(Resource::Ore, 12, &mut outcomes);
        }
    }

    #[test]
    fn someone_strong_enough_breaks_out_alone_and_the_pass_closes_behind_them() {
        let mut game = two_founders();
        hardened(&mut game, ASH, 75);
        let pass = pass_of(&game, ASH);
        // Go round the lurker if there is one on the way: start her at the place before the pass.
        let before = game.map().exits(pass)[0].to;
        let index = game.index_of(ASH).unwrap();
        game.occupancy.leave(game.characters[index].place, ASH);
        game.characters[index].place = before;
        game.occupancy.enter(before, ASH);
        for creature in game.creatures.iter().filter(|creature| creature.place == before).map(|creature| creature.id).collect::<Vec<_>>() {
            game.creatures.retain(|other| other.id != creature);
        }

        let mut now = walk_to(&mut game, ASH, pass, 1);
        let sentinel = game.creatures_at(pass).next().unwrap().id;
        let mut events = Vec::new();
        while game.creatures_at(pass).next().is_some() {
            game.apply(&Command::fight(ASH, sentinel), Tick::new(now), &mut events);
            ticks(&mut game, now + 1, now + ATTACK_TIME.as_ticks());
            now += ATTACK_TIME.as_ticks();
            assert!(!game.character(ASH).unwrap().is_spirit(), "she should be strong enough");
        }
        let being = Being { kind: CreatureKind::Sentinel, of: None };
        assert!(events.contains(&Event::Character { who: ASH, outcome: Outcome::Slew { being } }));

        game.apply(&Command::go(ASH, "beyond"), Tick::new(now), &mut events);
        let ground = game.meeting_ground().unwrap();
        assert_eq!(game.character(ASH).unwrap().place(), ground);
        assert_eq!(game.character(ASH).unwrap().home(), game.zones()[3].anchor(), "her home is still her own land");

        // In time another sentinel bars the pass.
        let later = ticks(&mut game, now + 1, now + LURKER_RETURNS.as_ticks() + 10);
        assert!(later.contains(&Event::CreatureAppeared { place: pass, being }));
    }

    #[test]
    fn two_who_break_out_meet_on_ground_where_anyone_may_be_struck() {
        let mut game = two_founders();
        let ground = game.meeting_ground().unwrap();
        for who in [ASH, BRYN] {
            let index = game.index_of(who).unwrap();
            game.occupancy.leave(game.characters[index].place, who);
            game.characters[index].place = ground;
            game.occupancy.enter(ground, who);
        }
        assert_eq!(game.occupants(ground), [ASH, BRYN]);
        assert_eq!(game.can_attack(ASH, BRYN), Ok(()), "nobody is marked, but this is contested ground");

        // Or they can work it together: it is the richest ground there is.
        let mut events = Vec::new();
        game.apply(&Command::gather(ASH), Tick::new(1), &mut events);
        game.apply(&Command::gather(BRYN), Tick::new(1), &mut events);
        assert!(game.character(ASH).unwrap().carried(Resource::Tideglass) > 0);
        assert!(game.character(BRYN).unwrap().carried(Resource::Tideglass) > 0);

        // Whoever dies here, alone, is pulled to the Meeting Ground's own anchor, and reborn at home.
        let mut now = 10;
        while !game.character(BRYN).unwrap().is_spirit() {
            game.apply(&Command::attack(ASH, BRYN), Tick::new(now), &mut events);
            now += ATTACK_TIME.as_ticks();
            if game.character(ASH).unwrap().is_spirit() {
                return;
            }
        }
        game.apply(&Command::relinquish(BRYN), Tick::new(now), &mut events);
        assert_eq!(game.character(BRYN).unwrap().place(), game.character(BRYN).unwrap().home());
    }

    /// How a party of `size`, each hardened to `level`, fares against a
    /// sentinel: whether it fell, and how many of them died.
    fn party_against_a_sentinel(size: u64, level: u32) -> (bool, usize) {
        let mut game = Game::new_world(1, 1, &[]);
        let mut events = Vec::new();
        game.apply(&Command::join(ASH, "Ash"), Tick::new(0), &mut events);
        let names = ["Bryn", "Cole", "Dara"];
        for extra in 1..size {
            game.apply(&Command::invite(ASH, names[extra as usize - 1]), Tick::new(0), &mut events);
            game.apply(&Command::join(CharacterId::new(extra), names[extra as usize - 1]), Tick::new(0), &mut events);
        }
        let pass = pass_of(&game, ASH);
        let party: Vec<CharacterId> = (0..size).map(CharacterId::new).collect();
        for &who in &party {
            hardened(&mut game, who, level);
            let index = game.index_of(who).unwrap();
            game.occupancy.leave(game.characters[index].place, who);
            game.characters[index].place = pass;
            game.occupancy.enter(pass, who);
        }
        let sentinel = game.creatures_at(pass).next().unwrap().id;
        let mut now = 10;
        while game.creatures_at(pass).next().is_some() && party.iter().any(|&who| !game.character(who).unwrap().is_spirit()) {
            for &who in &party {
                game.apply(&Command::fight(who, sentinel), Tick::new(now), &mut events);
            }
            ticks(&mut game, now + 1, now + ATTACK_TIME.as_ticks());
            now += ATTACK_TIME.as_ticks();
        }
        let dead = party.iter().filter(|&&who| game.character(who).unwrap().is_spirit()).count();
        let fell = game.creatures_at(pass).next().is_none();
        (fell, dead)
    }

    #[test]
    #[ignore = "prints a table for tuning"]
    fn print_who_gets_past_a_sentinel() {
        // The library has no standard output, so the table is the panic.
        let mut table = alloc::string::String::new();
        for size in 1..=4 {
            for level in [0, 15, 30, 45, 60, 75, 90] {
                table.push_str(&alloc::format!("{size}@{level}:{:?} ", party_against_a_sentinel(size, level)));
            }
            table.push('|');
        }
        panic!("{table}");
    }

    #[test]
    fn the_pass_is_a_matter_of_strength_or_of_company() {
        assert_eq!(party_against_a_sentinel(1, 0), (false, 1), "alone and new: no");
        assert_eq!(party_against_a_sentinel(4, 0), (false, 4), "four who are new: no, their blows glance off");
        assert_eq!(party_against_a_sentinel(1, 60), (false, 1), "alone and well grown: not yet");
        assert_eq!(party_against_a_sentinel(1, 75), (true, 0), "alone and strong: yes");
        assert_eq!(party_against_a_sentinel(2, 45), (true, 1), "two half-grown: yes, and it costs one of them");
        assert_eq!(party_against_a_sentinel(2, 60), (true, 0), "two well grown: yes");
        assert_eq!(party_against_a_sentinel(3, 30), (true, 1), "three part-grown: yes, and it costs one of them");
    }
}
