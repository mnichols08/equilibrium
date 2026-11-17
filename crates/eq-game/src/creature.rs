//! Things in the world that are not characters: hostile creatures and
//! the relics the dead leave behind.
//!
//! Nothing here changes the world. These are the nouns and the small
//! sums about them; the rules in `game` decide when they apply.

use alloc::vec::Vec;

use eq_clock::{Duration, Tick};
use eq_meter::Meter;
use eq_world_graph::PlaceId;

use crate::character::{Character, CharacterId};
use crate::content::{
    GUARD_THREAT_PERCENT, LURKER_HEALTH, MAX_HEALTH, SENTINEL_HEALTH, SENTINEL_HIDE, SENTINEL_MIGHT, TAKE_TIME,
};
use crate::types::{Attribute, Resource};

eq_ids::define_id!(
    /// Identifies a creature within one [`Game`](crate::Game). Ids are
    /// never reused, so a stale id names nothing.
    CreatureId
);

#[cfg(feature = "serde")]
crate::wire::as_number!(CreatureId);

/// What sort of creature something is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CreatureKind {
    /// A thing that lives in the deep places and goes for anyone it finds.
    Lurker,
    /// What bars the pass out of a land: too much for anyone new, and
    /// it goes for whoever comes.
    Sentinel,
    /// A body its spirit did not come back for. It keeps what its owner
    /// dropped and fights whoever troubles it.
    BodyGuard,
}

impl CreatureKind {
    /// Whether it goes for anyone living it finds, or only for those
    /// who trouble it.
    pub const fn is_aggressive(self) -> bool {
        matches!(self, Self::Lurker | Self::Sentinel)
    }

    /// The one word a player can call it by.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Lurker => "lurker",
            Self::Sentinel => "sentinel",
            Self::BodyGuard => "guard",
        }
    }
}

/// Enough to name a creature after it is gone: what it was and, for a
/// Body Guard, whose body.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Being {
    /// What sort of creature.
    pub kind: CreatureKind,
    /// Whose body it was, for a Body Guard.
    pub of: Option<CharacterId>,
}

/// A hostile creature in the world.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Creature {
    /// Its id.
    pub id: CreatureId,
    /// What it is.
    pub being: Being,
    /// Where it stands. Creatures do not wander.
    pub place: PlaceId,
    health: Meter,
    /// How hard it hits, on the same scale as a character's Vigor.
    pub vigor: u32,
    /// How much it shrugs off, on the same scale as Resilience.
    pub resilience: u32,
    /// A blow that would take this much or less glances off.
    pub hide: u32,
    /// Everyone who has troubled it, oldest first.
    pub(crate) foes: Vec<CharacterId>,
    /// The earliest it will next strike.
    pub(crate) next_blow: Tick,
    /// When it leaves the world by itself, if it ever does.
    pub gone_at: Option<Tick>,
}

impl Creature {
    /// A creature of the kind that makes its home in a place: a lurker
    /// or a sentinel. (A Body Guard is made from a body, not a lair.)
    pub(crate) fn native(kind: CreatureKind, id: CreatureId, place: PlaceId, now: Tick) -> Self {
        let (health, might, hide) = match kind {
            CreatureKind::Sentinel => (SENTINEL_HEALTH, SENTINEL_MIGHT, SENTINEL_HIDE),
            CreatureKind::Lurker | CreatureKind::BodyGuard => (LURKER_HEALTH, 0, 0),
        };
        Self {
            id,
            being: Being { kind, of: None },
            place,
            health: Meter::full(0, health),
            vigor: might,
            resilience: might,
            hide,
            foes: Vec::new(),
            next_blow: now,
            gone_at: None,
        }
    }

    /// The Body Guard that `body` becomes: its owner's strength and
    /// toughness, and a full measure of health, each raised by the
    /// threat modifier.
    pub(crate) fn body_guard(id: CreatureId, of: CharacterId, body: &Character, place: PlaceId, now: Tick, gone_at: Tick) -> Self {
        let raised = |level: u32| level * GUARD_THREAT_PERCENT / 100;
        Self {
            id,
            being: Being { kind: CreatureKind::BodyGuard, of: Some(of) },
            place,
            // The threat modifier is a little over a hundred percent.
            health: Meter::full(0, MAX_HEALTH * GUARD_THREAT_PERCENT as i64 / 100),
            vigor: raised(body.attribute(Attribute::Vigor).level()),
            resilience: raised(body.attribute(Attribute::Resilience).level()),
            hide: 0,
            foes: Vec::new(),
            next_blow: now,
            gone_at: Some(gone_at),
        }
    }

    /// Health left.
    pub const fn health(&self) -> u32 {
        // The meter never goes below zero and holds a small number.
        self.health.value() as u32
    }

    /// Lands a blow worth `damage`. A blow too weak to get through its
    /// hide takes nothing. Returns what the blow took and what is left.
    pub(crate) fn hurt(&mut self, damage: u32) -> (u32, u32) {
        let taken = if damage <= self.hide { 0 } else { damage };
        self.health.adjust(-i64::from(taken));
        (taken, self.health())
    }

    /// Remembers `who` as someone to go for.
    pub(crate) fn provoke(&mut self, who: CharacterId) {
        if !self.foes.contains(&who) {
            self.foes.push(who);
        }
    }
}

/// How much a relic is worth, which decides who can open it and how
/// long it lasts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RelicTier {
    /// Common things only. Anyone can pick it up at once.
    Faded,
    /// Holds ore. Takes a little longer to open.
    Resonant,
    /// Holds tide-glass. Sealed to all but its owner while the owner's
    /// body lies or stands guard.
    Ancient,
}

