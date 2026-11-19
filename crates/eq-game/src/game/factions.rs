//! The rules for factions: charters, vaults, markers and oaths.
//!
//! What a faction gives its members: warning when anyone else sets foot
//! on ground it holds, sleep there that nobody can rob, a quicker road
//! out of it, and shelter at its keep whatever land they come from.
//! What it costs them: a vault that must pay for every marker every
//! day, markers anyone can break, and an oath that is not lightly
//! broken.

use alloc::string::String;
use alloc::vec::Vec;

use eq_clock::Tick;
use eq_world_graph::PlaceId;

use super::{id_at, Game};
use crate::board::Lot;
use crate::character::CharacterId;
use crate::content::{
    self, BREAK_TIME, CLAIM_TIME, MARKER_COST, OATHBREAKER_LASTS, SIGN_TIME, TAINT_FOR_ROBBING, UPKEEP_EVERY,
    UPKEEP_PER_CLAIM, VAULT_SPILLS_PERCENT,
};
use crate::creature::Relic;
use crate::event::{Event, Outcome};
use crate::faction::{NOBODY, Dissolution, Faction, FactionId, FactionNews, FactionRefusal, Factions};
use crate::types::Safety;

impl Game {
    /// Every faction and charter in the world.
    pub const fn factions(&self) -> &Factions {
        &self.factions
    }

    /// Whether a house stands at `place`.
    fn house_at(&self, place: PlaceId) -> bool {
        self.zones.iter().any(|zone| zone.hall() == Some(place))
    }

    /// Whether `who` stands at the keep of the faction they are sworn to,
    /// where nobody can strike or rob them.
    pub fn at_own_keep(&self, who: CharacterId) -> bool {
        let Some(character) = self.character(who) else { return false };
        self.factions.of(who).is_some_and(|faction| faction.is_formed() && faction.keep == character.place())
    }

    /// Why `who` may not take an oath now, if they may not.
    fn may_swear(&self, who: CharacterId) -> Result<(), FactionRefusal> {
        if self.factions.oathbreaker(who).is_some() {
            Err(FactionRefusal::Oathbreaker)
        } else if self.factions.of(who).is_some() {
            Err(FactionRefusal::Sworn)
        } else {
            Ok(())
        }
    }

    /// Opens a charter at a house, signed first by whoever opens it.
    pub(super) fn charter(&mut self, index: usize, raw: &str, now: Tick, outcomes: &mut Vec<Outcome>) {
        let who = id_at(index);
        let here = self.characters[index].place();
        let checked = self.may_swear(who).and_then(|()| {
            if !self.house_at(here) {
                return Err(FactionRefusal::NotAtAHouse);
            }
            let taken = self.factions.all().iter().map(|faction| faction.name.as_str());
            eq_naming::check(raw, &content::LAND_NAME_RULES, taken, &content::BLOCKED_NAMES).map_err(FactionRefusal::Name)
        });
        match checked {
            Ok(name) => {
                let blood = self.characters[index].bloodline();
                self.factions.open(name.clone(), here, who, blood, now);
                self.characters[index].busy_until = now + SIGN_TIME;
                outcomes.push(Outcome::CharterOpened { name });
            }
            Err(why) => outcomes.push(Outcome::CannotSwear { why }),
        }
    }

    /// Signs a charter at its keep.
    pub(super) fn sign(&mut self, index: usize, raw: &str, now: Tick, outcomes: &mut Vec<Outcome>, events: &mut Vec<Event>) {
        let who = id_at(index);
        let here = self.characters[index].place();
        let checked = self.may_swear(who).and_then(|()| {
            let faction = self.factions.named(raw).ok_or(FactionRefusal::NoSuchCharter)?;
            if faction.keep == here {
                Ok(faction.id)
            } else {
                Err(FactionRefusal::NotAtKeep)
            }
        });
        let id = match checked {
            Ok(id) => id,
            Err(why) => return outcomes.push(Outcome::CannotSwear { why }),
        };
        let blood = self.characters[index].bloodline();
        let was_formed = self.factions.get(id).is_some_and(Faction::is_formed);
        let formed = self.factions.sign(id, who, blood, now, UPKEEP_EVERY);
        self.characters[index].busy_until = now + SIGN_TIME;
        let Some(faction) = self.factions.get(id) else { return };
        outcomes.push(Outcome::Signed {
            name: faction.name.clone(),
            signatories: faction.signatories.len(),
            bloodlines: faction.bloodlines(),
        });
        let news = if formed {
            Some(FactionNews::Formed)
        } else if was_formed {
            Some(FactionNews::Joined(who))
        } else {
            None
        };
        if let Some(news) = news {
            events.push(faction_event(faction, news));
        }
    }

