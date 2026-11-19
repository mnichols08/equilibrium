//! What each bloodline is gifted with, and what it is wounded by.
//!
//! Every bloodline has one of each, so that being born to it is a
//! reward and a cost at once. The names and their wording live in
//! [`Bloodline::passive`] and [`Bloodline::wound`]; the numbers in
//! `content`.

use alloc::vec::Vec;

use eq_clock::Tick;
use eq_world_graph::PlaceId;

use super::Game;
use crate::bloodline::Bloodline;
use crate::character::{CharacterId, Wounded};
use crate::content::{
    BLEED_LASTS, CINDER_RENEWAL_PERCENT, COVENANT_IN_HOUSE_PERCENT, CRACKS_LAST, EARTHEN_MASS_DIVISOR, FRAGILE_FOCUS_PERCENT,
    HEAVY_BLOW, LOW_TIDE, MAX_HEALTH, STASIS_AT_HEALTH, TIDAL_HIGH_TIDE_PERCENT, TIDE,
};
use crate::pact::Pact;
use crate::types::Attribute;

impl Game {
    /// The Vigor a blow by the character at `index` is struck with.
    /// Stone puts a quarter of its Resilience behind every blow.
    pub(super) fn striking_vigor(&self, index: usize) -> u32 {
        let striker = &self.characters[index];
        let vigor = striker.attribute(Attribute::Vigor).level();
        if striker.bloodline() == Bloodline::Stone {
            vigor + striker.attribute(Attribute::Resilience).level() / EARTHEN_MASS_DIVISOR
        } else {
            vigor
        }
    }

    /// How much of `damage` lands, given who struck it (if a character)
    /// and who it struck (if a character), at `now`. The Tidal strike
    /// harder while the tide is in; the Covenant take less in a house.
    pub(super) fn landing(&self, striker: Option<usize>, struck: Option<usize>, damage: u32, now: Tick) -> u32 {
        let tide_in = !TIDE.is_within(now, LOW_TIDE);
        let mut damage = damage;
        if striker.is_some_and(|index| self.characters[index].bloodline() == Bloodline::Tidal) && tide_in {
            damage = damage * TIDAL_HIGH_TIDE_PERCENT / 100;
        }
        if let Some(index) = struck {
            let character = &self.characters[index];
            let in_a_house = self.zones.iter().any(|zone| zone.hall() == Some(character.place()));
            if character.bloodline() == Bloodline::Covenant && in_a_house {
                damage = damage * COVENANT_IN_HOUSE_PERCENT / 100;
            }
        }
        damage
    }

    /// What a heavy blow opens in the one it landed on.
    pub(super) fn after_blow(&mut self, index: usize, damage: u32, now: Tick) {
        if damage < HEAVY_BLOW {
            return;
        }
        let character = &mut self.characters[index];
        match character.bloodline() {
            Bloodline::Tidal => character.wound = Some((Wounded::Bleeding, now + BLEED_LASTS)),
            Bloodline::Stone => character.wound = Some((Wounded::Cracked, now + CRACKS_LAST)),
            Bloodline::Lunar => character.scatter_ectoplasm(FRAGILE_FOCUS_PERCENT),
            Bloodline::Covenant | Bloodline::Ash | Bloodline::Nomadic => {}
        }
    }

    /// How long the way out of `from` takes the character at `index`, in
    /// percent of the usual: the Nomadic leave the wild quicker, and the
    /// Covenant near death go slowly.
    pub(super) fn travel_percent(&self, index: usize, from: PlaceId) -> u64 {
        let character = &self.characters[index];
        let wild = self.map.place(from).is_some_and(|place| place.safety != crate::types::Safety::Haven);
        match character.bloodline() {
            Bloodline::Nomadic if wild => crate::content::NOMADIC_TRAVEL_PERCENT,
            Bloodline::Covenant if character.health() <= STASIS_AT_HEALTH => 200,
            _ => 100,
        }
    }

