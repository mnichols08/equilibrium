//! The Behemoth: what the Void makes of a failing world.
//!
//! While a world of founded lands is in the Void Consumption one rises
//! in the land worst off, and roams it, going for anyone it finds. Its hide
//! turns weak blows, and a blow lands for a quarter for each bloodline
//! that has struck at it lately, so only many bloodlines together can
//! bring it down. Bringing it down restores the land it fell in and
//! leaves tide-glass for the taking. Every life it ends feeds it, and
//! sickens every land, not only its own. While the Void lasts another
//! rises an hour after one falls.

use alloc::vec::Vec;

use eq_clock::Tick;
use eq_world_graph::PlaceId;

use super::Game;
use crate::content::{
    BEHEMOTH_FEEDS_PERCENT, BEHEMOTH_RETURNS, BEHEMOTH_ROAMS_EVERY, BEHEMOTH_SHARDS, CORRUPTION_PER_DEATH,
};
use crate::creature::{BehemothNews, Being, Creature, CreatureKind, Relic};
use crate::event::Event;
use crate::faction::NOBODY;
use crate::types::{Resource, Safety};

impl Game {
    /// Whether `place` is somewhere the Behemoth can stand: not a haven,
    /// a lair or a house.
    fn open_ground(&self, place: PlaceId) -> bool {
        let lair_or_haven = self.map.place(place).is_none_or(|data| data.safety == Safety::Haven || data.lair.is_some());
        !lair_or_haven && !self.zones.iter().any(|zone| zone.hall() == Some(place))
    }

    /// Raises a Behemoth if the Void calls for one, and moves on any that roam.
    pub(super) fn tend_behemoth(&mut self, now: Tick, events: &mut Vec<Event>) {
        let standing = self.creatures.iter().any(|creature| creature.being.kind == CreatureKind::Behemoth);
        // It is the whole world's doing, so it comes only to a world of founded lands.
        if !standing && self.is_founded() && self.era.lairs_never_rest() && now >= self.behemoth_due {
            self.raise_behemoth(now, events);
        }
        if now.get() > 0 && now.get().is_multiple_of(BEHEMOTH_ROAMS_EVERY.as_ticks()) {
            self.roam(now, events);
        }
    }

    /// The land the Behemoth rises in: the founded land doing worst.
    fn worst_land(&self) -> usize {
        let founded = self.zones.iter().enumerate().filter(|(_, zone)| zone.founder().is_some());
        founded.min_by_key(|(_, zone)| zone.vitality()).map_or(0, |(index, _)| index)
    }

    fn raise_behemoth(&mut self, now: Tick, events: &mut Vec<Event>) {
        let land = self.worst_land();
        let ground: Vec<PlaceId> = self.zones[land].place_ids().filter(|&place| self.open_ground(place)).collect();
        if ground.is_empty() {
            return;
        }
        // The list is short, so the cast cannot truncate.
        let place = ground[self.rng.below(ground.len() as u64) as usize];
        let id = self.new_creature_id();
        let behemoth = Creature::native(CreatureKind::Behemoth, id, place, now);
        events.push(Event::CreatureAppeared { place, being: behemoth.being });
        events.push(Event::Behemoth { place, news: BehemothNews::Risen });
        self.creatures.push(behemoth);
    }

    /// Each Behemoth moves to a neighbouring place of its land, chosen by
    /// chance, unless it is fighting someone where it stands.
    fn roam(&mut self, now: Tick, events: &mut Vec<Event>) {
        for slot in 0..self.creatures.len() {
            let creature = &self.creatures[slot];
            if creature.being.kind != CreatureKind::Behemoth {
                continue;
            }
            let from = creature.place;
            let busy = creature.foes.iter().any(|foe| self.occupants(from).contains(foe));
            let land = self.zone_at(from);
            let ways: Vec<PlaceId> =
                self.map.exits(from).iter().map(|exit| exit.to).filter(|&to| land.contains(to) && self.open_ground(to)).collect();
            if busy || ways.is_empty() {
                continue;
            }
            // The list is short, so the cast cannot truncate.
            let to = ways[self.rng.below(ways.len() as u64) as usize];
            let being = self.creatures[slot].being;
            self.creatures[slot].place = to;
            self.creatures[slot].next_blow = now + crate::content::CREATURE_BLOW_EVERY;
            events.push(Event::CreatureGone { place: from, being });
            events.push(Event::CreatureAppeared { place: to, being });
        }
    }

