//! Lineage Resonances: what two bloodlines can do together that
//! neither can alone, and what it costs them.
//!
//! - Silt-Forming (Tidal and Stone standing together): their blows land
//!   harder there, but the mud holds them too, and leaving takes longer.
//! - Glyph Ward (Covenant and Lunar together in a house): nobody in that
//!   house can be struck or robbed, whatever their land, for as long as
//!   the Lunar have ectoplasm to feed it.
//! - Wildfire Trail (Ash and Nomadic together in the wild): both leave
//!   faster, and the fire scorches what the ground could give.

use alloc::vec::Vec;

use eq_clock::Tick;
use eq_world_graph::PlaceId;

use super::Game;
use crate::bloodline::Bloodline;
use crate::content::{GLYPH_WARD_FEEDS_EVERY, WILDFIRE_SCORCH_PERCENT};
use crate::types::Safety;

impl Game {
    /// The bloodlines of the living standing at `place`.
    fn bloods_at(&self, place: PlaceId) -> Vec<Bloodline> {
        let mut bloods = Vec::new();
        for &who in self.occupants(place) {
            if let Some(character) = self.character(who).filter(|character| !character.is_spirit()) {
                if !bloods.contains(&character.bloodline()) {
                    bloods.push(character.bloodline());
                }
            }
        }
        bloods
    }

    /// Whether people of both `a` and `b` stand living at `place`.
    fn resonates(&self, place: PlaceId, a: Bloodline, b: Bloodline) -> bool {
        let bloods = self.bloods_at(place);
        bloods.contains(&a) && bloods.contains(&b)
    }

    /// Whether `blood`, standing at `place`, is in the mud of Silt-Forming.
    pub(super) fn in_silt(&self, place: PlaceId, blood: Bloodline) -> bool {
        matches!(blood, Bloodline::Tidal | Bloodline::Stone) && self.resonates(place, Bloodline::Tidal, Bloodline::Stone)
    }

    /// Whether `blood`, standing at `place`, is on a Wildfire Trail: in
    /// the wild with someone of the other line.
    pub(super) fn on_wildfire(&self, place: PlaceId, blood: Bloodline) -> bool {
        let wild = self.map.place(place).is_some_and(|data| data.safety != Safety::Haven);
        wild && matches!(blood, Bloodline::Ash | Bloodline::Nomadic) && self.resonates(place, Bloodline::Ash, Bloodline::Nomadic)
    }

    /// Whether a Glyph Ward holds at `place`: a house with someone of the
    /// Covenant line in it, and someone of the Lunar line with ectoplasm
    /// to feed it.
    pub fn glyph_ward(&self, place: PlaceId) -> bool {
        let house = self.zones.iter().any(|zone| zone.hall() == Some(place));
        let lunar_fed = self.occupants(place).iter().any(|&who| {
            self.character(who).is_some_and(|character| {
                !character.is_spirit() && character.bloodline() == Bloodline::Lunar && character.ectoplasm() > 0
            })
        });
        house && lunar_fed && self.bloods_at(place).contains(&Bloodline::Covenant)
    }

    /// Every ward takes its due from a Lunar standing in it.
    pub(super) fn feed_wards(&mut self, now: Tick) {
        if now.get() == 0 || !now.get().is_multiple_of(GLYPH_WARD_FEEDS_EVERY.as_ticks()) {
            return;
        }
        let halls: Vec<PlaceId> = self.zones.iter().filter_map(crate::Zone::hall).collect();
        for hall in halls {
            if !self.glyph_ward(hall) {
                continue;
            }
            let feeder = self.occupants(hall).iter().copied().find(|&who| {
                self.character(who).is_some_and(|character| {
                    !character.is_spirit() && character.bloodline() == Bloodline::Lunar && character.ectoplasm() > 0
                })
            });
            if let Some(index) = feeder.and_then(|who| self.index_of(who)) {
                self.characters[index].spend_ectoplasm(1);
            }
        }
    }

