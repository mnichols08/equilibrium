//! What happens when players share the zone.
//!
//! - One camper cannot collapse the zone; a camper on every deposit can,
//!   and holds it collapsed for as long as they stay.
//! - The zone has a carrying capacity: a careful crowd of any size takes
//!   about the same total, so more heads means less each.
//! - A careful crowd holds peak health and takes about twice what a
//!   greedy crowd does.
//! - One greedy player among careful ones takes most of it. Greed pays the
//!   one who defects and costs everyone else: a real commons problem.

mod common;

use common::{careful, greedy, Crowd, Plan, Policy, SEED};
use eq_clock::Duration;
use eq_events::replay;
use eq_game::{Game, Health};

const RUN: Duration = Duration::seconds(10_000);

/// One camper on each of the four deposits.
fn campers(policy: Policy) -> [Plan; 4] {
    [
        Plan::camp(policy, &["north"]),
        Plan::camp(policy, &["east"]),
        Plan::camp(policy, &["east", "east"]),
        Plan::camp(policy, &["east", "cave"]),
    ]
}

fn run(plans: &[Plan]) -> Crowd {
    let mut crowd = Crowd::new(plans);
    crowd.run(RUN);
    crowd
}

fn roamers(policy: Policy, heads: usize) -> Crowd {
    run(&vec![Plan::roam(policy); heads])
}

/// Four roamers, the first `defectors` of them greedy and the rest careful.
fn mixed(defectors: usize) -> Crowd {
    let mut plans = vec![Plan::roam(careful); 4];
    for plan in plans.iter_mut().take(defectors) {
        *plan = Plan::roam(greedy);
    }
    run(&plans)
}

fn collapsed_samples(crowd: &Crowd) -> usize {
    crowd.samples.iter().filter(|sample| sample.vitality <= 30).count()
}

#[test]
fn one_camper_cannot_collapse_the_zone() {
    let crowd = run(&[Plan::camp(greedy, &["north"])]);
    assert!(crowd.lowest_vitality() > 30, "vitality fell to {}", crowd.lowest_vitality());
    assert_eq!(crowd.game().health(), Health::Balanced);
}

#[test]
fn a_camper_on_every_deposit_collapses_the_zone_and_holds_it_there() {
    let crowd = run(&campers(greedy));
    let second_half = &crowd.samples[crowd.samples.len() / 2..];
    let highest = second_half.iter().map(|sample| sample.vitality).max().unwrap();
    let collapsed = second_half.iter().filter(|sample| sample.vitality <= 30).count();

    // It may touch the edge of balanced for a moment, but never gets up.
    assert!(highest <= 40, "the zone got up again: vitality reached {highest}");
    assert!(collapsed * 10 >= second_half.len() * 9, "collapsed for only {collapsed} of {} samples", second_half.len());
    assert_eq!(crowd.game().health(), Health::Collapsed);
}

#[test]
fn a_collapsed_zone_recovers_when_the_crowd_stops() {
    let mut crowd = run(&campers(greedy));
    crowd.rest(Duration::minutes(30));
    assert_eq!((crowd.game().vitality(), crowd.game().health()), (100, Health::Peak));
}

#[test]
fn a_careful_crowd_holds_peak_and_takes_twice_what_a_greedy_one_does() {
    let (careful_crowd, greedy_crowd) = (roamers(careful, 4), roamers(greedy, 4));

    assert!(careful_crowd.lowest_vitality() >= 81, "vitality fell to {}", careful_crowd.lowest_vitality());
    assert!(collapsed_samples(&greedy_crowd) * 2 > greedy_crowd.samples.len());
    let (careful_total, greedy_total) = (careful_crowd.carried_by_all(), greedy_crowd.carried_by_all());
    assert!(careful_total > greedy_total * 2, "careful {careful_total} vs greedy {greedy_total}");
}

#[test]
fn the_zone_feeds_a_fixed_total_so_more_heads_means_less_each() {
    let totals: Vec<u64> = [1, 2, 4, 8].into_iter().map(|heads| roamers(careful, heads).carried_by_all()).collect();

    let (least, most) = (*totals.iter().min().unwrap(), *totals.iter().max().unwrap());
    assert!(most * 9 <= least * 10, "totals should sit within a tenth of each other: {totals:?}");

    let per_head: Vec<u64> = totals.iter().zip([1, 2, 4, 8]).map(|(total, heads)| total / heads).collect();
    assert!(per_head.windows(2).all(|pair| pair[0] > pair[1]), "{per_head:?}");
}

#[test]
fn one_greedy_player_among_careful_ones_takes_most_and_ruins_the_rest() {
    let fair = roamers(careful, 4);
    let fair_share = fair.carried_by_all() / 4;

    let crowd = mixed(1);
    let defector = crowd.carried(0);
    let best_of_the_rest = (1..4).map(|index| crowd.carried(index)).max().unwrap();

    assert!(defector > fair_share, "defecting should pay: {defector} vs a fair share of {fair_share}");
    assert!(best_of_the_rest * 3 < fair_share, "the careful should suffer: {best_of_the_rest} vs {fair_share}");
    assert!(crowd.carried_by_all() < fair.carried_by_all(), "the group as a whole should end up poorer");
    assert!(crowd.lowest_vitality() <= 30);
}

#[test]
fn a_crowded_run_replays_to_the_same_world() {
    let crowd = mixed(2);
    let names = ["Bot 0", "Bot 1", "Bot 2", "Bot 3"];
    let (rebuilt, _) = replay(Game::new(SEED, &names), crowd.session.log(), crowd.session.now());
    assert_eq!(&rebuilt, crowd.game());
}

#[test]
#[ignore = "prints numbers for tuning; run with --ignored --nocapture"]
fn print_crowd_numbers() {
    let describe = |name: &str, crowd: &Crowd, heads: u64| {
        println!(
            "{name}: per head {} total {} quarters {:?} lowest {} end {} collapsed {}/{} samples",
            crowd.carried_by_all() / heads,
            crowd.carried_by_all(),
            crowd.quarters(),
            crowd.lowest_vitality(),
            crowd.game().vitality(),
            collapsed_samples(crowd),
            crowd.samples.len(),
        );
    };
    describe("1 greedy camper  ", &run(&[Plan::camp(greedy, &["north"])]), 1);
    describe("4 greedy campers ", &run(&campers(greedy)), 4);
    describe("4 careful campers", &run(&campers(careful)), 4);
    for heads in [1, 2, 4, 8, 16] {
        describe(&format!("{heads:>2} greedy roamers "), &roamers(greedy, heads), heads as u64);
        describe(&format!("{heads:>2} careful roamers"), &roamers(careful, heads), heads as u64);
    }
    for defectors in [1, 2] {
        let crowd = mixed(defectors);
        let each: Vec<u64> = (0..4).map(|index| crowd.carried(index)).collect();
        println!("{defectors} greedy among 4 roamers: each {each:?} lowest {}", crowd.lowest_vitality());
    }
    for minutes in [2, 4, 6, 8, 10] {
        let mut crowd = run(&campers(greedy));
        crowd.rest(Duration::minutes(minutes));
        println!("collapsed zone after {minutes} min of rest: vitality {}", crowd.game().vitality());
    }
}
