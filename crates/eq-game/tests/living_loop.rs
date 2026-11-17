//! The Phase 1 exit test: does the living loop hold together?
//!
//! - Strip-mining a place visibly degrades it.
//! - Leaving it alone lets it recover.
//! - Mining raises Vigor, and Vigor raises yield.
//! - A bot playing 10,000 ticks neither breaks the world nor stalls it.
//! - Careful play holds the zone at peak; greed wears it down and earns less.
//! - Nobody can hold a deposit stripped by camping on it.

use eq_clock::{Duration, Tick};
use eq_events::{replay, Session};
use eq_game::content::MIN_WORKABLE_STOCK;
use eq_game::{yield_amount, Attribute, Command, Event, Game, Health, Resource, SkillKind};
use eq_nodes::Quality;

const SEED: u64 = 2026;

/// Submits a command now and lets its time cost pass.
fn act(session: &mut Session<Game>, command: Command) -> Vec<Event> {
    let mut events = session.submit(session.now(), command).unwrap();
    let free_at = session.world().hero().busy_until().max(session.now());
    events.extend(session.advance_to(free_at).unwrap());
    events.into_iter().map(|stamped| stamped.value).collect()
}

fn go(session: &mut Session<Game>, label: &str) -> Vec<Event> {
    act(session, Command::Go(label.into()))
}

fn wait(session: &mut Session<Game>, time: Duration) {
    session.advance_to(session.now() + time).unwrap();
}

fn gathered(events: &[Event]) -> Option<u64> {
    events.iter().find_map(|event| match event {
        Event::Gathered { amount, .. } => Some(*amount),
        _ => None,
    })
}

fn quality_here(game: &Game) -> Quality {
    game.here().deposit.expect("a deposit here").node.quality()
}

/// Gathers at the current place until nothing is left. Returns each haul.
fn strip(session: &mut Session<Game>) -> Vec<u64> {
    let mut hauls = Vec::new();
    while let Some(amount) = gathered(&act(session, Command::Gather)) {
        hauls.push(amount);
    }
    hauls
}

#[test]
fn strip_mining_degrades_the_place_and_the_zone() {
    let mut session = Session::new(Game::new(SEED));
    go(&mut session, "north");
    assert_eq!(quality_here(session.world()), Quality::Pristine);

    let hauls = strip(&mut session);

    let (first, last) = (hauls[0], hauls[hauls.len() - 1]);
    assert!(first >= 5 * last, "hauls should shrink sharply: {hauls:?}");
    assert_eq!(quality_here(session.world()), Quality::Depleted);
    assert_eq!(session.world().health(), Health::Balanced);
    assert!(session.world().vitality() < 81);
}

#[test]
fn leaving_a_stripped_place_alone_lets_it_recover() {
    let mut session = Session::new(Game::new(SEED));
    go(&mut session, "north");
    strip(&mut session);

    wait(&mut session, Duration::minutes(45));

    let game = session.world();
    let node = game.here().deposit.unwrap().node;
    assert_eq!(node.quality(), Quality::Pristine);
    assert_eq!(node.stock(), node.capacity());
    assert_eq!((game.vitality(), game.health()), (100, Health::Peak));
}

#[test]
fn a_stripped_place_regrows_slowly() {
    let mut session = Session::new(Game::new(SEED));
    go(&mut session, "north");
    strip(&mut session);

    wait(&mut session, Duration::minutes(10));
    let node = session.world().here().deposit.unwrap().node;
    assert!(node.stock() * 2 < node.capacity(), "{} of {}", node.stock(), node.capacity());
    assert!(node.quality() > Quality::Pristine);
}

#[test]
fn mining_raises_vigor_and_vigor_raises_yield() {
    let mut session = Session::new(Game::new(SEED));
    go(&mut session, "north");
    for _ in 0..3 {
        strip(&mut session);
        wait(&mut session, Duration::minutes(45));
    }

    let hero = session.world().hero();
    assert!(hero.skill(SkillKind::Mining).level() >= 2);
    assert!(hero.attribute(Attribute::Vigor).level() >= 1);
    assert!(hero.attribute(Attribute::Resilience).level() >= 1);
    assert_eq!(hero.attribute(Attribute::Finesse).level(), 0);

    let weak = yield_amount(12, Quality::Pristine, Health::Balanced, 0);
    let strong = yield_amount(12, Quality::Pristine, Health::Balanced, 30);
    assert!(strong > weak);
}

#[test]
fn camping_on_a_deposit_cannot_hold_it_stripped() {
    let mut session = Session::new(Game::new(SEED));
    go(&mut session, "north");
    strip(&mut session);
    assert_eq!(quality_here(session.world()), Quality::Depleted);

    // Stand on the ledge and take everything the moment it can be taken.
    let mut hauls = Vec::new();
    while session.now() < Tick::new(BOT_TICKS) {
        match gathered(&act(&mut session, Command::Gather)) {
            Some(amount) => hauls.push(amount),
            None => wait(&mut session, Duration::seconds(5)),
        }
    }

    assert!(quality_here(session.world()) <= Quality::Worn, "still {:?}", quality_here(session.world()));
    assert_ne!(session.world().health(), Health::Collapsed);
    assert!(hauls.iter().all(|&amount| amount > 0), "an empty haul: {hauls:?}");
}

