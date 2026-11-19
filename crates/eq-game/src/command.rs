//! What players ask their characters to do.

use alloc::string::String;

use crate::board::{ContractId, Lot};
use crate::character::CharacterId;
use crate::creature::CreatureId;
use crate::pact::Dealing;

/// Something a character can do.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Action {
    /// Come into the world under this name. The command must be for the
    /// next free id ([`Game::next_id`](crate::Game::next_id)).
    Join(String),
    /// Leave by the exit with this label.
    Go(String),
    /// Gather from the deposit here.
    Gather,
    /// Trade blows with someone in the same place.
    Attack(CharacterId),
    /// Strike a creature in the same place.
    Fight(CreatureId),
    /// Open every relic here that will open, and take what is in them.
    Take,
    /// Settle down to sleep where you stand.
    Rest,
    /// Wake up, or get up again before sleep comes.
    Wake,
    /// Go through the pack of someone asleep in the same place.
    Rob(CharacterId),
    /// As a spirit, go back into your body.
    Return,
    /// As a spirit, give up your body and be reborn.
    Relinquish,
    /// As a forgotten spirit, search this place for a memory of yourself.
    Seek,
    /// As a spirit, strike the shade that drifts where the living do
    /// not work. Nothing is risked; it is practice.
    Spar,
    /// At the shrine, speak the name of a forgotten spirit to redeem it.
    Name(CharacterId),
    /// As a founder, give your land its name. It keeps it for good.
    NameLand(String),
    /// As a founder, give your house its name. It keeps it for good.
    NameHouse(String),
    /// As a founder, give your bloodline its name. It keeps it for
    /// good, in every land of that blood.
    NameBloodline(String),
    /// As a founder, invite someone by name. Whoever joins the world
    /// under that name arrives in your land, as one of your house.
    Invite(String),
    /// As a founder, raise your land's house where you stand. Your
    /// land's people cannot be struck or robbed there.
    Raise,
    /// As a founder standing on the Meeting Ground, deal with another
    /// land: offer an alliance, declare war, or ask for a truce. The
    /// land is named by its own name or its founder's.
    Deal {
        /// The other land, by its name or its founder's.
        with: String,
        /// What is asked of it.
        dealing: Dealing,
    },
    /// Dig up a grave where you stand and take what was buried in it:
    /// your own, if one of yours is here, or else the oldest. It breaks
    /// the tie between that life and its owner's next.
    Dig,
    /// Bury someone's body where it lies. Whatever they dropped goes
    /// into the ground with them, and their next life remembers this one.
    Bury(CharacterId),
    /// Take part in a Restoration Rite over the deposit here. One
    /// person begins it; a second, answering in time, completes it.
    Rite,
    /// Tend someone asleep in the same place, so they can sleep longer
    /// before the long sleep takes them.
    Tend(CharacterId),
    /// At the board, pin up an offer: `give`, for `want`.
    Post {
        /// What is offered. It leaves the pack at once.
        give: Lot,
        /// What is asked for it.
        want: Lot,
    },
    /// At the board, put a price on someone's head. The reward leaves
    /// the pack at once and waits there for whoever kills them.
    Bounty {
        /// Whose death it pays for.
        on: CharacterId,
        /// What it pays.
        reward: Lot,
    },
    /// At the board, put up a reward for whoever sees you safely to a
    /// place: a land, by its name or its founder's, or "ground" for the
    /// Meeting Ground.
    Hire {
        /// What it pays.
        reward: Lot,
        /// Where to.
        to: String,
    },
    /// Take the escort job of someone standing with you.
    Guard(CharacterId),
    /// Give up on the escort you asked for. The reward goes back to the
    /// board it was put up on, for you to collect.
    Dismiss,
    /// At the board, pay a contract's price and take what it offers.
    Accept(ContractId),
    /// At the board, take down a contract of your own and have back
    /// what it held.
    Withdraw(ContractId),
    /// At the board, take what your accepted contracts earned.
    Collect,
    /// At a house, open a charter of this name and sign it first. It
    /// becomes a faction once enough of enough bloodlines have signed.
    Charter(String),
    /// At its keep, sign the charter of this name: before it is formed
    /// as one of its signatories, after as a member.
    Sign(String),
    /// At your faction's keep, put this into its vault.
    Give(Lot),
    /// Set your faction's boundary marker where you stand. The vault
    /// pays for it, and for its upkeep every day.
    Claim,
    /// Break your oath to your faction, or take back your signature
    /// from a charter not yet formed.
    Forsake,
    /// Break the marker of a faction you are not sworn to, where you
    /// stand. It takes time, and marks you from the start.
    Break,
}

/// An action and the character asked to take it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Command {
    /// Who acts.
    pub who: CharacterId,
    /// What they do.
    pub action: Action,
}

impl Command {
    /// A newcomer called `name` comes into the world as `who`, which
    /// must be the world's next free id.
    pub fn join(who: CharacterId, name: &str) -> Self {
        Self { who, action: Action::Join(name.into()) }
    }

    /// `who` leaves by the exit labelled `label`.
    pub fn go(who: CharacterId, label: &str) -> Self {
        Self { who, action: Action::Go(label.into()) }
    }

    /// `who` trades blows with `target`.
    pub const fn attack(who: CharacterId, target: CharacterId) -> Self {
        Self { who, action: Action::Attack(target) }
    }

    /// `who` strikes `creature`.
    pub const fn fight(who: CharacterId, creature: CreatureId) -> Self {
        Self { who, action: Action::Fight(creature) }
    }

    /// `who` picks up what lies on the ground where they stand.
    pub const fn take(who: CharacterId) -> Self {
        Self { who, action: Action::Take }
    }

