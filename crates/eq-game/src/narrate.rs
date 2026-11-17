//! Events and state to text, from one character's point of view.
//!
//! Nothing here changes the world. A graphical client would replace this
//! module and leave the rules untouched.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use eq_clock::{Duration, Tick};
use eq_nodes::Quality;
use eq_world_graph::{Passage, PlaceId};

use crate::combat::{Refusal, ReturnRefusal, RobRefusal};
use crate::content::{CORPSE_TIME, LOW_TIDE, MIN_WORKABLE_STOCK, TIDE};
use crate::{pressure_percent, CharacterId, Event, Game, Outcome};
use crate::types::{Attribute, Health, Passing, Resource, SkillKind, Standing};

/// The command list.
pub const HELP: &str = "\
Commands:
  look (l)             describe where you are
  go <exit>            walk somewhere; compass points, up and down work alone
  gather (g)           take from the deposit here
  inspect (x)          study the deposit and the health of the shoreline
  attack <name>        trade blows with someone here (see below)
  take                 pick up what lies dropped on the ground
  sleep [time]         sleep where you are: sleep, sleep 8h, sleep 4d
  rob <name>           go through the pack of someone asleep here
  return               as a spirit, go back into your body
  relinquish           as a spirit, give up your body and be reborn
  wait [time]          let time pass: wait, wait 30, wait 10m, wait 2h
  stats                show your skills and attributes
  pack (i)             show what you carry
  quit (q)             stop playing

Nobody can be harmed in a haven. In the wilderness only those the land
has marked can be attacked. In contested places, anyone can.

Sleep keeps you rested, and the rested learn faster. It is only safe in
a haven: anywhere else a sleeper can be robbed. Sleep for more than a
few days and the body starts to waste.

If you are killed your body stays where it fell and you become a spirit.
Get back to it within ten minutes and return, or be reborn at camp with
a little of what you knew.";

/// Shown when a view is asked for a character who is not in the world.
const NOBODY: &str = "There is nobody by that name here.";

