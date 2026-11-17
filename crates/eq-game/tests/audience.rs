//! `Game::audience` must name exactly the characters the narrator would
//! speak to. If the two drift apart, a server would either leak events to
//! people who should not see them or leave people in the dark.

use eq_clock::Tick;
use eq_events::World;
use eq_game::{narrate, CharacterId, Command, Event, Game};
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
        let command = match script.below(25) {
            0..=7 => Command::gather(who),
            8..=12 => Command::go(who, EXITS[script.below(EXITS.len() as u64) as usize]),
            13..=14 => Command::attack(who, someone),
            15 => Command::take(who),
            16 => Command::rest(who),
            17..=18 => Command::wake(who),
            19..=20 => Command::return_to_body(who),
            21 => Command::relinquish(who),
            22 => Command::seek(who),
            23 => Command::name(who, someone),
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
    let event = Event::VitalityChanged { from: eq_game::Health::Peak, to: eq_game::Health::Balanced };
    assert_eq!(game.audience(&event).len(), NAMES.len());
}