    /// `who` settles down to sleep.
    pub const fn rest(who: CharacterId) -> Self {
        Self { who, action: Action::Rest }
    }

    /// `who` wakes up.
    pub const fn wake(who: CharacterId) -> Self {
        Self { who, action: Action::Wake }
    }

    /// `who` goes through `target`'s pack as they sleep.
    pub const fn rob(who: CharacterId, target: CharacterId) -> Self {
        Self { who, action: Action::Rob(target) }
    }

    /// `who`, a spirit, goes back into their body.
    pub const fn return_to_body(who: CharacterId) -> Self {
        Self { who, action: Action::Return }
    }

    /// `who`, a spirit, gives up their body and is reborn.
    pub const fn relinquish(who: CharacterId) -> Self {
        Self { who, action: Action::Relinquish }
    }

    /// `who`, a spirit, strikes a shade for practice.
    pub const fn spar(who: CharacterId) -> Self {
        Self { who, action: Action::Spar }
    }

    /// `who`, a forgotten spirit, searches where they stand for a memory.
    pub const fn seek(who: CharacterId) -> Self {
        Self { who, action: Action::Seek }
    }

    /// `who` speaks `target`'s name at the shrine.
    pub const fn name(who: CharacterId, target: CharacterId) -> Self {
        Self { who, action: Action::Name(target) }
    }

    /// `who`, a founder, names their land.
    pub fn name_land(who: CharacterId, name: &str) -> Self {
        Self { who, action: Action::NameLand(name.into()) }
    }

    /// `who` opens a charter called `name`.
    pub fn charter(who: CharacterId, name: &str) -> Self {
        Self { who, action: Action::Charter(name.into()) }
    }

    /// `who` signs the charter called `name`.
    pub fn sign(who: CharacterId, name: &str) -> Self {
        Self { who, action: Action::Sign(name.into()) }
    }

    /// `who` puts `lot` into their faction's vault.
    pub const fn give(who: CharacterId, lot: Lot) -> Self {
        Self { who, action: Action::Give(lot) }
    }

    /// `who` sets their faction's marker where they stand.
    pub const fn claim(who: CharacterId) -> Self {
        Self { who, action: Action::Claim }
    }

    /// `who` breaks their oath.
    pub const fn forsake(who: CharacterId) -> Self {
        Self { who, action: Action::Forsake }
    }

    /// `who` sets to breaking the marker where they stand.
    pub const fn break_marker(who: CharacterId) -> Self {
        Self { who, action: Action::Break }
    }

    /// `who`, a founder, names their house.
    pub fn name_house(who: CharacterId, name: &str) -> Self {
        Self { who, action: Action::NameHouse(name.into()) }
    }

    /// `who` puts up `reward` for an escort to the place called `to`.
    pub fn hire(who: CharacterId, reward: Lot, to: &str) -> Self {
        Self { who, action: Action::Hire { reward, to: to.into() } }
    }

    /// `who` takes `client`'s escort job.
    pub const fn guard(who: CharacterId, client: CharacterId) -> Self {
        Self { who, action: Action::Guard(client) }
    }

    /// `who` gives up on the escort they asked for.
    pub const fn dismiss(who: CharacterId) -> Self {
        Self { who, action: Action::Dismiss }
    }

    /// `who` puts `reward` on `on`'s head.
    pub const fn bounty(who: CharacterId, on: CharacterId, reward: Lot) -> Self {
        Self { who, action: Action::Bounty { on, reward } }
    }

    /// `who` digs up a grave where they stand.
    pub const fn dig(who: CharacterId) -> Self {
        Self { who, action: Action::Dig }
    }

    /// `who`, a founder, raises their land's house where they stand.
    pub const fn raise(who: CharacterId) -> Self {
        Self { who, action: Action::Raise }
    }

    /// `who`, a founder, deals with the land called `with`.
    pub fn deal(who: CharacterId, with: &str, dealing: Dealing) -> Self {
        Self { who, action: Action::Deal { with: with.into(), dealing } }
    }

    /// `who`, a founder, names their bloodline.
    pub fn name_bloodline(who: CharacterId, name: &str) -> Self {
        Self { who, action: Action::NameBloodline(name.into()) }
    }

    /// `who`, a founder, invites whoever will join as `name`.
    pub fn invite(who: CharacterId, name: &str) -> Self {
        Self { who, action: Action::Invite(name.into()) }
    }

    /// `who` buries `target`'s body.
    pub const fn bury(who: CharacterId, target: CharacterId) -> Self {
        Self { who, action: Action::Bury(target) }
    }

    /// `who` takes part in a rite where they stand.
    pub const fn rite(who: CharacterId) -> Self {
        Self { who, action: Action::Rite }
    }

    /// `who` tends `target` as they sleep.
    pub const fn tend(who: CharacterId, target: CharacterId) -> Self {
        Self { who, action: Action::Tend(target) }
    }

    /// `who` pins up an offer of `give` for `want`.
    pub const fn post(who: CharacterId, give: Lot, want: Lot) -> Self {
        Self { who, action: Action::Post { give, want } }
    }

    /// `who` accepts contract `id`.
    pub const fn accept(who: CharacterId, id: ContractId) -> Self {
        Self { who, action: Action::Accept(id) }
    }

    /// `who` takes down their own contract `id`.
    pub const fn withdraw(who: CharacterId, id: ContractId) -> Self {
        Self { who, action: Action::Withdraw(id) }
    }

    /// `who` collects what the board holds for them.
    pub const fn collect(who: CharacterId) -> Self {
        Self { who, action: Action::Collect }
    }

    /// `who` gathers from the deposit where they stand.
    pub const fn gather(who: CharacterId) -> Self {
        Self { who, action: Action::Gather }
    }
}