impl RelicTier {
    /// How long a relic of this tier lasts before it crumbles.
    pub const fn lasts(self) -> Duration {
        match self {
            Self::Faded => Duration::minutes(15),
            Self::Resonant => Duration::minutes(30),
            Self::Ancient => Duration::minutes(60),
        }
    }

    /// How long it takes to open.
    pub const fn open_time(self) -> Duration {
        match self {
            Self::Faded => TAKE_TIME,
            Self::Resonant | Self::Ancient => Duration::seconds(TAKE_TIME.as_ticks() * 2),
        }
    }

    /// The name shown to players.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Faded => "faded relic",
            Self::Resonant => "resonant relic",
            Self::Ancient => "ancient relic",
        }
    }
}

/// What someone dropped when they died, lying where they fell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Relic {
    /// Whose it was.
    pub owner: CharacterId,
    /// Where it lies.
    pub place: PlaceId,
    /// How much of each resource it holds, in pack order.
    pub contents: [u64; Resource::ALL.len()],
    /// When it crumbles, with whatever is still in it.
    pub expires_at: Tick,
}

impl Relic {
    /// A relic of `contents` dropped at `now`, or `None` if there was
    /// nothing to drop.
    pub(crate) fn new(owner: CharacterId, place: PlaceId, contents: [u64; Resource::ALL.len()], now: Tick) -> Option<Self> {
        let tier = tier_of(&contents)?;
        Some(Self { owner, place, contents, expires_at: now + tier.lasts() })
    }

    /// The relic's tier: that of the richest thing in it.
    pub fn tier(&self) -> RelicTier {
        tier_of(&self.contents).unwrap_or(RelicTier::Faded)
    }
}

/// The tier of a pile, or `None` if the pile is empty.
fn tier_of(contents: &[u64; Resource::ALL.len()]) -> Option<RelicTier> {
    Resource::ALL
        .into_iter()
        .zip(contents)
        .filter(|&(_, &amount)| amount > 0)
        .map(|(resource, _)| match resource {
            Resource::Driftwood | Resource::Kelp => RelicTier::Faded,
            Resource::Ore => RelicTier::Resonant,
            Resource::Tideglass => RelicTier::Ancient,
        })
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNER: CharacterId = CharacterId::new(0);
    const HERE: PlaceId = PlaceId::new(1);

    fn relic(ore: u64, driftwood: u64, kelp: u64, glass: u64) -> Option<Relic> {
        Relic::new(OWNER, HERE, [ore, driftwood, kelp, glass], Tick::new(100))
    }

    #[test]
    fn nothing_dropped_leaves_no_relic() {
        assert_eq!(relic(0, 0, 0, 0), None);
    }

    #[test]
    fn a_relic_takes_the_tier_of_the_richest_thing_in_it() {
        assert_eq!(relic(0, 5, 9, 0).unwrap().tier(), RelicTier::Faded);
        assert_eq!(relic(1, 5, 9, 0).unwrap().tier(), RelicTier::Resonant);
        assert_eq!(relic(40, 5, 9, 1).unwrap().tier(), RelicTier::Ancient);
    }

    #[test]
    fn richer_relics_last_longer_and_open_slower() {
        assert_eq!(relic(0, 1, 0, 0).unwrap().expires_at, Tick::new(100) + Duration::minutes(15));
        assert_eq!(relic(1, 0, 0, 0).unwrap().expires_at, Tick::new(100) + Duration::minutes(30));
        assert_eq!(relic(0, 0, 0, 1).unwrap().expires_at, Tick::new(100) + Duration::minutes(60));
        assert!(RelicTier::Faded.open_time() < RelicTier::Ancient.open_time());
    }

    #[test]
    fn a_body_guard_is_its_owner_and_a_fifth_more() {
        let body = Character::new("Bryn", HERE);
        let guard = Creature::body_guard(CreatureId::new(7), OWNER, &body, HERE, Tick::ZERO, Tick::new(9));
        assert_eq!(guard.health(), 120);
        assert_eq!((guard.vigor, guard.resilience), (0, 0));
        assert_eq!(guard.being, Being { kind: CreatureKind::BodyGuard, of: Some(OWNER) });
        assert!(!guard.being.kind.is_aggressive());
    }

    #[test]
    fn a_sentinel_is_far_more_than_a_lurker() {
        let sentinel = Creature::native(CreatureKind::Sentinel, CreatureId::new(1), HERE, Tick::ZERO);
        let lurker = Creature::native(CreatureKind::Lurker, CreatureId::new(2), HERE, Tick::ZERO);
        assert_eq!((sentinel.health(), sentinel.vigor, sentinel.resilience, sentinel.hide), (300, 35, 35, 16));
        assert_eq!((lurker.health(), lurker.vigor, lurker.resilience), (60, 0, 0));
        assert!(sentinel.being.kind.is_aggressive());
    }

    #[test]
    fn weak_blows_glance_off_a_sentinel() {
        let mut sentinel = Creature::native(CreatureKind::Sentinel, CreatureId::new(1), HERE, Tick::ZERO);
        assert_eq!(sentinel.hurt(16), (0, 300));
        assert_eq!(sentinel.hurt(17), (17, 283));
    }

    #[test]
    fn a_creature_remembers_each_foe_once() {
        let mut lurker = Creature::native(CreatureKind::Lurker, CreatureId::new(0), HERE, Tick::ZERO);
        lurker.provoke(OWNER);
        lurker.provoke(OWNER);
        assert_eq!(lurker.foes, [OWNER]);
        assert_eq!(lurker.hurt(25), (25, 35));
        assert_eq!(lurker.hurt(99), (99, 0));
    }
}
