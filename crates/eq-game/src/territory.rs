//! Territories made from a seed.
//!
//! Each founder starts in a territory of their own. This module makes
//! one: a haven and nine places to work, laid out on a grid, with the
//! richest ground deepest in and the sea across the way to it.
//!
//! The same seed always makes the same territory. Every territory
//! holds the same amounts of the same things as the hand-built Rocky
//! Shoreline, so what the bots showed about that zone's carrying
//! capacity holds for these: only the shape differs.

use alloc::vec::Vec;

use eq_rng::Rng;
use eq_world_graph::{Graph, Passage, PlaceId};

use crate::content::{place, LOW_TIDE, TIDE};
use crate::creature::CreatureKind;
use crate::types::{Place, Resource, Safety};

/// A territory: its places and ways, and where newcomers arrive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Territory {
    /// The places and the ways between them.
    pub map: Graph<Place>,
    /// The haven at its heart.
    pub anchor: PlaceId,
}

/// How many places a territory has besides its haven.
pub const DEPOSITS: usize = 9;

/// A name and what a visitor sees.
type Scene = (&'static str, &'static str);

const HAVENS: [Scene; 6] = [
    ("Hearthstone Hollow", "A sheltered dip in the ground around a ring of blackened stones. Nothing here will harm you."),
    ("Saltwind Camp", "A windbreak of piled stone above the high-water mark, with a fire pit in its lee. Nothing here will harm you."),
    ("Gull's Rest", "A flat shelf of turf where the birds do not bother to move for you. Nothing here will harm you."),
    ("Cairn Steading", "Three old cairns and the ground worn bare between them. Nothing here will harm you."),
    ("Low Fires", "A hollow under an overhang, its roof black with old smoke. Nothing here will harm you."),
    ("Stillwater Camp", "A patch of level ground beside a pool the tide never reaches. Nothing here will harm you."),
];

const ORE: [Scene; 5] = [
    ("Iron Scar", "A long gash in the hillside where heavy ore shows dark against the pale rock."),
    ("Broken Stair", "Shelves of split stone climbing like steps, each one seamed with ore."),
    ("Rust Hollow", "A bowl of red-stained rock. Ore lies loose where the frost has cracked it out."),
    ("Black Shoulder", "A hump of dark stone standing clear of the turf, heavy with ore."),
    ("Crow Quarry", "An old face of rock, worked by someone long gone. The seams run on into the hill."),
];

const DRIFTWOOD: [Scene; 4] = [
    ("Bone Strand", "A pale beach heaped with sea-bleached timber, tangled like a wreck."),
    ("Storm Rake", "A bank of weed and broken spars thrown high by the winter seas."),
    ("Timber Cove", "A notch in the shore where every tide leaves its wood behind."),
    ("Grey Tangle", "Driftwood lies here in drifts, silver and dry above the wet sand."),
];

const KELP: [Scene; 4] = [
    ("Green Flats", "Shallow water over level rock, thick with slow-moving kelp."),
    ("Weed Garden", "Kelp stands here like a wood under the water, stirring with the swell."),
    ("Drowned Meadow", "A wide shelf the sea barely covers, bright with weed."),
    ("Selkie Shallows", "Clear water to the knee, and kelp wherever the rock gives it a hold."),
];

const TIDEGLASS: [Scene; 4] = [
    ("Glass Throat", "A narrow sea cave whose walls catch the light. Tide-glass grows in every crack."),
    ("Singing Vault", "A chamber the swell breathes through. Tide-glass crusts the roof."),
    ("Mirror Deep", "Black water under a low roof, and tide-glass shining just below its surface."),
    ("Lantern Cave", "A hollow in the headland, lit green from under the water. The walls are bright with tide-glass."),
];

/// What a territory holds, and how many places of each. The same as
/// the Rocky Shoreline.
const HOLDINGS: [(Resource, usize); 4] =
    [(Resource::Ore, 3), (Resource::Driftwood, 2), (Resource::Kelp, 2), (Resource::Tideglass, 2)];

/// The four ways out of a grid cell: the exit's name, the way back,
/// and the step it takes. `y` grows south.
const WAYS: [(&str, &str, (i32, i32)); 4] =
    [("north", "south", (0, -1)), ("east", "west", (1, 0)), ("south", "north", (0, 1)), ("west", "east", (-1, 0))];

