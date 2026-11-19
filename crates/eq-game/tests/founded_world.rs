//! A whole founded world, played by bots: four founders of different
//! temper, a friend each, and four strangers who arrive unborn.
//!
//! This is where the rules of Phase 5 are tried together. Forty days:
//! it takes a founder about four weeks to be strong enough for the pass. Run it with
//! `cargo test -p eq-game --test founded_world -- --nocapture` to see
//! the report.

use eq_clock::Tick;
use eq_events::Session;
use eq_game::bots::{Bots, Plan, Policy, Stance};
use eq_game::BehemothNews;
use eq_game::faction::FactionNews;
use eq_game::pact::Pact;
use eq_game::{CharacterId, Command, Event, Game, Outcome};

const DAY: u64 = 86_400;
/// Four founders, one of each temper but two peaceful. Lands take the
/// bloodlines in turn, so Ash is Tidal, Bryn Covenant, Cole Ash and Dara
/// Stone, whatever their temper.
type Cast = [(&'static str, Stance); 4];

/// The warlike founder is Cole, of the Ash line, whose rest burns away
/// fast: peace gets there first.
const PEACE_FIRST: Cast = [("Ash", Stance::Peaceful), ("Bryn", Stance::Peaceful), ("Cole", Stance::Warlike), ("Dara", Stance::Aloof)];

/// The warlike founder is Dara, of the Stone line: war gets there first.
const WAR_FIRST: Cast = [("Ash", Stance::Peaceful), ("Bryn", Stance::Peaceful), ("Cole", Stance::Aloof), ("Dara", Stance::Warlike)];
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
    /// How many bounties were posted, earned and collected.
    bounties: (usize, usize, usize),
    /// How many escorts were wanted, taken and paid.
    escorts: (usize, usize, usize),
    /// How many of the dead were buried by their own.
    burials: usize,
    /// How many times a land fell into collapse.
    collapses: usize,
    /// How many times each character died, in the order they joined.
    died: [usize; 12],
    news: Vec<String>,
    factions: FactionCount,
    /// How many lives the Behemoth ended.
    behemoth_fed: usize,
}

fn play(seed: u64, days: u64, cast: Cast) -> Report {
    let mut session = Session::new(Game::new_world(seed, cast.len(), &[]));
    let mut crew = Vec::new();
    for (index, (name, stance)) in cast.into_iter().enumerate() {
        session.submit(Tick::ZERO, Command::join(id(index), name)).unwrap();
        session.submit(Tick::ZERO, Command::invite(id(index), FRIENDS[index])).unwrap();
        crew.push((id(index), Plan::founder(stance)));
    }
    for (index, name) in FRIENDS.into_iter().chain(STRANGERS).enumerate() {
        let who = id(cast.len() + index);
        session.submit(Tick::ZERO, Command::join(who, name)).unwrap();
        // One greedy roamer among the friends, to give the land something
        // to mark. The strangers are wardens, and will hunt.
        // Three of the friends mean to see the Meeting Ground, and will pay
        // for a guard through the pass.
        let plan = match index {
            3 => Plan::roam(Policy::Greedy),
            friend if friend < FRIENDS.len() => Plan::traveller(),
            _ => Plan::roam(Policy::Careful).as_warden(),
        };
        crew.push((who, plan));
    }
    let mut bots = Bots::new(crew);
    let (mut deaths, mut storms, mut news) = (0, 0, Vec::new());
    // Posted, earned, collected.
    let mut bounties = (0, 0, 0);
    // Wanted, taken, paid.
    let mut escorts = (0, 0, 0);
    let mut burials = 0;
    let mut behemoth_fed = 0;
    let mut factions = FactionCount::default();
    let mut collapses = 0;
    let mut died = [0usize; 12];
    for day in 1..=days {
        for stamped in bots.run(&mut session, Tick::new(day * DAY)).unwrap() {
            let hour = stamped.tick.get() / 3_600;
            match &stamped.value {
                Event::Character { who, outcome: Outcome::Died { .. } } => {
                    deaths += 1;
                    died[who.get() as usize] += 1;
                }
                Event::Storm => storms += 1,
                Event::Behemoth { news: what, .. } => {
                    let line = match what {
                        BehemothNews::Risen => "the Behemoth rises",
                        BehemothNews::Fed => {
                            behemoth_fed += 1;
                            ""
                        }
                        BehemothNews::Fell => "the Behemoth falls",
                    };
                    if !line.is_empty() {
                        news.push(format!("hour {hour}: {line}"));
                    }
                }
                Event::Pact { dealt, .. } => news.push(format!("hour {hour}: {dealt:?}")),
                Event::MoodChanged { to, .. } => news.push(format!("hour {hour}: the world is {}", to.name())),
                Event::EraChanged { to, .. } => news.push(format!("hour {hour}: {}", to.name())),
                Event::Character { who, outcome: Outcome::Hiring { .. } } => {
                    escorts.0 += 1;
                    news.push(format!("hour {hour}: {} wants an escort", session.world().character(*who).unwrap().name()));
                }
                Event::Character { who, outcome: Outcome::Guarding { client, .. } } => {
                    escorts.1 += 1;
                    let name = |who: &CharacterId| session.world().character(*who).unwrap().name();
                    news.push(format!("hour {hour}: {} guards {}", name(who), name(client)));
                }
                Event::Character { who, outcome: Outcome::EscortOver { paid } } => {
                    escorts.2 += usize::from(*paid);
                    let name = session.world().character(*who).unwrap().name();
                    news.push(format!("hour {hour}: {name}'s escort is over, {}", if *paid { "paid" } else { "unpaid" }));
                }
                Event::Character { outcome: Outcome::Buried { .. }, .. } => burials += 1,
                Event::Character { who, outcome: Outcome::CharterOpened { name } } => {
                    news.push(format!("hour {hour}: {} opens the charter of {name}", session.world().character(*who).unwrap().name()));
                }
                Event::Faction { name, news: what, members } => {
                    let line = match what {
                        FactionNews::Formed => Some(format!("{name} is formed by {}", members.len())),
                        FactionNews::Joined(_) => {
                            factions.joined += 1;
                            None
                        }
                        FactionNews::Claimed(_) => {
                            factions.claimed += 1;
                            None
                        }
                        FactionNews::Trespass { .. } => {
                            factions.trespasses += 1;
                            None
                        }
                        FactionNews::Breaking { .. } => {
                            factions.breakings += 1;
                            None
                        }
                        FactionNews::MarkerBroken { .. } => {
                            factions.broken += 1;
                            None
                        }
                        FactionNews::Dissolved(how) => Some(format!("{name} is no more: {how:?}")),
                        FactionNews::Left(_) | FactionNews::UpkeepPaid { .. } => None,
                    };
                    news.extend(line.map(|line| format!("hour {hour}: {line}")));
                }
                Event::Character { outcome: Outcome::BountyPosted { .. }, .. } => bounties.0 += 1,
                Event::Character { outcome: Outcome::BountyEarned { .. }, .. } => bounties.1 += 1,
                Event::Character { outcome: Outcome::Collected { .. }, .. } => bounties.2 += 1,
                Event::VitalityChanged { to: eq_game::Health::Collapsed, .. } => collapses += 1,
                Event::Character { who, outcome: Outcome::HouseRaised } => {
                    news.push(format!("hour {hour}: {} raises a house", session.world().character(*who).unwrap().name()));
                }
                _ => {}
            }
        }
        let lands = session.world().zones().iter().filter(|land| land.founder().is_some());
        let vitality: Vec<i64> = lands.map(eq_game::Zone::vitality).collect();
        if day % 5 == 0 {
            news.push(format!("day {day}: vitality {vitality:?}, deaths so far {deaths}"));
        }
    }
    news.push(format!("the Behemoth ended {behemoth_fed} lives"));
    Report { game: session.world().clone(), deaths, storms, bounties, escorts, burials, collapses, died, news, factions, behemoth_fed }
}

/// What the factions did over the run.
#[derive(Debug, Default)]
struct FactionCount {
    joined: usize,
    claimed: usize,
    trespasses: usize,
    breakings: usize,
    broken: usize,
}

fn people_of(game: &Game, founder: CharacterId) -> usize {
    let land = game.zone_at(game.character(founder).unwrap().home());
    game.characters().filter(|(_, person)| !person.is_spirit() && land.contains(person.home())).count()
}

/// Prints what happened, for `--nocapture`.
fn show(report: &Report, cast: Cast) {
    let game = &report.game;
    println!("{}", report.news.join("\n"));
    println!("deaths {}, storms {}, lean {}, {}", report.deaths, report.storms, game.balance().lean(), game.era().name());
    println!("bounties posted {}, earned {}, collected {}", report.bounties.0, report.bounties.1, report.bounties.2);
    println!("escorts wanted {}, taken {}, paid {}", report.escorts.0, report.escorts.1, report.escorts.2);
    println!("burials {}", report.burials);
    println!("factions {:?}", report.factions);
    for faction in report.game.factions().all() {
        let names: Vec<&str> = faction.members.iter().map(|&member| report.game.character(member).unwrap().name()).collect();
        println!(
            "  {}: formed {}, {} bloodlines, sworn {:?}, holds {}, vault ore {}",
            faction.name,
            faction.is_formed(),
            faction.bloodlines(),
            names,
            faction.claims.len(),
            faction.held(eq_game::Resource::Ore)
        );
    }
    println!("collapses {}, deaths each {:?}", report.collapses, report.died);
    let greedy = game.character(id(7)).unwrap();
    println!("the greedy one is in life {} and the afterlife now asks level {} of them", greedy.life(), greedy.afterlife_goal());
    for (index, (name, _)) in cast.into_iter().enumerate() {
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
}

#[test]
fn peace_comes_first_and_parches_the_world() {
    let report = play(11, 40, PEACE_FIRST);
    show(&report, PEACE_FIRST);
    let game = &report.game;
    let home = |index: usize| game.character(id(index)).unwrap().home();
    // Every founder raised a house.
    assert!((0..4).all(|index| game.zone_at(home(index)).hall().is_some()));
    // The strangers were all born, and filled the lands evenly.
    assert!((8..12).all(|index| !game.character(id(index)).unwrap().is_spirit() || game.character(id(index)).unwrap().life() > 1));
    // The two peaceful founders found each other.
    assert_eq!(game.pact(home(0), home(1)), Some(Pact::Alliance));
    // Cole, warlike but of the Ash line, never got strong enough for the
    // pass, and once peace had parched the world nothing grew that he
    // could train on: there was never a war.
    let cole = game.character(id(2)).unwrap();
    assert!(cole.attribute(eq_game::Attribute::Resilience).level() < 75);
    assert!((0..4).all(|other| other == 2 || game.pact(home(2), home(other)).is_none()));
    assert!(game.balance().lean() < 0);
    assert_eq!(game.era(), eq_game::era::Era::Rebuilding);
    // The friends who wanted to see the Meeting Ground hired guards, and
    // guards strong enough for the pass saw them there and were paid.
    assert!(report.escorts.0 >= 3 && report.escorts.2 >= 1, "{:?}", report.escorts);
    // The greedy one was marked, priced, hunted and paid for, over and over.
    assert!(report.bounties.0 > 0 && report.bounties.1 > 0 && report.bounties.2 > 0);
    assert!(report.died[7] > report.deaths / 2, "most of the dying was the greedy one's");
    // The two peaceful founders' people, and the strong of other lands
    // not at war with them, swear to one faction of three bloodlines; it
    // holds ground in their lands, and some of its markers are broken.
    let formed: Vec<_> = game.factions().all().iter().filter(|faction| faction.is_formed()).collect();
    assert!(formed.iter().any(|faction| faction.bloodlines() >= 3 && faction.claims.len() > 1), "{:?}", report.factions);
    assert!(report.factions.broken > 0, "{:?}", report.factions);
}

#[test]
fn war_comes_first_and_the_void_follows() {
    let report = play(11, 40, WAR_FIRST);
    show(&report, WAR_FIRST);
    let game = &report.game;
    let home = |index: usize| game.character(id(index)).unwrap().home();
    assert!((0..4).all(|index| game.zone_at(home(index)).hall().is_some()));
    // Dara, of the Stone line, breaks out first and declares war on every land.
    assert!([0, 1, 2].into_iter().all(|other| game.pact(home(3), home(other)) == Some(Pact::War)));
    // The aloof one dealt with nobody, and was dealt with anyway.
    assert_eq!(game.pact(home(2), home(0)), None);
    // Three wars and nobody to end them: the world rages until the lands
    // fail and the Void has it, and the Void raises a Behemoth.
    assert!(game.balance().lean() > 0);
    assert_eq!(game.era(), eq_game::era::Era::VoidConsumption);
    assert!(report.news.iter().any(|line| line.ends_with("the Behemoth rises")));
    // Bots shelter from it rather than feed it.
    assert!(report.behemoth_fed <= 5, "the Behemoth ended {} lives", report.behemoth_fed);
    assert!(report.bounties.0 > 0 && report.bounties.1 > 0 && report.bounties.2 > 0);
}

#[test]
fn the_same_seed_makes_the_same_world() {
    let (first, second) = (play(5, 1, PEACE_FIRST), play(5, 1, PEACE_FIRST));
    assert_eq!(first.game, second.game);
    assert_eq!(first.news, second.news);
}

