//! A character: where they are, what they know and what they carry.

use alloc::string::String;
use alloc::vec::Vec;

use eq_clock::{Duration, Tick};
use eq_curve::stepped_bonus;
use eq_meter::Meter;
use eq_skills::Proficiency;
use eq_world_graph::PlaceId;

use crate::content::{
    ATTRIBUTE_RULES, ATTRIBUTE_XP_PER_UNIT, HEALTH_ON_RETURN, MAX_HEALTH, MEMORIES_TO_REDEEM, MOST_DEGRADATION, SKILL_RULES,
    SKILL_XP_PER_UNIT, STANDING_BANDS, TIERS_PER_WHOLE,
};
use crate::event::Outcome;
use crate::rest::{atrophy, atrophy_floor, rest_gained, with_rested_bonus, FULLY_RESTED};
use crate::types::{Attribute, Passing, Resource, SkillKind, Standing};

eq_ids::define_id!(
    /// Identifies a character within one [`Game`](crate::Game).
    CharacterId
);

/// A death: where the body lies and how long the spirit has to get back
/// to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Death {
    /// Where the body lies.
    pub body: PlaceId,
    /// How the spirit stands to the body.
    pub passing: Passing,
    /// The earliest the spirit may return.
    pub ready_at: Tick,
    /// When the body, if its spirit has not come back, rises as a Body Guard.
    pub lost_at: Tick,
    /// Whether the body has risen. There is no returning to it then.
    pub risen: bool,
}

/// A spirit whose body died in its sleep. It has no body to return to
/// and a limited time awake in which to redeem itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Forgotten {
    /// Time spent awake as a forgotten spirit. Dormant time is not counted.
    pub played: Duration,
    /// Whether the spirit is dormant, as it is until its player returns.
    pub dormant: bool,
    /// The places where it has recovered a memory.
    pub found_in: Vec<PlaceId>,
}

impl Forgotten {
    /// How many memories have been recovered.
    pub fn memories(&self) -> u32 {
        // A zone has only a handful of places.
        self.found_in.len() as u32
    }

    /// Whether enough memories have been recovered to be redeemed.
    pub fn is_redeemed(&self) -> bool {
        self.memories() >= MEMORIES_TO_REDEEM
    }
}

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
    death: Option<Death>,
    forgotten: Option<Forgotten>,
    /// Which life this is, counting the first as 1.
    life: u32,
    /// How worn the spirit is by recent deaths. It outlasts rebirth.
    degradation: Meter,
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
            death: None,
            forgotten: None,
            life: 1,
            degradation: Meter::empty(0, MOST_DEGRADATION),
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

    /// The character's death, if they are a spirit.
    pub const fn death(&self) -> Option<&Death> {
        self.death.as_ref()
    }

    /// The character's state as a forgotten spirit, if they are one.
    pub const fn forgotten(&self) -> Option<&Forgotten> {
        self.forgotten.as_ref()
    }

    pub(crate) const fn forgotten_mut(&mut self) -> Option<&mut Forgotten> {
        self.forgotten.as_mut()
    }

    /// Whether the character is a forgotten spirit lying dormant.
    pub fn is_dormant(&self) -> bool {
        self.forgotten.as_ref().is_some_and(|forgotten| forgotten.dormant)
    }

    /// Whether the character has died and not yet returned or been reborn.
    /// A spirit walks the same places, unseen by the living.
    pub const fn is_spirit(&self) -> bool {
        self.death.is_some() || self.forgotten.is_some()
    }

    /// Which life this is, counting the first as 1.
    pub const fn life(&self) -> u32 {
        self.life
    }

    /// How many recent deaths still wear on the spirit.
    pub const fn degradation(&self) -> u32 {
        // The meter is bounded to a handful.
        self.degradation.value() as u32
    }

    /// Lets one death's wear on the spirit fade.
    pub(crate) fn recover_spirit(&mut self) {
        self.degradation.adjust(-1);
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

    /// Dies, dropping everything carried and paying off all taint.
    /// Returns what was dropped and, if dying lifted the land's mark,
    /// that change of standing.
    pub(crate) fn die(&mut self, death: Death) -> ([u64; Resource::ALL.len()], Option<Outcome>) {
        self.death = Some(death);
        self.degradation.adjust(1);
        self.asleep_from = None;
        let cleansed = self.shift_taint(-i64::from(self.taint()));
        (core::mem::take(&mut self.pack), cleansed)
    }

    /// Dies of sleeping too long. The body wastes as far as it can and is
    /// gone; the spirit lies dormant where the body lay. Returns what
    /// was dropped and, if dying lifted the land's mark, that change.
    pub(crate) fn sleep_to_death(&mut self, now: Tick) -> ([u64; Resource::ALL.len()], Option<Outcome>) {
        if let Some(from) = self.asleep_from.take() {
            self.waste(now.since(from));
        }
        self.forgotten = Some(Forgotten { played: Duration::ZERO, dormant: true, found_in: Vec::new() });
        self.degradation.adjust(1);
        let cleansed = self.shift_taint(-i64::from(self.taint()));
        (core::mem::take(&mut self.pack), cleansed)
    }

    /// Marks the body as risen: it is a Body Guard now, and lost to its spirit.
    pub(crate) fn lose_body(&mut self) {
        if let Some(death) = &mut self.death {
            death.risen = true;
        }
    }

    /// Goes back into the body, weak but alive. Returns where the body lay.
    pub(crate) fn return_to_body(&mut self) -> Option<PlaceId> {
        let death = self.death.take()?;
        self.place = death.body;
        self.health.set(HEALTH_ON_RETURN);
        Some(death.body)
    }

    /// Begins a new life at `place`: whole and rested, with `carryover`
    /// percent of what the last life learned and no memory of its peaks.
    pub(crate) fn be_reborn(&mut self, place: PlaceId, now: Tick, carryover: u32) {
        self.death = None;
        self.forgotten = None;
        self.place = place;
        self.busy_until = now;
        self.life += 1;
        self.health.set(MAX_HEALTH);
        self.rested.set(i64::from(FULLY_RESTED));
        for proficiency in self.skills.iter_mut().chain(&mut self.attributes) {
            *proficiency = proficiency.reborn(carryover);
        }
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
