//! The Phase 2 exit test: whole lives, played from the player's seat,
//! each rebuilt exactly from its own command log.
//!
//! - life → death → spirit → rebirth
//! - life → death → back into the body
//! - oversleep → atrophy → the long sleep → forgotten → redeemed
//! - the same, ending in fading

use eq_game::spirit::SpiritTier;
use eq_game::{Attribute, SkillKind, Solo};

fn you(solo: &Solo) -> &eq_game::Character {
    solo.game().character(Solo::YOU).unwrap()
}

fn said(solo: &mut Solo, line: &str) -> String {
    solo.handle(line).lines.join("\n")
}

/// Mines the two ore shelves until mining reaches `level`.
fn learn_to_mine(solo: &mut Solo, level: u32) {
    solo.handle("north");
    let mut shelf = "north";
    while you(solo).skill(SkillKind::Mining).level() < level {
        if said(solo, "gather").contains("too little") {
            solo.handle(shelf);
            shelf = if shelf == "north" { "south" } else { "north" };
            solo.handle("wait 5m");
        }
    }
}

/// Walks from the ore shelves to the grotto and works there until the
/// lurker kills the player.
fn die_in_the_grotto(solo: &mut Solo) {
    while you(solo).place() != solo.game().anchor() {
        solo.handle("south");
    }
    solo.handle("east");
    while !said(solo, "cave").contains("Tidal Cave") {
        solo.handle("wait 1m");
    }
    solo.handle("down");
    for _ in 0..200 {
        if you(solo).is_spirit() {
            return;
        }
        solo.handle("gather");
    }
    panic!("the lurker never finished the job");
}

#[test]
fn a_life_ends_in_the_grotto_and_the_spirit_buys_a_better_next_one() {
    let mut solo = Solo::new(4);
    learn_to_mine(&mut solo, 12);
    let mining = you(&solo).skill(SkillKind::Mining).level();
    die_in_the_grotto(&mut solo);
    assert!(solo.look().contains("You are a spirit."));

    // Severed: pulled to camp. The beach is one step away and has ectoplasm.
    solo.handle("east");
    let mut drawn = String::new();
    while you(&solo).ectoplasm() < 110 {
        drawn = said(&mut solo, "gather");
    }
    assert!(drawn.contains("ectoplasm"), "{drawn}");
    assert_eq!(you(&solo).spirit_tier(), SpiritTier::WanderingPhantom);

    // By now the body has risen in the grotto. Nothing to go back to.
    assert!(said(&mut solo, "return").contains("risen as a guard") || said(&mut solo, "return").contains("not here"));
    let reborn = said(&mut solo, "relinquish");
    assert!(reborn.contains("into life 2"), "{reborn}");

    let next = you(&solo);
    assert!(!next.is_spirit());
    assert_eq!((next.life(), next.kept(), next.ectoplasm()), (2, 20, 10), "100 ectoplasm buys five percent");
    assert_eq!(next.skill(SkillKind::Mining).level(), mining * 20 / 100);
    assert!(next.attribute(Attribute::Lineage).level() > 0);
    assert!(solo.replays_identically());
}

#[test]
fn a_spirit_that_gets_back_in_time_stands_up_again() {
    let mut solo = Solo::new(4);
    solo.handle("east");
    while !said(&mut solo, "cave").contains("Tidal Cave") {
        solo.handle("wait 1m");
    }
    solo.handle("down");
    while !you(&solo).is_spirit() {
        solo.handle("gather");
    }
    // Three places back to the body; the tide is nothing to a spirit.
    for way in ["east", "cave", "down"] {
        solo.handle(way);
    }
    let back = said(&mut solo, "return");
    assert!(back.contains("back in your body"), "{back}");
    assert_eq!((you(&solo).life(), you(&solo).health() <= 25), (1, true));
    assert!(solo.replays_identically());
}

#[test]
fn oversleeping_wastes_the_body_then_loses_it_and_memories_win_a_new_life() {
    let mut solo = Solo::new(4);
    learn_to_mine(&mut solo, 12);
    let peak = you(&solo).skill(SkillKind::Mining).level();

    // Ten days: wasted to the floor, but alive.
    let woke = said(&mut solo, "sleep 10d");
    assert!(woke.contains("wasted"), "{woke}");
    assert_eq!(you(&solo).skill(SkillKind::Mining).level(), peak / 2);
    assert!(you(&solo).attribute(Attribute::Attunement).level() > 0, "the spirit grows as the body fails");

    // Three weeks: gone.
    let gone = said(&mut solo, "sleep 21d");
    assert!(gone.contains("You slept too long."), "{gone}");
    assert!(you(&solo).forgotten().is_some());

    let mut memories = 0;
    for way in ["", "north", "south"] {
        solo.handle(way);
        if said(&mut solo, "seek").contains("memories") {
            memories += 1;
        }
    }
    solo.handle("south");
    solo.handle("east");
    let last = said(&mut solo, "seek");
    assert_eq!(memories, 2);
    assert!(last.contains("You remember who you were."), "{last}");
    assert_eq!(you(&solo).life(), 2);
    assert!(solo.game().shrine().is_empty());
    assert!(solo.replays_identically());
}

#[test]
fn a_forgotten_spirit_that_only_waits_fades_and_leaves_its_name() {
    let mut solo = Solo::new(4);
    solo.handle("sleep 21d");
    assert!(you(&solo).forgotten().is_some());
    let faded = said(&mut solo, "wait 11h");
    assert!(faded.contains("Your time has run out."), "{faded}");
    assert_eq!((you(&solo).life(), you(&solo).kept()), (2, 5));
    assert!(solo.look().contains("Names cut in the stones: You."));
    assert!(solo.replays_identically());
}
