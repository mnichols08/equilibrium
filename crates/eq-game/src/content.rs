//! The Rocky Shoreline and every tuning number.
//!
//! All numbers are starting guesses. They live here, not in the rules, so
//! tuning is a matter of editing one file.

use eq_clock::{Cycle, Duration};
use eq_meter::Bands;
use eq_nodes::{Node, NodeConfig};
use eq_skills::Rules;
use eq_world_graph::{Graph, Passage, PlaceId};

use crate::types::{Attribute, Deposit, Health, Place, Resource, Safety, SkillKind};

/// How long it takes to walk between places.
pub const TRAVEL_TIME: Duration = Duration::seconds(30);

/// How long one gathering action takes.
pub const GATHER_TIME: Duration = Duration::seconds(5);

/// Units one gathering action tries to take, before the random extra.
pub const BASE_EFFORT: u32 = 10;

/// A gathering action takes `BASE_EFFORT` plus `0..EFFORT_SPREAD` units.
pub const EFFORT_SPREAD: u64 = 3;

/// Skill experience per unit taken.
pub const SKILL_XP_PER_UNIT: u64 = 3;

/// Attribute experience per unit taken, before the tier bonus.
pub const ATTRIBUTE_XP_PER_UNIT: u64 = 2;

/// Tiers that make up one whole in the tier bonus: `1 + tier / 10`.
pub const TIERS_PER_WHOLE: u64 = 10;

/// Percentage points of yield per level of the governing attribute.
pub const YIELD_PERCENT_PER_LEVEL: u64 = 1;

/// How skills grow.
pub const SKILL_RULES: Rules = Rules { cap: 100, xp_per_level: 100 };

/// How attributes grow.
pub const ATTRIBUTE_RULES: Rules = Rules { cap: 100, xp_per_level: 100 };

/// The tide: twenty minutes from one low water to the next.
pub const TIDE: Cycle = Cycle::new(Duration::minutes(20));

/// How long the tide stays low at the start of each cycle.
pub const LOW_TIDE: Duration = Duration::minutes(10);

/// Zone vitality bands, from the design document.
pub const VITALITY_BANDS: Bands<Health, 2> =
    Bands::new(Health::Collapsed, [(31, Health::Balanced), (81, Health::Peak)]);

impl Health {
    /// Regrowth points every deposit gains per tick.
    pub const fn growth(self) -> u32 {
        match self {
            Self::Peak => 3,
            Self::Balanced => 2,
            Self::Collapsed => 1,
        }
    }

    /// The zone's effect on every yield, as a percentage.
    pub const fn yield_percent(self) -> u32 {
        match self {
            Self::Peak => 125,
            Self::Balanced => 100,
            Self::Collapsed => 50,
        }
    }

    /// The name shown to players.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Peak => "peak health",
            Self::Balanced => "balanced",
            Self::Collapsed => "collapsed",
        }
    }
}

impl Resource {
    /// The skill that gathering this trains.
    pub const fn skill(self) -> SkillKind {
        match self {
            Self::Ore | Self::Tideglass => SkillKind::Mining,
            Self::Driftwood | Self::Kelp => SkillKind::Foraging,
        }
    }

    /// Difficulty tier. Higher tiers give more attribute experience.
    pub const fn tier(self) -> u64 {
        match self {
            Self::Driftwood | Self::Kelp => 0,
            Self::Ore => 1,
            Self::Tideglass => 3,
        }
    }

    /// The attributes that gathering this trains. The first one also
    /// raises the yield.
    pub const fn attributes(self) -> &'static [Attribute] {
        match self {
            Self::Ore => &[Attribute::Vigor, Attribute::Resilience],
            Self::Driftwood | Self::Kelp => &[Attribute::Finesse],
            Self::Tideglass => &[Attribute::Vigor, Attribute::Attunement],
        }
    }

    const fn node(self) -> NodeConfig {
        match self {
            Self::Ore => NodeConfig { capacity: 120, regrow_cost: 20, strain_per_unit: 8, recovery_every: 2 },
            Self::Driftwood => {
                NodeConfig { capacity: 80, regrow_cost: 30, strain_per_unit: 10, recovery_every: 2 }
            }
            Self::Kelp => NodeConfig { capacity: 150, regrow_cost: 10, strain_per_unit: 5, recovery_every: 2 },
            Self::Tideglass => {
                NodeConfig { capacity: 40, regrow_cost: 60, strain_per_unit: 22, recovery_every: 3 }
            }
        }
    }
}

const fn place(name: &'static str, description: &'static str, safety: Safety, resource: Option<Resource>) -> Place {
    let deposit = match resource {
        Some(resource) => Some(Deposit { resource, node: Node::new(resource.node()) }),
        None => None,
    };
    Place { name, description, safety, deposit }
}

/// Builds the Rocky Shoreline and returns it with the starting place.
pub fn rocky_shoreline() -> (Graph<Place>, PlaceId) {
    let mut map = Graph::new();
    let camp = map.add_place(place(
        "Tidewatch Camp",
        "A ring of sea-worn stones around a fire pit, above the reach of the tide. Nothing here will harm you.",
        Safety::Haven,
        None,
    ));
    let ledge = map.add_place(place(
        "Ore Ledge",
        "A shelf of dark rock runs along the foot of the cliff, veined with heavy ore.",
        Safety::Buffer,
        Some(Resource::Ore),
    ));
    let beach = map.add_place(place(
        "Shingle Beach",
        "Grey pebbles slope down to the water. Driftwood lies tangled along the wrack line, and a cave mouth gapes in the headland.",
        Safety::Buffer,
        Some(Resource::Driftwood),
    ));
    let shallows = map.add_place(place(
        "Kelp Shallows",
        "Knee-deep water over flat rock, thick with swaying kelp.",
        Safety::Buffer,
        Some(Resource::Kelp),
    ));
    let cave = map.add_place(place(
        "Tidal Cave",
        "A dripping hollow that the sea takes back with every tide. Tide-glass glints in the walls.",
        Safety::Contested,
        Some(Resource::Tideglass),
    ));

    let tidal = Passage::Cyclic { cycle: TIDE, open_for: LOW_TIDE };
    let links = [
        (camp, "north", ledge, "south", Passage::Open),
        (camp, "east", beach, "west", Passage::Open),
        (beach, "east", shallows, "west", Passage::Open),
        (beach, "cave", cave, "out", tidal),
    ];
    for (a, label_a, b, label_b, passage) in links {
        map.connect_both(a, label_a, b, label_b, passage).expect("both places were just added");
    }
    (map, camp)
}
