//! The Phase 3 exit test: twenty bots and three players share the
//! shoreline for a week of world time, the server stops halfway
//! through, and nobody can tell.

mod common;

use common::{pass, told, Scratch};
use eq_game::{CharacterId, Health};
use eq_server::Realm;

const HOUR: u64 = 3_600;
const DAY: u64 = 24 * HOUR;

/// What three players do with an evening: one works the near ore, one
/// the beach, one just looks around. Then they all leave.
fn an_evening(realm: &mut Realm) -> [CharacterId; 3] {
    let players = [("Ada", "north"), ("Ben", "east"), ("Cyd", "south")].map(|(name, way)| {
        let who = realm.enter(name, name).unwrap().who;
        // Whoever was left asleep gets up first.
        realm.line(who, "wake");
        (who, way)
    });
    for (who, way) in players {
        let at_camp = realm.game().character(who).unwrap().place() == realm.game().anchor();
        if at_camp {
            realm.line(who, way);
        }
    }
    // A dozen hauls each, well spaced: careful enough to stay clean.
    for _ in 0..12 {
        pass(realm, HOUR / 12);
        for (who, _) in players {
            realm.line(who, "gather");
        }
    }
    for (who, _) in players {
        realm.leave(who);
    }
    players.map(|(who, _)| who)
}

fn a_day(realm: &mut Realm) -> [CharacterId; 3] {
    let players = an_evening(realm);
    pass(realm, DAY - HOUR);
    players
}

#[test]
fn a_week_with_a_restart_in_the_middle() {
    let scratch = Scratch::new("week");
    let mut realm = scratch.realm(11, 20);
    let mut players = [CharacterId::new(0); 3];
    for _ in 0..3 {
        players = a_day(&mut realm);
    }

    // Stop. Everyone is offline and asleep where they worked.
    realm.close();
    let (world, now) = (realm.game().clone(), realm.now());
    let beds = players.map(|who| world.character(who).unwrap().place());
    assert!(players.iter().all(|&who| world.character(who).unwrap().is_asleep(now)));
    assert_ne!(beds[0], world.anchor(), "Ada sleeps out on the ledge");
    drop(realm);

    // Start again: the same world, to the tick, with the sleepers where they lay.
    let mut realm = scratch.realm(11, 20);
    assert_eq!(realm.now(), now);
    assert_eq!(realm.game(), &world);
    assert_eq!(players.map(|who| realm.game().character(who).unwrap().place()), beds);

    for _ in 3..7 {
        players = a_day(&mut realm);
    }
    assert_eq!(realm.now().get(), 7 * DAY);
    assert_eq!(realm.game().population(), 23);

    // A week on: the careful majority has kept the shoreline alive, and
    // the players have something to show for their evenings.
    assert_ne!(realm.game().health(), Health::Collapsed, "vitality {}", realm.game().vitality());
    let ada = realm.game().character(players[0]).unwrap();
    assert!(ada.skill(eq_game::SkillKind::Mining).level() > 0);

    // They are told where they are when they come back, and can carry on.
    let back = realm.enter("Ada", "Ada").unwrap();
    assert!(told(&back.deliveries, players[0]).contains("You are asleep."));
    assert!(told(&realm.line(players[0], "wake"), players[0]).starts_with("You wake after "));

    // And the whole week can be rebuilt from what was saved.
    realm.close();
    let (world, now) = (realm.game().clone(), realm.now());
    drop(realm);
    let rebuilt = scratch.realm(11, 20);
    assert_eq!((rebuilt.now(), rebuilt.game()), (now, &world));
}
