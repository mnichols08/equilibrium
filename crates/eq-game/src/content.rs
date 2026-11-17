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

/// The least stock a deposit needs before it is worth working. Stops
/// anyone picking a deposit clean one unit at a time.
pub const MIN_WORKABLE_STOCK: u32 = BASE_EFFORT;

/// Skill experience per unit taken.
pub const SKILL_XP_PER_UNIT: u64 = 1;

/// Attribute experience per unit taken, before the tier bonus.
pub const ATTRIBUTE_XP_PER_UNIT: u64 = 1;

/// Tiers that make up one whole in the tier bonus: `1 + tier / 10`.
pub const TIERS_PER_WHOLE: u64 = 10;

/// Percentage points of yield per level of the governing attribute.
pub const YIELD_PERCENT_PER_LEVEL: u64 = 1;

/// How skills grow: about a hundred units gathered for the first level.
pub const SKILL_RULES: Rules = Rules { cap: 100, xp_per_level: 100 };

/// How attributes grow: about two hundred and fifty units for the first level.
pub const ATTRIBUTE_RULES: Rules = Rules { cap: 100, xp_per_level: 250 };

/// The tide: twenty minutes from one low water to the next.
pub const TIDE: Cycle = Cycle::new(Duration::minutes(20));

/// How long the tide stays low at the start of each cycle.
pub const LOW_TIDE: Duration = Duration::minutes(10);

/// Zone vitality bands, from the design document.
pub const VITALITY_BANDS: Bands<Health, 2> =
    Bands::new(Health::Collapsed, [(31, Health::Balanced), (81, Health::Peak)]);

impl Health {
    /// Regrowth points every deposit gains per tick.
    ///
    /// A collapsed zone barely regrows: left alone it takes about an hour
    /// to climb back to balanced. Restoration rites, when they exist, are
    /// meant to be the fast way out.
    pub const fn growth(self) -> u32 {
        match self {
            Self::Peak => 30,
            Self::Balanced => 20,
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
            Self::Ore => NodeConfig { capacity: 360, regrow_cost: 67, strain_per_unit: 3, recovery_every: 2 },
            Self::Driftwood => {
                NodeConfig { capacity: 240, regrow_cost: 100, strain_per_unit: 4, recovery_every: 2 }
            }
            Self::Kelp => NodeConfig { capacity: 450, regrow_cost: 33, strain_per_unit: 2, recovery_every: 2 },
            Self::Tideglass => {
                NodeConfig { capacity: 120, regrow_cost: 200, strain_per_unit: 7, recovery_every: 3 }
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
///
/// ```text
///                 High Shelf
///                     |
///                 Ore Ledge
///                     |
///              Tidewatch Camp -- Shingle Beach -- Kelp Shallows ~~ Outer Reef
///                     |               ~
///                 Wrack Line      Tidal Cave
///                     |               |
///                 Stony Spit      Deep Grotto
///
///   ~ passable only at low tide
/// ```
pub fn rocky_shoreline() -> (Graph<Place>, PlaceId) {
    let mut map = Graph::new();
    let mut add = |name, description, safety, resource| map.add_place(place(name, description, safety, resource));

    let camp = add(
        "Tidewatch Camp",
        "A ring of sea-worn stones around a fire pit, above the reach of the tide. Nothing here will harm you.",
        Safety::Haven,
        None,
    );
    let ledge = add(
        "Ore Ledge",
        "A shelf of dark rock runs along the foot of the cliff, veined with heavy ore. A goat track climbs on to the north.",
        Safety::Buffer,
        Some(Resource::Ore),
    );
    let shelf = add(
        "High Shelf",
        "A broad step in the cliff above the ledge, where the ore runs in thick dark seams.",
        Safety::Buffer,
        Some(Resource::Ore),
    );
    let beach = add(
        "Shingle Beach",
        "Grey pebbles slope down to the water. Driftwood lies tangled along the wrack line, and a cave mouth gapes in the headland.",
        Safety::Buffer,
        Some(Resource::Driftwood),
    );
    let shallows = add(
        "Kelp Shallows",
        "Knee-deep water over flat rock, thick with swaying kelp. Further east the reef shows black at low water.",
        Safety::Buffer,
        Some(Resource::Kelp),
    );
    let reef = add(
        "Outer Reef",
        "Black rock at the edge of the deep water, heavy with kelp. The sea covers the way back at high tide.",
        Safety::Contested,
        Some(Resource::Kelp),
    );
    let cave = add(
        "Tidal Cave",
        "A dripping hollow that the sea takes back with every tide. Tide-glass glints in the walls, and a crack leads down.",
        Safety::Contested,
        Some(Resource::Tideglass),
    );
    let grotto = add(
        "Deep Grotto",
        "A chamber below the tide line, its walls thick with tide-glass. The only way out is up.",
        Safety::Contested,
        Some(Resource::Tideglass),
    );
    let wrack = add(
        "Wrack Line",
        "A long drift of weed and sea-bleached timber thrown up by the winter storms.",
        Safety::Buffer,
        Some(Resource::Driftwood),
    );
    let spit = add(
        "Stony Spit",
        "A finger of broken rock reaching into the sea. Ore shows where the waves have split the stone.",
        Safety::Buffer,
        Some(Resource::Ore),
    );

    let tidal = Passage::Cyclic { cycle: TIDE, open_for: LOW_TIDE };
    let links = [
        (camp, "north", ledge, "south", Passage::Open),
        (ledge, "north", shelf, "south", Passage::Open),
        (camp, "east", beach, "west", Passage::Open),
        (beach, "east", shallows, "west", Passage::Open),
        (shallows, "east", reef, "west", tidal),
        (beach, "cave", cave, "out", tidal),
        (cave, "down", grotto, "up", Passage::Open),
        (camp, "south", wrack, "north", Passage::Open),
        (wrack, "south", spit, "north", Passage::Open),
    ];
    for (a, label_a, b, label_b, passage) in links {
        map.connect_both(a, label_a, b, label_b, passage).expect("both places were just added");
    }
    (map, camp)
}
