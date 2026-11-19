//! What the world's mood does to the tide.

use alloc::vec::Vec;

use eq_clock::Tick;
use eq_events::World;

use super::Game;
use crate::command::Command;
use crate::content::{LOW_TIDE, TIDE, TIDE_CRASH_HARM};
use crate::event::{Event, Outcome};
use crate::game::id_at;

/// Ash on the shoreline, the world tipped by `lean`.
fn shore(lean: i64) -> Game {
    let mut game = Game::new(1, &["Ash"]);
    game.balance.tip(lean);
    game
}

/// The way from the beach into the cave, and whether it is clear at `at`.
fn cave_clear(game: &Game, at: u64) -> bool {
    let (_, exit) = game.map.all_exits().find(|(_, exit)| exit.label == "cave").unwrap();
    game.is_passable(&exit.passage, Tick::new(at))
}

#[test]
fn war_shortens_low_tide_and_peace_lengthens_it_until_the_water_fails() {
    let low = LOW_TIDE.as_ticks();
    let tide = TIDE.period().as_ticks();
    // An even world: clear for the usual ten minutes.
    let even = shore(0);
    assert!(cave_clear(&even, low - 1) && !cave_clear(&even, low));
    // Restless and raging: a quarter less, then half.
    let restless = shore(30);
    assert!(cave_clear(&restless, low * 3 / 4 - 1) && !cave_clear(&restless, low * 3 / 4));
    let raging = shore(80);
    assert!(cave_clear(&raging, low / 2 - 1) && !cave_clear(&raging, low / 2));
    // Dry: half again as long. Parched: the water never comes.
    let dry = shore(-30);
    assert!(cave_clear(&dry, low * 3 / 2 - 1) && !cave_clear(&dry, low * 3 / 2));
    let parched = shore(-80);
    assert!((0..tide).step_by(60).all(|at| cave_clear(&parched, at)));
}

#[test]
fn a_raging_tide_throws_about_whoever_it_catches_on_the_shore() {
    for (lean, harmed) in [(0, false), (80, true)] {
        let mut game = shore(lean);
        let mut events = Vec::new();
        game.apply(&Command::go(id_at(0), "east"), Tick::ZERO, &mut events);
        let beach = game.characters[0].place();
        assert!(game.map.exits(beach).iter().any(|exit| exit.label == "cave"));
        let came_in = if lean == 0 { LOW_TIDE.as_ticks() } else { LOW_TIDE.as_ticks() / 2 };
        let mut events = Vec::new();
        for tick in 1..=came_in {
            game.tick(Tick::new(tick), &mut events);
        }
        let battered = events.iter().any(|event| {
            matches!(event, Event::Character { outcome: Outcome::Battered { damage, .. }, .. } if *damage == TIDE_CRASH_HARM)
        });
        assert_eq!(battered, harmed, "lean {lean}");
    }
}