    /// Puts something into the vault of the faction `index` is sworn to.
    pub(super) fn give(&mut self, index: usize, lot: Lot, now: Tick, outcomes: &mut Vec<Outcome>) {
        let who = id_at(index);
        let here = self.characters[index].place();
        let checked = match self.factions.of(who) {
            None => Err(FactionRefusal::NotSworn),
            Some(faction) if faction.keep != here => Err(FactionRefusal::NotAtKeep),
            Some(_) if lot.amount == 0 || self.characters[index].carried(lot.resource) < lot.amount => {
                Err(FactionRefusal::NothingToGive)
            }
            Some(faction) => Ok(faction.id),
        };
        match checked {
            Ok(id) => {
                self.characters[index].spend(lot.resource, lot.amount);
                if let Some(faction) = self.factions.get_mut(id) {
                    faction.vault[lot.resource.index()] += lot.amount;
                }
                self.characters[index].busy_until = now + SIGN_TIME;
                outcomes.push(Outcome::Gave { lot });
            }
            Err(why) => outcomes.push(Outcome::CannotSwear { why }),
        }
    }

    /// Sets the marker of `index`'s faction where they stand, out of the vault.
    pub(super) fn claim(&mut self, index: usize, now: Tick, outcomes: &mut Vec<Outcome>, events: &mut Vec<Event>) {
        let who = id_at(index);
        let here = self.characters[index].place();
        let fit = self.map.place(here).is_some_and(|place| place.safety != Safety::Haven && place.lair.is_none());
        let (resource, cost) = MARKER_COST;
        let checked = match self.factions.of(who) {
            None => Err(FactionRefusal::NotSworn),
            Some(faction) if !faction.is_formed() => Err(FactionRefusal::NotFormed),
            Some(_) if !fit || self.house_at(here) => Err(FactionRefusal::CannotHoldHere),
            Some(_) if self.factions.holding(here).is_some() => Err(FactionRefusal::Held),
            Some(faction) if faction.held(resource) < cost => Err(FactionRefusal::VaultShort),
            Some(faction) => Ok(faction.id),
        };
        let id = match checked {
            Ok(id) => id,
            Err(why) => return outcomes.push(Outcome::CannotSwear { why }),
        };
        let Some(faction) = self.factions.get_mut(id) else { return };
        faction.vault[resource.index()] -= cost;
        faction.claims.push(crate::faction::Claim { place: here, breaking: None });
        events.push(faction_event(faction, FactionNews::Claimed(here)));
        self.characters[index].busy_until = now + CLAIM_TIME;
        outcomes.push(Outcome::Claimed);
    }

    /// Breaks `index`'s oath. Leaving a charter not yet formed costs
    /// nothing. Leaving a faction makes an oathbreaker of the one who
    /// leaves, whom those left behind may strike; if they were one of
    /// those who first signed it, the faction ends with their leaving.
    pub(super) fn forsake(&mut self, index: usize, now: Tick, outcomes: &mut Vec<Outcome>, events: &mut Vec<Event>) {
        let who = id_at(index);
        let Some(faction) = self.factions.of(who).cloned() else {
            return outcomes.push(Outcome::CannotSwear { why: FactionRefusal::NotSworn });
        };
        outcomes.push(Outcome::Forsook { name: faction.name.clone(), formed: faction.is_formed() });
        if !faction.is_formed() {
            self.factions.leave(who);
            return;
        }
        if faction.signed_first(who) {
            return self.dissolve(faction.id, Dissolution::Betrayed(who), now, events);
        }
        self.factions.leave(who);
        let wronged: Vec<CharacterId> = faction.members.iter().copied().filter(|&member| member != who).collect();
        self.factions.break_oath(who, now + OATHBREAKER_LASTS, wronged);
        events.push(faction_event(&faction, FactionNews::Left(who)));
    }