/// A place as the layout knows it, before it is given anything to hold.
#[derive(Clone, Copy, Debug)]
struct Cell {
    at: (i32, i32),
    /// The cell it was reached from, and which of [`WAYS`] led here.
    from: Option<(usize, usize)>,
    /// Steps from the haven.
    depth: u32,
}

/// Makes the territory for `seed`.
pub fn generate(seed: u64) -> Territory {
    let mut rng = Rng::from_seed(seed).split(0x7E44_1704);
    let cells = lay_out(&mut rng);
    let holdings = share_out(&cells);

    // The deepest place of all is a lair, and the way into the two
    // richest kinds of ground is across the tide.
    let deepest = (1..cells.len()).max_by_key(|&index| (cells[index].depth, index)).expect("a territory has places");
    let shift = (
        cells.iter().map(|cell| cell.at.0).min().unwrap_or(0),
        cells.iter().map(|cell| cell.at.1).min().unwrap_or(0),
    );
    // Where in each list of scenes this territory starts, and how many
    // it has used: the haven's list first, then one per resource.
    let lists: [&[Scene]; 5] = [&HAVENS, &ORE, &DRIFTWOOD, &KELP, &TIDEGLASS];
    let starts = lists.map(|scenes| rng.below(scenes.len() as u64) as usize);
    let mut used = [0usize; 5];
    let mut map = Graph::new();
    let mut ids = Vec::with_capacity(cells.len());
    for (index, cell) in cells.iter().enumerate() {
        let resource = holdings[index];
        let list = match resource {
            None => 0,
            Some(Resource::Ore) => 1,
            Some(Resource::Driftwood) => 2,
            Some(Resource::Kelp) => 3,
            Some(Resource::Tideglass) => 4,
        };
        // Each list is longer than a territory needs, so names never repeat.
        let (name, description) = lists[list][(starts[list] + used[list]) % lists[list].len()];
        used[list] += 1;
        let safety = match resource {
            None => Safety::Haven,
            Some(Resource::Tideglass) => Safety::Contested,
            Some(_) if index == deepest => Safety::Contested,
            Some(_) => Safety::Buffer,
        };
        let lair = (index == deepest).then_some(CreatureKind::Lurker);
        let position = (cell.at.0 - shift.0, cell.at.1 - shift.1);
        ids.push(map.add_place(place(name, description, safety, resource, position, lair)));
    }

    let tidal = Passage::Cyclic { cycle: TIDE, open_for: LOW_TIDE };
    for (index, cell) in cells.iter().enumerate() {
        let Some((parent, way)) = cell.from else { continue };
        let (out, back, _) = WAYS[way];
        // The sea lies across the way into contested ground from safer ground.
        let crossing = holdings[index] == Some(Resource::Tideglass) && holdings[parent] != Some(Resource::Tideglass);
        let passage = if crossing { tidal } else { Passage::Open };
        map.connect_both(ids[parent], out, ids[index], back, passage).expect("both places were just added");
    }
    Territory { map, anchor: ids[0] }
}

/// Grows a tree of cells out from the haven, one neighbour at a time.
fn lay_out(rng: &mut Rng) -> Vec<Cell> {
    let mut cells = alloc::vec![Cell { at: (0, 0), from: None, depth: 0 }];
    while cells.len() <= DEPOSITS {
        // Every free spot beside a cell that is already there.
        let mut open = Vec::new();
        for (index, cell) in cells.iter().enumerate() {
            for (way, (_, _, step)) in WAYS.iter().enumerate() {
                let at = (cell.at.0 + step.0, cell.at.1 + step.1);
                if cells.iter().all(|other| other.at != at) {
                    open.push((index, way, at));
                }
            }
        }
        let (parent, way, at) = open[rng.below(open.len() as u64) as usize];
        cells.push(Cell { at, from: Some((parent, way)), depth: cells[parent].depth + 1 });
    }
    cells
}

