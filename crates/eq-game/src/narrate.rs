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

use crate::board::{Lot, TradeRefusal};
use crate::combat::{FoundRefusal, NameRefusal, Refusal, ReturnRefusal, RiteRefusal, RobRefusal, TendRefusal};
use crate::content::{CORPSE_TIME, LOW_TIDE, MIN_WORKABLE_STOCK, REDEMPTION_CLOCK, TIDE};
use crate::balance::Mood;
use crate::creature::{Being, CreatureKind, RelicTier};
use crate::pact::{Dealing, Dealt, PactRefusal};
use crate::{pressure_percent, CharacterId, Event, Game, Outcome};
use crate::types::{Attribute, Cause, Health, Passing, Resource, SkillKind, Standing};

/// The command list.
pub const HELP: &str = "\
Commands:
  look (l)             describe where you are
  go <exit>            walk somewhere; compass points, up and down work alone
  gather (g)           take from the deposit here
  inspect (x)          study the deposit and the health of the shoreline
  attack <name>        trade blows with someone here (see below), or
                       strike a creature: attack lurker, attack guard
  take                 open the relics here and take what is in them
  sleep [time]         sleep where you are: sleep, sleep 8h, sleep 4d
  rob <name>           go through the pack of someone asleep here
  return               as a spirit, go back into your body
  bury <name>          lay someone's body in the ground where it lies
  gather               as a spirit, practise where the living would work:
                       reach level 10 in a skill there and you live again
  attack shade         as a spirit, fight a shade where the living do not
                       work: it never strikes back, and it counts the same
  seek                 as a forgotten spirit, search here for a memory
  name <name>          at camp, speak a forgotten spirit's name to redeem it
  call <name>          as a founder, name your land (for good)
  house <name>         as a founder, name your house (for good)
  raise                as a founder, raise your house where you stand
                       (40 driftwood, 20 ore): your land's people cannot
                       be struck or robbed under its roof
  invite <name>        as a founder, invite someone: whoever joins under
                       that name arrives in your land
  ally <land>          as a founder on the Meeting Ground, offer another
                       land an alliance; it holds once they offer it too
  war <land>           as a founder on the Meeting Ground, declare war
  truce <land>         leave an alliance, or offer to end a war
  rite                 begin a Restoration Rite over the deposit here, or
                       answer one someone else has begun
  tend <name>          look after someone asleep here, so they last longer
  board                at camp, read the Contract Board and its bounties
  bounty bryn 30 ore   at camp, put a price on someone's head: it pays
                       whoever kills them, wherever they are
  post 10 ore for 30 kelp   at camp, pin up an offer (what you offer is held)
  accept <number>      at camp, pay a contract's price and take what it holds
  withdraw <number>    at camp, take your own contract down
  collect              at camp, take what your contracts have earned
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
Get back to it within ten minutes and return. Leave it longer and it
rises as a guard over what you dropped. Then, or if someone buries it,
the only way back to the living is through the afterlife. Nothing can
hurt you there, so it is the place to practise: gather where the living
would work, learning each skill as if for the first time. Reach level
10 in one and you arise in whichever land has fewest people, starting
with whatever you practised.

Death itself gives you nothing. But if nobody took what lay beside
your body, or someone buried it, your next life remembers the last:
skills you had come back four times as fast, up to where they stood.
Someone standing at a body can bury it instead of looting it.

Ectoplasm is the one thing you keep through death. At rebirth it is
spent, twenty for each extra percent of the old life you keep, up to
fifteen. Your Lineage grows with every life and raises it further. Die
where something can be worked and you remember that work best.

Two people can mend a stripped deposit with a Restoration Rite: one
begins it, the other answers within a minute and a half. Its strain
eases, it is brought back to half full, and for fifteen minutes it is
warded: it yields only while it is
pristine and at least half full. The land will not answer the marked.

What the dead drop lies in a relic. Common things anyone can take. A
relic holding tide-glass is sealed to all but its owner until the body
is gone or its guard is brought down.

