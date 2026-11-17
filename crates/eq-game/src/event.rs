//! What the world reports as having happened.

use alloc::string::String;

use eq_clock::{Duration, Tick};
use eq_nodes::Quality;
use eq_world_graph::PlaceId;

use crate::character::CharacterId;
use crate::board::{ContractId, Lot, TradeRefusal};
use crate::combat::{FoundRefusal, NameRefusal, Refusal, ReturnRefusal, RiteRefusal, RobRefusal, TendRefusal};
use crate::balance::Mood;
use crate::creature::Being;
use crate::pact::{Dealing, Dealt, PactRefusal};
use crate::spirit::SpiritTier;
use crate::types::{Attribute, Cause, Health, Passing, Resource, SkillKind, Standing};

/// What came of one character's action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// They came into the world.
    Joined,
    /// They are the first in a new territory, made for them. It has no
    /// name until they give it one.
    Founded,
    /// Something only a founder can do was not done.
    CannotFound {
        /// Why not.
        why: FoundRefusal,
    },
    /// They named their land.
    LandNamed {
        /// The name, as the world will keep it.
        name: String,
    },
    /// They named their house.
    HouseNamed {
        /// The name, as the world will keep it.
        name: String,
    },
    /// They invited someone to their land.
    Invited {
        /// The name invited, as the world will keep it.
        name: String,
    },
    /// They came into the world uninvited after its lands were all
    /// founded, and so begin as a spirit that has not yet lived.
    Unborn,
    /// There is no body of that person's here to bury.
    CannotBury {
        /// Whose body they meant.
        target: CharacterId,
    },
    /// They buried someone's body.
    Buried {
        /// Whose.
        target: CharacterId,
    },
    /// Their body was buried. Their next life will remember this one.
    WasBuried {
        /// Who buried them.
        by: CharacterId,
    },
    /// They walked from one place to another.
    Moved {
        /// Where they left.
        from: PlaceId,
        /// Where they arrived.
        to: PlaceId,
    },
    /// There is no exit with that label where they stand.
    NoSuchExit {
        /// What was asked for.
        label: String,
    },
    /// The exit exists but cannot be used right now.
    ExitClosed {
        /// The exit's label.
        label: String,
    },
    /// They are still finishing their last action.
    Busy {
        /// When they will be free.
        until: Tick,
    },
    /// There is no deposit where they stand.
    NothingToGather,
    /// The deposit has too little stock to be worth working.
    TooSparse {
        /// What would have been gathered.
        resource: Resource,
    },
    /// They gathered something.
    Gathered {
        /// What was gathered.
        resource: Resource,
        /// How much went into the pack.
        amount: u64,
        /// The deposit's quality when the work began.
        quality: Quality,
    },
    /// A skill reached a new level.
    SkillRose {
        /// Which skill.
        skill: SkillKind,
        /// The new level.
        level: u32,
    },
    /// An attribute reached a new level.
    AttributeRose {
        /// Which attribute.
        attribute: Attribute,
        /// The new level.
        level: u32,
    },
    /// As a spirit, they got better at something by practising it in
    /// the afterlife.
    Practised {
        /// Which skill.
        skill: SkillKind,
        /// The level reached in the afterlife.
        level: u32,
        /// The level at which they live again.
        needed: u32,
    },
    /// Their soul came to the River of Souls, where the afterlife begins.
    CameToRiver,
    /// As a spirit, they struck a shade.
    Sparred {
        /// The damage done.
        damage: u32,
        /// The shade's health afterwards. At 0 it is gone and another
        /// has taken its place.
        left: u32,
    },
    /// There is no shade here: the living work this place, or the one
    /// asking is alive.
    NoShade,
    /// As a spirit, they drew ectoplasm from the place they are in.
    Drew {
        /// How much they drew.
        amount: u64,
        /// How much they now hold.
        held: u64,
    },
    /// They reached a new spirit tier.
    SpiritRose {
        /// The tier reached.
        tier: SpiritTier,
    },
    /// They are a spirit and have no hands for that.
    Bodiless,
    /// They are alive, so there is no body to return to or give up.
    NotDead,
    /// They were not allowed to attack.
    CannotAttack {
        /// Who they meant to attack.
        target: CharacterId,
        /// Why not.
        why: Refusal,
    },
    /// They landed a blow.
    Struck {
        /// Who they hit.
        target: CharacterId,
        /// The health it took.
        damage: u32,
    },
    /// They took a blow.
    Hurt {
        /// Who hit them.
        by: CharacterId,
        /// The health it took.
        damage: u32,
        /// The health they have left.
        health: u32,
    },
    /// There is no such creature where they stand.
    NoSuchCreature,
    /// They struck a creature.
    HitCreature {
        /// What they hit.
        being: Being,
        /// The health it took.
        damage: u32,
        /// The health the creature has left.
        left: u32,
    },
    /// They brought a creature down.
    Slew {
        /// What it was.
        being: Being,
    },
    /// A creature struck them.
    Mauled {
        /// What struck them.
        by: Being,
        /// The health it took.
        damage: u32,
        /// The health they have left.
        health: u32,
    },
    /// Their body lay too long and rose without them as a Body Guard.
    BodyRose,
    /// The relics here are sealed against them.
    RelicSealed,
    /// They died. Everything they carried is on the ground beside the
    /// body, and they are a spirit now.
    Died {
        /// What killed them.
        by: Cause,
        /// Where the body lies.
        place: PlaceId,
        /// How the spirit stands to the body.
        passing: Passing,
        /// When the body will be lost if the spirit has not returned.
        lost_at: Tick,
    },
    /// They could not return to their body.
    CannotReturn {
        /// Why not.
        why: ReturnRefusal,
    },
    /// They went back into their body, weak but alive.
    Returned,
    /// Their body was lost or given up, and they began a new life.
    Reborn {
        /// Which life this is, counting the first as 1.
        life: u32,
    },
    /// A forgotten spirit stirred and is awake. Its clock is running.
    Stirred,
    /// A forgotten spirit went dormant. Its clock has stopped.
    WentDormant,
    /// A forgotten spirit recovered a memory of itself.
    Remembered {
        /// How many it has now.
        found: u32,
        /// How many it needs.
        needed: u32,
    },
    /// There is no memory to find here, or this spirit already found it.
    NothingToRemember,
    /// Only a forgotten spirit can do that.
    NotForgotten,
    /// They could not speak a name at the shrine.
    CannotName {
        /// Whose name they meant to speak.
        target: CharacterId,
        /// Why not.
        why: NameRefusal,
    },
    /// A founder raised their land's house where they stand.
    HouseRaised,
    /// A founder's dealing with another land came to nothing.
    CannotDeal {
        /// Why not.
        why: PactRefusal,
    },
    /// A founder made another land an offer that waits for its answer.
    Offered {
        /// The haven of the land it was made to.
        land: PlaceId,
        /// What was offered: an alliance or a truce.
        dealing: Dealing,
    },
    /// Another land's founder has made this founder's land an offer.
    WasOffered {
        /// The haven of the land that made it.
        land: PlaceId,
        /// What was offered: an alliance or a truce.
        dealing: Dealing,
    },
    /// They spoke a forgotten spirit's name at the shrine.
    Named {
        /// Whose name it was.
        target: CharacterId,
    },
    /// A forgotten spirit was redeemed and began a new life.
    Redeemed {
        /// Who spoke their name, or `None` if they found their own way.
        by: Option<CharacterId>,
        /// Which life this is, counting the first as 1.
        life: u32,
    },
    /// A forgotten spirit ran out of time. The character it was is gone;
    /// a new life begins with almost nothing.
    Faded {
        /// Which life this is, counting the first as 1.
        life: u32,
    },
    /// The deposit is warded and is not thriving, so it would not yield.
    Warded {
        /// What they hoped to gather.
        resource: Resource,
    },
    /// They could not hold a rite.
    CannotRite {
        /// Why not.
        why: RiteRefusal,
    },
    /// They began a rite. It needs a second person to answer it.
    RiteBegun,
    /// They answered a rite another had begun, and it is done: the
    /// deposit is mended and warded.
    RiteDone {
        /// Who began it.
        with: CharacterId,
    },
    /// A rite they began was answered and is done.
    RiteAnswered {
        /// Who answered.
        by: CharacterId,
    },
    /// They could not tend a sleeper.
    CannotTend {
        /// Who they meant to tend.
        target: CharacterId,
        /// Why not.
        why: TendRefusal,
    },
    /// They tended a sleeper.
    Tended {
        /// Whom.
        target: CharacterId,
        /// How much extra sleep the sleeper now has in hand.
        vigil: Duration,
    },
    /// Something they tried at the Contract Board was not allowed.
    CannotTrade {
        /// Why not.
        why: TradeRefusal,
    },
    /// They put a price on someone's head.
    BountyPosted {
        /// Whose head.
        on: CharacterId,
        /// What it pays.
        reward: Lot,
    },
    /// Someone has put a price on their head.
    Hunted {
        /// What their death now pays.
        reward: Lot,
    },
    /// They killed someone with a price on their head. The reward waits
    /// for them at the board where it was posted.
    BountyEarned {
        /// Whose head it was.
        on: CharacterId,
        /// What it paid.
        reward: Lot,
        /// The haven whose board holds it.
        at: PlaceId,
    },
    /// They pinned a contract to the board.
    Posted {
        /// Its number.
        id: ContractId,
        /// What it offers.
        give: Lot,
        /// What it asks.
        want: Lot,
    },
    /// They accepted a contract: paid its price and took what it held.
    Accepted {
        /// Its number.
        id: ContractId,
        /// What they paid.
        paid: Lot,
        /// What they got.
        got: Lot,
    },
    /// A contract of theirs was accepted. The price waits at the board.
    Filled {
        /// Its number.
        id: ContractId,
        /// Who accepted it.
        by: CharacterId,
        /// What is waiting for them.
        earned: Lot,
    },
    /// They took down a contract of their own and had back what it held.
    Withdrew {
        /// Its number.
        id: ContractId,
        /// What came back.
        back: Lot,
    },
    /// They collected what the board held for them.
    Collected {
        /// How much of each resource, in pack order.
        spoils: [u64; Resource::ALL.len()],
    },
    /// They picked up what lay on the ground.
    Took {
        /// How much of each resource, in pack order.
        spoils: [u64; Resource::ALL.len()],
    },
    /// There was nothing on the ground to pick up.
    NothingToTake,
    /// They are asleep and must wake before doing that.
    Asleep,
    /// They settled down to sleep.
    LayDown {
        /// When sleep begins, if nothing disturbs them first.
        asleep_at: Tick,
    },
    /// They tried to wake but were not lying down.
    NotAsleep,
    /// They woke, or got up before sleep came.
    Woke {
        /// How long they slept. Zero if sleep never came.
        slept: Duration,
        /// How rested they now are, 0 to 100.
        rested: u32,
        /// Levels lost across body skills and attributes to lying too long.
        wasted: u32,
    },
    /// They were not allowed to rob someone.
    CannotRob {
        /// Whose pack they meant to go through.
        target: CharacterId,
        /// Why not.
        why: RobRefusal,
    },
    /// They went through a sleeper's pack.
    Stole {
        /// Whose pack it was.
        target: CharacterId,
        /// How much of each resource they took, in pack order.
        spoils: [u64; Resource::ALL.len()],
    },
    /// Their pack was emptied while they slept. They do not learn by whom.
    Robbed,
    /// Their taint carried them from one standing to another.
    StandingChanged {
        /// Their standing before.
        from: Standing,
        /// Their standing now.
        to: Standing,
    },
}

