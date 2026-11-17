//! What the world reports as having happened.

use alloc::string::String;

use eq_clock::{Duration, Tick};
use eq_nodes::Quality;
use eq_world_graph::PlaceId;

use crate::character::CharacterId;
use crate::combat::{Refusal, RobRefusal};
use crate::types::{Attribute, Health, Resource, SkillKind, Standing};

/// What came of one character's action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
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
    /// They are down and cannot act yet.
    Down {
        /// When they will get up.
        until: Tick,
    },
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
    /// They lost a fight. Everything they carried is on the ground.
    Downed {
        /// Who put them down.
        by: CharacterId,
    },
    /// They got back on their feet.
    Rose,
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
        /// The band before.
        from: Health,
        /// The band now.
        to: Health,
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
