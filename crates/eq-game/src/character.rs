//! A character: where they are, what they know and what they carry.

use alloc::string::String;
use alloc::vec::Vec;

use eq_clock::{Duration, Tick};
use eq_curve::stepped_bonus;
use eq_meter::Meter;
use eq_skills::Proficiency;
use eq_world_graph::PlaceId;

use crate::content::{
    ATTRIBUTE_RULES, ATTRIBUTE_XP_PER_UNIT, HEALTH_ON_RISING, MAX_HEALTH, SKILL_RULES, SKILL_XP_PER_UNIT, STANDING_BANDS,
    TIERS_PER_WHOLE,
};
use crate::event::Outcome;
use crate::rest::{atrophy, atrophy_floor, rest_gained, with_rested_bonus, FULLY_RESTED};
use crate::types::{Attribute, Resource, SkillKind, Standing};

eq_ids::define_id!(
    /// Identifies a character within one [`Game`](crate::Game).
    CharacterId
);

/// One character in the world.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Character {
    name: String,
    pub(crate) place: PlaceId,
    pub(crate) busy_until: Tick,
    skills: [Proficiency; SkillKind::ALL.len()],
    attributes: [Proficiency; Attribute::ALL.len()],
    pack: [u64; Resource::ALL.len()],
    taint: Meter,
    health: Meter,
    down_until: Option<Tick>,
    rested: Meter,
    /// When sleep began, or will begin once they have settled.
    asleep_from: Option<Tick>,
}

/// The attributes that waste when the body lies too long.
const BODY: [Attribute; 3] = [Attribute::Vigor, Attribute::Finesse, Attribute::Resilience];

impl Character {
    pub(crate) fn new(name: &str, place: PlaceId) -> Self {
        Self {
            name: name.into(),
            place,
            busy_until: Tick::ZERO,
            skills: [Proficiency::new(); SkillKind::ALL.len()],
            attributes: [Proficiency::new(); Attribute::ALL.len()],
            pack: [0; Resource::ALL.len()],
            taint: Meter::empty(0, 100),
            health: Meter::full(0, MAX_HEALTH),
            down_until: None,
            rested: Meter::full(0, FULLY_RESTED as i64),
            asleep_from: None,
        }
    }

    /// How rested the character is, from 0 to 100.
    pub const fn rested(&self) -> u32 {
        // The meter is bounded to 0..=100.
        self.rested.value() as u32
    }

    /// Whether the character is lying down, asleep or still settling.
    pub const fn is_lying_down(&self) -> bool {
        self.asleep_from.is_some()
    }

    /// When sleep began, or will begin once they have settled.
    pub const fn asleep_from(&self) -> Option<Tick> {
        self.asleep_from
    }

    /// Whether the character is asleep at `now`.
    pub fn is_asleep(&self, now: Tick) -> bool {
        self.asleep_from.is_some_and(|from| from <= now)
    }

    /// Lies down, to be asleep from `asleep_from` if nothing disturbs them.
    pub(crate) fn lie_down(&mut self, asleep_from: Tick) {
        self.asleep_from = Some(asleep_from);
        self.busy_until = asleep_from;
    }

    /// Spends one point of rest.
    pub(crate) fn tire(&mut self) {
        self.rested.adjust(-1);
    }

    /// Gets up. Returns `None` if they were not lying down; otherwise
    /// what the sleep did, as a [`Outcome::Woke`].
    pub(crate) fn wake(&mut self, now: Tick) -> Option<Outcome> {
        let from = self.asleep_from.take()?;
        let slept = now.since(from);
        self.rested.adjust(i64::from(rest_gained(slept)));
        Some(Outcome::Woke { slept, rested: self.rested(), wasted: self.waste(slept) })
    }

    /// Applies atrophy for a sleep of `slept` and returns the levels lost
    /// across every body skill and attribute.
    fn waste(&mut self, slept: Duration) -> u32 {
        let levels = atrophy(slept);
        let body = BODY.map(Attribute::index);
        let attributes = self.attributes.iter_mut().enumerate().filter(|(index, _)| body.contains(index));
        self.skills
            .iter_mut()
            .chain(attributes.map(|(_, attribute)| attribute))
            .map(|proficiency| proficiency.lose(levels, atrophy_floor(proficiency.peak())))
            .sum()
    }

    /// Empties the pack and returns what was in it.
    pub(crate) fn empty_pack(&mut self) -> [u64; Resource::ALL.len()] {
        core::mem::take(&mut self.pack)
    }