Sleep two weeks and the body dies where it lies. You wake a forgotten
spirit with ten waking hours to find three memories of yourself in the
places of the shoreline, or to be named at camp by the living. Run out
of time and you fade: a new life begins with almost nothing.";

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
        Event::MoodChanged { to, .. } => Some(mood_changed(*to).into()),
        Event::Storm => Some("A storm comes through. The sea tears at the shore, and everything that can be worked is the poorer for it.".into()),
        Event::Pact { by, with, dealt } => Some(pact_news(game, *by, *with, *dealt)),
        Event::DepositChanged { place, resource, from, to } if *place == here => {
            Some(deposit_changed(*resource, *from, *to))
        }
        Event::DepositChanged { .. } => None,
        Event::VitalityChanged { anchor, from, to } if game.zone_at(*anchor).contains(here) => {
            Some(vitality_changed(*from, *to).into())
        }
        Event::VitalityChanged { .. } => None,
        Event::PassageChanged { place, label, open } if *place == here => Some(if *open {
            format!("The tide draws back. The way {label} is clear.")
        } else {
            format!("The tide comes in. The way {label} is under water.")
        }),
        Event::PassageChanged { .. } => None,
        Event::CreatureAppeared { place, being } if *place == here => Some(match being.kind {
            CreatureKind::Lurker => "The black water stirs. A lurker has come back.".into(),
            CreatureKind::Sentinel => "Stone grinds on stone. A sentinel stands in the pass again.".into(),
            CreatureKind::BodyGuard => {
                format!("{} rises and stands guard over what it dropped.", capitalise(&being_name(game, *being, viewer)))
            }
        }),
        Event::CreatureGone { place, being } if *place == here => {
            Some(format!("{} crumbles to salt and sand.", capitalise(&being_name(game, *being, viewer))))
        }
        Event::CreatureAppeared { .. } | Event::CreatureGone { .. } => None,
    }
}

