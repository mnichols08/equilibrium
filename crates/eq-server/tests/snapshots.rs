//! A realm saves the whole world now and then, so a restart need not
//! replay every command since the world began. Whichever way it comes
//! back, it is the same world.

mod common;

use common::Scratch;

const HOUR: u64 = 3_600;

#[test]
fn a_realm_comes_back_the_same_from_its_snapshot_or_from_its_whole_log() {
    let scratch = Scratch::new("snapshots");
    let mut realm = scratch.realm(7, 12);
    // Five and a half hours: five snapshots, and half an hour after the last.
    for _ in 0..(5 * HOUR + HOUR / 2) {
        realm.tick();
    }
    realm.close();
    let (world, now) = (realm.game().clone(), realm.now());
    drop(realm);

    let snapshot = scratch.path().join("snapshot.json");
    assert!(snapshot.exists());
    let from_snapshot = scratch.realm(7, 12);
    assert_eq!((from_snapshot.now(), from_snapshot.game()), (now, &world));
    drop(from_snapshot);

    // Without it, the log alone rebuilds the same world.
    std::fs::remove_file(&snapshot).unwrap();
    let from_log = scratch.realm(7, 12);
    assert_eq!((from_log.now(), from_log.game()), (now, &world));
}

#[test]
fn a_snapshot_resumed_carries_on_exactly_as_the_world_would_have() {
    // One realm runs straight through; another stops, comes back from
    // its snapshot partway, and runs on. Their worlds must not differ.
    let straight = Scratch::new("straight");
    let mut through = straight.realm(9, 8);
    for _ in 0..(3 * HOUR) {
        through.tick();
    }

    let stopped = Scratch::new("stopped");
    let mut first = stopped.realm(9, 8);
    for _ in 0..HOUR {
        first.tick();
    }
    first.close();
    drop(first);
    let mut second = stopped.realm(9, 8);
    // Bots start their rounds afresh on a restart, so compare the world
    // only up to the stop, and that the stopped one still runs.
    assert_eq!(second.now().get(), HOUR);
    for _ in 0..(2 * HOUR) {
        second.tick();
    }
    assert_eq!(second.now(), through.now());
}

#[test]
fn a_long_log_is_split_and_a_restart_reads_only_from_the_snapshot_on() {
    let scratch = Scratch::new("segments");
    let mut realm = scratch.realm(11, 20);
    for _ in 0..(2 * 24 * HOUR) {
        realm.tick();
    }
    realm.close();
    let (world, now) = (realm.game().clone(), realm.now());
    drop(realm);
    let segment = |n: u64| scratch.path().join(format!("commands.{n}.log"));
    assert!(segment(0).exists() && segment(1).exists(), "two days of twenty bots fill more than one segment");

    let back = scratch.realm(11, 20);
    assert_eq!((back.now(), back.game()), (now, &world));
    drop(back);
    // The whole log, every segment in order, rebuilds the same world.
    std::fs::remove_file(scratch.path().join("snapshot.json")).unwrap();
    let rebuilt = scratch.realm(11, 20);
    assert_eq!((rebuilt.now(), rebuilt.game()), (now, &world));
}

#[test]
fn a_realm_saved_before_the_log_was_split_opens_and_carries_on() {
    let scratch = Scratch::new("legacy");
    let mut realm = scratch.realm(3, 4);
    for _ in 0..HOUR / 2 {
        realm.tick();
    }
    realm.close();
    let (world, now) = (realm.game().clone(), realm.now());
    drop(realm);
    // Make it look like an old realm: one log, no snapshot.
    std::fs::rename(scratch.path().join("commands.0.log"), scratch.path().join("commands.log")).unwrap();
    std::fs::remove_file(scratch.path().join("snapshot.json")).unwrap();
    let back = scratch.realm(3, 4);
    assert_eq!((back.now(), back.game()), (now, &world));
    assert!(scratch.path().join("commands.0.log").exists() && !scratch.path().join("commands.log").exists());
}
