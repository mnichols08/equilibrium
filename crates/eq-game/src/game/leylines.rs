//! Leyline Harnessing: the living tap the land for ectoplasm.
//!
//! The dead draw ectoplasm wherever the living would work. The living
//! can too, at a price the dead do not pay: every tap takes from the
//! deposit as a haul would, with the strain that brings, and tapping a
//! deposit already under pressure taints whoever does it.

use alloc::vec::Vec;

use eq_clock::Tick;

use super::Game;
use crate::content::{DRAW_TIME, MIN_WORKABLE_STOCK, TAP_TAKES};
use crate::event::Outcome;
use crate::haul::{pressure_percent, taint_for};
use crate::types::Safety;

impl Game {
    /// Taps the deposit where the character at `index` stands.
    pub(super) fn tap(&mut self, index: usize, now: Tick, outcomes: &mut Vec<Outcome>) {
        let place = self.characters[index].place();
        let open = self.map.place(place).is_some_and(|data| data.safety != Safety::Haven);
        let Some(deposit) = self.map.place_mut(place).and_then(|data| data.deposit.as_mut()).filter(|_| open) else {
            return outcomes.push(Outcome::NothingToGather);
        };
        if deposit.node.stock() < MIN_WORKABLE_STOCK {
            return outcomes.push(Outcome::TooSparse { resource: deposit.resource });
        }
        let pressure = pressure_percent(&deposit.node);
        deposit.node.harvest(TAP_TAKES);
        let character = &mut self.characters[index];
        let amount = character.draw(outcomes);
        character.busy_until = now + DRAW_TIME;
        outcomes.push(Outcome::Drew { amount, held: character.ectoplasm() });
        outcomes.extend(character.shift_taint(taint_for(pressure)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Command;
    use crate::game::id_at;
    use eq_events::World;

    #[test]
    fn the_living_tap_the_land_for_ectoplasm_and_the_land_pays() {
        let mut game = Game::new(1, &["Ash"]);
        let mut events = Vec::new();
        // Not in a haven.
        game.apply(&Command::tap(id_at(0)), Tick::ZERO, &mut events);
        assert!(events.iter().any(|event| matches!(event, crate::Event::Character { outcome: Outcome::NothingToGather, .. })));
        game.apply(&Command::go(id_at(0), "north"), Tick::ZERO, &mut events);
        let ledge = game.characters[0].place();
        let stock = |game: &Game| game.map.place(ledge).unwrap().deposit.unwrap().node.stock();
        let before = stock(&game);
        let mut events = Vec::new();
        game.apply(&Command::tap(id_at(0)), Tick::new(100), &mut events);
        let held = game.character(id_at(0)).unwrap().ectoplasm();
        assert!(held > 0);
        assert!(events.iter().any(|event| matches!(event, crate::Event::Character { outcome: Outcome::Drew { amount, .. }, .. } if *amount == held)));
        assert_eq!(before - stock(&game), TAP_TAKES);
        // Tapping a deposit pressed hard enough taints.
        let node = &mut game.map.place_mut(ledge).unwrap().deposit.as_mut().unwrap().node;
        while node.stock() > node.capacity() / 3 {
            node.harvest(10);
        }
        game.apply(&Command::tap(id_at(0)), Tick::new(200), &mut Vec::new());
        assert!(game.character(id_at(0)).unwrap().taint() > 0);
    }
}
