//! The Phase 1 exit test: does the living loop hold together for one
//! player?
//!
//! - Strip-mining a place visibly degrades it.
//! - Leaving it alone lets it recover.
//! - Mining raises Vigor, and Vigor raises yield.
//! - A bot playing 10,000 ticks neither breaks the world nor stalls it.
//! - Careful play holds the zone at peak; greed wears it down and earns less,
//!   but one greedy player cannot collapse it alone.
//!
//! What happens when players share the zone is in `shared_zone.rs`.

mod common;

use common::{fresh_world, gathered, Crowd, SEED};
use eq_clock::Duration;
use eq_events::{replay, Session};
use eq_game::bots::{Plan, Policy};
use eq_game::{Attribute, CharacterId, Command, Event, Game, Haul, Health, SkillKind};
use eq_nodes::{Node, Quality};

const YOU: CharacterId = CharacterId::new(0);
const BOT_TIME: Duration = Duration::seconds(10_000);

fn solo() -> Session<Game> {
    Session::new(Game::new(SEED, &["You"]))
}

/// Submits a command now and lets its time cost pass.
fn act(session: &mut Session<Game>, command: Command) -> Vec<Event> {
    let mut events = session.submit(session.now(), command).unwrap();
    let free_at = session.world().character(YOU).unwrap().busy_until().max(session.now());
    events.extend(session.advance_to(free_at).unwrap());
    events.into_iter().map(|stamped| stamped.value).collect()
}

fn wait(session: &mut Session<Game>, time: Duration) {
    session.advance_to(session.now() + time).unwrap();
}

fn node_here(game: &Game) -> Node {
    game.place_of(YOU).unwrap().deposit.expect("a deposit here").node
}

/// Gathers at the current place until nothing more can be taken. Returns
/// each haul.
fn strip(session: &mut Session<Game>) -> Vec<u64> {
    let mut hauls = Vec::new();
    while let Some(amount) = gathered(&act(session, Command::gather(YOU))) {
        hauls.push(amount);
    }
    hauls
}

/// A session with the player standing on a freshly stripped ore ledge.
fn stripped_ledge() -> (Session<Game>, Vec<u64>) {
    let mut session = solo();
    act(&mut session, Command::go(YOU, "north"));
    let hauls = strip(&mut session);
    (session, hauls)
}

#[test]
fn strip_mining_degrades_the_place_and_dents_the_zone() {
    let (session, hauls) = stripped_ledge();

    let (best, last) = (*hauls.iter().max().unwrap(), hauls[hauls.len() - 1]);
    assert!(best >= 4 * last, "hauls should shrink sharply: {hauls:?}");
    assert_eq!(node_here(session.world()).quality(), Quality::Depleted);
    // One stripped place out of nine hurts the zone without breaking it.
    let vitality = session.world().vitality();
    assert!((81..100).contains(&vitality), "vitality {vitality}");
}

#[test]
fn leaving_a_stripped_place_alone_lets_it_recover() {
    let (mut session, _) = stripped_ledge();

    wait(&mut session, Duration::minutes(45));

    let game = session.world();
    let node = node_here(game);
    assert_eq!(node.quality(), Quality::Pristine);
    assert_eq!(node.stock(), node.capacity());
    assert_eq!((game.vitality(), game.health()), (100, Health::Peak));
}

#[test]
fn a_stripped_place_regrows_slowly() {
    let (mut session, _) = stripped_ledge();

    wait(&mut session, Duration::minutes(10));

    let node = node_here(session.world());
    assert!(node.stock() * 2 < node.capacity(), "{} of {}", node.stock(), node.capacity());
    assert!(node.quality() > Quality::Pristine);
}

#[test]
fn mining_raises_vigor_and_vigor_raises_yield() {
    let (mut session, _) = stripped_ledge();
    for _ in 0..2 {
        wait(&mut session, Duration::minutes(45));
        strip(&mut session);
    }

    let you = session.world().character(YOU).unwrap();
    assert!(you.skill(SkillKind::Mining).level() >= 2);
    assert!(you.attribute(Attribute::Vigor).level() >= 1);
    assert!(you.attribute(Attribute::Resilience).level() >= 1);
    assert_eq!(you.attribute(Attribute::Finesse).level(), 0);

    let haul = |attribute_level| Haul { taken: 12, quality: Quality::Pristine, health: Health::Balanced, attribute_level, taint: 0 };
    assert!(haul(30).amount() > haul(0).amount());
}

fn run_alone(plan: Plan) -> Crowd {
    let mut crowd = Crowd::new(&[plan]);
    crowd.run(BOT_TIME);
    crowd
}

#[test]
fn a_careful_bot_earns_steadily_and_holds_the_zone_at_peak() {
    let run = run_alone(Plan::roam(Policy::Careful));
    let quarters = run.quarters();

    let (least, most) = (quarters.iter().min().unwrap(), quarters.iter().max().unwrap());
    assert!(*least > 0, "the loop flatlined: {quarters:?}");
    assert!(most < &(least * 2), "the loop is unstable: {quarters:?}");
    assert!(run.carried_by_all() < 2 * BOT_TIME.as_ticks(), "the loop exploded: {}", run.carried_by_all());
    assert!(run.lowest_vitality() >= 81, "the zone left peak health: {}", run.lowest_vitality());
}

#[test]
fn a_greedy_bot_wears_the_zone_down_and_earns_less_for_it() {
    let greedy_run = run_alone(Plan::roam(Policy::Greedy));
    let careful_run = run_alone(Plan::roam(Policy::Careful));

    let lowest = greedy_run.lowest_vitality();
    assert!((31..=50).contains(&lowest), "greed alone should wear the zone down but not collapse it: {lowest}");
    assert!(greedy_run.quarters().iter().all(|&amount| amount > 0), "greed flatlined: {:?}", greedy_run.quarters());
    let (greedy_total, careful_total) = (greedy_run.carried_by_all(), careful_run.carried_by_all());
    assert!(careful_total * 4 > greedy_total * 5, "careful {careful_total} vs greedy {greedy_total}");
}

#[test]
fn the_zone_recovers_once_a_greedy_bot_stops() {
    let mut run = run_alone(Plan::roam(Policy::Greedy));
    run.rest(Duration::hours(2));
    assert_eq!((run.game().vitality(), run.game().health()), (100, Health::Peak));
}

#[test]
fn ten_thousand_ticks_replay_to_the_same_world() {
    for plan in [Plan::roam(Policy::Greedy), Plan::roam(Policy::Careful)] {
        let run = run_alone(plan);
        let (rebuilt, _) = replay(fresh_world(1), run.session.log(), run.session.now());
        assert_eq!(&rebuilt, run.game());

        let (other_seed, _) = replay(Game::new(SEED + 1, &["Bot 0"]), run.session.log(), run.session.now());
        assert_ne!(&other_seed, run.game());
    }
}