    /// Sets to breaking the marker where `index` stands. The land marks
    /// them at once, and the faction hears of it.
    pub(super) fn begin_breaking(&mut self, index: usize, now: Tick, outcomes: &mut Vec<Outcome>, events: &mut Vec<Event>) {
        let who = id_at(index);
        let here = self.characters[index].place();
        let checked = match self.factions.holding(here) {
            None => Err(FactionRefusal::NothingToBreak),
            Some(faction) if faction.has(who) => Err(FactionRefusal::OwnMarker),
            Some(faction) if faction.claims.iter().any(|claim| claim.place == here && claim.breaking.is_some()) => {
                Err(FactionRefusal::AlreadyBreaking)
            }
            Some(faction) => Ok(faction.id),
        };
        let id = match checked {
            Ok(id) => id,
            Err(why) => return outcomes.push(Outcome::CannotSwear { why }),
        };
        let done_at = now + BREAK_TIME;
        let Some(faction) = self.factions.get_mut(id) else { return };
        if let Some(claim) = faction.claims.iter_mut().find(|claim| claim.place == here) {
            claim.breaking = Some((who, done_at));
        }
        let name = faction.name.clone();
        events.push(faction_event(faction, FactionNews::Breaking { who, place: here }));
        let breaker = &mut self.characters[index];
        breaker.busy_until = done_at;
        outcomes.push(Outcome::BeganBreaking { faction: name, done_at });
        outcomes.extend(breaker.shift_taint(TAINT_FOR_ROBBING));
    }

    /// Whoever walked onto ground a faction holds, from another land and
    /// not sworn to it, is seen by everyone who is. People going about
    /// their own land are nothing to warn of.
    pub(super) fn warn_of_trespass(&self, index: usize, outcomes: &[Outcome], events: &mut Vec<Event>) {
        let who = id_at(index);
        let Some(Outcome::Moved { to, .. }) = outcomes.last() else { return };
        if self.zone_at(*to).contains(self.characters[index].home()) {
            return;
        }
        if let Some(faction) = self.factions.holding(*to).filter(|faction| !faction.has(who)) {
            events.push(faction_event(faction, FactionNews::Trespass { who, place: *to }));
        }
    }

    /// Markers give under those breaking them; vaults pay for their
    /// markers, or the faction ends; oathbreakers serve their time.
    pub(super) fn settle_factions(&mut self, now: Tick, events: &mut Vec<Event>) {
        self.factions.forgive(now);
        for (id, place, breaker, at) in self.factions.breakings() {
            let still_at_it = self.character(breaker).is_some_and(|character| !character.is_spirit() && character.place() == place);
            if !still_at_it {
                self.stop_breaking(id, place);
            } else if at <= now {
                self.marker_broken(id, place, breaker, now, events);
            }
        }
        let due: Vec<FactionId> = self
            .factions
            .all()
            .iter()
            .filter(|faction| faction.is_formed() && faction.upkeep_due <= now)
            .map(|faction| faction.id)
            .collect();
        for id in due {
            self.pay_upkeep(id, now, events);
        }
    }

    fn stop_breaking(&mut self, id: FactionId, place: PlaceId) {
        let claims = self.factions.get_mut(id).map(|faction| &mut faction.claims);
        if let Some(claim) = claims.and_then(|claims| claims.iter_mut().find(|claim| claim.place == place)) {
            claim.breaking = None;
        }
    }

    fn marker_broken(&mut self, id: FactionId, place: PlaceId, by: CharacterId, now: Tick, events: &mut Vec<Event>) {
        let Some(faction) = self.factions.get_mut(id) else { return };
        faction.claims.retain(|claim| claim.place != place);
        let (name, keep) = (faction.name.clone(), faction.keep);
        events.push(faction_event(faction, FactionNews::MarkerBroken { by, place }));
        let (resource, amount) = MARKER_COST;
        let spoils = Lot { resource, amount };
        if let Some(index) = self.index_of(by) {
            self.characters[index].stow(resource, amount);
        }
        events.push(Event::Character { who: by, outcome: Outcome::BrokeMarker { faction: name, spoils } });
        if place == keep {
            self.dissolve(id, Dissolution::Broken(by), now, events);
        }
    }

