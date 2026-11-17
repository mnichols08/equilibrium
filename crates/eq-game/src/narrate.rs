//! Events and state to text, from one character's point of view.
//!
//! Nothing here changes the world. A graphical client would replace this
//! module and leave the rules untouched.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use eq_clock::Tick;
use eq_nodes::Quality;
use eq_world_graph::{Passage, PlaceId};

use crate::content::{LOW_TIDE, MIN_WORKABLE_STOCK, TIDE};
use crate::game::{pressure_percent, CharacterId, Event, Game, Outcome};
use crate::types::{Attribute, Health, Resource, SkillKind};

/// The command list.
pub const HELP: &str = "\
Commands:
  look (l)             describe where you are
  go <exit>            walk somewhere; compass points, up and down work alone
  gather (g)           take from the deposit here
  inspect (x)          study the deposit and the health of the shoreline
  wait [time]          let time pass: wait, wait 30, wait 10m, wait 2h
  stats                show your skills and attributes
  pack (i)             show what you carry
  quit (q)             stop playing";

/// Shown when a view is asked for a character who is not in the world.
const NOBODY: &str = "There is nobody by that name here.";

/// Tells `viewer` about an event, or returns `None` if it happened out
/// of their sight.
pub fn event(event: &Event, game: &Game, viewer: CharacterId) -> Option<String> {
    let here = game.character(viewer)?.place();
    match event {
        Event::Character { who, outcome } if *who == viewer => Some(own(outcome, game)),
        Event::Character { who, outcome } => witnessed(*who, outcome, game, here),
        Event::UnknownCharacter { .. } => None,
        Event::DepositChanged { place, resource, from, to } if *place == here => {
            Some(deposit_changed(*resource, *from, *to))
        }
        Event::DepositChanged { .. } => None,
        Event::VitalityChanged { from, to } => Some(vitality_changed(*from, *to).into()),
        Event::PassageChanged { place, label, open } if *place == here => Some(if *open {
            format!("The tide draws back. The way {label} is clear.")
        } else {
            format!("The tide comes in. The way {label} is under water.")
        }),
        Event::PassageChanged { .. } => None,
    }
}

/// What the viewer is told about their own action.
fn own(outcome: &Outcome, game: &Game) -> String {
    match outcome {
        Outcome::Moved { to, .. } => format!("You make your way to {}.", place_name(game, *to)),
        Outcome::NoSuchExit { label } => format!("You can't go \"{label}\" from here."),
        Outcome::ExitClosed { label } => format!("The way {label} is under water. Wait for the tide."),
        Outcome::Busy { .. } => "You are still busy.".into(),
        Outcome::NothingToGather => "There is nothing to gather here.".into(),
        Outcome::TooSparse { resource } => {
            format!("There is too little {} here to be worth working. Give it time.", resource.name())
        }
        Outcome::Gathered { resource, amount: 0, .. } => {
            format!("You work at the {} but come away with nothing worth keeping.", resource.name())
        }
        Outcome::Gathered { resource, amount, .. } => format!("You gather {amount} {}.", resource.name()),
        Outcome::SkillRose { skill, level } => format!("Your {} improves to {level}.", skill.name()),
        Outcome::AttributeRose { attribute, level } => {
            format!("You feel your {} grow to {level}.", attribute.name())
        }
    }
}

/// What the viewer notices of someone else's action, if they are close
/// enough to see it.
fn witnessed(who: CharacterId, outcome: &Outcome, game: &Game, here: PlaceId) -> Option<String> {
    let actor = game.character(who)?;
    let name = actor.name();
    match outcome {
        Outcome::Moved { from, to } if *to == here => {
            Some(format!("{name} arrives from {}.", place_name(game, *from)))
        }
        Outcome::Moved { from, to } if *from == here => {
            Some(format!("{name} leaves for {}.", place_name(game, *to)))
        }
        Outcome::Gathered { resource, .. } if actor.place() == here => {
            Some(format!("{name} gathers {}.", resource.name()))
        }
        _ => None,
    }
}