/// Decides what each cell holds: nothing at the haven, then the common
/// things near it and the rare things furthest in.
fn share_out(cells: &[Cell]) -> Vec<Option<Resource>> {
    let mut by_depth: Vec<usize> = (1..cells.len()).collect();
    by_depth.sort_by_key(|&index| (cells[index].depth, index));
    let mut holdings = alloc::vec![None; cells.len()];
    let in_order = HOLDINGS.iter().flat_map(|&(resource, places)| core::iter::repeat_n(resource, places));
    for (index, resource) in by_depth.into_iter().zip(in_order) {
        holdings[index] = Some(resource);
    }
    holdings
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::collections::BTreeSet;

    fn places(territory: &Territory) -> Vec<&Place> {
        territory.map.places().map(|(_, place)| place).collect()
    }

    #[test]
    fn the_same_seed_makes_the_same_territory_and_another_seed_another() {
        assert_eq!(generate(7), generate(7));
        let distinct: BTreeSet<Vec<(i32, i32)>> =
            (0..20).map(|seed| places(&generate(seed)).iter().map(|place| place.position).collect()).collect();
        assert!(distinct.len() > 15, "only {} different layouts in 20 seeds", distinct.len());
    }

    #[test]
    fn every_territory_holds_what_the_shoreline_holds() {
        for seed in 0..50 {
            let territory = generate(seed);
            let all = places(&territory);
            assert_eq!(all.len(), DEPOSITS + 1, "seed {seed}");
            for (resource, wanted) in HOLDINGS {
                let found = all.iter().filter(|place| place.deposit.is_some_and(|deposit| deposit.resource == resource)).count();
                assert_eq!(found, wanted, "seed {seed}: {resource:?}");
            }
            let havens: Vec<_> = all.iter().filter(|place| place.safety == Safety::Haven).collect();
            assert_eq!(havens.len(), 1, "seed {seed}");
            assert!(havens[0].deposit.is_none());
            assert_eq!(territory.map.place(territory.anchor).map(|place| place.safety), Some(Safety::Haven));
            assert_eq!(all.iter().filter(|place| place.lair.is_some()).count(), 1, "seed {seed}: one lair");
        }
    }

    #[test]
    fn every_place_can_be_reached_and_no_two_share_a_name_or_a_spot() {
        for seed in 0..50 {
            let territory = generate(seed);
            let all = places(&territory);
            let names: BTreeSet<&str> = all.iter().map(|place| place.name).collect();
            let spots: BTreeSet<(i32, i32)> = all.iter().map(|place| place.position).collect();
            assert_eq!((names.len(), spots.len()), (all.len(), all.len()), "seed {seed}");
            assert!(spots.iter().all(|&(x, y)| x >= 0 && y >= 0), "seed {seed}: the map starts at its corner");
            for (id, _) in territory.map.places() {
                let reachable = id == territory.anchor || territory.map.first_step(territory.anchor, id, |_| true).is_some();
                assert!(reachable, "seed {seed}: {id:?} cannot be reached");
            }
        }
    }

    #[test]
    fn every_way_is_one_step_on_the_map_and_leads_back() {
        for seed in 0..50 {
            let territory = generate(seed);
            for (from, exit) in territory.map.all_exits() {
                let (a, b) = (territory.map.place(from).unwrap().position, territory.map.place(exit.to).unwrap().position);
                assert_eq!((a.0 - b.0).abs() + (a.1 - b.1).abs(), 1, "seed {seed}");
                let (_, back, _) = WAYS.iter().find(|(out, _, _)| *out == exit.label).copied().unwrap();
                assert_eq!(territory.map.exit(exit.to, back).map(|way| way.to), Some(from), "seed {seed}");
            }
        }
    }

    #[test]
    fn the_rare_things_are_furthest_in_and_the_sea_lies_across_the_way() {
        for seed in 0..50 {
            let territory = generate(seed);
            let depth = |id: PlaceId| {
                let mut steps = 0;
                let mut at = id;
                while at != territory.anchor {
                    at = territory.map.first_step(at, territory.anchor, |_| true).unwrap().to;
                    steps += 1;
                }
                steps
            };
            let of = |resource: Resource| -> Vec<u32> {
                territory
                    .map
                    .places()
                    .filter(|(_, place)| place.deposit.is_some_and(|deposit| deposit.resource == resource))
                    .map(|(id, _)| depth(id))
                    .collect()
            };
            let nearest_glass = of(Resource::Tideglass).into_iter().min().unwrap();
            let furthest_ore = of(Resource::Ore).into_iter().max().unwrap();
            assert!(nearest_glass >= furthest_ore, "seed {seed}: glass at {nearest_glass}, ore at {furthest_ore}");

            // Nobody reaches tide-glass from the haven without crossing the tide.
            let dry = |exit: &eq_world_graph::Exit| matches!(exit.passage, Passage::Open);
            for (id, place) in territory.map.places() {
                if place.deposit.is_some_and(|deposit| deposit.resource == Resource::Tideglass) {
                    assert!(territory.map.first_step(territory.anchor, id, dry).is_none(), "seed {seed}");
                    assert_eq!(place.safety, Safety::Contested);
                }
            }
        }
    }
}
