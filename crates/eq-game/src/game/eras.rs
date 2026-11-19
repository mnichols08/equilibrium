//! What each era does to trade and to who may be struck where.

use alloc::vec::Vec;

use eq_clock::Tick;
use eq_events::World;
use eq_world_graph::PlaceId;

use super::Game;
use crate::board::{ContractId, Lot};
use crate::character::CharacterId;
use crate::combat::{Refusal, RobRefusal};
use crate::command::Command;
use crate::era::Era;
use crate::event::Event;
use crate::types::{Resource, Safety};

const ASH: CharacterId = CharacterId::new(0);
const BRYN: CharacterId = CharacterId::new(1);
const COLE: CharacterId = CharacterId::new(2);
const DOV: CharacterId = CharacterId::new(3);

/// Ash and Bryn found a land each; Cole and Dov are of Ash's.
fn two_lands() -> Game {
    let mut game = Game::new_world(1, 2, &[]);
    let mut events = Vec::new();
    game.apply(&Command::join(ASH, "Ash"), Tick::ZERO, &mut events);
    game.apply(&Command::join(BRYN, "Bryn"), Tick::ZERO, &mut events);
    for (who, name) in [(COLE, "Cole"), (DOV, "Dov")] {
        game.apply(&Command::invite(ASH, name), Tick::ZERO, &mut events);
        game.apply(&Command::join(who, name), Tick::ZERO, &mut events);
    }
    game
}

fn put(game: &mut Game, who: CharacterId, place: PlaceId) {
    let index = game.index_of(who).unwrap();
    game.occupancy.leave(game.characters[index].place, who);
    game.characters[index].place = place;
    game.characters[index].busy_until = Tick::ZERO;
    game.occupancy.enter(place, who);
}

/// A place in Ash's land that is neither a haven nor contested.
fn wilds(game: &Game) -> PlaceId {
    let land = game.zone_at(game.character(ASH).unwrap().home());
    land.place_ids().find(|&place| game.map.place(place).is_some_and(|data| data.safety == Safety::Buffer)).unwrap()
}

/// `by` sells 10 ore for 50 kelp to `buyer` at the board where `by`
/// stands. Returns the kelp waiting there for `by`, and for Ash.
fn sell(game: &mut Game, by: CharacterId, buyer: CharacterId, now: u64) -> (u64, u64) {
    let mut events = Vec::new();
    let index = game.index_of(by).unwrap();
    game.characters[index].stow(Resource::Ore, 10);
    let buyer_index = game.index_of(buyer).unwrap();
    game.characters[buyer_index].stow(Resource::Kelp, 50);
    let ore = Lot { resource: Resource::Ore, amount: 10 };
    let kelp = Lot { resource: Resource::Kelp, amount: 50 };
    game.apply(&Command::post(by, ore, kelp), Tick::new(now), &mut events);
    let id = game.zone_at(game.character(by).unwrap().place()).board().contracts().last().map_or(ContractId::new(0), |c| c.id);
    game.apply(&Command::accept(buyer, id), Tick::new(now + 20), &mut events);
    assert!(!events.iter().any(|event| matches!(event, Event::Character { outcome: crate::Outcome::CannotTrade { .. }, .. })), "{events:?}");
    let board = game.zone_at(game.character(by).unwrap().place()).board();
    (board.owed_to(by)[Resource::Kelp.index()], board.owed_to(ASH)[Resource::Kelp.index()])
}

#[test]
fn the_founder_takes_a_share_of_trade_at_their_board_and_twice_as_much_in_peace() {
    let mut game = two_lands();
    assert_eq!(game.era, Era::Rebuilding);
    // Cole sells to Dov at Ash's board: a fifth of the kelp is Ash's.
    assert_eq!(sell(&mut game, COLE, DOV, 100), (40, 10));
    game.era = Era::Conflict;
    // A tenth in harder times, on top of what Cole had waiting.
    assert_eq!(sell(&mut game, COLE, DOV, 200), (40 + 45, 10 + 5));
    // Ash pays nobody on his own board.
    let before = game.zone_at(game.character(ASH).unwrap().place()).board().owed_to(ASH)[Resource::Kelp.index()];
    sell(&mut game, ASH, DOV, 300);
    let after = game.zone_at(game.character(ASH).unwrap().place()).board().owed_to(ASH)[Resource::Kelp.index()];
    assert_eq!(after - before, 50);
}

