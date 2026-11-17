//! How the simulation holds up with many characters and long runs.

mod common;

use std::time::Instant;

use common::{fresh_world, Crowd};
use eq_clock::{Duration, Tick};
use eq_events::{replay, Session};
use eq_game::bots::{Plan, Policy};
use eq_game::content::DEPOSIT_ROUTES;
use eq_game::Game;

/// A crowd of `heads`, alternating greedy and careful, spread over every
/// deposit and the roaming circuit.
fn mixed_crowd(heads: usize) -> Crowd {
    let plans: Vec<Plan> = (0..heads)
        .map(|index| {
            let policy = if index % 2 == 0 { Policy::Greedy } else { Policy::Careful };
            match index % 3 {
                0 => Plan::roam(policy),
                _ => Plan::camp(policy, DEPOSIT_ROUTES[index % DEPOSIT_ROUTES.len()]),
            }
        })
        .collect();
    Crowd::new(&plans)
}

#[test]
fn five_hundred_characters_share_the_zone_and_replay_identically() {
    const HEADS: usize = 500;
    let mut crowd = mixed_crowd(HEADS);
    crowd.run(Duration::seconds(1_000));

    let game = crowd.game();
    assert_eq!(game.population(), HEADS);
    let placed: usize = game.map().places().map(|(place, _)| game.occupants(place).len()).sum();
    assert_eq!(placed, HEADS, "everyone should be in exactly one place");
    assert!(crowd.session.log().len() > HEADS, "the crowd should have been busy");

    let (rebuilt, _) = replay(fresh_world(HEADS), crowd.session.log(), crowd.session.now());
    assert_eq!(&rebuilt, game);
}

#[test]
fn a_world_week_with_nobody_playing_runs_and_stays_healthy() {
    let mut session = Session::new(Game::new(1, &["Sleeper"]));
    session.advance_to(Tick::new(Duration::days(7).as_ticks())).unwrap();
    assert_eq!(session.world().vitality(), 100);
}

#[test]
#[ignore = "prints timings; run with --release -- --ignored --nocapture"]
fn print_timings() {
    let idle_ticks = 5_000_000;
    let started = Instant::now();
    let mut session = Session::new(Game::new(1, &["Sleeper"]));
    session.advance_to(Tick::new(idle_ticks)).unwrap();
    let per_second = idle_ticks as f64 / started.elapsed().as_secs_f64();
    println!("idle world: {:.1} million ticks per second ({:.0} world-days per second)", per_second / 1e6, per_second / 86_400.0);

    for heads in [10, 100, 1_000, 5_000] {
        let ticks = 2_000;
        let started = Instant::now();
        let mut crowd = mixed_crowd(heads);
        crowd.run(Duration::seconds(ticks));
        let played = started.elapsed();
        let commands = crowd.session.log().len();

        let started = Instant::now();
        let (rebuilt, events) = replay(fresh_world(heads), crowd.session.log(), crowd.session.now());
        let replayed = started.elapsed();
        assert_eq!(&rebuilt, crowd.game());
        println!(
            "{heads:>5} characters, {ticks} ticks: {commands:>8} commands, {:>8} events; play {:>7.1} ms; replay {:>7.1} ms ({:.2} million commands per second)",
            events.len(),
            played.as_secs_f64() * 1e3,
            replayed.as_secs_f64() * 1e3,
            commands as f64 / replayed.as_secs_f64() / 1e6,
        );
    }
}