/// What the viewer is told about their own action.
fn own(outcome: &Outcome, game: &Game, viewer: CharacterId) -> String {
    match outcome {
        Outcome::Joined => format!("You walk out of the tide at {}, new to the shoreline.", place_name(game, home_of(game, viewer))),
        Outcome::Founded => "Nobody has stood here before you. This land is yours to name (call <name>), and your house with it (house <name>). Those you invite (invite <name>) will arrive here.".into(),
        Outcome::CannotFound { why } => cannot_found(*why),
        Outcome::HouseRaised => "You raise your house here, out of what you carried. From now on nobody can strike or rob your land's people under its roof.".into(),
        Outcome::CannotDeal { why } => cannot_deal(*why).into(),
        Outcome::Offered { land, dealing } => format!(
            "You offer {} {}. It waits on their founder saying the same of you, here on the Meeting Ground.",
            land_name(game, *land),
            offer(*dealing),
        ),
        Outcome::WasOffered { land, dealing } => format!(
            "Word comes that {} offers your land {}. To accept, stand on the Meeting Ground and offer them the same.",
            land_name(game, *land),
            offer(*dealing),
        ),
        Outcome::LandNamed { name } => format!("You name this land {name}. It will carry that name for good."),
        Outcome::HouseNamed { name } => format!("You name your house {name}. Those you invite will carry it."),
        Outcome::Invited { name } => format!("You send word for {name}. When they come into the world, it will be here."),
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
        Outcome::Drew { amount: 0, .. } => "You can hold no more ectoplasm.".into(),
        Outcome::Drew { amount, held } => format!("You draw {amount} ectoplasm from this place. You hold {held}."),
        Outcome::SpiritRose { tier } => format!("Your spirit has grown. You are a {} now.", tier.name()),
        Outcome::Bodiless => "You are a spirit. You have no hands for that.".into(),
        Outcome::NotDead => "You are alive. There is no body to go back to or give up.".into(),
        Outcome::CannotAttack { target, why } => cannot_attack(name_of(game, *target), *why),
        Outcome::Struck { target, damage } => format!("You strike {} for {damage}.", name_of(game, *target)),
        Outcome::Hurt { by, damage, health } => {
            format!("{} strikes you for {damage}. You have {health} health left.", name_of(game, *by))
        }
        Outcome::Died { by: Cause::Character(by), place, passing, .. } => died(game, name_of(game, *by), *place, *passing),
        Outcome::Died { by: Cause::Creature(being), place, passing, .. } => {
            died(game, &capitalise(&being_name(game, *being, viewer)), *place, *passing)
        }
        Outcome::Sparred { damage, left: 0 } => {
            format!("You strike the shade for {damage} and it comes apart like smoke. Another gathers where it stood.")
        }
        Outcome::Sparred { damage, left } => format!("You strike the shade for {damage}. It does not strike back ({left} health)."),
        Outcome::NoShade => "There is no shade here. Shades drift where the living do not work, and only a spirit can see them.".into(),
        Outcome::NoSuchCreature => "There is nothing like that here to fight.".into(),
        Outcome::HitCreature { being, damage: 0, .. } => format!(
            "Your blow glances off {}. You are not strong enough to hurt it yet.",
            being_name(game, *being, viewer),
        ),
        Outcome::HitCreature { being, damage, left } => {
            format!("You strike {} for {damage}. It has {left} health left.", being_name(game, *being, viewer))
        }
        Outcome::Slew { being: Being { kind: CreatureKind::Lurker, .. } } => {
            "The lurker sinks back into the black water. In time another will come.".into()
        }
        Outcome::Slew { being: Being { kind: CreatureKind::Sentinel, .. } } => {
            "The sentinel breaks and falls. The pass is open, until another rises to bar it.".into()
        }
        Outcome::Slew { being } => {
            format!("{} falls and is still. What it kept is there for the taking.", capitalise(&being_name(game, *being, viewer)))
        }
        Outcome::Mauled { by, damage, health } => format!(
            "{} strikes you for {damage}. You have {health} health left.",
            capitalise(&being_name(game, *by, viewer)),
        ),
        Outcome::BodyRose => {
            "You left it too long. Your body has risen without you and stands guard over what you dropped. The way back to the living is through the afterlife now: gather where the living would work, until you reach level 10 in a skill.".into()
        }
        Outcome::RelicSealed => {
            "The relic here is sealed. It opens for its owner, or once the body it belongs to is gone or brought down.".into()
        }
        Outcome::CannotReturn { why: ReturnRefusal::Risen } => {
            "Your body has risen as a guard. There is no going back into it. Practise in the afterlife to live again.".into()
        }
        Outcome::Died { by: Cause::LongSleep, place, .. } => format!(
            "You slept too long. Your body has died where it lay at {}, and what you carried lies there. \
             You are a forgotten spirit now, with no body to go back to.",
            place_name(game, *place),
        ),
        Outcome::CannotReturn { why: ReturnRefusal::Buried } => {
            "Your body is in the ground. There is no going back into it. Practise in the afterlife to live again.".into()
        }
        Outcome::CannotReturn { why: ReturnRefusal::Unredeemed } => {
            "There is no giving up and starting again. Return to your body if you still can, or practise in the afterlife: gather where the living would work until you reach level 10 in a skill.".into()
        }
        Outcome::Unborn => "You have not lived yet. Every land was founded before you came, and nobody sent for you, so you begin as a spirit. Nothing can hurt you here, so practise: gather where the living would work. Reach level 10 in a skill and you will be born wherever the world has most need of people, already knowing it.".into(),
        Outcome::Practised { skill, level, needed } => {
            format!("Your {} comes on in the afterlife: {level} of the {needed} that will carry you back to the living.", skill.name())
        }
        Outcome::CannotBury { target } => format!("{}'s body does not lie here.", name_of(game, *target)),
        Outcome::Buried { target } => format!(
            "You lay {} in the ground, and what they dropped with them. Nobody can disturb them now: their next life will remember this one.",
            name_of(game, *target),
        ),
        Outcome::WasBuried { by } => format!(
            "{} has buried your body. You cannot go back to it, but the life you had will not be lost: you will learn it again fast.",
            name_of(game, *by),
        ),
        Outcome::CannotReturn { why: ReturnRefusal::NoBody } => {
            "You have no body to go back to or give up. Seek your memories, or hope to be named at camp.".into()
        }
        Outcome::Stirred => format!(
            "You stir, a spirit without a body. You have {} awake to redeem yourself: seek three memories of yourself where the shoreline can be worked, or be named at camp by the living.",
            span(REDEMPTION_CLOCK.saturating_sub(played(game, viewer))),
        ),
        Outcome::WentDormant => "You let go and lie dormant. Your time stops running until you wake.".into(),
        Outcome::Remembered { found, needed } => {
            format!("Something of who you were comes back to you here. That is {found} of {needed} memories.")
        }
        Outcome::NothingToRemember => "There is nothing of you to find here. Try another place where the shoreline can be worked.".into(),
        Outcome::NotForgotten => "Only a forgotten spirit has anything to look for.".into(),
        Outcome::CannotName { why: NameRefusal::NotAtShrine, .. } => {
            format!("Names are only heard at {}.", place_name(game, anchor_near(game, viewer)))
        }
        Outcome::CannotName { target, why: NameRefusal::NotForgotten } => {
            format!("{} is not forgotten.", name_of(game, *target))
        }
        Outcome::Named { target } => {
            format!("You speak {}'s name over the stones, and the tide gives them back.", name_of(game, *target))
        }
        Outcome::Redeemed { by: Some(by), life } => format!(
            "{} has spoken your name at {}. You walk out of the tide, whole and rested, into life {life}.",
            name_of(game, *by),
            place_name(game, home_of(game, viewer)),
        ),
        Outcome::Redeemed { by: None, life } => format!(
            "You remember who you were. You walk out of the tide at {}, whole and rested, into life {life}.",
            place_name(game, home_of(game, viewer)),
        ),
        Outcome::Faded { life } => format!(
            "Your time has run out. Who you were is gone, and your name is cut in the stones at {}. You walk out of the tide into life {life} with almost nothing.",
            place_name(game, home_of(game, viewer)),
        ),
        Outcome::CannotReturn { why: ReturnRefusal::NotAtBody } => {
            let body = game.character(viewer).and_then(|you| you.death()).map(|death| death.body);
            format!("Your body is not here. It lies at {}.", body.map_or("no place you know", |place| place_name(game, place)))
        }
        Outcome::CannotReturn { why: ReturnRefusal::TooSoon(ready_at) } => {
            format!("Something still holds you back. You cannot return before {}.", time_of_day(*ready_at))
        }
        Outcome::Returned => "You are back in your body, weak but alive.".into(),
        Outcome::Reborn { life } => format!(
            "You remember enough to live. You walk out of the tide at {}, whole and rested, into life {life}. Little of what you knew has come with you.",
            place_name(game, home_of(game, viewer)),
        ),
        Outcome::Warded { resource } => format!(
            "A ward lies on the {} here. It will not yield until it is pristine and at least half full again.",
            resource.name(),
        ),
        Outcome::CannotRite { why: RiteRefusal::NothingToMend } => "There is nothing here for a rite to mend.".into(),
        Outcome::CannotRite { why: RiteRefusal::NoNeed } => "The ground here is not hard pressed enough to need a rite.".into(),
        Outcome::CannotRite { why: RiteRefusal::Marked } => "The land will not answer you while it has you marked.".into(),
        Outcome::CannotRite { why: RiteRefusal::Warded } => "A ward already lies on this place.".into(),
        Outcome::RiteBegun => "You begin the rite. It needs a second voice: someone here must answer it soon.".into(),
        Outcome::RiteDone { with } => format!(
            "You answer {}'s rite. The ground eases, some of what was taken comes back, and a ward settles over the place.",
            name_of(game, *with),
        ),
        Outcome::RiteAnswered { by } => format!(
            "{} answers your rite. The ground eases, some of what was taken comes back, and a ward settles over the place.",
            name_of(game, *by),
        ),
        Outcome::CannotTend { target, why } => cannot_tend(name_of(game, *target), *why),
        Outcome::Tended { target, vigil } => format!(
            "You see to {} as they sleep. They can lie {} longer than they could alone.",
            name_of(game, *target),
            span(*vigil),
        ),
        Outcome::CannotTrade { why } => cannot_trade(game, viewer, *why),
        Outcome::BountyPosted { on, reward } => format!(
            "You put {} on {}'s head. The board holds it for whoever kills them.",
            lot(*reward),
            name_of(game, *on),
        ),
        Outcome::Hunted { reward } => format!("Word reaches you: there is a price on your head. {} to whoever kills you.", lot(*reward)),
        Outcome::BountyEarned { on, reward, at } => format!(
            "There was a price on {}'s head, and it is yours: {} waits for you at the board at {}.",
            name_of(game, *on),
            lot(*reward),
            place_name(game, *at),
        ),
        Outcome::Posted { id, give, want } => {
            format!("You pin contract {} to the board: {} for {}. The board holds what you offered.", id.get(), lot(*give), lot(*want))
        }
        Outcome::Accepted { paid, got, .. } => format!("You pay {} and take {} from the board.", lot(*paid), lot(*got)),
        Outcome::Filled { id, by, earned } => format!(
            "{} has taken up your contract {}. {} is waiting for you at the board.",
            name_of(game, *by),
            id.get(),
            capitalise(&lot(*earned)),
        ),
        Outcome::Withdrew { id, back } => format!("You take contract {} down and have your {} back.", id.get(), lot(*back)),
        Outcome::Collected { spoils } => format!("You collect {} from the board.", pile(spoils)),
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
    if let Outcome::Died { by, place, .. } = outcome {
        return (*place == here).then(|| match by {
            Cause::Character(_) => format!("{name} dies. What they carried lies beside the body."),
            Cause::LongSleep => format!("{name} has slept too long and will not wake. What they carried lies where they lay."),
            Cause::Creature(being) => {
                format!("{} kills {name}. What they carried lies beside the body.", capitalise(&being_name(game, *being, viewer)))
            }
        });
    }
    if !game.can_see(viewer, who) {
        return None;
    }
    match outcome {
        Outcome::Joined if actor.place() == here => Some(format!("{name} walks out of the tide, new to the shoreline.")),
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
        Outcome::HitCreature { being, .. } if actor.place() == here => {
            Some(format!("{name} strikes {}.", being_name(game, *being, viewer)))
        }
        Outcome::Slew { being } if actor.place() == here => {
            Some(format!("{name} brings down {}.", being_name(game, *being, viewer)))
        }
        Outcome::Mauled { by, .. } if actor.place() == here => {
            Some(format!("{} strikes {name}.", capitalise(&being_name(game, *by, viewer))))
        }
        Outcome::LandNamed { name: land } if actor.place() == here => Some(format!("{name} names this land {land}.")),
        Outcome::HouseNamed { name: house } if actor.place() == here => Some(format!("{name} names their house {house}.")),
        Outcome::RiteBegun if actor.place() == here => {
            Some(format!("{name} begins a rite over the ground here. It needs a second voice (rite)."))
        }
        Outcome::RiteDone { with } if actor.place() == here && *with != viewer => {
            Some(format!("{name} answers {}'s rite, and a ward settles over the place.", name_of(game, *with)))
        }
        Outcome::Tended { target, .. } if actor.place() == here => {
            Some(format!("{name} sees to {} as they sleep.", name_of(game, *target)))
        }
        Outcome::Posted { .. } if actor.place() == here => Some(format!("{name} pins a contract to the board.")),
        Outcome::Accepted { .. } if actor.place() == here => Some(format!("{name} takes a contract from the board.")),
        Outcome::Buried { target } if actor.place() == here => {
            Some(format!("{name} lays {}'s body in the ground.", name_of(game, *target)))
        }
        Outcome::Returned if actor.place() == here => Some(format!("{name}'s body stirs, and {name} stands.")),
        Outcome::Reborn { .. } | Outcome::Redeemed { .. } | Outcome::Faded { .. } if actor.place() == here => {
            Some(format!("{name} walks out of the tide, reborn."))
        }
        Outcome::Named { target } if actor.place() == here => {
            Some(format!("{name} speaks {}'s name over the stones.", name_of(game, *target)))
        }
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
    let needed = crate::content::MEMORIES_TO_REDEEM;
    let practised = format!("Practised in the afterlife: level {} of {}.", character.best_practised(), crate::content::AFTERLIFE_LEVEL);
    match character.forgotten() {
        Some(forgotten) if forgotten.unborn => lines.push(format!("You have not lived yet. {practised}")),
        Some(forgotten) => lines.push(format!(
            "You are forgotten: {} of {needed} memories found, {} left awake.",
            forgotten.memories(),
            span(REDEMPTION_CLOCK.saturating_sub(forgotten.played)),
        )),
        None if character.is_spirit() => lines.push(practised),
        None => {}
    }
    let zone = game.zone_at(character.place());
    if character.place() == zone.anchor() {
        if let Some(founder) = zone.founder() {
            let land = zone.name().map_or_else(|| "A land without a name yet".into(), String::from);
            let house = zone.house().map_or_else(String::new, |house| format!(" of House {house}"));
            let whose = if founder == viewer { "you".into() } else { format!("{}{house}", name_of(game, founder)) };
            lines.push(format!("{land}, founded by {whose}."));
        }
    }
    let mood = game.balance().mood();
    if mood != Mood::Even {
        lines.push(format!("The world is {}.", mood.name()));
    }
    if zone.hall() == Some(character.place()) {
        let whose = zone.house().map_or_else(|| "this land".into(), |house| format!("House {house}"));
        let roof = if game.is_sheltered(character) { " Under its roof nobody can strike or rob you." } else { "" };
        lines.push(format!("The house of {whose} stands here.{roof}"));
    }
    if character.place() == zone.anchor() && !zone.shrine().is_empty() {
        let names: Vec<&str> = zone.shrine().iter().map(|epitaph| epitaph.name.as_str()).collect();
        lines.push(format!("Names cut in the stones: {}.", names.join(", ")));
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
        // To a spirit, a sleeper is hardly there: an ethereal.
        .map(|other| match condition(other, now).map(|note| if character.is_spirit() && note == "asleep" { "an ethereal" } else { note }) {
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
    if let Some(deposit) = here.deposit {
        if deposit.is_warded(now) {
            lines.push("A ward lies on this place: it yields only while it is pristine and at least half full.".into());
        } else if let Some(beginner) = deposit.rite_open(now).filter(|beginner| *beginner != viewer) {
            lines.push(format!("{} has begun a rite here. It needs a second voice (rite).", name_of(game, beginner)));
        }
    }
    // Ways that lead out of this land, or into one.
    let zone = game.zone_at(character.place());
    for exit in game.map().exits(character.place()).iter().filter(|exit| !zone.contains(exit.to)) {
        let beyond = game.zone_at(exit.to);
        lines.push(match beyond.founder() {
            Some(_) => format!(
                "The {} pass leads down into {}.",
                exit.label,
                beyond.name().map_or_else(|| "a land without a name".into(), String::from),
            ),
            None => format!("{} lies {}, where the lands meet.", place_name(game, exit.to), exit.label),
        });
    }
    for creature in game.creatures_at(character.place()) {
        lines.push(match creature.being.kind {
            CreatureKind::Lurker => format!("A lurker waits here, half in the water ({} health).", creature.health()),
            CreatureKind::Sentinel => format!("A sentinel bars the pass, taller than you and in no hurry ({} health).", creature.health()),
            CreatureKind::BodyGuard => format!(
                "{} stands guard here ({} health).",
                capitalise(&being_name(game, creature.being, viewer)),
                creature.health(),
            ),
        });
    }
    for relic in game.relics_at(character.place()) {
        let tier = relic.tier();
        let article = if tier == RelicTier::Ancient { "An" } else { "A" };
        let whose = if relic.owner == viewer { "your own".into() } else { format!("{}'s", name_of(game, relic.owner)) };
        lines.push(if game.can_open(viewer, relic) {
            format!("{article} {} lies here, {whose}: {}.", tier.name(), pile(&relic.contents))
        } else {
            format!("{article} {} lies here, {whose}, sealed.", tier.name())
        });
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
    let land = game.zone_at(game.character(viewer).map_or(game.anchor(), |you| you.place()));
    let zone = format!("{} vitality: {} of 100 ({}).", land.name().unwrap_or("Shoreline"), land.vitality(), land.health().name());
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
    let began = match character.kept() {
        0 => String::new(),
        kept => format!("  (began with {kept}% of the last)"),
    };
    lines.push(format!("Life         {:>3}{began}", character.life()));
    lines.push(format!("Ectoplasm    {:>3}  ({})", character.ectoplasm(), character.spirit_tier().name()));
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

/// How long `viewer` has been awake as a forgotten spirit.
fn played(game: &Game, viewer: CharacterId) -> Duration {
    game.character(viewer).and_then(|you| you.forgotten()).map_or(Duration::ZERO, |forgotten| forgotten.played)
}

/// What to call a creature, as it would follow "you strike".
fn being_name(game: &Game, being: Being, viewer: CharacterId) -> String {
    match (being.kind, being.of) {
        (CreatureKind::Lurker, _) => "the lurker".into(),
        (CreatureKind::Sentinel, _) => "the sentinel".into(),
        (CreatureKind::BodyGuard, Some(owner)) if owner == viewer => "your own body".into(),
        (CreatureKind::BodyGuard, Some(owner)) => format!("{}'s body", name_of(game, owner)),
        (CreatureKind::BodyGuard, None) => "a risen body".into(),
    }
}

/// The anchor of the territory `viewer` belongs to, where their lives begin.
fn home_of(game: &Game, viewer: CharacterId) -> PlaceId {
    game.character(viewer).map_or(game.anchor(), |you| you.home())
}

/// The anchor of the territory `viewer` is standing in.
fn anchor_near(game: &Game, viewer: CharacterId) -> PlaceId {
    game.zone_of(viewer).map_or(game.anchor(), |zone| zone.anchor())
}

fn name_of(game: &Game, who: CharacterId) -> &str {
    game.character(who).map_or("someone", |character| character.name())
}

/// What a land is called: its name, or its founder's, or nothing much.
fn land_name(game: &Game, anchor: PlaceId) -> String {
    let zone = game.zone_at(anchor);
    match (zone.name(), zone.founder()) {
        (Some(name), _) => name.into(),
        (None, Some(founder)) => format!("{}'s land", name_of(game, founder)),
        (None, None) => "the old shoreline".into(),
    }
}

/// An offer, as the thing offered.
const fn offer(dealing: Dealing) -> &'static str {
    match dealing {
        Dealing::Ally => "an alliance",
        Dealing::Truce => "a truce",
        Dealing::War => "war",
    }
}

/// News of what two lands have become to each other, with what it
/// means for the one hearing it.
fn pact_news(game: &Game, by: PlaceId, with: PlaceId, dealt: Dealt) -> String {
    let (by, with) = (land_name(game, by), land_name(game, with));
    match dealt {
        Dealt::Allied => format!(
            "{by} and {with} are allies. The sentinel in each land's pass will let the other's people by."
        ),
        Dealt::AtWar => format!(
            "{by} has declared war on {with}. Their people may strike each other anywhere but a haven, and every death on either side will sicken both lands."
        ),
        Dealt::Peace => format!("{by} and {with} have made a truce. The war between them is over."),
        Dealt::Parted => format!("{by} has left its alliance with {with}. The sentinels will not know each other's people now."),
        Dealt::Offered => format!("{by} has made {with} an offer."),
    }
}

/// What everyone is told when the world itself changes.
const fn mood_changed(to: Mood) -> &'static str {
    match to {
        Mood::Parched => "There has been too much peace. The springs have failed and the fields are waste. Nothing taken from the ground comes back now.",
        Mood::Dry => "The world is going still. Water runs thin, and what is taken from the ground comes back slowly.",
        Mood::Even => "The world settles. Water runs as it should, and the weather is only weather.",
        Mood::Restless => "The world is restless with war. The rivers are loud, the tide comes harder, and storms will take their share.",
        Mood::Raging => "There has been too much war. The rivers are in flood, the mountains smoke, and the storms will leave little standing.",
    }
}

fn cannot_deal(why: PactRefusal) -> &'static str {
    match why {
        PactRefusal::NotFounder => "Only the founder of a land speaks for it, and you did not found yours.",
        PactRefusal::NotOnMeetingGround => "Lands deal with each other on the Meeting Ground and nowhere else. It lies beyond the pass.",
        PactRefusal::NoSuchLand => "No land goes by that name, and no founder either.",
        PactRefusal::OwnLand => "That is your own land.",
        PactRefusal::AtWar => "You are at war with them. There must be a truce first.",
        PactRefusal::AlreadyAllied => "You are allies already.",
        PactRefusal::AlreadyAtWar => "You are at war with them already.",
        PactRefusal::NoPact => "There is nothing between your lands to end.",
    }
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
fn died(game: &Game, killer: &str, place: PlaceId, passing: Passing) -> String {
    let killed = format!("{killer} has killed you. What you carried lies beside your body.");
    let anchor = place_name(game, game.zone_at(place).anchor());
    let body = place_name(game, place);
    let limit = span(CORPSE_TIME);
    let next = match passing {
        Passing::Tethered => format!(
            "Someone stood by, so your spirit stays close. Wait a moment, then return to your body. You have {limit} before it rises without you."
        ),
        Passing::Severed => format!(
            "You died alone, and your spirit is pulled to {anchor}. Get back to your body at {body} and return to it within {limit}, before it rises without you."
        ),
        Passing::Forgotten => String::new(),
        Passing::Corrupted => format!(
            "The land here is failing, and it holds your spirit at {anchor} before you can start back to your body at {body}. You have {limit} before it rises without you."
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

/// The Contract Board as someone standing at it reads it.
pub fn board(game: &Game, viewer: CharacterId) -> String {
    let Some(character) = game.character(viewer) else { return NOBODY.into() };
    let zone = game.zone_at(character.place());
    if character.place() != zone.anchor() {
        return cannot_trade(game, viewer, TradeRefusal::NotAtBoard);
    }
    let mut lines: Vec<String> = zone
        .board()
        .contracts()
        .iter()
        .map(|contract| {
            let whose = if contract.by == viewer { "yours".into() } else { format!("from {}", name_of(game, contract.by)) };
            format!("  {}: {} for {} ({whose})", contract.id.get(), lot(contract.give), lot(contract.want))
        })
        .collect();
    lines.insert(0, if lines.is_empty() { "The Contract Board is bare.".into() } else { "The Contract Board:".into() });
    let hunted: Vec<String> = zone
        .board()
        .bounties()
        .iter()
        .map(|bounty| {
            let marked = game.character(bounty.on).is_some_and(|on| on.standing() == Standing::Marked);
            let note = if marked { ", marked: fair game anywhere but a haven" } else { "" };
            format!("  {} for the death of {}{note}", lot(bounty.reward), name_of(game, bounty.on))
        })
        .collect();
    if !hunted.is_empty() {
        lines.push("Bounties:".into());
        lines.extend(hunted);
    }
    let owed = zone.board().owed_to(viewer);
    if owed.iter().any(|&amount| amount > 0) {
        lines.push(format!("Waiting for you to collect: {}.", pile(&owed)));
    }
    lines.join("\n")
}

/// An amount of something, as "30 kelp".
fn lot(lot: Lot) -> String {
    format!("{} {}", lot.amount, lot.resource.name())
}

fn cannot_found(why: FoundRefusal) -> String {
    use eq_naming::Problem;
    match why {
        FoundRefusal::NotFounder => "Only the founder of a land can do that, and you did not found yours.".into(),
        FoundRefusal::AlreadyNamed => "It has its name already. Names are for good.".into(),
        FoundRefusal::AlreadyHere => "Someone of that name is already in the world.".into(),
        FoundRefusal::TooManyInvites => "You have as many invitations waiting as a founder may.".into(),
        FoundRefusal::CannotStandHere => {
            "A house is raised in your own land, somewhere that is not a haven already and is nothing's lair.".into()
        }
        FoundRefusal::HouseStands => "Your land has its house already.".into(),
        FoundRefusal::CannotAfford => "A house takes 40 driftwood and 20 ore, and you are not carrying that.".into(),
        FoundRefusal::Name(Problem::TooShort) => "That name is too short.".into(),
        FoundRefusal::Name(Problem::TooLong) => "That name is too long.".into(),
        FoundRefusal::Name(Problem::BadCharacter(bad)) => {
            format!("A name is made of letters, apostrophes and hyphens. \"{bad}\" is none of those.")
        }
        FoundRefusal::Name(Problem::Blocked) => "This world will not carry that name.".into(),
        FoundRefusal::Name(Problem::Taken) => "That name is already taken.".into(),
    }
}

fn cannot_tend(name: &str, why: TendRefusal) -> String {
    match why {
        TendRefusal::Yourself => "You cannot keep vigil over yourself.".into(),
        TendRefusal::NotHere => format!("{name} is not here."),
        TendRefusal::Awake => format!("{name} is awake and needs no tending."),
        TendRefusal::Enough => format!("{name} has been tended as much as tending can help."),
    }
}

fn cannot_trade(game: &Game, viewer: CharacterId, why: TradeRefusal) -> String {
    match why {
        TradeRefusal::NotAtBoard => format!("The Contract Board is at {}.", place_name(game, anchor_near(game, viewer))),
        TradeRefusal::NoSuchContract => "There is no contract with that number on the board.".into(),
        TradeRefusal::CannotPay => "You do not carry enough for that.".into(),
        TradeRefusal::YourOwn => "That contract is your own. Withdraw it if you want it back.".into(),
        TradeRefusal::NotYours => "That contract is not yours to take down.".into(),
        TradeRefusal::TooMany => "You have as many contracts up as one person may.".into(),
        TradeRefusal::Empty => "A contract has to offer something and ask something.".into(),
        TradeRefusal::NothingOwed => "The board holds nothing for you.".into(),
        TradeRefusal::OnYourself => "Nobody puts a price on their own head.".into(),
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
        let zone = Event::VitalityChanged { anchor: game.anchor(), from: Health::Peak, to: Health::Balanced };
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
