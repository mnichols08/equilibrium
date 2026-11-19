//! A character: where they are, what they know and what they carry.

use alloc::string::String;
use alloc::vec::Vec;

use eq_clock::{Duration, Tick};
use eq_curve::stepped_bonus;
use eq_meter::Meter;
use eq_skills::Proficiency;
use eq_world_graph::PlaceId;

use crate::content::{
    AFFINITY_XP_PER_WASTED_LEVEL, ATTRIBUTE_RULES, ATTRIBUTE_XP_PER_UNIT, DEATH_PLACE_CARRYOVER_TIMES,
    AFTERLIFE_LEVEL, AFTERLIFE_LEVEL_PER_DEATH, GRAVE_SHIFT, MOST_AFTERLIFE_LEVEL, BLOODLINE_LEARNING_PERCENT, FIGHTING_XP_PER_BLOW, NEWCOMER_BELOW_LEVEL, SHADE_HEALTH, LINEAGE_XP_PER_LIFE, MOST_ECTOPLASM, MAX_HEALTH, MEMORIES_TO_REDEEM, SKILL_RULES,
    SKILL_XP_PER_UNIT, STANDING_BANDS, TIERS_PER_WHOLE,
};
use crate::bloodline::Bloodline;
use crate::event::Outcome;
use crate::rest::{atrophy, atrophy_floor, rest_gained, with_rested_bonus, FULLY_RESTED};
use crate::spirit::{ectoplasm_per_draw, inheritance, SpiritTier};
use crate::types::{Attribute, Cause, Passing, Resource, SkillKind, Standing};

eq_ids::define_id!(
    /// Identifies a character within one [`Game`](crate::Game).
    CharacterId
);

#[cfg(feature = "serde")]
crate::wire::as_number!(CharacterId);

/// A death: where the body lies and how long the spirit has to get back
/// to it.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Death {
    /// Where the body lies.
    pub body: PlaceId,
    /// What killed them. While it stands over the body there is no
    /// getting back into it.
    pub by: Cause,
    /// How the spirit stands to the body.
    pub passing: Passing,
    /// When the body, if its spirit has not come back, rises as a Body Guard.
    pub lost_at: Tick,
    /// Whether the body has risen. There is no returning to it then.
    pub risen: bool,
    /// Whether someone buried the body. There is no returning to it,
    /// and nobody can disturb it.
    pub buried: bool,
    /// Whether someone else took what lay beside the body. A life whose
    /// body was disturbed is not remembered by the next.
    pub disturbed: bool,
}

impl Death {
    /// Whether the next life will remember this one, and so learn
    /// again fast what it knew: nobody else took what lay beside the
    /// body, or dug up its grave.
    pub const fn is_remembered(&self) -> bool {
        !self.disturbed
    }
}

/// A spirit whose body died in its sleep. It has no body to return to
/// and a limited time awake in which to redeem itself.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Forgotten {
    /// Time spent awake as a forgotten spirit. Dormant time is not counted.
    pub played: Duration,
    /// Whether the spirit is dormant, as it is until its player returns.
    pub dormant: bool,
    /// The places where it has recovered a memory.
    pub found_in: Vec<PlaceId>,
    /// Whether it has never lived: it came into a world whose lands
    /// were all founded, uninvited. No clock runs on the unborn.
    pub unborn: bool,
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

/// What a bloodline's wound does while it is open.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wounded {
    /// Brine in the blood: they lose health where they would mend.
    Bleeding,
    /// Cracked like stone: they do not mend at all.
    Cracked,
}