/// How a bot decides whether to gather from the deposit in front of it.
type Policy = fn(&Game) -> bool;

/// Takes whatever can be taken.
fn greedy(game: &Game) -> bool {
    game.here().deposit.is_some_and(|deposit| deposit.node.stock() >= MIN_WORKABLE_STOCK)
}

/// Takes only from deposits that are pristine and at least half full.
fn careful(game: &Game) -> bool {
    game.here().deposit.is_some_and(|deposit| {
        deposit.node.quality() == Quality::Pristine && deposit.node.stock() * 2 >= deposit.node.capacity()
    })
}

struct BotRun {
    session: Session<Game>,
    /// Amount gathered in each quarter of the run.
    quarters: [u64; 4],
    /// The lowest vitality seen.
    lowest_vitality: i64,
}

/// Walks a fixed circuit of the shoreline for `ticks`, gathering wherever
/// `policy` allows.
fn run_bot(policy: Policy, ticks: u64) -> BotRun {
    const CIRCUIT: [&str; 8] = ["north", "south", "east", "cave", "out", "east", "west", "west"];
    let mut run = BotRun { session: Session::new(Game::new(SEED)), quarters: [0; 4], lowest_vitality: 100 };
    let mut step = 0;

    while run.session.now() < Tick::new(ticks) {
        let quarter = (run.session.now().get() * 4 / ticks) as usize;
        if policy(run.session.world()) {
            let events = act(&mut run.session, Command::Gather);
            run.quarters[quarter] += gathered(&events).unwrap_or(0);
        } else {
            let label = CIRCUIT[step % CIRCUIT.len()];
            let events = go(&mut run.session, label);
            let trapped = label == "out" && events.contains(&Event::ExitClosed { label: label.into() });
            if trapped {
                wait(&mut run.session, Duration::minutes(1));
            } else {
                step += 1;
            }
        }
        run.lowest_vitality = run.lowest_vitality.min(run.session.world().vitality());
    }
    run
}

fn carried(game: &Game) -> u64 {
    Resource::ALL.into_iter().map(|resource| game.hero().carried(resource)).sum()
}

#[test]
#[ignore = "prints numbers for tuning; run with --ignored --nocapture"]
fn print_bot_numbers() {
    for (name, policy) in [("greedy", greedy as Policy), ("careful", careful as Policy)] {
        let run = run_bot(policy, 10_000);
        let game = run.session.world();
        println!(
            "{name}: quarters {:?} total {} lowest vitality {} end vitality {} mining {} foraging {} vigor {} finesse {} commands {}",
            run.quarters,
            carried(game),
            run.lowest_vitality,
            game.vitality(),
            game.hero().skill(SkillKind::Mining).level(),
            game.hero().skill(SkillKind::Foraging).level(),
            game.hero().attribute(Attribute::Vigor).level(),
            game.hero().attribute(Attribute::Finesse).level(),
            run.session.log().len(),
        );
    }
}

const BOT_TICKS: u64 = 10_000;

#[test]
fn a_careful_bot_earns_steadily_and_holds_the_zone_at_peak() {
    let run = run_bot(careful, BOT_TICKS);
    let game = run.session.world();

    let (least, most) = (run.quarters.iter().min().unwrap(), run.quarters.iter().max().unwrap());
    assert!(*least > 0, "the loop flatlined: {:?}", run.quarters);
    assert!(most < &(least * 2), "the loop is unstable: {:?}", run.quarters);
    assert!(carried(game) < BOT_TICKS, "the loop exploded: {}", carried(game));
    assert!(run.lowest_vitality >= 81, "the zone left peak health: vitality fell to {}", run.lowest_vitality);
    assert!(game.hero().skill(SkillKind::Foraging).level() < 100);
}

#[test]
fn a_greedy_bot_wears_the_zone_down_and_earns_less_for_it() {
    let greedy_run = run_bot(greedy, BOT_TICKS);
    let careful_run = run_bot(careful, BOT_TICKS);

    assert!(greedy_run.lowest_vitality <= 50, "vitality only fell to {}", greedy_run.lowest_vitality);
    assert!(greedy_run.quarters.iter().all(|&amount| amount > 0), "greed flatlined: {:?}", greedy_run.quarters);
    let (greedy_total, careful_total) = (carried(greedy_run.session.world()), carried(careful_run.session.world()));
    assert!(careful_total * 2 > greedy_total * 3, "careful {careful_total} vs greedy {greedy_total}");
}

#[test]
fn the_zone_recovers_once_a_greedy_bot_stops() {
    let mut run = run_bot(greedy, BOT_TICKS);
    wait(&mut run.session, Duration::hours(2));
    let game = run.session.world();
    assert_eq!((game.vitality(), game.health()), (100, Health::Peak));
}

#[test]
fn ten_thousand_ticks_replay_to_the_same_world() {
    for policy in [greedy as Policy, careful as Policy] {
        let run = run_bot(policy, BOT_TICKS);
        let (rebuilt, _) = replay(Game::new(SEED), run.session.log(), run.session.now());
        assert_eq!(&rebuilt, run.session.world());

        let (other_seed, _) = replay(Game::new(SEED + 1), run.session.log(), run.session.now());
        assert_ne!(&other_seed, run.session.world());
    }
}
