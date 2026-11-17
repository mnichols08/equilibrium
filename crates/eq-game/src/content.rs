//! The Rocky Shoreline and every tuning number.
//!
//! All numbers are starting guesses. They live here, not in the rules, so
//! tuning is a matter of editing one file.

use eq_clock::{Cycle, Duration};
use eq_meter::Bands;
use eq_nodes::{Node, NodeConfig};
use eq_skills::Rules;
use eq_world_graph::{Graph, Passage, PlaceId};

use crate::bots::Leg;
use crate::creature::CreatureKind;
use crate::types::{Attribute, Deposit, Health, Place, Resource, Safety, SkillKind, Standing};

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

/// Taint gained per haul from a deposit under full pressure. A haul from
/// a deposit under half pressure gains half this, and so on. Hauls from a
/// deposit under no pressure gain none: careful players stay clean.
pub const TAINT_AT_FULL_PRESSURE: u32 = 10;

/// How long one point of taint takes to fade.
pub const TAINT_FADES_EVERY: Duration = Duration::seconds(30);

/// The share of their yield a fully tainted character loses, as a
/// percentage. The loss grows evenly with taint.
pub const YIELD_LOST_AT_FULL_TAINT: u32 = 75;

/// Taint bands. A marked character is fair game in the wilderness once
/// combat exists; until then the mark is only recorded and shown.
pub const STANDING_BANDS: Bands<Standing, 1> = Bands::new(Standing::Clean, [(50, Standing::Marked)]);

/// How fast a wasted skill or attribute comes back, up to its old peak,
/// as a percentage of the normal pace. Wide on purpose: a returning
/// player should be weak for an evening, not set back a month.
pub const MUSCLE_MEMORY_PERCENT: u32 = 400;

/// How long it takes to settle down before sleep begins. Being struck in
/// that time leaves you standing.
pub const SETTLE_TIME: Duration = Duration::seconds(30);

/// How long a character can be awake before the rested bonus is spent.
pub const RESTED_LASTS: Duration = Duration::hours(15);

/// How long asleep it takes to be fully rested from nothing.
pub const FULL_REST: Duration = Duration::hours(8);

/// The extra skill and attribute experience a fully rested character
/// earns, as a percentage. It shrinks evenly as the rest wears off.
pub const RESTED_BONUS_PERCENT: u64 = 50;

/// How long anyone can sleep before the body starts to waste.
pub const FORGIVING_STRETCH: Duration = Duration::days(3);

/// Past the forgiving stretch, how long asleep costs one level of every
/// body skill and attribute.
pub const ATROPHY_EVERY: Duration = Duration::hours(6);

/// Atrophy stops at this share of each peak, as a percentage. The body
/// wastes to a floor, never to nothing.
pub const ATROPHY_FLOOR_PERCENT: u32 = 50;

/// How long a character whose player has dropped away stays on their
/// feet before they slump down to sleep where they stand. Long enough
/// to ride out a lost connection; short enough that leaving is no way
/// out of a fight.
pub const DISCONNECT_GRACE: Duration = Duration::seconds(120);

/// How long anyone can sleep before the body dies where it lies.
pub const LONG_SLEEP: Duration = Duration::days(14);

/// How long a forgotten spirit has to redeem itself before it fades.
/// Only time spent awake as a spirit counts, so nobody comes back to
/// find the matter already settled.
pub const REDEMPTION_CLOCK: Duration = Duration::hours(10);

/// How many memories a forgotten spirit must recover to be redeemed.
pub const MEMORIES_TO_REDEEM: u32 = 3;

/// How long it takes a forgotten spirit to search one place for a memory.
pub const SEEK_TIME: Duration = Duration::minutes(10);

/// How long it takes to speak a forgotten name at the shrine.
pub const NAME_TIME: Duration = Duration::minutes(1);

/// The share of each skill and attribute a faded character's next life
/// keeps, as a percentage. Less than an ordinary rebirth.
pub const FADED_CARRYOVER_PERCENT: u32 = 5;

/// Taint for going through the pack of a sleeper the land has not
/// marked. Enough to be marked for it at once.
pub const TAINT_FOR_ROBBING: i64 = 50;

/// The most health a character can have.
pub const MAX_HEALTH: i64 = 100;

/// How long one exchange of blows takes the attacker.
pub const ATTACK_TIME: Duration = Duration::seconds(3);

/// Health a blow takes before anything else is counted.
pub const BASE_DAMAGE: u32 = 12;

/// A blow adds `0..DAMAGE_SPREAD` to its base.
pub const DAMAGE_SPREAD: u64 = 7;

/// Percentage points of damage per level of the attacker's Vigor.
pub const DAMAGE_PERCENT_PER_VIGOR: u32 = 1;

/// Levels of the defender's Resilience that turn aside one percent of a blow.
pub const RESILIENCE_PER_PERCENT_RESISTED: u32 = 2;

/// The most of a blow that Resilience can turn aside, as a percentage.
pub const MOST_DAMAGE_RESISTED: u32 = 50;