#[test]
fn in_a_hard_era_anyone_abroad_may_be_struck_without_a_mark() {
    let mut game = two_lands();
    let wilds = wilds(&game);
    put(&mut game, BRYN, wilds);
    put(&mut game, COLE, wilds);
    assert_eq!(game.can_attack(COLE, BRYN), Err(Refusal::Unmarked), "in peace a stranger is safe");
    game.era = Era::Conflict;
    assert_eq!(game.can_attack(COLE, BRYN), Ok(()), "abroad in a time of conflict");
    assert_eq!(game.can_attack(BRYN, COLE), Err(Refusal::Unmarked), "at home is home");
    // A haven still shelters everyone, until the Void.
    let haven = game.character(ASH).unwrap().home();
    put(&mut game, BRYN, haven);
    put(&mut game, COLE, haven);
    assert_eq!(game.can_attack(COLE, BRYN), Err(Refusal::Haven));
}

#[test]
fn in_the_void_a_haven_shelters_only_its_own() {
    let mut game = two_lands();
    let haven = game.character(ASH).unwrap().home();
    put(&mut game, BRYN, haven);
    put(&mut game, COLE, haven);
    game.era = Era::VoidConsumption;
    assert_eq!(game.can_attack(COLE, BRYN), Ok(()));
    assert_eq!(game.can_attack(BRYN, COLE), Err(Refusal::Haven));
    // And a stranger asleep there can be robbed.
    let (bryn, cole) = (game.index_of(BRYN).unwrap(), game.index_of(COLE).unwrap());
    game.characters[bryn].stow(Resource::Kelp, 5);
    game.characters[cole].stow(Resource::Kelp, 5);
    let mut events = Vec::new();
    game.apply(&Command::rest(BRYN), Tick::new(10), &mut events);
    game.apply(&Command::rest(COLE), Tick::new(10), &mut events);
    let asleep = Tick::new(100);
    assert_eq!(game.can_rob(COLE, BRYN, asleep), Ok(()));
    assert_eq!(game.can_rob(BRYN, COLE, asleep), Err(RobRefusal::Haven));
}

#[test]
fn in_the_void_the_veil_is_thin_the_dead_are_seen_and_learn_fast_and_the_land_pays() {
    use crate::types::{Cause, SkillKind};
    let draw = |era: Era| {
        let mut game = Game::new(1, &["Ash", "Bryn"]);
        game.era = era;
        let ledge = game.map.exit(game.anchor(), "north").unwrap().to;
        for index in 0..2 {
            game.occupancy.leave(game.characters[index].place, super::id_at(index));
            game.characters[index].place = ledge;
            game.occupancy.enter(ledge, super::id_at(index));
        }
        game.characters[1].hurt(100);
        game.fell(1, Cause::LongSleep, 0, Tick::new(1), &mut Vec::new());
        // Carried off to camp; walk back to the ledge as a spirit.
        game.occupancy.leave(game.characters[1].place, BRYN);
        game.characters[1].place = ledge;
        game.occupancy.enter(ledge, BRYN);
        game.characters[1].busy_until = Tick::ZERO;
        let stock = |game: &Game| game.map.place(ledge).unwrap().deposit.unwrap().node.stock();
        let before = stock(&game);
        let mut events = Vec::new();
        game.apply(&Command::gather(BRYN), Tick::new(100), &mut events);
        let practised = game.character(BRYN).unwrap().practised(SkillKind::Mining).progress();
        (game.can_see(ASH, BRYN), practised, before - stock(&game))
    };
    let (seen, learned, taken) = draw(Era::Rebuilding);
    assert_eq!((seen, taken), (false, 0));
    let (seen_in_void, learned_in_void, taken_in_void) = draw(Era::VoidConsumption);
    assert!(seen_in_void, "the living see the dead");
    assert_eq!(learned_in_void, learned * 2, "practice counts double");
    assert_eq!(taken_in_void, crate::content::VOID_DRAW_TAKES, "and the land pays for it");
}

#[test]
fn kin_keep_a_better_vigil_than_strangers() {
    use crate::content::{SETTLE_TIME, TEND_TIME};
    use crate::Outcome;
    use eq_clock::Duration;
    // Cole is Ash's kin; Bryn is of another land.
    let mut game = two_lands();
    let haven = game.character(ASH).unwrap().home();
    for who in [ASH, BRYN, COLE] {
        put(&mut game, who, haven);
    }
    let mut events = Vec::new();
    game.apply(&Command::rest(ASH), Tick::ZERO, &mut events);
    let asleep = SETTLE_TIME.as_ticks();
    let tended = |events: &[Event]| {
        events.iter().rev().find_map(|event| match event {
            Event::Character { outcome: Outcome::Tended { vigil, .. }, .. } => Some(*vigil),
            _ => None,
        })
    };
    let mut events = Vec::new();
    game.apply(&Command::tend(BRYN, ASH), Tick::new(asleep), &mut events);
    assert_eq!(tended(&events), Some(Duration::days(1)), "a stranger: a day");
    game.apply(&Command::tend(COLE, ASH), Tick::new(asleep + TEND_TIME.as_ticks()), &mut events);
    assert_eq!(tended(&events), Some(Duration::days(3)), "kin: two more");
}