/// Tells `viewer` about an event, or returns `None` if it happened out
/// of their sight.
pub fn event(event: &Event, game: &Game, viewer: CharacterId) -> Option<String> {
    let here = game.character(viewer)?.place();
    match event {
        Event::Character { who, outcome } if *who == viewer => Some(own(outcome, game, viewer)),
        Event::Character { who, outcome } => witnessed(*who, outcome, game, viewer, here),
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
fn own(outcome: &Outcome, game: &Game, viewer: CharacterId) -> String {
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
        Outcome::Bodiless => "You are a spirit. You have no hands for that.".into(),
        Outcome::NotDead => "You are alive. There is no body to go back to or give up.".into(),
        Outcome::CannotAttack { target, why } => cannot_attack(name_of(game, *target), *why),
        Outcome::Struck { target, damage } => format!("You strike {} for {damage}.", name_of(game, *target)),
        Outcome::Hurt { by, damage, health } => {
            format!("{} strikes you for {damage}. You have {health} health left.", name_of(game, *by))
        }
        Outcome::Died { by, place, passing, .. } => died(game, *by, *place, *passing),
        Outcome::CannotReturn { why: ReturnRefusal::NotAtBody } => {
            let body = game.character(viewer).and_then(|you| you.death()).map(|death| death.body);
            format!("Your body is not here. It lies at {}.", body.map_or("no place you know", |place| place_name(game, place)))
        }
        Outcome::CannotReturn { why: ReturnRefusal::TooSoon(ready_at) } => {
            format!("Something still holds you back. You cannot return before {}.", time_of_day(*ready_at))
        }
        Outcome::Returned => "You are back in your body, weak but alive.".into(),
        Outcome::Reborn { life } => format!(
            "Your old body is gone. You walk out of the tide at {}, whole and rested, into life {life}. Little of what you knew has come with you.",
            place_name(game, game.anchor()),
        ),
        Outcome::Took { spoils } => format!("You take {} from the ground.", pile(spoils)),
        Outcome::NothingToTake => "There is nothing lying here to take.".into(),
        Outcome::Asleep => "You are asleep. Wake first.".into(),
        Outcome::LayDown { .. } => "You settle down to sleep.".into(),
        Outcome::NotAsleep => "You are not asleep.".into(),
        Outcome::Woke { slept, rested, wasted } => woke(*slept, *rested, *wasted),
        Outcome::CannotRob { target, why } => cannot_rob(name_of(game, *target), *why),
        Outcome::Stole { target, spoils } => {
            format!("You go through {}'s pack and take {}.", name_of(game, *target), pile(spoils))
        }
        Outcome::Robbed => "Something stirs at the edge of your sleep. Your pack is lighter than it was.".into(),
        Outcome::StandingChanged { to: Standing::Marked, .. } => {
            "The land has marked you for what you have taken. Until it fades, your hauls are thin and anyone may challenge you in the wilds.".into()
        }
        Outcome::StandingChanged { to: Standing::Clean, .. } => "The land's mark on you has faded.".into(),
    }
}

/// What the viewer notices of someone else's action, if they are close
/// enough to see it.
fn witnessed(who: CharacterId, outcome: &Outcome, game: &Game, viewer: CharacterId, here: PlaceId) -> Option<String> {
    let actor = game.character(who)?;
    let name = actor.name();
    // A death is seen by everyone where the body falls, though the spirit
    // it leaves is not.
    if let Outcome::Died { place, .. } = outcome {
        return (*place == here).then(|| format!("{name} dies. What they carried lies beside the body."));
    }
    if !game.can_see(viewer, who) {
        return None;
    }
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
        Outcome::StandingChanged { to: Standing::Marked, .. } if actor.place() == here => {
            Some(format!("The land marks {name}."))
        }
        // A blow that lands on the viewer reaches them as their own `Hurt`.
        Outcome::Struck { target, .. } if actor.place() == here && *target != viewer => {
            Some(format!("{name} strikes {}.", name_of(game, *target)))
        }
        Outcome::Returned if actor.place() == here => Some(format!("{name}'s body stirs, and {name} stands.")),
        Outcome::Reborn { .. } if actor.place() == here => Some(format!("{name} walks out of the tide, reborn.")),
        Outcome::LayDown { .. } if actor.place() == here => Some(format!("{name} settles down to sleep.")),
        Outcome::Woke { slept, .. } if actor.place() == here && slept.as_ticks() > 0 => {
            Some(format!("{name} wakes."))
        }
        Outcome::Stole { target, .. } if actor.place() == here => {
            Some(format!("{name} goes through {}'s pack.", name_of(game, *target)))
        }
        Outcome::Took { .. } if actor.place() == here => {
            Some(format!("{name} takes what was lying on the ground."))
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
    if character.is_spirit() {
        lines.push("You are a spirit. The living cannot see you, and the tide is nothing to you.".into());
    }
    if let Some(deposit) = &here.deposit {
        lines.push(format!(
            "The {} here is {} and looks {}.",
            deposit.resource.name(),
            abundance(deposit.node.stock(), deposit.node.capacity()),
            quality_name(deposit.node.quality()),
        ));
    }
    let others: Vec<String> = game
        .occupants(character.place())
        .iter()
        .filter(|&&id| id != viewer && game.can_see(viewer, id))
        .filter_map(|&id| game.character(id))
        .map(|other| match condition(other, now) {
            Some(note) => format!("{} ({note})", other.name()),
            None => other.name().into(),
        })
        .collect();
    if !others.is_empty() {
        lines.push(format!("Also here: {}.", others.join(", ")));
    }
    for (id, body) in game.bodies_at(character.place()) {
        lines.push(if id == viewer { "Your body lies here.".into() } else { format!("{}'s body lies here.", body.name()) });
    }
    if here.spoils.iter().any(|&amount| amount > 0) {
        lines.push(format!("Lying on the ground: {}.", pile(&here.spoils)));
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
    lines.push(format!("Life         {:>3}", character.life()));
    lines.push(format!("Health       {:>3}", character.health()));
    lines.push(format!("Rested       {:>3}", character.rested()));
    let standing = match character.standing() {
        Standing::Clean => "clean",
        Standing::Marked => "marked",
    };
    lines.push(format!("Taint        {:>3}  ({standing})", character.taint()));
    lines.join("\n")
}

/// Lists what the viewer carries.
pub fn pack(game: &Game, viewer: CharacterId) -> String {
    let Some(character) = game.character(viewer) else { return NOBODY.into() };
    let carried = Resource::ALL.map(|resource| character.carried(resource));
    if carried.iter().all(|&amount| amount == 0) {
        "You carry nothing.".into()
    } else {
        format!("You carry: {}.", pile(&carried))
    }
}

/// Amounts by resource, in pack order, as "12 heavy ore, 4 kelp".
fn pile(amounts: &[u64; Resource::ALL.len()]) -> String {
    let parts: Vec<String> = Resource::ALL
        .into_iter()
        .zip(amounts)
        .filter(|&(_, &amount)| amount > 0)
        .map(|(resource, amount)| format!("{amount} {}", resource.name()))
        .collect();
    parts.join(", ")
}

fn name_of(game: &Game, who: CharacterId) -> &str {
    game.character(who).map_or("someone", |character| character.name())
}

/// The one thing most worth knowing about someone at a glance.
pub fn condition(character: &crate::Character, now: Tick) -> Option<&'static str> {
    if character.is_spirit() {
        Some("spirit")
    } else if character.is_asleep(now) {
        Some("asleep")
    } else if character.standing() == Standing::Marked {
        Some("marked")
    } else {
        None
    }
}

/// What a character is told on dying, which depends on how their spirit
/// stands to the body.
fn died(game: &Game, by: CharacterId, place: PlaceId, passing: Passing) -> String {
    let killed = format!("{} has killed you. What you carried lies beside your body.", name_of(game, by));
    let anchor = place_name(game, game.anchor());
    let body = place_name(game, place);
    let limit = span(CORPSE_TIME);
    let next = match passing {
        Passing::Tethered => format!(
            "Someone stood by, so your spirit stays close. Wait a moment, then return to your body. You have {limit} before it is lost."
        ),
        Passing::Severed => format!(
            "You died alone, and your spirit is pulled to {anchor}. Get back to your body at {body} and return to it within {limit}, or be reborn."
        ),
        Passing::Corrupted => format!(
            "The land here is failing, and it holds your spirit at {anchor} before you can start back to your body at {body}. You have {limit} before it is lost."
        ),
    };
    format!("{killed} {next}")
}

/// World time as `h:mm:ss`, without the tide.
fn time_of_day(now: Tick) -> String {
    let seconds = now.get();
    format!("{}:{:02}:{:02}", seconds / 3_600, seconds / 60 % 60, seconds % 60)
}

fn woke(slept: Duration, rested: u32, wasted: u32) -> String {
    if slept.as_ticks() == 0 {
        return "You are on your feet again before sleep comes.".into();
    }
    let how = match rested {
        90.. => "well rested",
        40..=89 => "somewhat rested",
        _ => "still tired",
    };
    let woke = format!("You wake after {}, {how}.", span(slept));
    if wasted == 0 {
        woke
    } else {
        format!("{woke} You have lain too long. Your body has wasted, though it will remember its strength faster than it first learned it.")
    }
}

/// A length of time in its two largest units: "3 days 4 hours".
fn span(time: Duration) -> String {
    const UNITS: [(u64, &str); 4] = [(86_400, "day"), (3_600, "hour"), (60, "minute"), (1, "second")];
    let mut left = time.as_ticks();
    let mut parts: Vec<String> = Vec::new();
    for (size, name) in UNITS {
        let count = left / size;
        left %= size;
        if count > 0 && parts.len() < 2 {
            parts.push(format!("{count} {name}{}", if count == 1 { "" } else { "s" }));
        }
    }
    if parts.is_empty() {
        "no time".into()
    } else {
        parts.join(" ")
    }
}

fn cannot_rob(name: &str, why: RobRefusal) -> String {
    match why {
        RobRefusal::Yourself => "Your pack is already yours.".into(),
        RobRefusal::NotHere => format!("{name} is not here."),
        RobRefusal::Awake => format!("{name} is awake."),
        RobRefusal::Haven => "Sleepers are safe here.".into(),
        RobRefusal::NothingToSteal => format!("{name} carries nothing."),
    }
}

fn cannot_attack(name: &str, why: Refusal) -> String {
    match why {
        Refusal::Yourself => "You think better of it.".into(),
        Refusal::NotHere => format!("{name} is not here."),
        Refusal::Haven => "Nobody can be harmed here.".into(),
        Refusal::Unmarked => {
            format!("{name} carries no mark. Out here you may only challenge those the land has marked.")
        }
    }
}

/// World time and the state of the tide, for a prompt.
pub fn clock(now: Tick) -> String {
    let tide = if TIDE.is_within(now, LOW_TIDE) { "low tide" } else { "high tide" };
    format!("{} {tide}", time_of_day(now))
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
    use crate::Command;
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
    fn spans_read_naturally() {
        assert_eq!(span(Duration::hours(8)), "8 hours");
        assert_eq!(span(Duration::days(3) + Duration::hours(4) + Duration::minutes(9)), "3 days 4 hours");
        assert_eq!(span(Duration::seconds(61)), "1 minute 1 second");
        assert_eq!(span(Duration::ZERO), "no time");
    }

    #[test]
    fn waking_says_how_it_went() {
        assert_eq!(woke(Duration::hours(8), 100, 0), "You wake after 8 hours, well rested.");
        assert_eq!(woke(Duration::hours(1), 20, 0), "You wake after 1 hour, still tired.");
        assert!(woke(Duration::days(9), 100, 30).contains("lain too long"));
        assert!(woke(Duration::ZERO, 50, 0).contains("before sleep comes"));
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
