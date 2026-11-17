//! `Game::audience` must name exactly the characters the narrator would
//! speak to. If the two drift apart, a server would either leak events to
//! people who should not see them or leave people in the dark.

use eq_clock::Tick;
use eq_events::World;
use eq_game::{narrate, CharacterId, Command, ContractId, CreatureId, Event, Game, Lot, Resource};
use eq_rng::Rng;

const NAMES: [&str; 12] = ["Ash", "Bryn", "Cole", "Dara", "Eli", "Fen", "Gil", "Hale", "Ivo", "Jun", "Kit", "Lark"];
const EXITS: [&str; 8] = ["north", "south", "east", "west", "cave", "out", "up", "down"];

/// Everyone the narrator has something to say to about `event`.
fn told(game: &Game, event: &Event) -> Vec<CharacterId> {
    game.characters().map(|(id, _)| id).filter(|&id| narrate::event(event, game, id).is_some()).collect()
}

fn check(game: &Game, events: &[Event]) {
    for event in events {
        assert_eq!(game.audience(event), told(game, event), "audience and narrator disagree about {event:?}");
    }
}

#[test]
fn audience_matches_who_the_narrator_speaks_to() {
    let mut game = Game::new(11, &NAMES);
    let mut script = Rng::from_seed(5);
    let mut checked = 0;

    // Twelve characters wander, gather, fight, die, come back, loot,
    // sleep, wake and rob at random for two world-hours.
    for tick in 1..=7_200 {
        let mut events = Vec::new();
        game.tick(Tick::new(tick), &mut events);
        check(&game, &events);
        checked += events.len();

        let who = CharacterId::new(script.below(NAMES.len() as u64));
        let someone = CharacterId::new(script.below(NAMES.len() as u64));
        let command = match script.below(35) {
            0..=7 => Command::gather(who),
            8..=12 => Command::go(who, EXITS[script.below(EXITS.len() as u64) as usize]),
            13..=14 => Command::attack(who, someone),
            15 => Command::take(who),
            16 => Command::rest(who),
            17..=18 => Command::wake(who),
            19..=20 => Command::return_to_body(who),
            21 => Command::bury(who, someone),
            22 => Command::seek(who),
            23 => Command::name(who, someone),
            // Creature ids only grow, so the low ones cover every lurker and guard so far.
            24..=25 => Command::fight(who, CreatureId::new(script.below(4))),
            26 => Command::tend(who, someone),
            27..=28 => {
                let lot = |script: &mut Rng| Lot { resource: Resource::ALL[script.below(4) as usize], amount: 1 + script.below(20) };
                Command::post(who, lot(&mut script), lot(&mut script))
            }
            29..=30 => Command::accept(who, ContractId::new(script.below(40))),
            31 => Command::withdraw(who, ContractId::new(script.below(40))),
            32 => Command::collect(who),
            33..=34 => Command::rite(who),
            _ => Command::rob(who, someone),
        };
        let mut events = Vec::new();
        game.apply(&command, Tick::new(tick), &mut events);
        // Each event is judged against the world as the command left it.
        check(&game, &events);
        checked += events.len();
    }
    assert!(checked > 5_000, "only {checked} events were checked");
}

#[test]
fn audience_matches_the_narrator_through_a_long_sleep_and_a_redemption() {
    let mut game = Game::new(3, &["Ash", "Bryn", "Cole"]);
    let (ash, bryn, cole) = (CharacterId::new(0), CharacterId::new(1), CharacterId::new(2));
    let mut checked = 0;
    let mut step = |game: &mut Game, tick: u64, command: Option<Command>| {
        let mut events = Vec::new();
        match command {
            Some(command) => game.apply(&command, Tick::new(tick), &mut events),
            None => game.tick(Tick::new(tick), &mut events),
        }
        check(game, &events);
        checked += events.len();
    };

    // Ash and Cole lie down on the ledge; Cole gets up, Ash never does.
    for who in [ash, cole] {
        step(&mut game, 0, Some(Command::go(who, "north")));
        step(&mut game, 30, Some(Command::rest(who)));
    }
    let two_weeks = 14 * 86_400 + 120;
    for tick in 1..=two_weeks {
        step(&mut game, tick, None);
        if tick == 3_600 {
            step(&mut game, tick, Some(Command::wake(cole)));
        }
    }
    assert!(game.character(ash).unwrap().is_dormant());

    step(&mut game, two_weeks, Some(Command::wake(ash)));
    step(&mut game, two_weeks, Some(Command::seek(ash)));
    step(&mut game, two_weeks, Some(Command::name(cole, ash)));
    step(&mut game, two_weeks, Some(Command::name(bryn, ash)));
    assert!(!game.character(ash).unwrap().is_spirit());
    assert!(checked > 20, "only {checked} events were checked");
}

#[test]
fn audience_matches_the_narrator_across_several_lands() {
    // Three founders in lands of their own, three more placed among
    // them, all wandering, working, naming and inviting at random.
    let mut game = Game::new_world(21, 3, &[]);
    let people = 6;
    let mut script = Rng::from_seed(8);
    let mut checked = 0;
    for (index, name) in NAMES.iter().take(people).enumerate() {
        let mut events = Vec::new();
        game.apply(&Command::join(CharacterId::new(index as u64), name), Tick::new(0), &mut events);
        check(&game, &events);
    }
    assert_eq!(game.zones().len(), 5);
    for tick in 1..=4_000 {
        let mut events = Vec::new();
        game.tick(Tick::new(tick), &mut events);
        check(&game, &events);
        checked += events.len();

        let who = CharacterId::new(script.below(people as u64));
        let someone = CharacterId::new(script.below(people as u64));
        let word = ["Saltmarch", "Thorne", "Gull Reach", "x", "Ash", "Zed"][script.below(6) as usize];
        let command = match script.below(20) {
            0..=6 => Command::gather(who),
            7..=11 => Command::go(who, ["north", "south", "east", "west", "beyond", "first"][script.below(6) as usize]),
            12 => Command::attack(who, someone),
            13 => Command::rite(who),
            14 => Command::name_land(who, word),
            15 => Command::name_house(who, word),
            16 => Command::invite(who, word),
            17 => Command::rest(who),
            _ => Command::wake(who),
        };
        let mut events = Vec::new();
        game.apply(&command, Tick::new(tick), &mut events);
        check(&game, &events);
        checked += events.len();
    }
    assert!(checked > 1_000, "only {checked} events were checked");
    assert!(game.zones().iter().any(|zone| zone.name().is_some()), "somebody should have named a land");
}

#[test]
fn a_private_outcome_reaches_only_the_one_it_happened_to() {
    let mut game = Game::new(1, &["Ash", "Bryn"]);
    let ash = CharacterId::new(0);
    let mut events = Vec::new();
    game.apply(&Command::go(ash, "nowhere"), Tick::new(0), &mut events);
    assert_eq!(game.audience(&events[0]), [ash]);
}

#[test]
fn the_zone_changing_reaches_everyone() {
    let game = Game::new(1, &NAMES);
    let event = Event::VitalityChanged { anchor: game.anchor(), from: eq_game::Health::Peak, to: eq_game::Health::Balanced };
    assert_eq!(game.audience(&event).len(), NAMES.len());
}
