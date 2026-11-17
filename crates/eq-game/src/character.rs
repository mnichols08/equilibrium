//! A character: where they are, what they know and what they carry.

use alloc::string::String;
use alloc::vec::Vec;

use eq_clock::Tick;
use eq_curve::stepped_bonus;
use eq_meter::Meter;
use eq_skills::Proficiency;
use eq_world_graph::PlaceId;

use crate::content::{
    ATTRIBUTE_RULES, ATTRIBUTE_XP_PER_UNIT, SKILL_RULES, SKILL_XP_PER_UNIT, STANDING_BANDS, TIERS_PER_WHOLE,
};
use crate::event::Outcome;
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
}

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

    /// Puts `amount` of `resource` in the pack.
    pub(crate) fn stow(&mut self, resource: Resource, amount: u64) {
        self.pack[resource.index()] += amount;
    }

    /// Trains the skill and attributes that gathering `resource` uses.
    pub(crate) fn train(&mut self, resource: Resource, units: u32, outcomes: &mut Vec<Outcome>) {
        let units = u64::from(units);
        let skill = resource.skill();
        let progress = &mut self.skills[skill.index()];
        if progress.practice(units * SKILL_XP_PER_UNIT, &SKILL_RULES).levels_gained > 0 {
            outcomes.push(Outcome::SkillRose { skill, level: progress.level() });
        }

        let xp = stepped_bonus(units * ATTRIBUTE_XP_PER_UNIT, resource.tier(), TIERS_PER_WHOLE);
        for &attribute in resource.attributes() {
            let progress = &mut self.attributes[attribute.index()];
            if progress.practice(xp, &ATTRIBUTE_RULES).levels_gained > 0 {
                outcomes.push(Outcome::AttributeRose { attribute, level: progress.level() });
            }
        }
    }
}
