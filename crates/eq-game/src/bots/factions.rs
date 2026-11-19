//! What bots do about factions.
//!
//! A peaceful founder opens a charter at its house. Careful bots walk
//! to any keep they can reach, of their own land or of one that is not
//! at war with it, and sign. Members carry ore to the keep while the
//! vault runs low, set markers on good ground at home when the vault
//! can bear it, and go for anyone breaking a marker they stand by.
//! Greedy bots break markers nobody is watching, for the ore in them,
//! and so do bots at war with the keep's land.

use eq_world_graph::PlaceId;

use super::{strong_enough, Bot, Policy, Stance, HEALTH_TO_FIGHT};
use crate::content::{MARKER_COST, UPKEEP_PER_CLAIM};
use crate::faction::Faction;
use crate::pact::Pact;
use crate::{Character, Command, Game, Lot, Resource, Safety};

/// How many days of upkeep a vault should hold before members stop
/// bringing ore, and before they will spend on another marker.
const DAYS_IN_HAND: u64 = 3;

/// What the faction owes for its markers each day, with `more` besides.
fn daily(faction: &Faction, more: usize) -> u64 {
    UPKEEP_PER_CLAIM.1 * (faction.claims.len() + more) as u64
}

/// Whether the bot `me` may go to `faction`'s keep: it is in their own
/// land, or they are strong enough to get there, their land is not at
/// war with the keep's, and they are not one to keep to themselves.
fn can_reach(game: &Game, bot: &Bot, me: &Character, faction: &Faction) -> bool {
    let land = game.zone_at(faction.keep);
    let goes_abroad = bot.plan.stance != Some(Stance::Aloof) && strong_enough(me);
    land.contains(me.home()) || (goes_abroad && game.pact(me.home(), land.anchor()) != Some(Pact::War))
}

/// The keep a careful bot means to walk to, if any: one it can sign, or
/// its own faction's, while it carries ore the vault needs.
pub(super) fn faction_errand(game: &Game, bot: &Bot) -> Option<PlaceId> {
    let me = game.character(bot.who)?;
    if bot.plan.policy != Policy::Careful {
        return None;
    }
    let factions = game.factions();
    if let Some(mine) = factions.of(bot.who) {
        let short = mine.held(Resource::Ore) < daily(mine, 0) * DAYS_IN_HAND;
        return (mine.is_formed() && short && me.carried(Resource::Ore) >= UPKEEP_PER_CLAIM.1).then_some(mine.keep);
    }
    if factions.oathbreaker(bot.who).is_some() {
        return None;
    }
    factions.all().iter().find(|faction| can_reach(game, bot, me, faction)).map(|faction| faction.keep)
}

/// What a bot does about factions where it stands, if anything.
pub(super) fn faction_duty(game: &Game, bot: &Bot) -> Option<Command> {
    defend(game, bot)
        .or_else(|| move_on(game, bot))
        .or_else(|| charter(game, bot))
        .or_else(|| sign(game, bot))
        .or_else(|| give(game, bot))
        .or_else(|| claim(game, bot))
        .or_else(|| break_marker(game, bot))
}

/// A member fit to fight goes for whoever is breaking its marker here.
fn defend(game: &Game, bot: &Bot) -> Option<Command> {
    let me = game.character(bot.who)?;
    let faction = game.factions().holding(me.place()).filter(|faction| faction.has(bot.who))?;
    let claim = faction.claims.iter().find(|claim| claim.place == me.place())?;
    let (breaker, _) = claim.breaking?;
    (me.health() >= HEALTH_TO_FIGHT && game.can_attack(bot.who, breaker).is_ok()).then(|| Command::attack(bot.who, breaker))
}

/// A bot whose charter is not formed takes its name back, which costs
/// nothing, if it could reach one that is formed or further along: it
/// will go and sign that instead.
fn move_on(game: &Game, bot: &Bot) -> Option<Command> {
    let me = game.character(bot.who)?;
    let mine = game.factions().of(bot.who).filter(|faction| !faction.is_formed())?;
    let further = |other: &Faction| other.is_formed() || other.signatories.len() > mine.signatories.len();
    let better = game.factions().all().iter().any(|other| other.id != mine.id && further(other) && can_reach(game, bot, me, other));
    better.then(|| Command::forsake(bot.who))
}

