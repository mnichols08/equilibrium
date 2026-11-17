//! The realm without a network: who gets in, who is told what, and
//! what survives a restart.

mod common;

use common::{pass, told, unsaved, Scratch};
use eq_game::content::{DISCONNECT_GRACE, SETTLE_TIME};
use eq_server::Refusal;

#[test]
fn a_new_name_makes_a_character_and_the_same_name_and_key_comes_back_to_it() {
    let mut realm = unsaved(1, 0);
    let mikey = realm.enter("Mikey", "pass", false).unwrap();
    assert!(mikey.new);
    assert!(told(&mikey.deliveries, mikey.who).contains("You walk out of the tide at Tidewatch Camp"));
    assert!(told(&mikey.deliveries, mikey.who).contains("Tidewatch Camp [haven]"));

    realm.leave(mikey.who);
    let again = realm.enter("mikey", "pass", false).unwrap();
    assert_eq!((again.who, again.new, again.name.as_str()), (mikey.who, false, "Mikey"));
    assert_eq!(realm.game().population(), 1);
}

#[test]
fn bad_names_wrong_keys_and_bot_names_are_refused() {
    let mut realm = unsaved(1, 2);
    for name in ["", "ab", "has space", "tr0ll", "waytoolongtobeaname"] {
        assert_eq!(realm.enter(name, "k", false).unwrap_err(), Refusal::BadName, "{name:?}");
    }
    realm.enter("Mikey", "pass", false).unwrap();
    assert_eq!(realm.enter("MIKEY", "guess", false).unwrap_err(), Refusal::WrongKey);
    assert_eq!(realm.enter("Ash", "anything", false).unwrap_err(), Refusal::Taken);
    assert_eq!(realm.game().population(), 3);
}

#[test]
fn each_player_is_told_only_what_they_would_notice() {
    let mut realm = unsaved(1, 0);
    let ada = realm.enter("Ada", "a", false).unwrap().who;
    let ben = realm.enter("Ben", "b", false).unwrap();
    assert!(told(&ben.deliveries, ada).contains("Ben walks out of the tide"), "those at camp see a newcomer");
    let ben = ben.who;

    let moved = realm.line(ada, "north");
    assert!(told(&moved, ada).contains("You make your way to Ore Ledge."));
    assert!(told(&moved, ada).contains("Ore Ledge [wilderness]"), "the mover is shown the new place");
    assert_eq!(told(&moved, ben), "Ada leaves for Ore Ledge.");

    pass(&mut realm, 30);
    let hauled = realm.line(ada, "gather");
    assert!(told(&hauled, ada).contains("You gather "));
    assert_eq!(told(&hauled, ben), "", "Ben is at camp and sees nothing of it");

    let view = realm.line(ben, "look");
    assert!(told(&view, ben).contains("Tidewatch Camp"));
    assert_eq!(told(&view, ada), "");
}

#[test]
fn time_is_not_the_players_to_spend_and_sleep_lasts_until_they_wake() {
    let mut realm = unsaved(1, 0);
    let ada = realm.enter("Ada", "a", false).unwrap().who;
    let before = realm.now();
    assert!(told(&realm.line(ada, "wait 1h"), ada).contains("Time passes by itself here"));
    assert_eq!(realm.now(), before);

    assert_eq!(told(&realm.line(ada, "sleep 8h"), ada), "You settle down to sleep.");
    pass(&mut realm, 600);
    assert!(realm.game().character(ada).unwrap().is_asleep(realm.now()));
    assert!(told(&realm.line(ada, "wake"), ada).starts_with("You wake after "));
    assert!(told(&realm.line(ada, "quit"), ada).contains("stays in the world"));
}

#[test]
fn someone_who_drops_away_stays_up_for_the_grace_then_lies_down_where_they_stand() {
    let mut realm = unsaved(1, 0);
    let ada = realm.enter("Ada", "a", false).unwrap().who;
    let ben = realm.enter("Ben", "b", false).unwrap().who;
    realm.line(ada, "north");
    realm.line(ben, "north");
    pass(&mut realm, 30);

    realm.leave(ada);
    pass(&mut realm, DISCONNECT_GRACE.as_ticks() - 1);
    assert!(!realm.game().character(ada).unwrap().is_lying_down(), "still up within the grace");
    let seen = pass(&mut realm, 2);
    assert_eq!(told(&seen, ben), "Ada settles down to sleep.");
    assert_eq!(told(&seen, ada), "", "nobody is listening for Ada");
    pass(&mut realm, SETTLE_TIME.as_ticks());
    assert!(realm.game().character(ada).unwrap().is_asleep(realm.now()));

    // Back again: told where they are, and that they are asleep.
    let back = realm.enter("Ada", "a", false).unwrap();
    assert!(told(&back.deliveries, ada).contains("You are asleep. Type \"wake\" to get up."));
}