    /// Health, from 0 to 100.
    pub const fn health(&self) -> u32 {
        // The meter is bounded to 0..=100.
        self.health.value() as u32
    }

    /// When the character will get back up, if they are down.
    pub const fn down_until(&self) -> Option<Tick> {
        self.down_until
    }

    /// Whether the character has lost a fight and not yet got up.
    pub const fn is_down(&self) -> bool {
        self.down_until.is_some()
    }

    /// Takes `damage` from health and returns what is left.
    pub(crate) fn hurt(&mut self, damage: u32) -> u32 {
        self.health.adjust(-i64::from(damage));
        self.health()
    }

    /// Restores up to `points` of health.
    pub(crate) fn mend(&mut self, points: i64) {
        self.health.adjust(points);
    }

    /// Goes down until `until`, dropping everything carried and paying
    /// off all taint. Returns what was dropped and, if the fall lifted
    /// the land's mark, that change of standing.
    pub(crate) fn fall(&mut self, until: Tick) -> ([u64; Resource::ALL.len()], Option<Outcome>) {
        self.down_until = Some(until);
        let cleansed = self.shift_taint(-i64::from(self.taint()));
        (core::mem::take(&mut self.pack), cleansed)
    }

    /// Gets back up, weak but standing.
    pub(crate) fn rise(&mut self) {
        self.down_until = None;
        self.health.set(HEALTH_ON_RISING);
    }

    /// What the character is called.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Where the character is.
    pub const fn place(&self) -> PlaceId {
        self.place
    }

    /// When the character's current action ends.
    pub const fn busy_until(&self) -> Tick {
        self.busy_until
    }

    /// A skill's progress.
    pub const fn skill(&self, skill: SkillKind) -> &Proficiency {
        &self.skills[skill.index()]
    }

    /// An attribute's progress.
    pub const fn attribute(&self, attribute: Attribute) -> &Proficiency {
        &self.attributes[attribute.index()]
    }

    /// How much of a resource the character carries.
    pub const fn carried(&self, resource: Resource) -> u64 {
        self.pack[resource.index()]
    }

    /// Everything the character carries, added together.
    pub fn carried_total(&self) -> u64 {
        self.pack.iter().sum()
    }

    /// How tainted the character is by over-harvesting, from 0 to 100.
    pub const fn taint(&self) -> u32 {
        // The meter is bounded to 0..=100.
        self.taint.value() as u32
    }

    /// How the land regards the character.
    pub fn standing(&self) -> Standing {
        STANDING_BANDS.band_of(self.taint.value())
    }

    /// Moves taint by `delta` and reports a change of standing, if any.
    pub(crate) fn shift_taint(&mut self, delta: i64) -> Option<Outcome> {
        let change = self.taint.adjust(delta);
        STANDING_BANDS.crossing(change).map(|(from, to)| Outcome::StandingChanged { from, to })
    }

    /// Puts a whole pile in the pack, by resource in pack order.
    pub(crate) fn stow_all(&mut self, pile: [u64; Resource::ALL.len()]) {
        for (held, found) in self.pack.iter_mut().zip(pile) {
            *held += found;
        }
    }

    /// Puts `amount` of `resource` in the pack.
    pub(crate) fn stow(&mut self, resource: Resource, amount: u64) {
        self.pack[resource.index()] += amount;
    }

    /// Trains the skill and attributes that gathering `resource` uses.
    pub(crate) fn train(&mut self, resource: Resource, units: u32, outcomes: &mut Vec<Outcome>) {
        let units = u64::from(units);
        let rested = self.rested();
        let skill = resource.skill();
        let progress = &mut self.skills[skill.index()];
        let xp = with_rested_bonus(units * SKILL_XP_PER_UNIT, rested);
        if progress.practice(xp, &SKILL_RULES).levels_gained > 0 {
            outcomes.push(Outcome::SkillRose { skill, level: progress.level() });
        }

        let xp = stepped_bonus(units * ATTRIBUTE_XP_PER_UNIT, resource.tier(), TIERS_PER_WHOLE);
        let xp = with_rested_bonus(xp, rested);
        for &attribute in resource.attributes() {
            let progress = &mut self.attributes[attribute.index()];
            if progress.practice(xp, &ATTRIBUTE_RULES).levels_gained > 0 {
                outcomes.push(Outcome::AttributeRose { attribute, level: progress.level() });
            }
        }
    }
}