/// Describes the place the viewer is in.
pub fn place(game: &Game, viewer: CharacterId, now: Tick) -> String {
    let (Some(character), Some(here)) = (game.character(viewer), game.place_of(viewer)) else {
        return NOBODY.into();
    };
    let mut lines = Vec::new();
    lines.push(format!("{} [{}]", here.name, here.safety.name()));
    lines.push(here.description.into());
    if let Some(deposit) = &here.deposit {
        lines.push(format!(
            "The {} here is {} and looks {}.",
            deposit.resource.name(),
            abundance(deposit.node.stock(), deposit.node.capacity()),
            quality_name(deposit.node.quality()),
        ));
    }
    let others: Vec<&str> = game
        .characters()
        .filter(|&(id, other)| id != viewer && other.place() == character.place())
        .map(|(_, other)| other.name())
        .collect();
    if !others.is_empty() {
        lines.push(format!("Also here: {}.", others.join(", ")));
    }
    let exits: Vec<String> = game
        .map()
        .exits(character.place())
        .iter()
        .map(|exit| match exit.passage {
            Passage::Open => exit.label.clone(),
            _ if exit.passage.is_open(now) => format!("{} (clear)", exit.label),
            _ => format!("{} (under water)", exit.label),
        })
        .collect();
    lines.push(format!("Exits: {}.", exits.join(", ")));
    lines.join("\n")
}

/// Gives the numbers behind the deposit where the viewer stands and the
/// zone's health.
pub fn inspect(game: &Game, viewer: CharacterId) -> String {
    let Some(here) = game.place_of(viewer) else { return NOBODY.into() };
    let zone = format!("Shoreline vitality: {} of 100 ({}).", game.vitality(), game.health().name());
    match &here.deposit {
        Some(deposit) => format!(
            "{}: {} of {} left, {}.\nPressure {}% (wear {}%, bareness {}%).\n{zone}",
            capitalise(deposit.resource.name()),
            deposit.node.stock(),
            deposit.node.capacity(),
            quality_name(deposit.node.quality()),
            pressure_percent(&deposit.node),
            deposit.node.wear_percent(),
            deposit.node.bareness_percent(),
        ),
        None => format!("There is no deposit here.\n{zone}"),
    }
}

/// Lists the viewer's skills and attributes.
pub fn stats(game: &Game, viewer: CharacterId) -> String {
    let Some(character) = game.character(viewer) else { return NOBODY.into() };
    let mut lines = Vec::new();
    lines.push("Skills".into());
    for skill in SkillKind::ALL {
        lines.push(format!("  {:<11}{:>3}", capitalise(skill.name()), character.skill(skill).level()));
    }
    lines.push("Attributes".into());
    for attribute in Attribute::ALL {
        lines.push(format!("  {:<11}{:>3}", attribute.name(), character.attribute(attribute).level()));
    }
    lines.join("\n")
}

/// Lists what the viewer carries.
pub fn pack(game: &Game, viewer: CharacterId) -> String {
    let Some(character) = game.character(viewer) else { return NOBODY.into() };
    let carried: Vec<String> = Resource::ALL
        .into_iter()
        .map(|resource| (resource, character.carried(resource)))
        .filter(|&(_, amount)| amount > 0)
        .map(|(resource, amount)| format!("{amount} {}", resource.name()))
        .collect();
    if carried.is_empty() {
        "You carry nothing.".into()
    } else {
        format!("You carry: {}.", carried.join(", "))
    }
}

/// World time and the state of the tide, for a prompt.
pub fn clock(now: Tick) -> String {
    let seconds = now.get();
    let tide = if TIDE.is_within(now, LOW_TIDE) { "low tide" } else { "high tide" };
    format!("{}:{:02}:{:02} {tide}", seconds / 3_600, seconds / 60 % 60, seconds % 60)
}

fn place_name(game: &Game, id: PlaceId) -> &'static str {
    game.map().place(id).map_or("somewhere unknown", |place| place.name)
}

fn deposit_changed(resource: Resource, from: Quality, to: Quality) -> String {
    let name = resource.name();
    if to < from {
        return format!("The {name} here has recovered. It looks {} again.", quality_name(to));
    }
    match to {
        Quality::Healthy => format!("The {name} here is starting to show the work done on it."),
        Quality::Worn => format!("The {name} here is worn. What is taken now is poorer for it."),
        _ => format!("The {name} here is stripped. There is little of worth left to take."),
    }
}

const fn vitality_changed(from: Health, to: Health) -> &'static str {
    match (from, to) {
        (_, Health::Collapsed) => {
            "The shoreline is failing. The water smells wrong and nothing grows back as it should."
        }
        (Health::Peak, _) => "The shoreline feels less alive than it did.",
        (_, Health::Balanced) => "The shoreline is steadying.",
        (_, Health::Peak) => "The shoreline is thriving again.",
    }
}

const fn quality_name(quality: Quality) -> &'static str {
    match quality {
        Quality::Pristine => "pristine",
        Quality::Healthy => "healthy",
        Quality::Worn => "worn",
        Quality::Depleted => "stripped",
    }
}