/// The share of their damage a fully tainted character loses, as a
/// percentage. A clean challenger has the better of a marked one.
pub const DAMAGE_LOST_AT_FULL_TAINT: u32 = 50;

/// How often health mends by itself.
pub const HEALTH_MENDS_EVERY: Duration = Duration::seconds(10);

/// Health mended each time in the wilderness and in contested places.
pub const MENDING: i64 = 1;

/// Health mended each time in a haven.
pub const HAVEN_MENDING: i64 = 5;

/// How long a body lies where it fell before it is lost and its spirit
/// must be reborn.
pub const CORPSE_TIME: Duration = Duration::minutes(10);

/// How long a tethered spirit waits before it can return to its body.
pub const TETHERED_WAIT: Duration = Duration::minutes(1);

/// How long a corrupted spirit is held before it can start back.
pub const CORRUPTED_WAIT: Duration = Duration::minutes(5);

/// Dying wears the spirit. Each recent death holds the next one back
/// this much longer before it can return, so someone killed again and
/// again soon cannot reach their body in time and must be reborn.
pub const DEGRADATION_WAIT: Duration = Duration::minutes(3);

/// How long one death's wear on the spirit takes to fade.
pub const DEGRADATION_FADES_EVERY: Duration = Duration::minutes(30);

/// The most deaths' worth of wear a spirit can carry.
pub const MOST_DEGRADATION: i64 = 5;

/// How long a body its spirit never came back for stands as a Body
/// Guard, counted from when it rose.
pub const GUARD_LASTS: Duration = Duration::minutes(50);

/// A Body Guard has its owner's strength, toughness and health raised
/// to this percentage.
pub const GUARD_THREAT_PERCENT: u32 = 120;

/// A lurker's health.
pub const LURKER_HEALTH: i64 = 60;

/// How long after a lurker is brought down another takes its lair.
pub const LURKER_RETURNS: Duration = Duration::minutes(30);

/// How often a creature strikes while it has someone to strike. It
/// also waits this long after someone walks in before its first blow.
pub const CREATURE_BLOW_EVERY: Duration = Duration::seconds(6);

/// The health a character has on returning to their body.
pub const HEALTH_ON_RETURN: i64 = 25;

/// The share of each skill and attribute that carries into a new life,
/// as a percentage.
pub const REBIRTH_CARRYOVER_PERCENT: u32 = 15;

/// How long one draw of ectoplasm takes a spirit.
pub const DRAW_TIME: Duration = Duration::seconds(10);

/// The most ectoplasm a spirit can hold.
pub const MOST_ECTOPLASM: u64 = 500;

/// Ectoplasm spent at rebirth for each extra percent of the old life kept.
pub const ECTOPLASM_PER_PERCENT: u64 = 20;

/// The most that ectoplasm can add to an inheritance, in percent.
pub const MOST_ECTOPLASM_BONUS_PERCENT: u32 = 15;

/// Levels of Lineage for each extra percent of the old life kept.
pub const LINEAGE_PER_PERCENT: u32 = 2;

/// The most of an old life that any rebirth can keep, in percent.
pub const MOST_CARRYOVER_PERCENT: u32 = 50;

/// Lineage experience for each life lived to its end. Lineage is the
/// one attribute that is never cut back at rebirth.
pub const LINEAGE_XP_PER_LIFE: u64 = 500;

/// How many times its share the attribute of the place of death keeps:
/// die on the ore and the next life remembers the ore.
pub const DEATH_PLACE_CARRYOVER_TIMES: u32 = 2;

/// Attunement experience for each level the body wastes in too long a
/// sleep. The spirit grows as the body fails.
pub const AFFINITY_XP_PER_WASTED_LEVEL: u64 = 100;

/// Ectoplasm ever drawn at which each spirit tier is reached.
pub const SPIRIT_TIER_AT: [u64; 4] = [0, 100, 400, 1_000];

/// Vitality a zone loses for each death in it. Small on purpose: one
/// death is a scratch, and it takes a massacre to sicken a zone.
pub const CORRUPTION_PER_DEATH: i64 = 2;

/// Vitality a zone loses when a sleeper dies in it, by a blow or by
/// sleeping too long. Less than a waking death, so sleepers cannot be
/// farmed to sicken a zone.
pub const CORRUPTION_PER_SLEEPER_DEATH: i64 = 1;

/// How long one point of corruption takes to fade.
pub const CORRUPTION_FADES_EVERY: Duration = Duration::minutes(2);

/// How long it takes to pick up what lies on the ground.
pub const TAKE_TIME: Duration = Duration::seconds(5);

/// How skills grow: about a hundred units gathered for the first level.
pub const SKILL_RULES: Rules = Rules { cap: 100, xp_per_level: 100, relearning_percent: MUSCLE_MEMORY_PERCENT };

/// How attributes grow: about two hundred and fifty units for the first level.
pub const ATTRIBUTE_RULES: Rules =
    Rules { cap: 100, xp_per_level: 250, relearning_percent: MUSCLE_MEMORY_PERCENT };

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
            Self::Peak => 60,
            Self::Balanced => 40,
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