    fn pay_upkeep(&mut self, id: FactionId, now: Tick, events: &mut Vec<Event>) {
        let Some(faction) = self.factions.get_mut(id) else { return };
        let (resource, each) = UPKEEP_PER_CLAIM;
        let owed = each * faction.claims.len() as u64;
        if faction.held(resource) < owed {
            return self.dissolve(id, Dissolution::Exhausted, now, events);
        }
        faction.vault[resource.index()] -= owed;
        faction.upkeep_due = faction.upkeep_due + UPKEEP_EVERY;
        events.push(faction_event(faction, FactionNews::UpkeepPaid { amount: owed }));
    }

    /// Ends a faction. Half its vault spills at the keep for anyone to
    /// take; the rest is lost. Its members cannot swear again for a
    /// while, unless one of them betrayed it, in which case only the
    /// traitor pays, and those betrayed may strike them for it.
    pub(super) fn dissolve(&mut self, id: FactionId, how: Dissolution, now: Tick, events: &mut Vec<Event>) {
        let Some(faction) = self.factions.dissolve(id) else { return };
        let spilt = faction.vault.map(|held| held * VAULT_SPILLS_PERCENT / 100);
        self.relics.extend(Relic::new(NOBODY, faction.keep, spilt, now));
        let until = now + OATHBREAKER_LASTS;
        match how {
            Dissolution::Betrayed(traitor) => {
                let wronged = faction.members.iter().copied().filter(|&member| member != traitor).collect();
                self.factions.break_oath(traitor, until, wronged);
            }
            Dissolution::Exhausted | Dissolution::Broken(_) => {
                for &member in &faction.members {
                    self.factions.break_oath(member, until, Vec::new());
                }
            }
        }
        events.push(faction_event(&faction, FactionNews::Dissolved(how)));
    }
}