#[test]
fn coming_back_within_the_grace_keeps_you_on_your_feet() {
    let mut realm = unsaved(1, 0);
    let ada = realm.enter("Ada", "a", false).unwrap().who;
    realm.leave(ada);
    pass(&mut realm, 60);
    realm.enter("Ada", "a", false).unwrap();
    pass(&mut realm, DISCONNECT_GRACE.as_ticks());
    assert!(!realm.game().character(ada).unwrap().is_lying_down());
}

#[test]
fn a_realm_comes_back_from_disk_exactly_as_it_was_left() {
    let scratch = Scratch::new("restart");
    let mut realm = scratch.realm(9, 3);
    let ada = realm.enter("Ada", "a", false).unwrap().who;
    realm.line(ada, "north");
    pass(&mut realm, 3_000);
    realm.line(ada, "gather");
    pass(&mut realm, 777);
    realm.close();
    let (world, now) = (realm.game().clone(), realm.now());
    drop(realm);

    // Different flags on the second start change nothing: the realm is what was saved.
    let mut again = scratch.realm(1234, 0);
    assert_eq!(again.seed(), 9);
    assert_eq!(again.now(), now);
    assert_eq!(again.game(), &world);
    assert!(!again.is_online(ada), "nobody is connected after a restart");
    assert_eq!(again.enter("Ada", "wrong", false).unwrap_err(), Refusal::WrongKey, "keys are saved too");
    assert_eq!(again.enter("Ada", "a", false).unwrap().who, ada);
}

#[test]
fn a_restart_changes_nothing_about_what_happens_next() {
    // No bots here on purpose. The world comes back exactly, but bots do
    // not remember where they were in their rounds, so after a restart
    // they may choose differently from bots that never stopped.
    let scratch = Scratch::new("continuity");
    let mut straight = unsaved(5, 0);
    let mut stopped = scratch.realm(5, 0);
    for realm in [&mut straight, &mut stopped] {
        let ada = realm.enter("Ada", "a", false).unwrap().who;
        realm.line(ada, "east");
        pass(realm, 2_000);
    }
    stopped.close();
    drop(stopped);
    let mut resumed = scratch.realm(5, 0);
    assert_eq!(resumed.game(), straight.game());

    for realm in [&mut straight, &mut resumed] {
        let ada = realm.enter("Ada", "a", false).unwrap().who;
        for line in ["gather", "east", "gather"] {
            realm.line(ada, line);
            pass(realm, 40);
        }
    }
    assert_eq!(resumed.game(), straight.game());
}

#[test]
fn a_client_that_draws_gets_a_scene_with_everything_it_is_told_and_every_few_ticks() {
    let mut realm = unsaved(1, 0);
    let entered = realm.enter("Ada", "a", true).unwrap();
    let ada = entered.who;
    let ben = realm.enter("Ben", "b", false).unwrap().who;
    let first = entered.deliveries.iter().find(|delivery| delivery.to == ada).unwrap();
    let scene = first.scene.as_ref().expect("a scene with the greeting");
    assert_eq!(scene["you"]["place_name"], "Tidewatch Camp");

    // Acting brings a fresh scene, even when nothing is said.
    let moved = realm.line(ada, "north");
    let after = moved.iter().find(|delivery| delivery.to == ada).unwrap().scene.as_ref().unwrap();
    assert_eq!(after["you"]["place_name"], "Ore Ledge");
    assert!(moved.iter().filter(|delivery| delivery.to == ben).all(|delivery| delivery.scene.is_none()), "Ben only reads lines");

    // Views cost nothing and change nothing: no scene.
    assert!(realm.line(ada, "stats").iter().all(|delivery| delivery.scene.is_none()));

    // Left alone, the picture is still kept fresh.
    let quiet = pass(&mut realm, 10);
    let scenes: Vec<_> = quiet.iter().filter(|delivery| delivery.to == ada && delivery.scene.is_some()).collect();
    assert_eq!(scenes.len(), 2, "one scene every five ticks");
    assert!(scenes.iter().all(|delivery| delivery.lines.is_empty()));
    assert!(quiet.iter().all(|delivery| delivery.to != ben));

    // Coming back as a terminal turns the scenes off.
    realm.leave(ada);
    let plain = realm.enter("Ada", "a", false).unwrap();
    assert!(plain.deliveries.iter().all(|delivery| delivery.scene.is_none()));
}
