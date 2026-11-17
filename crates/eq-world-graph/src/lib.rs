//! Places joined by labelled exits.
//!
//! Space is a graph, not coordinates: you are *at* a place, and you leave
//! by a named exit. An exit can be always open, or open for part of a
//! repeating cycle, like a cave mouth that floods at high tide.
//!
//! The graph is generic over what a place holds, so it knows nothing
//! about any particular game.
//!
//! ```
//! use eq_clock::{Cycle, Duration, Tick};
//! use eq_world_graph::{Graph, Passage};
//!
//! let mut map = Graph::new();
//! let beach = map.add_place("beach");
//! let cave = map.add_place("cave");
//!
//! let tide = Passage::Cyclic {
//!     cycle: Cycle::new(Duration::minutes(20)),
//!     open_for: Duration::minutes(10),
//! };
//! map.connect(beach, "cave", cave, tide).unwrap();
//!
//! let exit = map.exit(beach, "cave").unwrap();
//! assert!(exit.passage.is_open(Tick::new(0)));
//! assert!(!exit.passage.is_open(Tick::new(700)));
//! ```
#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use eq_clock::{Cycle, Duration, Tick};

eq_ids::define_id!(
    /// Identifies a place within one [`Graph`].
    PlaceId
);

/// When an exit can be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Passage {
    /// Always usable.
    Open,
    /// Usable for the first `open_for` of every turn of `cycle`.
    Cyclic {
        /// The repeating period.
        cycle: Cycle,
        /// How long the exit stays open at the start of each turn.
        open_for: Duration,
    },
}

impl Passage {
    /// Whether the exit can be used at `tick`.
    pub const fn is_open(&self, tick: Tick) -> bool {
        match self {
            Self::Open => true,
            Self::Cyclic { cycle, open_for } => cycle.is_within(tick, *open_for),
        }
    }
}

/// A one-way link out of a place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exit {
    /// What the exit is called from this side, such as `"north"`.
    pub label: String,
    /// Where it leads.
    pub to: PlaceId,
    /// When it can be used.
    pub passage: Passage,
}

/// Returned when an id does not belong to the graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnknownPlace(pub PlaceId);

impl fmt::Display for UnknownPlace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} is not in this graph", self.0)
    }
}

impl core::error::Error for UnknownPlace {}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry<P> {
    data: P,
    exits: Vec<Exit>,
}

/// A set of places and the exits between them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Graph<P> {
    entries: Vec<Entry<P>>,
}

impl<P> Default for Graph<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P> Graph<P> {
    /// An empty graph.
    pub const fn new() -> Self {
        Self { entries: Vec::new() }
    }

    /// Adds a place and returns its id.
    pub fn add_place(&mut self, data: P) -> PlaceId {
        let id = PlaceId::new(self.entries.len() as u64);
        self.entries.push(Entry { data, exits: Vec::new() });
        id
    }

    /// Adds a one-way exit from `from` to `to`.
    pub fn connect(
        &mut self,
        from: PlaceId,
        label: &str,
        to: PlaceId,
        passage: Passage,
    ) -> Result<(), UnknownPlace> {
        if self.entry(to).is_none() {
            return Err(UnknownPlace(to));
        }
        let entry = self.entry_mut(from).ok_or(UnknownPlace(from))?;
        entry.exits.push(Exit { label: label.into(), to, passage });
        Ok(())
    }

    /// Adds an exit each way between `a` and `b`, sharing one passage.
    pub fn connect_both(
        &mut self,
        a: PlaceId,
        label_from_a: &str,
        b: PlaceId,
        label_from_b: &str,
        passage: Passage,
    ) -> Result<(), UnknownPlace> {
        self.connect(a, label_from_a, b, passage)?;
        self.connect(b, label_from_b, a, passage)
    }

    /// What the place holds.
    pub fn place(&self, id: PlaceId) -> Option<&P> {
        self.entry(id).map(|entry| &entry.data)
    }

    /// What the place holds, for changing.
    pub fn place_mut(&mut self, id: PlaceId) -> Option<&mut P> {
        self.entry_mut(id).map(|entry| &mut entry.data)
    }

    /// The exits leaving a place. Empty for an unknown id.
    pub fn exits(&self, from: PlaceId) -> &[Exit] {
        self.entry(from).map_or(&[], |entry| &entry.exits)
    }

    /// The exit with this label, if the place has one.
    pub fn exit(&self, from: PlaceId, label: &str) -> Option<&Exit> {
        self.exits(from).iter().find(|exit| exit.label == label)
    }

    /// How many places the graph holds.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the graph has no places.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every place with its id, in the order added.
    pub fn places(&self) -> impl Iterator<Item = (PlaceId, &P)> {
        self.entries.iter().enumerate().map(|(index, entry)| (PlaceId::new(index as u64), &entry.data))
    }

    /// Every place with its id, for changing, in the order added.
    pub fn places_mut(&mut self) -> impl Iterator<Item = (PlaceId, &mut P)> {
        self.entries
            .iter_mut()
            .enumerate()
            .map(|(index, entry)| (PlaceId::new(index as u64), &mut entry.data))
    }

    /// Every exit with the place it leaves, in the order added.
    pub fn all_exits(&self) -> impl Iterator<Item = (PlaceId, &Exit)> {
        self.entries.iter().enumerate().flat_map(|(index, entry)| {
            entry.exits.iter().map(move |exit| (PlaceId::new(index as u64), exit))
        })
    }