/// A peaceful founder standing in its own house opens a charter there,
/// if there is none.
fn charter(game: &Game, bot: &Bot) -> Option<Command> {
    if bot.plan.stance != Some(Stance::Peaceful) {
        return None;
    }
    let me = game.character(bot.who)?;
    let land = game.zone_at(me.home());
    let factions = game.factions();
    let free = factions.of(bot.who).is_none() && factions.oathbreaker(bot.who).is_none();
    // Nobody opens a charter of their own while they could sign another.
    let open = factions.all().iter().any(|faction| faction.keep == me.place() || can_reach(game, bot, me, faction));
    let at_home = land.founder() == Some(bot.who) && land.hall() == Some(me.place());
    (free && at_home && !open).then(|| Command::charter(bot.who, &alloc::format!("{}ward", me.name())))
}

/// A careful bot at a keep it could reach signs what is there.
fn sign(game: &Game, bot: &Bot) -> Option<Command> {
    let me = game.character(bot.who)?;
    let factions = game.factions();
    if bot.plan.policy != Policy::Careful || factions.of(bot.who).is_some() || factions.oathbreaker(bot.who).is_some() {
        return None;
    }
    let faction = factions.all().iter().find(|faction| faction.keep == me.place() && can_reach(game, bot, me, faction))?;
    Some(Command::sign(bot.who, &faction.name))
}

/// A member at its keep, while the vault is short, puts in enough of
/// the ore it carries for a marker more and some days of upkeep.
fn give(game: &Game, bot: &Bot) -> Option<Command> {
    let me = game.character(bot.who)?;
    let faction = game.factions().of(bot.who).filter(|faction| faction.keep == me.place() && faction.is_formed())?;
    let held = faction.held(Resource::Ore);
    if held >= daily(faction, 0) * DAYS_IN_HAND {
        return None;
    }
    let wanted = MARKER_COST.1 + daily(faction, 1) * DAYS_IN_HAND * 2 - held;
    let amount = me.carried(Resource::Ore).min(wanted);
    (amount > 0).then(|| Command::give(bot.who, Lot { resource: Resource::Ore, amount }))
}

/// A member on unheld ground worth working in its own land sets a
/// marker, if the vault can pay for it and for some days besides.
fn claim(game: &Game, bot: &Bot) -> Option<Command> {
    let me = game.character(bot.who)?;
    let faction = game.factions().of(bot.who).filter(|faction| faction.is_formed())?;
    let here = me.place();
    let place = game.map().place(here)?;
    let fit = place.deposit.is_some() && place.lair.is_none() && place.safety != Safety::Haven;
    let ours = game.zone_at(me.home()).contains(here);
    let held = game.factions().holding(here).is_some();
    let means = faction.held(Resource::Ore) >= MARKER_COST.1 + daily(faction, 1) * DAYS_IN_HAND;
    (fit && ours && !held && means).then(|| Command::claim(bot.who))
}

/// A greedy bot breaks a marker nobody of its faction is standing by,
/// for the ore in it. A bot whose land is at war with the keep's breaks
/// it whoever is watching, if it is fit to.
fn break_marker(game: &Game, bot: &Bot) -> Option<Command> {
    let me = game.character(bot.who)?;
    let faction = game.factions().holding(me.place()).filter(|faction| !faction.has(bot.who))?;
    let begun = faction.claims.iter().any(|claim| claim.place == me.place() && claim.breaking.is_some());
    let watched = game.occupants(me.place()).iter().any(|&other| {
        faction.has(other) && game.character(other).is_some_and(|other| !other.is_spirit() && !other.is_lying_down())
    });
    let at_war = game.pact(me.home(), game.zone_at(faction.keep).anchor()) == Some(Pact::War);
    let greedy = bot.plan.policy == Policy::Greedy && !watched;
    let warring = at_war && me.health() >= HEALTH_TO_FIGHT;
    (!begun && (greedy || warring)).then(|| Command::break_marker(bot.who))
}