    /// The Behemoth has fallen at `place`: the land it fell in is
    /// restored, it leaves tide-glass for the taking, and another may
    /// rise in an hour.
    pub(super) fn behemoth_fell(&mut self, place: PlaceId, now: Tick, events: &mut Vec<Event>) {
        let land = self.zone_index(place);
        self.zones[land].cleanse();
        let places: Vec<PlaceId> = self.zones[land].place_ids().collect();
        for place in places {
            if let Some(deposit) = self.map.place_mut(place).and_then(|data| data.deposit.as_mut()) {
                deposit.node.mend(100);
            }
        }
        let mut shards = [0; Resource::ALL.len()];
        shards[Resource::Tideglass.index()] = BEHEMOTH_SHARDS;
        self.relics.extend(Relic::new(NOBODY, place, shards, now));
        self.behemoth_due = now + BEHEMOTH_RETURNS;
        events.push(Event::Behemoth { place, news: BehemothNews::Fell });
    }

    /// The Behemoth at `slot` has ended a life: it takes back some of
    /// its health, and every land but the one already mourning sickens.
    pub(super) fn behemoth_fed(&mut self, slot: usize, being: Being, events: &mut Vec<Event>) {
        let place = self.creatures[slot].place;
        self.creatures[slot].heal(BEHEMOTH_FEEDS_PERCENT);
        let here = self.zone_index(place);
        for (index, zone) in self.zones.iter_mut().enumerate() {
            if index != here && zone.founder().is_some() {
                zone.mourn(CORRUPTION_PER_DEATH);
            }
        }
        let _ = being;
        events.push(Event::Behemoth { place, news: BehemothNews::Fed });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::CharacterId;
    use crate::command::Command;
    use crate::era::Era;
    use crate::event::Outcome;
    use crate::types::Attribute;
    use eq_events::World;

    /// Four founders, each of their own land and bloodline, and the Void upon them.
    fn four_lands_in_the_void() -> Game {
        let mut game = Game::new_world(3, 4, &[]);
        let mut events = Vec::new();
        for (n, name) in ["Ash", "Bryn", "Cole", "Dara"].into_iter().enumerate() {
            game.apply(&Command::join(CharacterId::new(n as u64), name), Tick::ZERO, &mut events);
            let index = game.index_of(CharacterId::new(n as u64)).unwrap();
            for attribute in [Attribute::Vigor, Attribute::Resilience] {
                game.characters[index].strengthen(attribute, 75);
            }
        }
        game.era = Era::VoidConsumption;
        game
    }

    fn behemoth(game: &Game) -> Option<&Creature> {
        game.creatures.iter().find(|creature| creature.being.kind == CreatureKind::Behemoth)
    }

    fn put(game: &mut Game, who: CharacterId, place: PlaceId) {
        let index = game.index_of(who).unwrap();
        game.occupancy.leave(game.characters[index].place, who);
        game.characters[index].place = place;
        game.characters[index].busy_until = Tick::ZERO;
        game.occupancy.enter(place, who);
    }

    fn strike(game: &mut Game, who: CharacterId, now: u64) -> u32 {
        let id = behemoth(game).unwrap().id;
        let mut events = Vec::new();
        game.apply(&Command::fight(who, id), Tick::new(now), &mut events);
        events
            .iter()
            .find_map(|event| match event {
                Event::Character { outcome: Outcome::HitCreature { damage, .. }, .. } => Some(*damage),
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn the_void_raises_one_in_the_worst_off_land_on_open_ground() {
        let mut game = four_lands_in_the_void();
        game.era = Era::Conflict;
        let mut events = Vec::new();
        game.tick(Tick::new(1), &mut events);
        assert!(behemoth(&game).is_none(), "not before the Void");

        // Bryn's land is the worst off by the time the Void comes.
        let worst = game.zone_index(game.character(CharacterId::new(1)).unwrap().home());
        game.zones[worst].mourn(30);
        for tick in 2..60 {
            game.tick(Tick::new(tick), &mut Vec::new());
        }
        game.era = Era::VoidConsumption;
        let mut events = Vec::new();
        game.tick(Tick::new(60), &mut events);
        let place = behemoth(&game).unwrap().place;
        assert_eq!(game.zone_index(place), worst);
        assert!(game.open_ground(place));
        assert!(events.contains(&Event::Behemoth { place, news: BehemothNews::Risen }));
        assert_eq!(game.audience(events.last().unwrap()).len(), 4, "everyone hears");
        // Only one at a time.
        game.tick(Tick::new(61), &mut Vec::new());
        assert_eq!(game.creatures.iter().filter(|creature| creature.being.kind == CreatureKind::Behemoth).count(), 1);
    }

    #[test]
    fn one_bloodline_cannot_touch_it_and_four_strike_in_full() {
        let mut game = four_lands_in_the_void();
        game.tick(Tick::new(1), &mut Vec::new());
        let place = behemoth(&game).unwrap().place;
        for n in 0..4 {
            put(&mut game, CharacterId::new(n), place);
        }
        // Ash alone: a quarter of a blow does not get through its hide.
        assert_eq!(strike(&mut game, CharacterId::new(0), 10), 0);
        let mut lands = 0;
        for n in 1..4 {
            lands = strike(&mut game, CharacterId::new(n), 10);
        }
        // With four bloodlines striking, a blow lands whole.
        assert!(lands > crate::content::BEHEMOTH_HIDE, "{lands}");
        assert_eq!(behemoth(&game).unwrap().blows_by_blood.len(), 4);
    }

    #[test]
    fn bringing_it_down_restores_its_land_and_leaves_shards_and_another_rises_an_hour_later() {
        let mut game = four_lands_in_the_void();
        game.tick(Tick::new(1), &mut Vec::new());
        let place = behemoth(&game).unwrap().place;
        let land = game.zone_index(place);
        game.zones[land].mourn(40);
        for n in 0..4 {
            put(&mut game, CharacterId::new(n), place);
        }
        let mut now = 10;
        let mut events = Vec::new();
        while behemoth(&game).is_some() {
            for n in 0..4 {
                let id = behemoth(&game).map(|creature| creature.id);
                if let Some(id) = id {
                    game.apply(&Command::fight(CharacterId::new(n), id), Tick::new(now), &mut events);
                    game.characters[n as usize].busy_until = Tick::ZERO;
                    game.characters[n as usize].health_for_test(100);
                }
            }
            now += 3;
        }
        assert!(events.contains(&Event::Behemoth { place, news: BehemothNews::Fell }));
        assert_eq!(game.zones[land].corruption(), 0);
        let shards: u64 = game.relics_at(place).map(|relic| relic.contents[Resource::Tideglass.index()]).sum();
        assert_eq!(shards, BEHEMOTH_SHARDS);
        // Not another until the hour is up.
        game.tick(Tick::new(now + 60), &mut Vec::new());
        assert!(behemoth(&game).is_none());
        game.tick(Tick::new(now + BEHEMOTH_RETURNS.as_ticks()), &mut Vec::new());
        assert!(behemoth(&game).is_some());
    }

    #[test]
    fn each_life_it_ends_feeds_it_and_sickens_every_land() {
        let mut game = four_lands_in_the_void();
        game.tick(Tick::new(1), &mut Vec::new());
        let (place, health) = (behemoth(&game).unwrap().place, behemoth(&game).unwrap().health());
        let slot = game.creatures.iter().position(|creature| creature.being.kind == CreatureKind::Behemoth).unwrap();
        game.creatures[slot].hurt(1_000);
        let land = game.zone_index(place);
        let before: Vec<i64> = game.zones.iter().map(|zone| zone.corruption()).collect();
        let mut events = Vec::new();
        let being = game.creatures[slot].being;
        game.behemoth_fed(slot, being, &mut events);
        assert_eq!(game.creatures[slot].health(), health - 1_000 + 600);
        for (index, zone) in game.zones.iter().enumerate() {
            let sickened = zone.founder().is_some() && index != land;
            assert_eq!(zone.corruption() - before[index], if sickened { CORRUPTION_PER_DEATH } else { 0 });
        }
        assert!(events.contains(&Event::Behemoth { place, news: BehemothNews::Fed }));
    }

    #[test]
    fn it_roams_its_land_but_never_into_a_haven_or_a_house() {
        let mut game = four_lands_in_the_void();
        game.tick(Tick::new(1), &mut Vec::new());
        let land = game.zone_index(behemoth(&game).unwrap().place);
        let mut seen = Vec::new();
        for step in 1..=20 {
            game.tick(Tick::new(step * BEHEMOTH_ROAMS_EVERY.as_ticks()), &mut Vec::new());
            let place = behemoth(&game).unwrap().place;
            assert!(game.open_ground(place) && game.zone_index(place) == land);
            if !seen.contains(&place) {
                seen.push(place);
            }
        }
        assert!(seen.len() >= 2, "it should move: {seen:?}");
    }
}