/// One character in the world.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Character {
    name: String,
    /// The anchor of the territory they belong to: where each of their
    /// lives begins.
    pub(crate) home: PlaceId,
    pub(crate) place: PlaceId,
    /// The bloodline they were born into. It outlasts every life.
    pub(crate) bloodline: Bloodline,
    pub(crate) busy_until: Tick,
    skills: [Proficiency; SkillKind::ALL.len()],
    attributes: [Proficiency; Attribute::ALL.len()],
    pack: [u64; Resource::ALL.len()],
    taint: Meter,
    /// Where they have been gathering lately, and how worked out they are there.
    pub(crate) worked: crate::worked::Worked,
    /// A bloodline's wound, opened by a heavy blow, and when it closes.
    pub(crate) wound: Option<(Wounded, Tick)>,
    /// When they came to where they stand.
    pub(crate) arrived_at: Tick,
    health: Meter,
    death: Option<Death>,
    forgotten: Option<Forgotten>,
    /// What the spirit has practised in the afterlife since it left
    /// the living. It is learned from nothing, and the next life
    /// begins with it.
    practised: [Proficiency; SkillKind::ALL.len()],
    /// The health of the shade the spirit is fighting for practice.
    shade: u32,
    /// Which life this is, counting the first as 1.
    life: u32,
    rested: Meter,
    /// When sleep began, or will begin once they have settled.
    asleep_from: Option<Tick>,
    /// Ectoplasm held. It is the one thing death does not take.
    ectoplasm: u64,
    /// Ectoplasm drawn over every life, which sets the spirit tier.
    drawn: u64,
    /// The share of the last life this one began with, as a percentage.
    kept: u32,
    /// Extra sleep this sleeper has in hand from being tended. It is
    /// gone when they wake.
    vigil: Duration,
}

/// How a new life begins.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rebirth {
    /// The share of the old life kept before Lineage and ectoplasm.
    pub base: u32,
    /// The attribute of the place of death, which keeps more.
    pub favoured: Option<Attribute>,
    /// Whether ectoplasm may be spent on a better inheritance. If not,
    /// it is lost.
    pub spend_ectoplasm: bool,
    /// The bloodline of the land they arise in. Only someone who has
    /// never lived takes it: blood, once had, is kept.
    pub bloodline: Bloodline,
}

/// The attributes that waste when the body lies too long.
const BODY: [Attribute; 3] = [Attribute::Vigor, Attribute::Finesse, Attribute::Resilience];

impl Character {
    pub(crate) fn new(name: &str, place: PlaceId) -> Self {
        Self {
            name: name.into(),
            home: place,
            place,
            bloodline: Bloodline::default(),
            busy_until: Tick::ZERO,
            skills: [Proficiency::new(); SkillKind::ALL.len()],
            attributes: [Proficiency::new(); Attribute::ALL.len()],
            pack: [0; Resource::ALL.len()],
            taint: Meter::empty(0, 100),
            worked: crate::worked::Worked::new(),
            wound: None,
            arrived_at: Tick::ZERO,
            health: Meter::full(0, MAX_HEALTH),
            death: None,
            forgotten: None,
            practised: [Proficiency::new(); SkillKind::ALL.len()],
            shade: SHADE_HEALTH,
            life: 1,
            rested: Meter::full(0, FULLY_RESTED as i64),
            asleep_from: None,
            ectoplasm: 0,
            drawn: 0,
            kept: 0,
            vigil: Duration::ZERO,
        }
    }

    /// Extra sleep in hand from being tended.
    pub const fn vigil(&self) -> Duration {
        self.vigil
    }

    /// Adds `more` to the sleep in hand, up to `most`. Returns the new total.
    pub(crate) fn tend(&mut self, more: Duration, most: Duration) -> Duration {
        self.vigil = (self.vigil + more).min(most);
        self.vigil
    }

    /// Takes `amount` of `resource` out of the pack, if it is all there.
    pub(crate) fn spend(&mut self, resource: Resource, amount: u64) -> bool {
        let held = &mut self.pack[resource.index()];
        let enough = *held >= amount;
        if enough {
            *held -= amount;
        }
        enough
    }