    /// When someone of the Ash line kills or is killed at `place`,
    /// their allies standing there mend a share of what they are
    /// missing. `ash` is whoever of the Ash line it was.
    pub(super) fn cinder_renewal(&mut self, ash: CharacterId, place: PlaceId) {
        let Some(home) = self.character(ash).map(|character| self.land_of(character)) else { return };
        if self.character(ash).map(crate::Character::bloodline) != Some(Bloodline::Ash) {
            return;
        }
        let allies: Vec<usize> = self
            .occupants(place)
            .iter()
            .filter_map(|&who| self.index_of(who))
            .filter(|&index| {
                let other = &self.characters[index];
                let land = self.land_of(other);
                !other.is_spirit() && (land == home || self.pacts.between(land, home) == Some(Pact::Alliance))
            })
            .collect();
        for index in allies {
            let missing = MAX_HEALTH - i64::from(self.characters[index].health());
            self.characters[index].mend(missing * i64::from(CINDER_RENEWAL_PERCENT) / 100);
        }
    }

    /// How much the character at `index` mends this time, given what
    /// they would mend: an open wound bleeds or stops it, and the
    /// Nomadic tire of a haven.
    pub(super) fn mending(&mut self, index: usize, usual: i64, in_haven: bool, now: Tick) -> i64 {
        let character = &mut self.characters[index];
        if character.wound.is_some_and(|(_, until)| until <= now) {
            character.wound = None;
        }
        match character.wound {
            Some((Wounded::Bleeding, _)) => return -crate::content::MENDING,
            Some((Wounded::Cracked, _)) => return 0,
            None => {}
        }
        let restless = character.bloodline() == Bloodline::Nomadic
            && in_haven
            && now.since(character.arrived_at) > crate::content::RESTLESS_AFTER;
        if restless { crate::content::MENDING } else { usual }
    }

