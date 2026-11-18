//! A whole founded world, played by bots: four founders of different
//! temper, a friend each, and four strangers who arrive unborn.
//!
//! This is where the rules of Phase 5 are tried together. Run it with
//! `cargo test -p eq-game --test founded_world -- --nocapture` to see
//! the report.

use eq_clock::Tick;
use eq_events::Session;
use eq_game::bots::{Bots, Plan, Policy, Stance};
use eq_game::pact::Pact;
use eq_game::{CharacterId, Command, Event, Game, Outcome};

const DAY: u64 = 86_400;
const FOUNDERS: [(&str, Stance); 4] =
    [("Ash", Stance::Peaceful), ("Bryn", Stance::Peaceful), ("Cole", Stance::Warlike), ("Dara", Stance::Aloof)];
const FRIENDS: [&str; 4] = ["Edda", "Fenn", "Gale", "Hale"];
const STRANGERS: [&str; 4] = ["Ilse", "Joss", "Kell", "Lund"];

fn id(index: usize) -> CharacterId {
    CharacterId::new(index as u64)
}

/// What a run of the world came to.
struct Report {
    game: Game,
    deaths: usize,
    storms: usize,
    news: Vec<String>,
}

fn play(seed: u64, days: u64) -> Report {
    let mut session = Session::new(Game::new_world(seed, FOUNDERS.len(), &[]));
    let mut crew = Vec::new();
    for (index, (name, stance)) in FOUNDERS.into_iter().enumerate() {
        session.submit(Tick::ZERO, Command::join(id(index), name)).unwrap();
        session.submit(Tick::ZERO, Command::invite(id(index), FRIENDS[index])).unwrap();
        crew.push((id(index), Plan::founder(stance)));
    }
    for (index, name) in FRIENDS.into_iter().chain(STRANGERS).enumerate() {
        let who = id(FOUNDERS.len() + index);
        session.submit(Tick::ZERO, Command::join(who, name)).unwrap();
        // One greedy roamer among the friends, to give the land something to mark.
        crew.push((who, Plan::roam(if index == 3 { Policy::Greedy } else { Policy::Careful })));
    }
    let mut bots = Bots::new(crew);
    let (mut deaths, mut storms, mut news) = (0, 0, Vec::new());
    for day in 1..=days {
        for stamped in bots.run(&mut session, Tick::new(day * DAY)).unwrap() {
            let hour = stamped.tick.get() / 3_600;
            match &stamped.value {
                Event::Character { outcome: Outcome::Died { .. }, .. } => deaths += 1,
                Event::Storm => storms += 1,
                Event::Pact { dealt, .. } => news.push(format!("hour {hour}: {dealt:?}")),
                Event::MoodChanged { to, .. } => news.push(format!("hour {hour}: the world is {}", to.name())),
                Event::EraChanged { to, .. } => news.push(format!("hour {hour}: {}", to.name())),
                Event::Character { who, outcome: Outcome::HouseRaised } => {
                    news.push(format!("hour {hour}: {} raises a house", session.world().character(*who).unwrap().name()));
                }
                _ => {}
            }
        }
    }
    Report { game: session.world().clone(), deaths, storms, news }
}

fn people_of(game: &Game, founder: CharacterId) -> usize {
    let land = game.zone_at(game.character(founder).unwrap().home());
    game.characters().filter(|(_, person)| !person.is_spirit() && land.contains(person.home())).count()
}

#[test]
fn four_founders_of_different_temper_make_a_world() {
    let report = play(11, 10);
    let game = &report.game;
    println!("{}", report.news.join("\n"));
    println!("deaths {}, storms {}, lean {}, {}", report.deaths, report.storms, game.balance().lean(), game.era().name());
    for (index, (name, _)) in FOUNDERS.into_iter().enumerate() {
        let land = game.zone_at(game.character(id(index)).unwrap().home());
        println!("{name}'s land: {} people, vitality {}, house {}", people_of(game, id(index)), land.vitality(), land.hall().is_some());
        let me = game.character(id(index)).unwrap();
        let level = |attribute| me.attribute(attribute).level();
        println!(
            "  {name}: life {}, Vigor {}, Resilience {}, at {}, health {}",
            me.life(),
            level(eq_game::Attribute::Vigor),
            level(eq_game::Attribute::Resilience),
            game.place_of(id(index)).unwrap().name,
            me.health(),
        );
    }

    let home = |index: usize| game.character(id(index)).unwrap().home();
    // Every founder raised a house.
    assert!((0..4).all(|index| game.zone_at(home(index)).hall().is_some()));
    // The strangers were all born, and filled the lands evenly.
    assert!((8..12).all(|index| !game.character(id(index)).unwrap().is_spirit() || game.character(id(index)).unwrap().life() > 1));
    // The two peaceful founders found each other; the warlike one found everybody.
    assert_eq!(game.pact(home(0), home(1)), Some(Pact::Alliance));
    assert!([0, 1, 3].into_iter().all(|other| game.pact(home(2), home(other)) == Some(Pact::War)));
    // The aloof one dealt with nobody, and was dealt with anyway.
    assert_eq!(game.pact(home(3), home(0)), None);
    // Three wars to one alliance: the world leans toward fury.
    assert!(game.balance().lean() > 0);
}

#[test]
fn the_same_seed_makes_the_same_world() {
    let (first, second) = (play(5, 1), play(5, 1));
    assert_eq!(first.game, second.game);
    assert_eq!(first.news, second.news);
}
