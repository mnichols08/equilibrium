//! What happens when players share the zone.
//!
//! - One greedy player cannot collapse the zone. Two roaming can, and a
//!   camper on every deposit holds it collapsed for as long as they stay.
//! - A collapsed zone left alone stays collapsed for a long while.
//! - The zone has a carrying capacity: a careful crowd takes about the
//!   same total whatever its size, so more heads means less each.
//! - A careful crowd holds peak health and takes several times what a
//!   greedy crowd does.
//! - Wardens, who challenge the marked and take what they drop, give the
//!   careful most of their share back and spare the zone the worst.
//! - Over-harvesting taints whoever does it, cutting their own yields. One
//!   greedy player among careful ones now earns far less than they would
//!   by staying careful, but still drags the others down with them.

mod common;

use common::{fresh_world, Crowd};
use eq_clock::Duration;
use eq_events::replay;
use eq_game::bots::{Plan, Policy};
use eq_game::content::DEPOSIT_ROUTES;
use eq_game::{Health, Standing};

const RUN: Duration = Duration::seconds(10_000);

/// One camper on each deposit.
fn campers(policy: Policy) -> Vec<Plan> {
    DEPOSIT_ROUTES.iter().map(|approach| Plan::camp(policy, approach)).collect()
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
    let mut plans = vec![Plan::roam(Policy::Careful); 4];
    for plan in plans.iter_mut().take(defectors) {
        *plan = Plan::roam(Policy::Greedy);
    }
    run(&plans)
}

/// Four roamers: one greedy, three careful ones who are also wardens.
fn warded() -> Crowd {
    let warden = Plan::roam(Policy::Careful).as_warden();
    run(&[Plan::roam(Policy::Greedy), warden, warden, warden])
}

fn collapsed_samples(crowd: &Crowd) -> usize {
    crowd.samples.iter().filter(|sample| sample.vitality <= 30).count()
}

#[test]
fn one_greedy_player_cannot_collapse_the_zone() {
    let camper = run(&[Plan::camp(Policy::Greedy, DEPOSIT_ROUTES[0])]);
    assert!(camper.lowest_vitality() >= 81, "a lone camper took the zone to {}", camper.lowest_vitality());

    let roamer = roamers(Policy::Greedy, 1);
    assert!(roamer.lowest_vitality() > 30, "a lone roamer took the zone to {}", roamer.lowest_vitality());
    assert!(roamer.lowest_vitality() < 81, "greed should still leave a mark");
}

#[test]
fn two_greedy_roamers_collapse_the_zone() {
    let crowd = roamers(Policy::Greedy, 2);
    assert!(collapsed_samples(&crowd) * 2 > crowd.samples.len());
    assert_eq!(crowd.game().health(), Health::Collapsed);
}

#[test]
fn a_camper_on_every_deposit_collapses_the_zone_and_holds_it_there() {
    let crowd = run(&campers(Policy::Greedy));
    let second_half = &crowd.samples[crowd.samples.len() / 2..];
    let highest = second_half.iter().map(|sample| sample.vitality).max().unwrap();
    let collapsed = second_half.iter().filter(|sample| sample.vitality <= 30).count();

    // It may touch the edge of balanced for a moment, but never gets up.
    assert!(highest <= 40, "the zone got up again: vitality reached {highest}");
    assert!(collapsed * 10 >= second_half.len() * 9, "collapsed for only {collapsed} of {} samples", second_half.len());
    assert_eq!(crowd.game().health(), Health::Collapsed);
}

#[test]
fn a_collapsed_zone_is_slow_to_recover_even_when_left_alone() {
    let mut crowd = run(&campers(Policy::Greedy));

    crowd.rest(Duration::minutes(20));
    assert_eq!(crowd.game().health(), Health::Collapsed, "it should still be collapsed after 20 minutes");

    crowd.rest(Duration::minutes(70));
    assert_eq!((crowd.game().vitality(), crowd.game().health()), (100, Health::Peak));
}

#[test]
fn a_careful_crowd_holds_peak_and_takes_far_more_than_a_greedy_one() {
    let (careful_crowd, greedy_crowd) = (roamers(Policy::Careful, 4), roamers(Policy::Greedy, 4));

    assert!(careful_crowd.lowest_vitality() >= 81, "vitality fell to {}", careful_crowd.lowest_vitality());
    assert!(collapsed_samples(&greedy_crowd) * 2 > greedy_crowd.samples.len());
    let (careful_total, greedy_total) = (careful_crowd.carried_by_all(), greedy_crowd.carried_by_all());
    assert!(careful_total > greedy_total * 2, "careful {careful_total} vs greedy {greedy_total}");
}

#[test]
fn the_zone_feeds_a_fixed_total_so_more_heads_means_less_each() {
    // One roamer cannot reach everything the zone offers; from two up, the
    // zone is the limit.
    const HEADS: [usize; 3] = [2, 4, 8];
    let totals: Vec<u64> = HEADS.into_iter().map(|heads| roamers(Policy::Careful, heads).carried_by_all()).collect();

    let (least, most) = (*totals.iter().min().unwrap(), *totals.iter().max().unwrap());
    assert!(most * 9 <= least * 10, "totals should sit within a tenth of each other: {totals:?}");

    let per_head: Vec<u64> = totals.iter().zip(HEADS).map(|(total, heads)| total / heads as u64).collect();
    assert!(per_head.windows(2).all(|pair| pair[0] > pair[1]), "{per_head:?}");
}

