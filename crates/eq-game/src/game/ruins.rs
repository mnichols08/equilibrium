//! Houses fall to ruin when their land forgets them.
//!
//! A house shelters its land's people, and costs its founder what it
//! took to raise and the ground it stands on. If nobody of the land
//! sets foot in it for a week it falls: it shelters nobody, half of
//! what built it lies in the rubble for anyone to take, and the founder
//! may raise another.

use eq_clock::Tick;

use super::Game;
use crate::content::{HOUSE_ABANDONED_AFTER, RUIN_SALVAGE_PERCENT};
use crate::creature::Relic;
use crate::event::Event;
use crate::faction::NOBODY;
use crate::types::Resource;
use crate::zone::Chronicle;

/// How often the world looks in on its houses.
const LOOK_IN_EVERY: u64 = 60;

impl Game {
    /// Notes every house someone of its land is standing in, and lets
    /// fall any nobody has stood in for too long.
    pub(super) fn tend_houses(&mut self, now: Tick, events: &mut alloc::vec::Vec<Event>) {
        if !now.get().is_multiple_of(LOOK_IN_EVERY) {
            return;
        }
        for land in 0..self.zones.len() {
            let Some(hall) = self.zones[land].hall() else { continue };
            let tended = self.occupants(hall).iter().any(|&who| {
                self.character(who).is_some_and(|character| !character.is_spirit() && self.land_of(character) == land)
            });
            if tended {
                self.zones[land].tended_at = now;
            } else if now.since(self.zones[land].tended_at) > HOUSE_ABANDONED_AFTER {
                self.ruin(land, now, events);
            }
        }
    }

    /// The house of `land` falls.
    fn ruin(&mut self, land: usize, now: Tick, events: &mut alloc::vec::Vec<Event>) {
        let Some(hall) = self.zones[land].hall.take() else { return };
        self.zones[land].ruin = Some(hall);
        self.zones[land].record(now, Chronicle::Ruined);
        let mut salvage = [0; Resource::ALL.len()];
        for (resource, amount) in self.house_cost() {
            salvage[resource.index()] += amount * RUIN_SALVAGE_PERCENT / 100;
        }
        self.relics.extend(Relic::new(NOBODY, hall, salvage, now));
        events.push(Event::HouseRuined { anchor: self.zones[land].anchor(), place: hall });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Command;
    use crate::game::id_at;
    use crate::types::Safety;
    use eq_events::World;
    use eq_world_graph::PlaceId;

    /// Ash, a founder, with a house raised at hour zero, and Bryn of another land.
    fn a_house() -> (Game, PlaceId) {
        let mut game = Game::new_world(1, 2, &[]);
        let mut events = alloc::vec::Vec::new();
        game.apply(&Command::join(id_at(0), "Ash"), Tick::ZERO, &mut events);
        game.apply(&Command::join(id_at(1), "Bryn"), Tick::ZERO, &mut events);
        let land = game.zone_index(game.character(id_at(0)).unwrap().home());
        let site = game.zones[land]
            .place_ids()
            .find(|&place| game.map.place(place).is_some_and(|data| data.safety == Safety::Buffer && data.lair.is_none()))
            .unwrap();
        game.occupancy.leave(game.characters[0].place, id_at(0));
        game.characters[0].place = site;
        game.occupancy.enter(site, id_at(0));
        for (resource, amount) in game.house_cost() {
            game.characters[0].stow(resource, amount);
        }
        game.apply(&Command::raise(id_at(0)), Tick::ZERO, &mut events);
        assert_eq!(game.zones[land].hall(), Some(site));
        (game, site)
    }

    fn ticks(game: &mut Game, from: u64, to: u64) -> alloc::vec::Vec<Event> {
        let mut events = alloc::vec::Vec::new();
        for tick in (from..=to).step_by(LOOK_IN_EVERY as usize) {
            game.tick(Tick::new(tick), &mut events);
        }
        events
    }

    #[test]
    fn a_house_its_people_keep_stands_and_one_they_forget_falls_to_ruin() {
        let (mut game, site) = a_house();
        let land = game.zone_index(site);
        let week = HOUSE_ABANDONED_AFTER.as_ticks();
        // Ash stands in it: it stands.
        ticks(&mut game, LOOK_IN_EVERY, week + 2 * LOOK_IN_EVERY);
        assert_eq!(game.zones[land].hall(), Some(site));
        // A stranger standing in it does not count.
        let haven = game.zones[land].anchor();
        game.occupancy.leave(site, id_at(0));
        game.characters[0].place = haven;
        game.occupancy.enter(haven, id_at(0));
        let bryn_was = game.characters[1].place;
        game.occupancy.leave(bryn_was, id_at(1));
        game.characters[1].place = site;
        game.occupancy.enter(site, id_at(1));
        let from = week + 3 * LOOK_IN_EVERY;
        let events = ticks(&mut game, from, from + week + 2 * LOOK_IN_EVERY);
        assert_eq!(game.zones[land].hall(), None);
        assert_eq!(game.zones[land].ruin(), Some(site));
        assert!(events.iter().any(|event| matches!(event, Event::HouseRuined { place, .. } if *place == site)));
        assert!(game.zones[land].history().iter().any(|entry| entry.what == Chronicle::Ruined));
        assert!(game.zones[land].history().first().is_some_and(|entry| entry.what == Chronicle::Raised));
        // Half of what built it lies in the rubble, for anyone.
        let salvage: u64 = game.relics_at(site).map(|relic| relic.contents.iter().sum::<u64>()).sum();
        let cost: u64 = game.house_cost().iter().map(|(_, amount)| amount).sum();
        assert_eq!(salvage, cost / 2);
        assert!(game.relics_at(site).all(|relic| game.can_open(id_at(1), relic)));
        // It shelters nobody now: Ash can be robbed asleep there.
        assert!(!game.is_sheltered(game.character(id_at(0)).unwrap()) || game.character(id_at(0)).unwrap().place() != site);
    }
}
