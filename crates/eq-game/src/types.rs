//! The nouns of the game.

use eq_nodes::Node;

/// Something that can be gathered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Resource {
    /// Heavy ore from the cliff foot.
    Ore,
    /// Sea-dried wood from the wrack line.
    Driftwood,
    /// Kelp from the shallows.
    Kelp,
    /// Tide-glass from the cave walls.
    Tideglass,
}

impl Resource {
    /// Every resource, in pack order.
    pub const ALL: [Self; 4] = [Self::Ore, Self::Driftwood, Self::Kelp, Self::Tideglass];

    /// Position in [`Resource::ALL`].
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The name shown to players.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ore => "heavy ore",
            Self::Driftwood => "driftwood",
            Self::Kelp => "kelp",
            Self::Tideglass => "tide-glass",
        }
    }
}

/// A skill trained by gathering.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SkillKind {
    /// Breaking rock.
    Mining,
    /// Collecting what grows and washes up.
    Foraging,
}

impl SkillKind {
    /// Every skill, in display order.
    pub const ALL: [Self; 2] = [Self::Mining, Self::Foraging];

    /// Position in [`SkillKind::ALL`].
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The name shown to players.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Mining => "mining",
            Self::Foraging => "foraging",
        }
    }
}

/// One of the six core attributes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Attribute {
    /// Strength and stamina.
    Vigor,
    /// Precision and speed.
    Finesse,
    /// Toughness.
    Resilience,
    /// Sensitivity to essence.
    Attunement,
    /// Mental fortitude.
    Willpower,
    /// The pull of the bloodline.
    Lineage,
}

impl Attribute {
    /// Every attribute, in display order.
    pub const ALL: [Self; 6] =
        [Self::Vigor, Self::Finesse, Self::Resilience, Self::Attunement, Self::Willpower, Self::Lineage];

    /// Position in [`Attribute::ALL`].
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The name shown to players.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Vigor => "Vigor",
            Self::Finesse => "Finesse",
            Self::Resilience => "Resilience",
            Self::Attunement => "Attunement",
            Self::Willpower => "Willpower",
            Self::Lineage => "Lineage",
        }
    }
}

/// How safe a place is. Nothing reads this until sleeping and combat
/// arrive; it is recorded now so places never need reshaping.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Safety {
    /// No harm can come to anyone here.
    Haven,
    /// Wilderness near shelter.
    Buffer,
    /// Fully exposed.
    Contested,
}

impl Safety {
    /// The name shown to players.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Haven => "haven",
            Self::Buffer => "wilderness",
            Self::Contested => "contested",
        }
    }
}

/// The state of the zone as a whole, from its vitality.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Health {
    /// Vitality 0 to 30.
    Collapsed,
    /// Vitality 31 to 80.
    Balanced,
    /// Vitality 81 to 100.
    Peak,
}

/// How the land regards a character, from their taint.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Standing {
    /// Little or no taint. Cannot be attacked outside contested places.
    Clean,
    /// Enough taint that anyone may challenge them in the wilderness.
    Marked,
}

/// A resource node and what it gives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Deposit {
    /// What is gathered here.
    pub resource: Resource,
    /// The stock and its wear.
    pub node: Node,
}

/// One location on the map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place {
    /// Short name.
    pub name: &'static str,
    /// What a visitor sees.
    pub description: &'static str,
    /// How safe it is.
    pub safety: Safety,
    /// What can be gathered, if anything.
    pub deposit: Option<Deposit>,
    /// What lies dropped on the ground here, by resource, for anyone to take.
    pub spoils: [u64; Resource::ALL.len()],
    /// Where it sits on a map seen from above: `x` grows east, `y` grows
    /// south. Only for drawing; the rules never read it.
    pub position: (i32, i32),
}