    /// The fire of a Wildfire Trail scorches the ground left behind.
    pub(super) fn scorch(&mut self, place: PlaceId) {
        if let Some(deposit) = self.map.place_mut(place).and_then(|data| data.deposit.as_mut()) {
            deposit.node.scour(WILDFIRE_SCORCH_PERCENT);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::Refusal;
    use crate::command::Command;
    use crate::game::id_at;
    use eq_events::World;

    /// Ash, Bryn and Cole at the ledge, of the given bloodlines.
    fn three_at_the_ledge(bloods: [Bloodline; 3]) -> (Game, PlaceId) {
        let mut game = Game::new(1, &["Ash", "Bryn", "Cole"]);
        let ledge = game.map.exit(game.anchor(), "north").unwrap().to;
        for (index, blood) in bloods.into_iter().enumerate() {
            game.characters[index].bloodline = blood;
            game.occupancy.leave(game.characters[index].place, id_at(index));
            game.characters[index].place = ledge;
            game.occupancy.enter(ledge, id_at(index));
        }
        (game, ledge)
    }

    #[test]
    fn silt_forming_mires_what_the_tidal_and_stone_strike_and_them_with_it() {
        let (mut game, ledge) = three_at_the_ledge([Bloodline::Tidal, Bloodline::Stone, Bloodline::Ash]);
        assert_eq!(game.landing(Some(0), None, 100, Tick::ZERO), 115);
        assert_eq!(game.landing(Some(1), None, 100, Tick::ZERO), 115);
        assert_eq!(game.landing(Some(2), None, 100, Tick::ZERO), 100, "not the Ash line");
        assert_eq!((game.travel_percent(0, ledge), game.travel_percent(1, ledge), game.travel_percent(2, ledge)), (150, 150, 100));
        // Apart, nothing.
        game.characters[1].bloodline = Bloodline::Covenant;
        assert_eq!(game.landing(Some(0), None, 100, Tick::ZERO), 100);
        assert_eq!(game.travel_percent(0, ledge), 100);
    }

    #[test]
    fn a_wildfire_trail_speeds_the_ash_and_nomadic_out_of_the_wild_and_burns_what_they_leave() {
        let (mut game, ledge) = three_at_the_ledge([Bloodline::Ash, Bloodline::Nomadic, Bloodline::Tidal]);
        assert_eq!(game.travel_percent(0, ledge), 67);
        assert_eq!(game.travel_percent(1, ledge), 80 * 67 / 100);
        assert_eq!(game.travel_percent(2, ledge), 100);
        let stock = |game: &Game| game.map.place(ledge).unwrap().deposit.unwrap().node.stock();
        let before = stock(&game);
        let mut events = Vec::new();
        game.apply(&Command::go(id_at(0), "south"), Tick::ZERO, &mut events);
        assert_eq!(stock(&game), before - before * 5 / 100);
        // Not in a haven: there is nothing there to burn and no trail.
        let camp = game.anchor();
        assert!(!game.on_wildfire(camp, Bloodline::Ash));
    }

    #[test]
    fn a_glyph_ward_shelters_everyone_in_the_house_while_the_lunar_can_feed_it() {
        // Ash founds a land and raises its house; Bryn and Cole are of another land.
        let mut game = Game::new_world(1, 2, &[]);
        let mut events = Vec::new();
        game.apply(&Command::join(id_at(0), "Ash"), Tick::ZERO, &mut events);
        game.apply(&Command::join(id_at(1), "Bryn"), Tick::ZERO, &mut events);
        game.apply(&Command::invite(id_at(1), "Cole"), Tick::ZERO, &mut events);
        game.apply(&Command::join(id_at(2), "Cole"), Tick::ZERO, &mut events);
        let land = game.zone_index(game.character(id_at(0)).unwrap().home());
        let house = game.zones[land]
            .place_ids()
            .find(|&place| game.map.place(place).is_some_and(|data| data.safety == Safety::Buffer && data.lair.is_none()))
            .unwrap();
        game.zones[land].hall = Some(house);
        for (index, blood) in [Bloodline::Covenant, Bloodline::Lunar, Bloodline::Ash].into_iter().enumerate() {
            game.characters[index].bloodline = blood;
            game.occupancy.leave(game.characters[index].place, id_at(index));
            game.characters[index].place = house;
            game.occupancy.enter(house, id_at(index));
        }
        game.characters[2].shift_taint(100);
        assert_eq!(game.can_attack(id_at(0), id_at(2)), Ok(()), "a stranger in a house, marked");
        game.characters[1].ectoplasm_for_test(2);
        assert!(game.glyph_ward(house));
        assert_eq!(game.can_attack(id_at(0), id_at(2)), Err(Refusal::Haven), "the ward holds for everyone");
        // It feeds a minute at a time, and fails when there is nothing left.
        let minute = GLYPH_WARD_FEEDS_EVERY.as_ticks();
        game.feed_wards(Tick::new(minute));
        game.feed_wards(Tick::new(2 * minute));
        assert_eq!(game.characters[1].ectoplasm(), 0);
        assert!(!game.glyph_ward(house));
        assert_eq!(game.can_attack(id_at(0), id_at(2)), Ok(()));
    }
}
