//! Events and state to text.
//!
//! Nothing here changes the world. A graphical client would replace this
//! module and leave the rules untouched.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use eq_clock::Tick;
use eq_nodes::Quality;
use eq_world_graph::{Passage, PlaceId};

use crate::content::{LOW_TIDE, TIDE};
use crate::game::{Event, Game};
use crate::types::{Attribute, Health, Resource, SkillKind};

/// The command list.
pub const HELP: &str = "\
Commands:
  look (l)             describe where you are
  go <exit>            walk somewhere; north/south/east/west also work alone
  gather (g)           take from the deposit here
  inspect (x)          study the deposit and the health of the shoreline
  wait [time]          let time pass: wait, wait 30, wait 10m, wait 2h
  stats                show your skills and attributes
  pack (i)             show what you carry
  quit (q)             stop playing";

/// Tells the player about an event, or returns `None` if it happened out
/// of their sight.
pub fn event(event: &Event, game: &Game) -> Option<String> {
    let here = game.hero().place();
    let text = match event {
        Event::Moved { to, .. } => format!("You make your way to {}.", place_name(game, *to)),
        Event::NoSuchExit { label } => format!("You can't go \"{label}\" from here."),
        Event::ExitClosed { label } => format!("The way {label} is under water. Wait for the tide."),
        Event::Busy { .. } => "You are still busy.".into(),
        Event::NothingToGather => "There is nothing to gather here.".into(),
        Event::TooSparse { resource } => {
            format!("There is too little {} here to be worth working. Give it time.", resource.name())
        }
        Event::Gathered { resource, amount: 0, .. } => {
            format!("You work at the {} but come away with nothing worth keeping.", resource.name())
        }
        Event::Gathered { resource, amount, .. } => format!("You gather {amount} {}.", resource.name()),
        Event::DepositChanged { place, resource, from, to } if *place == here => {
            deposit_changed(*resource, *from, *to)
        }
        Event::DepositChanged { .. } => return None,
        Event::SkillRose { skill, level } => format!("Your {} improves to {level}.", skill.name()),
        Event::AttributeRose { attribute, level } => {
            format!("You feel your {} grow to {level}.", attribute.name())
        }
        Event::VitalityChanged { from, to } => vitality_changed(*from, *to).into(),
        Event::PassageChanged { place, label, open } if *place == here => {
            if *open {
                format!("The tide draws back. The way {label} is clear.")
            } else {
                format!("The tide comes in. The way {label} is under water.")
            }
        }
        Event::PassageChanged { .. } => return None,
    };
    Some(text)
}

/// Describes the place the character is in.
pub fn place(game: &Game, now: Tick) -> String {
    let here = game.here();
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
    let exits: Vec<String> = game
        .map()
        .exits(game.hero().place())
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

/// Gives the numbers behind the deposit here and the zone's health.
pub fn inspect(game: &Game) -> String {
    let zone = format!("Shoreline vitality: {} of 100 ({}).", game.vitality(), game.health().name());
    match &game.here().deposit {
        Some(deposit) => format!(
            "{}: {} of {} left, {} ({}% strain, {}% wear).\n{zone}",
            capitalise(deposit.resource.name()),
            deposit.node.stock(),
            deposit.node.capacity(),
            quality_name(deposit.node.quality()),
            deposit.node.strain_percent(),
            deposit.node.wear_percent(),
        ),
        None => format!("There is no deposit here.\n{zone}"),
    }
}

/// Lists skills and attributes.
pub fn stats(game: &Game) -> String {
    let hero = game.hero();
    let mut lines = Vec::new();
    lines.push("Skills".into());
    for skill in SkillKind::ALL {
        lines.push(format!("  {:<11}{:>3}", capitalise(skill.name()), hero.skill(skill).level()));
    }
    lines.push("Attributes".into());
    for attribute in Attribute::ALL {
        lines.push(format!("  {:<11}{:>3}", attribute.name(), hero.attribute(attribute).level()));
    }
    lines.join("\n")
}

/// Lists what the character carries.
pub fn pack(game: &Game) -> String {
    let carried: Vec<String> = Resource::ALL
        .into_iter()
        .map(|resource| (resource, game.hero().carried(resource)))
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
        Quality::Healthy => format!("The {name} here is starting to show your work."),
        Quality::Worn => format!("The {name} here is worn. What you take is poorer for it."),
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
    if stock < crate::content::MIN_WORKABLE_STOCK {
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

    #[test]
    fn the_starting_place_reads_well() {
        let text = place(&Game::new(1), Tick::ZERO);
        assert!(text.starts_with("Tidewatch Camp [haven]"));
        assert!(text.ends_with("Exits: north, east."));
    }

    #[test]
    fn the_beach_shows_the_state_of_the_cave_mouth() {
        let mut game = Game::new(1);
        game.apply(&Command::Go("east".into()), Tick::ZERO, &mut Vec::new());
        assert!(place(&game, Tick::new(100)).contains("cave (clear)"));
        assert!(place(&game, Tick::new(700)).contains("cave (under water)"));
        assert!(place(&game, Tick::new(100)).contains("driftwood here is plentiful and looks pristine"));
    }

    #[test]
    fn distant_events_stay_silent() {
        let game = Game::new(1);
        let elsewhere = PlaceId::new(4);
        let far = Event::PassageChanged { place: elsewhere, label: "out".into(), open: false };
        assert_eq!(event(&far, &game), None);
        let near = Event::VitalityChanged { from: Health::Peak, to: Health::Balanced };
        assert!(event(&near, &game).is_some());
    }

    #[test]
    fn an_empty_haul_is_not_reported_as_zero() {
        let text = event(
            &Event::Gathered { resource: Resource::Kelp, amount: 0, quality: Quality::Depleted },
            &Game::new(1),
        );
        assert!(text.unwrap().contains("nothing worth keeping"));
    }

    #[test]
    fn clock_shows_time_and_tide() {
        assert_eq!(clock(Tick::new(0)), "0:00:00 low tide");
        assert_eq!(clock(Tick::new(3_725)), "1:02:05 low tide");
        assert_eq!(clock(Tick::new(600)), "0:10:00 high tide");
    }

    #[test]
    fn pack_and_stats_start_empty() {
        let game = Game::new(1);
        assert_eq!(pack(&game), "You carry nothing.");
        assert!(stats(&game).contains("Mining       0"));
        assert!(inspect(&game).contains("Shoreline vitality: 100 of 100 (peak health)."));
    }
}