    /// Whether `viewer`, alive, can see the spirit `other`: the Lunar can.
    pub(super) fn lunar_sight(&self, viewer: CharacterId) -> bool {
        self.character(viewer).is_some_and(|character| !character.is_spirit() && character.bloodline() == Bloodline::Lunar)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{BLEED_LASTS, CRACKS_LAST, HAVEN_MENDING, MENDING, RESTLESS_AFTER};
    use crate::types::Safety;

    /// Ash and Bryn on the shoreline, Bryn of `blood`, both at the ledge.
    fn bryn_of(blood: Bloodline) -> (Game, PlaceId) {
        let mut game = Game::new(1, &["Ash", "Bryn"]);
        game.characters[1].bloodline = blood;
        let ledge = game.map.exit(game.anchor(), "north").unwrap().to;
        for index in 0..2 {
            game.occupancy.leave(game.characters[index].place, crate::game::id_at(index));
            game.characters[index].place = ledge;
            game.occupancy.enter(ledge, crate::game::id_at(index));
        }
        (game, ledge)
    }

    const HIGH_TIDE: Tick = Tick::new(LOW_TIDE.as_ticks() + 1);

    #[test]
    fn stone_puts_its_resilience_behind_every_blow() {
        let (mut game, _) = bryn_of(Bloodline::Stone);
        game.characters[1].strengthen(Attribute::Resilience, 40);
        game.characters[0].strengthen(Attribute::Resilience, 40);
        assert_eq!(game.striking_vigor(1), 10);
        assert_eq!(game.striking_vigor(0), 0);
    }

    #[test]
    fn the_tidal_strike_harder_while_the_tide_is_in() {
        let (game, _) = bryn_of(Bloodline::Tidal);
        assert_eq!(game.landing(Some(1), None, 100, Tick::ZERO), 100, "low tide");
        assert_eq!(game.landing(Some(1), None, 100, HIGH_TIDE), 115);
    }

    #[test]
    fn the_covenant_take_less_in_a_house() {
        let (mut game, ledge) = bryn_of(Bloodline::Covenant);
        assert_eq!(game.landing(None, Some(1), 100, Tick::ZERO), 100);
        game.zones[0].hall = Some(ledge);
        assert_eq!(game.landing(None, Some(1), 100, Tick::ZERO), 90);
        assert_eq!(game.landing(None, Some(0), 100, Tick::ZERO), 100, "only the Covenant");
    }

    #[test]
    fn a_heavy_blow_opens_each_lines_wound_and_a_light_one_does_not() {
        let now = Tick::new(100);
        let (mut game, _) = bryn_of(Bloodline::Tidal);
        game.after_blow(1, HEAVY_BLOW - 1, now);
        assert_eq!(game.characters[1].wound, None);
        game.after_blow(1, HEAVY_BLOW, now);
        assert_eq!(game.mending(1, MENDING, false, now), -MENDING, "brine: bleeding");
        assert_eq!(game.mending(1, MENDING, false, now + BLEED_LASTS), MENDING, "and then it closes");

        let (mut game, _) = bryn_of(Bloodline::Stone);
        game.after_blow(1, HEAVY_BLOW, now);
        assert_eq!(game.mending(1, HAVEN_MENDING, true, now), 0, "cracks: no mending at all");
        assert_eq!(game.mending(1, MENDING, false, now + CRACKS_LAST), MENDING);

        let (mut game, _) = bryn_of(Bloodline::Lunar);
        game.characters[1].ectoplasm_for_test(100);
        game.after_blow(1, HEAVY_BLOW, now);
        assert_eq!(game.characters[1].ectoplasm(), 80, "fragile focus");
    }

    #[test]
    fn cinder_renewal_mends_allies_where_one_of_the_ash_line_kills_or_falls() {
        let (mut game, ledge) = bryn_of(Bloodline::Ash);
        game.characters[0].hurt(50);
        game.cinder_renewal(crate::game::id_at(1), ledge);
        assert_eq!(game.characters[0].health(), 54, "8% of the 50 missing");
        // Nothing comes of anyone else.
        game.cinder_renewal(crate::game::id_at(0), ledge);
        assert_eq!(game.characters[0].health(), 54);
    }

    #[test]
    fn the_nomadic_leave_the_wild_quickly_and_tire_of_a_haven() {
        let (mut game, ledge) = bryn_of(Bloodline::Nomadic);
        let camp = game.anchor();
        assert_eq!(game.map.place(camp).unwrap().safety, Safety::Haven);
        assert_eq!(game.travel_percent(1, ledge), 80);
        assert_eq!(game.travel_percent(1, camp), 100);
        assert_eq!(game.travel_percent(0, ledge), 100);
        game.characters[1].arrived_at = Tick::new(1_000);
        assert_eq!(game.mending(1, HAVEN_MENDING, true, Tick::new(1_000) + RESTLESS_AFTER), HAVEN_MENDING);
        assert_eq!(game.mending(1, HAVEN_MENDING, true, Tick::new(1_001) + RESTLESS_AFTER), MENDING, "restless");
    }

    #[test]
    fn the_covenant_near_death_go_slowly() {
        let (mut game, ledge) = bryn_of(Bloodline::Covenant);
        assert_eq!(game.travel_percent(1, ledge), 100);
        game.characters[1].hurt(100 - STASIS_AT_HEALTH);
        assert_eq!(game.travel_percent(1, ledge), 200);
    }

    #[test]
    fn the_lunar_living_see_spirits() {
        let (mut game, _) = bryn_of(Bloodline::Lunar);
        game.characters[0].hurt(100);
        game.fell(0, crate::types::Cause::LongSleep, 0, Tick::new(10), &mut Vec::new());
        let ash = crate::game::id_at(0);
        assert!(game.can_see(crate::game::id_at(1), ash));
        game.characters[1].bloodline = Bloodline::Tidal;
        assert!(!game.can_see(crate::game::id_at(1), ash));
    }

    #[test]
    fn the_ash_line_burns_through_rest_faster() {
        let (mut game, _) = bryn_of(Bloodline::Ash);
        let every = crate::content::RESTED_LASTS.as_ticks() / u64::from(crate::rest::FULLY_RESTED);
        for step in 1..=20 {
            game.tire(Tick::new(step * every));
        }
        let (ash, bryn) = (game.characters[0].rested(), game.characters[1].rested());
        assert_eq!(crate::rest::FULLY_RESTED - ash, 20);
        assert_eq!(crate::rest::FULLY_RESTED - bryn, 30, "half again as fast");
    }
}