/// News of `faction`, for everyone sworn to it now.
fn faction_event(faction: &Faction, news: FactionNews) -> Event {
    let name: String = faction.name.clone();
    Event::Faction { name, members: faction.members.clone(), news }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Command;
    use crate::combat::{Refusal, RobRefusal};
    use crate::types::Resource;
    use eq_clock::Duration;

    const DAY: Duration = Duration::days(1);
    use eq_events::World;

    /// Founders of three lands (0, 1, 2), each with a friend (3, 4, 5),
    /// and one more of the first land (6) who signs nothing.
    const FOUNDERS: [&str; 3] = ["Ash", "Bryn", "Cole"];
    const FRIENDS: [&str; 4] = ["Dov", "Eli", "Fay", "Gus"];
    const OUTSIDER: CharacterId = CharacterId::new(6);

    fn id(n: u64) -> CharacterId {
        CharacterId::new(n)
    }

    fn put(game: &mut Game, who: CharacterId, place: PlaceId) {
        let index = game.index_of(who).unwrap();
        game.occupancy.leave(game.characters[index].place, who);
        game.characters[index].place = place;
        game.characters[index].busy_until = Tick::ZERO;
        game.occupancy.enter(place, who);
    }

    fn act(game: &mut Game, command: Command, now: u64) -> Vec<Event> {
        let mut events = Vec::new();
        game.apply(&command, Tick::new(now), &mut events);
        events
    }

    fn own(events: &[Event], who: CharacterId) -> Option<Outcome> {
        events.iter().find_map(|event| match event {
            Event::Character { who: actor, outcome } if *actor == who => Some(outcome.clone()),
            _ => None,
        })
    }

    fn news(events: &[Event]) -> Vec<FactionNews> {
        events.iter().filter_map(|event| if let Event::Faction { news, .. } = event { Some(*news) } else { None }).collect()
    }

    fn refused(why: FactionRefusal) -> Option<Outcome> {
        Some(Outcome::CannotSwear { why })
    }

    /// Everyone standing in a house raised in Ash's land, at a place
    /// with a neighbour that is neither a haven nor a lair.
    fn at_a_house() -> (Game, PlaceId) {
        let mut game = Game::new_world(5, FOUNDERS.len(), &[]);
        for (n, name) in FOUNDERS.iter().enumerate() {
            act(&mut game, Command::join(id(n as u64), name), 0);
        }
        // Dov and Gus to Ash's land, Eli to Bryn's, Fay to Cole's.
        for (n, (host, name)) in [(0, FRIENDS[0]), (1, FRIENDS[1]), (2, FRIENDS[2]), (0, FRIENDS[3])].into_iter().enumerate() {
            act(&mut game, Command::invite(id(host), name), 0);
            act(&mut game, Command::join(id(3 + n as u64), name), 0);
        }
        let land = game.zone_index(game.character(id(0)).unwrap().home());
        let open = |game: &Game, place: PlaceId| {
            game.map.place(place).is_some_and(|data| data.safety != Safety::Haven && data.lair.is_none())
        };
        let keep = game
            .map
            .places()
            .map(|(place, _)| place)
            .find(|&place| {
                game.zones[land].contains(place) && open(&game, place) && game.map.exits(place).iter().any(|exit| open(&game, exit.to))
            })
            .unwrap();
        game.zones[land].hall = Some(keep);
        for n in 0..7 {
            put(&mut game, id(n), keep);
        }
        (game, keep)
    }

    /// A formed faction, Saltward, of the five signatories 0 to 4.
    fn formed() -> (Game, PlaceId) {
        let (mut game, keep) = at_a_house();
        act(&mut game, Command::charter(id(0), "Saltward"), 1);
        for n in 1..5 {
            act(&mut game, Command::sign(id(n), "saltward"), 1);
        }
        assert!(game.factions().named("Saltward").unwrap().is_formed());
        for character in &mut game.characters {
            character.busy_until = Tick::ZERO;
        }
        (game, keep)
    }

    fn neighbour(game: &Game, keep: PlaceId) -> (PlaceId, String) {
        let exit = game
            .map
            .exits(keep)
            .iter()
            .find(|exit| game.map.place(exit.to).is_some_and(|data| data.safety != Safety::Haven && data.lair.is_none()))
            .unwrap();
        let back = game.map.exits(exit.to).iter().find(|back| back.to == keep).unwrap().label.clone();
        (exit.to, back)
    }

    #[test]
    fn a_charter_is_opened_in_a_house_and_formed_by_five_of_three_bloodlines() {
        let (mut game, keep) = at_a_house();
        let bloods: Vec<_> = (0..5).map(|n| game.character(id(n)).unwrap().bloodline()).collect();
        assert_eq!((bloods[0], bloods[1], bloods[2]), (bloods[3], bloods[4], bloods[2]), "friends share their founder's blood");

        let haven = game.character(id(0)).unwrap().home();
        put(&mut game, id(0), haven);
        assert_eq!(own(&act(&mut game, Command::charter(id(0), "Saltward"), 1), id(0)), refused(FactionRefusal::NotAtAHouse));
        put(&mut game, id(0), keep);
        let opened = act(&mut game, Command::charter(id(0), "Saltward"), 1);
        assert_eq!(own(&opened, id(0)), Some(Outcome::CharterOpened { name: "Saltward".into() }));
        assert_eq!(own(&act(&mut game, Command::charter(id(0), "Tidewall"), 20), id(0)), refused(FactionRefusal::Sworn));
        assert_eq!(own(&act(&mut game, Command::sign(id(1), "Tidewall"), 1), id(1)), refused(FactionRefusal::NoSuchCharter));

        // Four from two bloodlines are not enough, however many sign.
        for n in [3, 1, 4] {
            let signed = act(&mut game, Command::sign(id(n), "saltward"), 1);
            assert!(news(&signed).is_empty());
        }
        let (_, place) = (keep, neighbour(&game, keep).0);
        put(&mut game, id(2), place);
        assert_eq!(own(&act(&mut game, Command::sign(id(2), "Saltward"), 1), id(2)), refused(FactionRefusal::NotAtKeep));
        put(&mut game, id(2), keep);
        // Cole is of a third blood, and makes five.
        let formed = act(&mut game, Command::sign(id(2), "Saltward"), 1);
        assert_eq!(own(&formed, id(2)), Some(Outcome::Signed { name: "Saltward".into(), signatories: 5, bloodlines: 3 }));
        assert_eq!(news(&formed), [FactionNews::Formed]);
        assert_eq!(game.audience(formed.last().unwrap()), [id(0), id(1), id(2), id(3), id(4)]);
        assert_eq!(game.factions().holding(keep).map(|faction| faction.name.as_str()), Some("Saltward"));

        // After that, anyone may sign, and is sworn.
        let joined = act(&mut game, Command::sign(id(5), "Saltward"), 1);
        assert_eq!(news(&joined), [FactionNews::Joined(id(5))]);
        assert!(crate::narrate::faction(&game, id(5)).contains("You are sworn to Saltward"));
    }

    #[test]
    fn the_keep_shelters_its_members_from_any_land_and_held_ground_warns_and_keeps_sleepers() {
        let (mut game, keep) = formed();
        // Cole is of another land, and not under his own roof: the keep shelters him.
        game.characters[2].shift_taint(100);
        assert_eq!(game.can_attack(OUTSIDER, id(2)), Err(Refusal::Haven));
        // Nobody else gets that: Fay is of Cole's land and not sworn.
        game.characters[5].shift_taint(100);
        assert_eq!(game.can_attack(OUTSIDER, id(5)), Ok(()));

        // Hold the next place too, and see who walks in.
        let (next, back) = neighbour(&game, keep);
        act(&mut game, Command::give(id(0), Lot { resource: Resource::Ore, amount: 0 }), 1);
        game.characters[0].stow(Resource::Ore, 100);
        let gave = act(&mut game, Command::give(id(0), Lot { resource: Resource::Ore, amount: 100 }), 1);
        assert_eq!(own(&gave, id(0)), Some(Outcome::Gave { lot: Lot { resource: Resource::Ore, amount: 100 } }));
        put(&mut game, id(1), next);
        let claimed = act(&mut game, Command::claim(id(1)), 30);
        assert_eq!(own(&claimed, id(1)), Some(Outcome::Claimed));
        assert_eq!(news(&claimed), [FactionNews::Claimed(next)]);
        assert_eq!(game.factions().named("Saltward").unwrap().held(Resource::Ore), 80);
        assert_eq!(own(&act(&mut game, Command::claim(id(1)), 200), id(1)), refused(FactionRefusal::Held));

        let to_next = game.map.exits(keep).iter().find(|exit| exit.to == next).unwrap().label.clone();
        // Gus is of this land, going about it: nothing to warn of.
        put(&mut game, OUTSIDER, keep);
        assert!(news(&act(&mut game, Command::go(OUTSIDER, &to_next), 400)).is_empty());
        // Fay is from Cole's land, and sworn to nothing here.
        put(&mut game, id(5), keep);
        let walked = act(&mut game, Command::go(id(5), &to_next), 400);
        assert_eq!(news(&walked), [FactionNews::Trespass { who: id(5), place: next }]);
        assert!(game.audience(walked.last().unwrap()).contains(&id(4)));
        // A member walking in is nothing to warn of.
        put(&mut game, id(4), keep);
        assert!(news(&act(&mut game, Command::go(id(4), &to_next), 500)).is_empty());

        // Asleep on held ground, a member keeps what they carry.
        game.characters[1].stow(Resource::Kelp, 10);
        act(&mut game, Command::rest(id(1)), 600);
        let asleep = Tick::new(700);
        assert_eq!(game.can_rob(OUTSIDER, id(1), asleep), Err(RobRefusal::Haven));
        game.characters[OUTSIDER.get() as usize].stow(Resource::Kelp, 10);
        act(&mut game, Command::rest(OUTSIDER), 600);
        assert_eq!(game.can_rob(id(4), OUTSIDER, asleep), Ok(()), "the ground keeps only its own");

        // And members travel out of it twice as fast.
        put(&mut game, id(3), next);
        act(&mut game, Command::go(id(3), &back), 800);
        put(&mut game, id(5), next);
        act(&mut game, Command::go(id(5), &back), 800);
        let (sworn, stranger) = (game.characters[3].busy_until(), game.characters[5].busy_until());
        assert_eq!((sworn.get() - 800) * 2, stranger.get() - 800);
    }

    #[test]
    fn the_vault_pays_for_its_markers_each_day_and_a_vault_that_cannot_ends_the_faction() {
        let (mut game, keep) = formed();
        let saltward = game.factions().named("Saltward").unwrap().id;
        assert_eq!(own(&act(&mut game, Command::claim(id(1)), 2), id(1)), refused(FactionRefusal::CannotHoldHere), "the keep is held already");
        // One day's upkeep of the keep alone, and no more.
        game.characters[0].stow(Resource::Ore, 15);
        act(&mut game, Command::give(id(0), Lot { resource: Resource::Ore, amount: 15 }), 2);
        let due = game.factions().get(saltward).unwrap().upkeep_due.get();
        let mut events = Vec::new();
        for tick in 3..=due {
            game.tick(Tick::new(tick), &mut events);
        }
        assert_eq!(news(&events), [FactionNews::UpkeepPaid { amount: 10 }]);
        assert_eq!(game.factions().get(saltward).unwrap().held(Resource::Ore), 5);

        let mut events = Vec::new();
        for tick in due + 1..=due + DAY.as_ticks() {
            game.tick(Tick::new(tick), &mut events);
        }
        assert_eq!(news(&events), [FactionNews::Dissolved(Dissolution::Exhausted)]);
        assert!(game.factions().get(saltward).is_none() && game.factions().holding(keep).is_none());
        // Half of what was left spills at the keep, for anyone.
        let spilt: Vec<_> = game.relics_at(keep).collect();
        assert_eq!(spilt.len(), 1);
        assert_eq!((spilt[0].owner, spilt[0].contents[Resource::Ore.index()]), (NOBODY, 2));
        assert!(game.can_open(OUTSIDER, spilt[0]));
        // Everyone sworn to it waits a week before swearing again.
        let now = due + DAY.as_ticks() + 1;
        assert_eq!(own(&act(&mut game, Command::charter(id(3), "Tidewall"), now), id(3)), refused(FactionRefusal::Oathbreaker));
        let (next, _) = neighbour(&game, keep);
        put(&mut game, id(3), next);
        put(&mut game, OUTSIDER, next);
        assert_eq!(game.can_attack(OUTSIDER, id(3)), Err(Refusal::Unmarked), "a failed faction makes nobody fair game");
    }

    #[test]
    fn breaking_a_marker_marks_the_breaker_pays_them_the_ore_and_at_the_keep_ends_the_faction() {
        let (mut game, keep) = formed();
        game.characters[0].stow(Resource::Ore, 100);
        act(&mut game, Command::give(id(0), Lot { resource: Resource::Ore, amount: 100 }), 2);
        let (next, _) = neighbour(&game, keep);
        put(&mut game, id(1), next);
        act(&mut game, Command::claim(id(1)), 3);

        put(&mut game, id(1), keep);
        assert_eq!(own(&act(&mut game, Command::break_marker(id(1)), 100), id(1)), refused(FactionRefusal::OwnMarker));
        put(&mut game, OUTSIDER, next);
        let begun = act(&mut game, Command::break_marker(OUTSIDER), 100);
        assert!(matches!(own(&begun, OUTSIDER), Some(Outcome::BeganBreaking { .. })));
        assert!(begun.iter().any(|event| matches!(event, Event::Character { outcome: Outcome::StandingChanged { .. }, .. })));
        assert_eq!(news(&begun), [FactionNews::Breaking { who: OUTSIDER, place: next }]);
        put(&mut game, id(1), next);
        assert_eq!(game.can_attack(id(1), OUTSIDER), Ok(()), "marked from the start");
        put(&mut game, id(5), next);
        assert_eq!(own(&act(&mut game, Command::break_marker(id(5)), 101), id(5)), refused(FactionRefusal::AlreadyBreaking));

        let mut events = Vec::new();
        for tick in 101..=100 + BREAK_TIME.as_ticks() {
            game.tick(Tick::new(tick), &mut events);
        }
        assert_eq!(news(&events), [FactionNews::MarkerBroken { by: OUTSIDER, place: next }]);
        let spoils = Lot { resource: Resource::Ore, amount: 20 };
        assert!(events.contains(&Event::Character { who: OUTSIDER, outcome: Outcome::BrokeMarker { faction: "Saltward".into(), spoils } }));
        assert!(game.factions().holding(next).is_none());

        // Someone who walks off leaves the marker standing.
        put(&mut game, id(5), keep);
        act(&mut game, Command::break_marker(id(5)), 2_000);
        put(&mut game, id(5), next);
        let mut events = Vec::new();
        for tick in 2_001..=2_000 + BREAK_TIME.as_ticks() {
            game.tick(Tick::new(tick), &mut events);
        }
        assert!(news(&events).is_empty());
        assert!(game.factions().holding(keep).is_some());

        // The keep's marker is the faction.
        put(&mut game, id(5), keep);
        let at = 3_000;
        act(&mut game, Command::break_marker(id(5)), at);
        let mut events = Vec::new();
        for tick in at + 1..=at + BREAK_TIME.as_ticks() {
            game.tick(Tick::new(tick), &mut events);
        }
        assert_eq!(news(&events), [FactionNews::MarkerBroken { by: id(5), place: keep }, FactionNews::Dissolved(Dissolution::Broken(id(5)))]);
        assert!(game.factions().named("Saltward").is_none());
        // 100 given, 20 for the marker: half the 80 left spills.
        assert_eq!(game.relics_at(keep).map(|relic| relic.contents[Resource::Ore.index()]).sum::<u64>(), 40);
    }

    #[test]
    fn an_oath_is_broken_at_a_price_and_one_of_the_five_breaking_it_ends_the_faction() {
        let (mut game, keep) = formed();
        // An unformed charter can be left freely.
        act(&mut game, Command::sign(id(5), "Saltward"), 2);
        let (next, _) = neighbour(&game, keep);

        // Fay joined after; leaving makes her an oathbreaker, fair game to those she left.
        let left = act(&mut game, Command::forsake(id(5)), 20);
        assert_eq!(own(&left, id(5)), Some(Outcome::Forsook { name: "Saltward".into(), formed: true }));
        assert_eq!(news(&left), [FactionNews::Left(id(5))]);
        put(&mut game, id(5), next);
        put(&mut game, id(1), next);
        put(&mut game, OUTSIDER, next);
        assert_eq!(game.can_attack(id(1), id(5)), Ok(()));
        assert_eq!(game.can_attack(OUTSIDER, id(5)), Err(Refusal::Unmarked), "only those she left");
        put(&mut game, id(5), keep);
        assert_eq!(own(&act(&mut game, Command::sign(id(5), "Saltward"), 40), id(5)), refused(FactionRefusal::Oathbreaker));
        // A week later it is behind her.
        game.factions.forgive(Tick::new(20) + OATHBREAKER_LASTS);
        put(&mut game, id(5), next);
        assert_eq!(game.can_attack(id(1), id(5)), Err(Refusal::Unmarked));

        // Bryn signed it into being; Bryn leaving ends it.
        let betrayed = act(&mut game, Command::forsake(id(1)), 50);
        assert_eq!(news(&betrayed), [FactionNews::Dissolved(Dissolution::Betrayed(id(1)))]);
        assert!(game.factions().named("Saltward").is_none());
        put(&mut game, id(3), next);
        assert_eq!(game.can_attack(id(3), id(1)), Ok(()), "those betrayed may strike the traitor");
        assert!(game.factions().oathbreaker(id(3)).is_none(), "and are not oathbreakers for it");
        assert!(game.factions().oathbreaker(id(1)).is_some());

        // Taking back a signature from a charter not yet formed costs nothing.
        put(&mut game, id(3), keep);
        act(&mut game, Command::charter(id(3), "Tidewall"), 60);
        let withdrawn = act(&mut game, Command::forsake(id(3)), 80);
        assert_eq!(own(&withdrawn, id(3)), Some(Outcome::Forsook { name: "Tidewall".into(), formed: false }));
        assert!(game.factions().oathbreaker(id(3)).is_none() && game.factions().named("Tidewall").is_none());
    }
}