const fn abundance(stock: u32, capacity: u32) -> &'static str {
    if stock < MIN_WORKABLE_STOCK {
        "all but gone"
    } else if stock * 3 >= capacity * 2 {
        "plentiful"
    } else if stock * 3 >= capacity {
        "thinning"
    } else {
        "sparse"
    }
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Command;
    use eq_events::World;

    const ASH: CharacterId = CharacterId::new(0);
    const BRYN: CharacterId = CharacterId::new(1);

    fn apply(game: &mut Game, command: Command, tick: u64) -> Vec<Event> {
        let mut events = Vec::new();
        game.apply(&command, Tick::new(tick), &mut events);
        events
    }

    #[test]
    fn the_starting_place_reads_well() {
        let text = place(&Game::new(1, &["Ash"]), ASH, Tick::ZERO);
        assert!(text.starts_with("Tidewatch Camp [haven]"));
        assert!(text.ends_with("Exits: north, east, south."));
        assert!(!text.contains("Also here"));
    }

    #[test]
    fn the_beach_shows_the_state_of_the_cave_mouth() {
        let mut game = Game::new(1, &["Ash"]);
        apply(&mut game, Command::go(ASH, "east"), 0);
        assert!(place(&game, ASH, Tick::new(100)).contains("cave (clear)"));
        assert!(place(&game, ASH, Tick::new(700)).contains("cave (under water)"));
        assert!(place(&game, ASH, Tick::new(100)).contains("driftwood here is plentiful and looks pristine"));
    }

    #[test]
    fn others_in_the_same_place_are_listed() {
        let mut game = Game::new(1, &["Ash", "Bryn", "Cole"]);
        assert!(place(&game, ASH, Tick::ZERO).contains("Also here: Bryn, Cole."));
        apply(&mut game, Command::go(BRYN, "north"), 0);
        assert!(place(&game, ASH, Tick::ZERO).contains("Also here: Cole."));
    }

    #[test]
    fn you_see_others_come_go_and_work_only_where_you_stand() {
        let mut game = Game::new(1, &["Ash", "Bryn"]);
        let leaving = apply(&mut game, Command::go(BRYN, "north"), 0);
        assert_eq!(event(&leaving[0], &game, ASH).unwrap(), "Bryn leaves for Ore Ledge.");
        assert_eq!(event(&leaving[0], &game, BRYN).unwrap(), "You make your way to Ore Ledge.");

        let working = apply(&mut game, Command::gather(BRYN), 30);
        assert_eq!(event(&working[0], &game, ASH), None);

        let arriving = apply(&mut game, Command::go(ASH, "north"), 30);
        assert_eq!(event(&arriving[0], &game, BRYN).unwrap(), "Ash arrives from Tidewatch Camp.");
        let working = apply(&mut game, Command::gather(BRYN), 60);
        assert_eq!(event(&working[0], &game, ASH).unwrap(), "Bryn gathers heavy ore.");
    }

    #[test]
    fn distant_events_stay_silent_but_the_zone_is_felt_everywhere() {
        let game = Game::new(1, &["Ash"]);
        let far = Event::PassageChanged { place: PlaceId::new(4), label: "out".into(), open: false };
        assert_eq!(event(&far, &game, ASH), None);
        let zone = Event::VitalityChanged { from: Health::Peak, to: Health::Balanced };
        assert!(event(&zone, &game, ASH).is_some());
    }

    #[test]
    fn an_empty_haul_is_not_reported_as_zero() {
        let haul = Event::Character {
            who: ASH,
            outcome: Outcome::Gathered { resource: Resource::Kelp, amount: 0, quality: Quality::Depleted },
        };
        assert!(event(&haul, &Game::new(1, &["Ash"]), ASH).unwrap().contains("nothing worth keeping"));
    }

    #[test]
    fn clock_shows_time_and_tide() {
        assert_eq!(clock(Tick::new(0)), "0:00:00 low tide");
        assert_eq!(clock(Tick::new(3_725)), "1:02:05 low tide");
        assert_eq!(clock(Tick::new(600)), "0:10:00 high tide");
    }

    #[test]
    fn pack_and_stats_start_empty() {
        let game = Game::new(1, &["Ash"]);
        assert_eq!(pack(&game, ASH), "You carry nothing.");
        assert!(stats(&game, ASH).contains("Mining       0"));
        assert!(inspect(&game, ASH).contains("Shoreline vitality: 100 of 100 (peak health)."));
    }

    #[test]
    fn views_for_a_missing_character_say_so() {
        let game = Game::new(1, &["Ash"]);
        let ghost = CharacterId::new(7);
        assert_eq!(place(&game, ghost, Tick::ZERO), NOBODY);
        assert_eq!(stats(&game, ghost), NOBODY);
        assert_eq!(pack(&game, ghost), NOBODY);
        assert_eq!(inspect(&game, ghost), NOBODY);
    }
}