/// Something that happened in the world.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// Something one character did or experienced.
    Character {
        /// Who it happened to.
        who: CharacterId,
        /// What happened.
        outcome: Outcome,
    },
    /// Two lands changed how they stand to each other. Everyone who
    /// belongs to either hears of it, wherever they are.
    Pact {
        /// The haven of the land whose founder spoke last.
        by: PlaceId,
        /// The haven of the other land.
        with: PlaceId,
        /// What they are to each other now.
        dealt: Dealt,
    },
    /// The world itself changed, pushed by the peace and war between
    /// its lands. Everyone feels it.
    MoodChanged {
        /// What it was like.
        from: Mood,
        /// What it is like now.
        to: Mood,
    },
    /// A storm tore at everything that can be worked. Everyone feels it.
    Storm,
    /// A command named a character who does not exist.
    UnknownCharacter {
        /// The id that was asked for.
        who: CharacterId,
    },
    /// A deposit became more or less worn.
    DepositChanged {
        /// Where the deposit is.
        place: PlaceId,
        /// What it gives.
        resource: Resource,
        /// Its quality before.
        from: Quality,
        /// Its quality now.
        to: Quality,
    },
    /// The zone moved from one vitality band to another.
    VitalityChanged {
        /// The anchor of the zone it happened in.
        anchor: PlaceId,
        /// The band before.
        from: Health,
        /// The band now.
        to: Health,
    },
    /// A creature came into the world.
    CreatureAppeared {
        /// Where.
        place: PlaceId,
        /// What it is.
        being: Being,
    },
    /// A creature left the world without being brought down.
    CreatureGone {
        /// Where it stood.
        place: PlaceId,
        /// What it was.
        being: Being,
    },
    /// A cyclic exit opened or closed.
    PassageChanged {
        /// The place the exit leaves.
        place: PlaceId,
        /// The exit's label.
        label: String,
        /// Whether it is now open.
        open: bool,
    },
}