#[test]
fn one_greedy_player_among_careful_ones_loses_by_it_and_the_rest_mend_what_they_strip() {
    let fair = roamers(Policy::Careful, 4);
    let fair_share = fair.carried_by_all() / 4;

    let crowd = mixed(1);
    let defector = crowd.carried(0);
    let worst_of_the_rest = (1..4).map(|index| crowd.carried(index)).min().unwrap();

    // Taint: the defector would have done far better by staying careful.
    assert!(defector * 2 < fair_share, "defecting should not pay: {defector} vs a fair share of {fair_share}");
    // Rites: the careful mend what the defector strips and ward it, so
    // restraint still earns them most of a fair share. (Before rites
    // they were left with under a third of one.)
    assert!(worst_of_the_rest * 5 > fair_share * 4, "the careful should hold their own: {worst_of_the_rest} vs {fair_share}");
    assert!(crowd.lowest_vitality() > 60, "the zone should stay well clear of collapse: {}", crowd.lowest_vitality());
}

#[test]
fn wardens_leave_a_defector_with_nothing() {
    let fair_share = roamers(Policy::Careful, 4).carried_by_all() / 4;
    let unguarded = mixed(1);
    let guarded = warded();

    let wardens: Vec<u64> = (1..4).map(|index| guarded.carried(index)).collect();
    let worst_warden = *wardens.iter().min().unwrap();

    // Rites already protect the careful. What wardens add is that the
    // defector is left with next to nothing instead of a poor living.
    // (For a while it kept a quarter of one: a price outlasts the mark,
    // the mark is what weakens the arm, and a defector hunted at full
    // strength sometimes won. Wardens now size up the priced first.)
    assert!(guarded.carried(0) * 10 < unguarded.carried(0), "{} vs {}", guarded.carried(0), unguarded.carried(0));
    assert!(guarded.carried(0) * 10 < fair_share, "the defector kept {}", guarded.carried(0));
    println!("defector {} guarded, {} unguarded; wardens {wardens:?}; fair share {fair_share}", guarded.carried(0), unguarded.carried(0));
    // And it costs the wardens nothing to speak of.
    assert!(worst_warden * 5 > fair_share * 4, "wardens {wardens:?} vs a fair share of {fair_share}");
    assert!(guarded.lowest_vitality() > 60);
}

#[test]
fn wardens_leave_the_careful_alone() {
    let warden = Plan::roam(Policy::Careful).as_warden();
    let crowd = run(&[warden; 4]);
    let hurt = crowd.game().characters().filter(|(_, character)| character.health() < 100).count();
    assert_eq!(hurt, 0);
    assert!(crowd.lowest_vitality() >= 81);
}

#[test]
fn the_land_marks_the_greedy_and_leaves_the_careful_clean() {
    let crowd = mixed(1);
    let standings: Vec<(u32, Standing)> =
        crowd.game().characters().map(|(_, character)| (character.taint(), character.standing())).collect();

    assert_eq!(standings[0].1, Standing::Marked, "the greedy one: {standings:?}");
    assert!(standings[1..].iter().all(|&(taint, _)| taint == 0), "the careful ones: {standings:?}");
}

#[test]
fn a_crowded_run_replays_to_the_same_world() {
    let crowd = mixed(2);
    let (rebuilt, _) = replay(fresh_world(4), crowd.session.log(), crowd.session.now());
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
    describe("1 greedy camper  ", &run(&[Plan::camp(Policy::Greedy, DEPOSIT_ROUTES[0])]), 1);
    describe("9 greedy campers ", &run(&campers(Policy::Greedy)), 9);
    describe("9 careful campers", &run(&campers(Policy::Careful)), 9);
    for heads in [1, 2, 3, 4, 8, 16] {
        describe(&format!("{heads:>2} greedy roamers "), &roamers(Policy::Greedy, heads), heads as u64);
        describe(&format!("{heads:>2} careful roamers"), &roamers(Policy::Careful, heads), heads as u64);
    }
    for defectors in [1, 2] {
        let crowd = mixed(defectors);
        let each: Vec<u64> = (0..4).map(|index| crowd.carried(index)).collect();
        println!("{defectors} greedy among 4 roamers: each {each:?} lowest {}", crowd.lowest_vitality());
    }
    let crowd = warded();
    let each: Vec<u64> = (0..4).map(|index| crowd.carried(index)).collect();
    println!("1 greedy among 3 careful wardens: each {each:?} lowest {} end {}", crowd.lowest_vitality(), crowd.game().vitality());
    for minutes in [10, 20, 30, 40, 50, 60, 90] {
        let mut crowd = run(&campers(Policy::Greedy));
        crowd.rest(Duration::minutes(minutes));
        println!("collapsed zone after {minutes} min of rest: vitality {}", crowd.game().vitality());
    }
}

/// This was an open problem until Restoration Rites.
///
/// Three greedy bots among twenty, eight of them wardens, used to
/// collapse the shoreline within the hour and hold it there: bots do
/// not mind dying, so killing them did not stop them. Now the careful
/// majority mends each stripped deposit and wards it, and a warded
/// deposit yields nothing until it is thriving again. Restoring
/// outpaces stripping.
#[test]
fn a_careful_majority_holds_a_crowded_zone_against_defectors_who_do_not_mind_dying() {
    let warden = Plan::roam(Policy::Careful).as_warden();
    let careful = Plan::roam(Policy::Careful);
    let mut plans = vec![Plan::roam(Policy::Greedy); 3];
    plans.extend([warden; 8]);
    plans.extend([careful; 9]);
    let crowd = run(&plans);
    assert_eq!(collapsed_samples(&crowd), 0, "lowest vitality {}", crowd.lowest_vitality());
}

/// Rites need someone to hold them. With nobody careful about, the
/// greedy still strip the zone and keep it stripped.
#[test]
fn rites_do_nothing_for_a_zone_with_nobody_careful_in_it() {
    let crowd = roamers(Policy::Greedy, 4);
    assert!(collapsed_samples(&crowd) * 2 > crowd.samples.len());
}