    /// The first exit to take on a shortest way from `from` to `to`,
    /// walking only exits that `usable` allows.
    ///
    /// Returns `None` if `to` cannot be reached, or if `from` is already
    /// `to`. Among equally short ways, the one found first in the order
    /// exits were added wins, so the answer is always the same.
    pub fn first_step(&self, from: PlaceId, to: PlaceId, usable: impl Fn(&Exit) -> bool) -> Option<&Exit> {
        // For each place reached, the first exit that was taken out of
        // `from` to get there.
        let mut first: Vec<Option<&Exit>> = alloc::vec![None; self.entries.len()];
        let mut reached = alloc::vec![false; self.entries.len()];
        let mut queue = alloc::collections::VecDeque::new();
        *reached.get_mut(index_of(from)?)? = true;
        queue.push_back(from);

        while let Some(place) = queue.pop_front() {
            for exit in self.exits(place).iter().filter(|exit| usable(exit)) {
                let next = index_of(exit.to)?;
                if reached[next] {
                    continue;
                }
                reached[next] = true;
                first[next] = first[index_of(place)?].or(Some(exit));
                if exit.to == to {
                    return first[next];
                }
                queue.push_back(exit.to);
            }
        }
        None
    }

    fn entry(&self, id: PlaceId) -> Option<&Entry<P>> {
        self.entries.get(index_of(id)?)
    }

    fn entry_mut(&mut self, id: PlaceId) -> Option<&mut Entry<P>> {
        self.entries.get_mut(index_of(id)?)
    }
}

/// Where a place sits in the graph's list, if its id fits a `usize`.
fn index_of(id: PlaceId) -> Option<usize> {
    usize::try_from(id.get()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tide() -> Passage {
        Passage::Cyclic { cycle: Cycle::new(Duration::minutes(20)), open_for: Duration::minutes(10) }
    }

    #[test]
    fn places_keep_their_data_and_order() {
        let mut map = Graph::new();
        let camp = map.add_place("camp");
        let ledge = map.add_place("ledge");
        assert_eq!(map.place(camp), Some(&"camp"));
        assert_eq!(map.places().collect::<Vec<_>>(), [(camp, &"camp"), (ledge, &"ledge")]);
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn exits_are_one_way_unless_connected_both() {
        let mut map = Graph::new();
        let a = map.add_place(());
        let b = map.add_place(());
        let c = map.add_place(());
        map.connect(a, "north", b, Passage::Open).unwrap();
        map.connect_both(a, "east", c, "west", Passage::Open).unwrap();

        assert_eq!(map.exit(a, "north").map(|exit| exit.to), Some(b));
        assert!(map.exits(b).is_empty());
        assert_eq!(map.exit(c, "west").map(|exit| exit.to), Some(a));
        assert!(map.exit(a, "down").is_none());
        assert_eq!(map.all_exits().count(), 3);
    }

    #[test]
    fn cyclic_passage_opens_and_closes() {
        let passage = tide();
        assert!(passage.is_open(Tick::new(599)));
        assert!(!passage.is_open(Tick::new(600)));
        assert!(passage.is_open(Tick::new(1_200)));
        assert!(Passage::Open.is_open(Tick::new(u64::MAX)));
    }

    #[test]
    fn connecting_unknown_places_fails_cleanly() {
        let mut map = Graph::new();
        let a = map.add_place(());
        let ghost = PlaceId::new(99);
        assert_eq!(map.connect(a, "x", ghost, Passage::Open), Err(UnknownPlace(ghost)));
        assert_eq!(map.connect(ghost, "x", a, Passage::Open), Err(UnknownPlace(ghost)));
        assert!(map.exits(a).is_empty());
        assert!(map.place(ghost).is_none());
    }

    /// camp - beach - shallows, with a tidal short cut from camp to shallows.
    fn shore() -> (Graph<&'static str>, [PlaceId; 3]) {
        let mut map = Graph::new();
        let places = [map.add_place("camp"), map.add_place("beach"), map.add_place("shallows")];
        let [camp, beach, shallows] = places;
        map.connect_both(camp, "east", beach, "west", Passage::Open).unwrap();
        map.connect_both(beach, "east", shallows, "west", Passage::Open).unwrap();
        map.connect_both(camp, "wade", shallows, "wade", tide()).unwrap();
        (map, places)
    }

    #[test]
    fn first_step_takes_the_shortest_usable_way() {
        let (map, [camp, beach, shallows]) = shore();
        let label = |from, to, usable: fn(&Exit) -> bool| map.first_step(from, to, usable).map(|exit| exit.label.as_str());

        assert_eq!(label(camp, shallows, |_| true), Some("wade"));
        assert_eq!(label(camp, shallows, |exit| exit.passage == Passage::Open), Some("east"));
        assert_eq!(label(shallows, camp, |exit| exit.passage == Passage::Open), Some("west"));
        assert_eq!(label(camp, beach, |_| true), Some("east"));
    }

    #[test]
    fn first_step_gives_up_when_there_is_no_way_or_nowhere_to_go() {
        let (mut map, [camp, _, shallows]) = shore();
        let island = map.add_place("island");
        assert!(map.first_step(camp, island, |_| true).is_none());
        assert!(map.first_step(camp, camp, |_| true).is_none());
        assert!(map.first_step(camp, shallows, |_| false).is_none());
        assert!(map.first_step(PlaceId::new(99), camp, |_| true).is_none());
        assert!(map.first_step(camp, PlaceId::new(99), |_| true).is_none());
    }

    #[test]
    fn place_data_can_be_changed() {
        let mut map = Graph::new();
        let a = map.add_place(1);
        *map.place_mut(a).unwrap() += 1;
        for (_, value) in map.places_mut() {
            *value *= 10;
        }
        assert_eq!(map.place(a), Some(&20));
    }
}