const fn place(
    name: &'static str,
    description: &'static str,
    safety: Safety,
    resource: Option<Resource>,
    position: (i32, i32),
    lair: Option<CreatureKind>,
) -> Place {
    let deposit = match resource {
        Some(resource) => Some(Deposit { resource, node: Node::new(resource.node()) }),
        None => None,
    };
    Place { name, description, safety, deposit, lair, position }
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
///
/// Each place's `position` matches this drawing, one step per row and
/// column, with the camp at `(2, 2)`.
pub fn rocky_shoreline() -> (Graph<Place>, PlaceId) {
    let mut map = Graph::new();
    let mut add = |name, description, safety, resource, position| {
        // The grotto is the one lair on the shoreline.
        let lair = (name == "Deep Grotto").then_some(CreatureKind::Lurker);
        map.add_place(place(name, description, safety, resource, position, lair))
    };

    let camp = add(
        "Tidewatch Camp",
        "A ring of sea-worn stones around a fire pit, above the reach of the tide. Nothing here will harm you.",
        Safety::Haven,
        None,
        (2, 2),
    );
    let ledge = add(
        "Ore Ledge",
        "A shelf of dark rock runs along the foot of the cliff, veined with heavy ore. A goat track climbs on to the north.",
        Safety::Buffer,
        Some(Resource::Ore),
        (2, 1),
    );
    let shelf = add(
        "High Shelf",
        "A broad step in the cliff above the ledge, where the ore runs in thick dark seams.",
        Safety::Buffer,
        Some(Resource::Ore),
        (2, 0),
    );
    let beach = add(
        "Shingle Beach",
        "Grey pebbles slope down to the water. Driftwood lies tangled along the wrack line, and a cave mouth gapes in the headland.",
        Safety::Buffer,
        Some(Resource::Driftwood),
        (3, 2),
    );
    let shallows = add(
        "Kelp Shallows",
        "Knee-deep water over flat rock, thick with swaying kelp. Further east the reef shows black at low water.",
        Safety::Buffer,
        Some(Resource::Kelp),
        (4, 2),
    );
    let reef = add(
        "Outer Reef",
        "Black rock at the edge of the deep water, heavy with kelp. The sea covers the way back at high tide.",
        Safety::Contested,
        Some(Resource::Kelp),
        (5, 2),
    );
    let cave = add(
        "Tidal Cave",
        "A dripping hollow that the sea takes back with every tide. Tide-glass glints in the walls, and a crack leads down.",
        Safety::Contested,
        Some(Resource::Tideglass),
        (3, 3),
    );
    let grotto = add(
        "Deep Grotto",
        "A chamber below the tide line, its walls thick with tide-glass. Something lives in the black water. The only way out is up.",
        Safety::Contested,
        Some(Resource::Tideglass),
        (3, 4),
    );
    let wrack = add(
        "Wrack Line",
        "A long drift of weed and sea-bleached timber thrown up by the winter storms.",
        Safety::Buffer,
        Some(Resource::Driftwood),
        (2, 3),
    );
    let spit = add(
        "Stony Spit",
        "A finger of broken rock reaching into the sea. Ore shows where the waves have split the stone.",
        Safety::Buffer,
        Some(Resource::Ore),
        (2, 4),
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

/// A walk round the whole shoreline from camp and back, visiting every
/// deposit: both ore shelves, the beach, the cave and grotto, the
/// shallows and reef, then the wrack line and the spit.
///
/// The two ways back across the tide are waited for; the ways in are
/// skipped when flooded.
pub const CIRCUIT: &[Leg] = &[
    Leg::go("north"),
    Leg::go("north"),
    Leg::go("south"),
    Leg::go("south"),
    Leg::go("east"),
    Leg::go("cave"),
    Leg::go("down"),
    Leg::go("up"),
    Leg::wait_for("out"),
    Leg::go("east"),
    Leg::go("east"),
    Leg::wait_for("west"),
    Leg::go("west"),
    Leg::go("west"),
    Leg::go("south"),
    Leg::go("south"),
    Leg::go("north"),
    Leg::go("north"),
];

/// How to walk from camp to each of the nine deposits: Ore Ledge, High
/// Shelf, Shingle Beach, Kelp Shallows, Outer Reef, Tidal Cave, Deep
/// Grotto, Wrack Line and Stony Spit.
pub const DEPOSIT_ROUTES: [&[Leg]; 9] = [
    &[Leg::go("north")],
    &[Leg::go("north"), Leg::go("north")],
    &[Leg::go("east")],
    &[Leg::go("east"), Leg::go("east")],
    &[Leg::go("east"), Leg::go("east"), Leg::go("east")],
    &[Leg::go("east"), Leg::go("cave")],
    &[Leg::go("east"), Leg::go("cave"), Leg::go("down")],
    &[Leg::go("south")],
    &[Leg::go("south"), Leg::go("south")],
];