    /// The anchor of the territory they belong to, where each of their
    /// lives begins.
    pub const fn home(&self) -> PlaceId {
        self.home
    }

    /// Ectoplasm held.
    pub const fn ectoplasm(&self) -> u64 {
        self.ectoplasm
    }

    /// How far the character has come as a spirit, over every life.
    pub fn spirit_tier(&self) -> SpiritTier {
        SpiritTier::of(self.drawn)
    }

    /// The share of the last life this one began with, as a percentage.
    /// Zero in a first life.
    pub const fn kept(&self) -> u32 {
        self.kept
    }

    /// Draws ectoplasm from the place the spirit is in. Returns how
    /// much was drawn and what else came of it.
    pub(crate) fn draw(&mut self, outcomes: &mut Vec<Outcome>) -> u64 {
        let attunement = &mut self.attributes[Attribute::Attunement.index()];
        let amount = ectoplasm_per_draw(attunement.level()).min(MOST_ECTOPLASM - self.ectoplasm);
        if attunement.practice(amount * ATTRIBUTE_XP_PER_UNIT, &ATTRIBUTE_RULES).levels_gained > 0 {
            outcomes.push(Outcome::AttributeRose { attribute: Attribute::Attunement, level: attunement.level() });
        }
        let before = self.spirit_tier();
        self.ectoplasm += amount;
        self.drawn += amount;
        if self.spirit_tier() != before {
            outcomes.push(Outcome::SpiritRose { tier: self.spirit_tier() });
        }
        amount
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
        self.vigil = Duration::ZERO;
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
        let wasted: u32 = self
            .skills
            .iter_mut()
            .chain(attributes.map(|(_, attribute)| attribute))
            .map(|proficiency| proficiency.lose(levels, atrophy_floor(proficiency.peak())))
            .sum();
        // The spirit grows as the body fails.
        let affinity = u64::from(wasted) * AFFINITY_XP_PER_WASTED_LEVEL;
        self.attributes[Attribute::Attunement.index()].practice(affinity, &ATTRIBUTE_RULES);
        wasted
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

    /// Takes `damage` from health and returns what is left.
    pub(crate) fn hurt(&mut self, damage: u32) -> u32 {
        self.health.adjust(-i64::from(damage));
        self.health()
    }

    /// Restores up to `points` of health.
    pub(crate) fn mend(&mut self, points: i64) {
        self.health.adjust(points);
    }

    /// Spends `amount` of the ectoplasm they hold, as far as it goes.
    pub(crate) fn spend_ectoplasm(&mut self, amount: u64) {
        self.ectoplasm = self.ectoplasm.saturating_sub(amount);
    }

    /// Scatters `percent` of the ectoplasm they hold.
    pub(crate) fn scatter_ectoplasm(&mut self, percent: u64) {
        self.ectoplasm -= self.ectoplasm * percent / 100;
    }

    /// Dies, dropping everything carried and paying off all taint.
    /// Returns what was dropped and, if dying lifted the land's mark,
    /// that change of standing.
    pub(crate) fn die(&mut self, death: Death) -> ([u64; Resource::ALL.len()], Option<Outcome>) {
        self.death = Some(death);
        self.asleep_from = None;
        self.vigil = Duration::ZERO;
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
        self.vigil = Duration::ZERO;
        self.forgotten = Some(Forgotten { played: Duration::ZERO, dormant: true, found_in: Vec::new(), unborn: false });
        let cleansed = self.shift_taint(-i64::from(self.taint()));
        (core::mem::take(&mut self.pack), cleansed)
    }

    /// Makes this character a spirit that has not yet lived.
    pub(crate) fn be_unborn(&mut self) {
        self.forgotten = Some(Forgotten { played: Duration::ZERO, dormant: false, found_in: Vec::new(), unborn: true });
    }

    /// Marks the body as buried.
    pub(crate) fn be_buried(&mut self) {
        if let Some(death) = &mut self.death {
            death.buried = true;
        }
    }

    /// The bloodline they were born into.
    pub const fn bloodline(&self) -> Bloodline {
        self.bloodline
    }

    /// Whether the character is new to the world: in their first life,
    /// with no skill yet at the level a spirit must reach to live again.
    pub fn is_newcomer(&self) -> bool {
        self.life == 1 && self.skills.iter().all(|skill| skill.level() < NEWCOMER_BELOW_LEVEL)
    }

    /// Forgets how high everything once stood: each skill and
    /// attribute is remembered only as it is now. This is what breaking
    /// the tie to an old life costs, and it cannot be undone.
    pub(crate) fn forget_the_old_life(&mut self) {
        for known in self.skills.iter_mut().chain(self.attributes.iter_mut()) {
            *known = known.reborn(100);
        }
    }

    /// What having their grave dug up does to its owner, whoever did
    /// the digging: the body is the less for it and the spirit the
    /// more. Up to [`GRAVE_SHIFT`] levels leave whichever of Vigor,
    /// Finesse and Resilience stands highest, and as many are added to
    /// Attunement. Then the old life is forgotten. Returns which
    /// attribute fell, and by how much.
    pub(crate) fn be_unearthed(&mut self) -> (Attribute, u32) {
        let body = [Attribute::Vigor, Attribute::Finesse, Attribute::Resilience];
        // The first of the highest: `max_by_key` would take the last.
        let strongest = body.into_iter().rev().max_by_key(|&attribute| self.attribute(attribute).level()).unwrap_or(Attribute::Vigor);
        let moved = self.attributes[strongest.index()].lose(GRAVE_SHIFT, 0);
        let attunement = &mut self.attributes[Attribute::Attunement.index()];
        *attunement = attunement.at_least((attunement.level() + moved).min(ATTRIBUTE_RULES.cap));
        self.forget_the_old_life();
        (strongest, moved)
    }

    /// Notes that someone else took what lay beside the body.
    pub(crate) fn be_disturbed(&mut self) {
        if let Some(death) = &mut self.death {
            death.disturbed = true;
        }
    }

    /// How far the spirit has come in `skill` in the afterlife.
    pub const fn practised(&self, skill: SkillKind) -> &Proficiency {
        &self.practised[skill.index()]
    }

    /// The level this spirit must reach in one skill before it lives
    /// again. Someone who has never lived is asked the least; every
    /// death after that asks more.
    pub fn afterlife_goal(&self) -> u32 {
        let unborn = self.forgotten.as_ref().is_some_and(|forgotten| forgotten.unborn);
        let deaths = if unborn { 0 } else { self.life };
        (AFTERLIFE_LEVEL + AFTERLIFE_LEVEL_PER_DEATH * deaths).min(MOST_AFTERLIFE_LEVEL)
    }

    /// The best the spirit has done in any one skill in the afterlife.
    pub fn best_practised(&self) -> u32 {
        self.practised.iter().map(Proficiency::level).max().unwrap_or(0)
    }

    /// Practises `skill` as a spirit, by `xp`. It is learned as if for
    /// the first time, whatever the living character knew. Returns the
    /// level reached.
    pub(crate) fn practise(&mut self, skill: SkillKind, xp: u64, outcomes: &mut Vec<Outcome>) -> u32 {
        let progress = &mut self.practised[skill.index()];
        if progress.practice(xp, &SKILL_RULES).levels_gained > 0 {
            let level = progress.level();
            outcomes.push(Outcome::Practised { skill, level, needed: self.afterlife_goal() });
            return level;
        }
        progress.level()
    }

    /// Lands a spirit's blow worth `damage` on the shade it is fighting.
    /// Returns the shade's health afterwards; at 0 a fresh one stands
    /// in its place for the next blow.
    pub(crate) fn hurt_shade(&mut self, damage: u32) -> u32 {
        let left = self.shade.saturating_sub(damage);
        self.shade = if left == 0 { SHADE_HEALTH } else { left };
        left
    }

    /// Learns a little fighting from a blow struck while alive.
    ///
    /// `percent` is how much the times make a blow worth: 100 as a rule.
    pub(crate) fn learn_from_blow(&mut self, percent: u64, outcomes: &mut Vec<Outcome>) {
        let skill = SkillKind::Fighting;
        let xp = with_rested_bonus(FIGHTING_XP_PER_BLOW * percent / 100, self.rested());
        let progress = &mut self.skills[skill.index()];
        if progress.practice(xp, &SKILL_RULES).levels_gained > 0 {
            outcomes.push(Outcome::SkillRose { skill, level: progress.level() });
        }
    }

    /// Marks the body as risen: it is a Body Guard now, and lost to its spirit.
    pub(crate) fn lose_body(&mut self) {
        if let Some(death) = &mut self.death {
            death.risen = true;
        }
    }

    /// Begins a new life at `place`: whole and rested, with a share of
    /// what the last life learned and no memory of its peaks.
    ///
    /// The share starts from `rebirth.base` and is raised by Lineage
    /// and by ectoplasm, which is spent. The attribute of the place of
    /// death keeps more than the rest. Lineage itself is never cut
    /// back, and grows with every life lived.
    pub(crate) fn be_reborn(&mut self, place: PlaceId, now: Tick, rebirth: Rebirth) {
        let lineage = Attribute::Lineage.index();
        let held = if rebirth.spend_ectoplasm { self.ectoplasm } else { 0 };
        let inheritance = inheritance(rebirth.base, self.attributes[lineage].level(), held);
        self.ectoplasm = held - inheritance.spent;
        self.kept = inheritance.percent;
        // A body buried or left in peace is remembered: what it knew
        // comes back fast. What was practised in the afterlife comes
        // along whole.
        let remembered = self.death.is_some_and(|death| death.is_remembered());
        let practised = core::mem::replace(&mut self.practised, [Proficiency::new(); SkillKind::ALL.len()]);
        let unborn = self.forgotten.as_ref().is_some_and(|forgotten| forgotten.unborn);
        self.death = None;
        self.forgotten = None;
        self.place = place;
        self.home = place;
        self.busy_until = now;
        // The unborn have had no life yet: this is their first.
        self.life += u32::from(!unborn);
        if unborn {
            self.bloodline = rebirth.bloodline;
        }
        self.health.set(MAX_HEALTH);
        // A first body wakes fresh. One come back from the dead wakes
        // weary, and has to sleep before it is at its best.
        self.rested.set(if unborn { i64::from(FULLY_RESTED) } else { 0 });
        let favoured = rebirth.favoured.map(Attribute::index);
        let carry = |known: &Proficiency, percent: u32| {
            if remembered {
                known.reborn_remembering(percent)
            } else {
                known.reborn(percent)
            }
        };
        for (skill, practised) in self.skills.iter_mut().zip(practised) {
            *skill = carry(skill, inheritance.percent).at_least(practised.level());
        }
        for (index, attribute) in self.attributes.iter_mut().enumerate().filter(|(index, _)| *index != lineage) {
            let times = if favoured == Some(index) { DEATH_PLACE_CARRYOVER_TIMES } else { 1 };
            *attribute = carry(attribute, (inheritance.percent * times).min(100));
        }
        if !unborn {
            self.attributes[lineage].practice(LINEAGE_XP_PER_LIFE, &ATTRIBUTE_RULES);
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

    /// Sets the ectoplasm held, for tests.
    #[cfg(test)]
    pub(crate) fn ectoplasm_for_test(&mut self, ectoplasm: u64) {
        self.ectoplasm = ectoplasm;
    }

    /// Sets health, for tests.
    #[cfg(test)]
    pub(crate) fn health_for_test(&mut self, health: u32) {
        self.health.set(i64::from(health));
    }

    /// Raises an attribute to at least `level`, for tests that need
    /// someone strong without weeks of work.
    #[cfg(test)]
    pub(crate) fn strengthen(&mut self, attribute: Attribute, level: u32) {
        let known = &mut self.attributes[attribute.index()];
        *known = known.at_least(level);
    }

    /// Where they have been gathering lately, and how worked out they are there.
    pub const fn worked(&self) -> &crate::worked::Worked {
        &self.worked
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
            // Blood tells: what the bloodline leans toward comes easier.
            let xp = if self.bloodline.leans_to(attribute) { xp * BLOODLINE_LEARNING_PERCENT / 100 } else { xp };
            let progress = &mut self.attributes[attribute.index()];
            if progress.practice(xp, &ATTRIBUTE_RULES).levels_gained > 0 {
                outcomes.push(Outcome::AttributeRose { attribute, level: progress.level() });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAMP: PlaceId = PlaceId::new(0);

    /// Someone who has mined enough ore to have levels worth losing.
    fn miner() -> Character {
        let mut miner = Character::new("Ash", CAMP);
        let mut outcomes = Vec::new();
        for _ in 0..2_000 {
            miner.train(Resource::Ore, 12, &mut outcomes);
        }
        miner
    }

    fn rebirth(favoured: Option<Attribute>) -> Rebirth {
        Rebirth { base: 15, favoured, spend_ectoplasm: true, bloodline: Bloodline::default() }
    }

    #[test]
    fn the_attribute_of_the_place_of_death_keeps_twice_the_share() {
        let before = miner();
        let (vigor, resilience) = (before.attribute(Attribute::Vigor).level(), before.attribute(Attribute::Resilience).level());
        assert!(vigor >= 10 && resilience >= 10, "not enough to test with: {vigor}, {resilience}");

        let mut after = before.clone();
        after.be_reborn(CAMP, Tick::ZERO, rebirth(Some(Attribute::Vigor)));
        assert_eq!(after.attribute(Attribute::Vigor).level(), vigor * 30 / 100);
        assert_eq!(after.attribute(Attribute::Resilience).level(), resilience * 15 / 100);
        assert_eq!(after.kept(), 15);
    }

    #[test]
    fn lineage_is_never_cut_back_and_raises_what_later_lives_keep() {
        let mut ash = miner();
        for life in 2..=8 {
            let lineage = ash.attribute(Attribute::Lineage).level();
            ash.be_reborn(CAMP, Tick::ZERO, rebirth(None));
            assert_eq!(ash.life(), life);
            assert!(ash.attribute(Attribute::Lineage).level() > lineage, "life {life} should add to Lineage");
            assert_eq!(ash.kept(), 15 + lineage / 2);
        }
    }

    #[test]
    fn fading_loses_the_ectoplasm_with_everything_else() {
        let mut ash = miner();
        let mut outcomes = Vec::new();
        for _ in 0..60 {
            ash.draw(&mut outcomes);
        }
        assert_eq!(ash.ectoplasm(), 60);
        ash.be_reborn(CAMP, Tick::ZERO, Rebirth { base: 5, favoured: None, spend_ectoplasm: false, bloodline: Bloodline::default() });
        assert_eq!((ash.kept(), ash.ectoplasm()), (5, 0));
        assert_eq!(ash.spirit_tier(), crate::spirit::SpiritTier::LostShade, "the tier counts what was ever drawn: 60");
    }

    #[test]
    fn a_spirit_can_hold_only_so_much_ectoplasm() {
        let mut ash = Character::new("Ash", CAMP);
        let mut outcomes = Vec::new();
        let drawn: u64 = (0..600).map(|_| ash.draw(&mut outcomes)).sum();
        assert_eq!((drawn, ash.ectoplasm()), (MOST_ECTOPLASM, MOST_ECTOPLASM));
    }
}
