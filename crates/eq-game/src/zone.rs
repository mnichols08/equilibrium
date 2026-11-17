//! A zone: one territory of the world, with its own ecology, its own
//! anchor and its own name.
//!
//! The world has one map. A zone is a run of that map's places: the
//! ones that were added together when the territory was made. Zones do
//! not share deposits, vitality, a board or a shrine.

use alloc::string::String;
use alloc::vec::Vec;

use eq_clock::Tick;
use eq_meter::Meter;
use eq_world_graph::{Graph, PlaceId};

use crate::board::Board;
use crate::character::CharacterId;
use crate::content::{CORRUPTION_FADES_EVERY, VITALITY_BANDS};
use crate::event::Event;
use crate::haul::pressure_percent;
use crate::types::{Health, Place};

/// A name cut in a shrine's stones: someone who faded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Epitaph {
    /// Who they were.
    pub name: String,
    /// Which of their lives ended this way.
    pub life: u32,
    /// When they faded.
    pub at: Tick,
}

/// What the world at large does to every land's deposits this tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Weather {
    /// How much of their usual regrowth deposits get, as a percentage.
    pub growth_percent: u32,
    /// The share of their stock a storm tears away this tick, as a
    /// percentage. Zero on a tick with no storm.
    pub storm_percent: u32,
}

/// One territory of the world, with its own ecology.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Zone {
    /// The first of its places on the world's map.
    first: u64,
    /// How many places it has.
    places: u64,
    anchor: PlaceId,
    vitality: Meter,
    /// What deaths have cost the zone, fading with time.
    corruption: Meter,
    pub(crate) board: Board,
    pub(crate) shrine: Vec<Epitaph>,
    /// Who was first here, if it was made for a founder.
    pub(crate) founder: Option<CharacterId>,
    /// What its founder called the land.
    pub(crate) name: Option<String>,
    /// What its founder called their house, which those they invite share.
    pub(crate) house: Option<String>,
    /// Where its house stands, once its founder has raised it. The
    /// land's own people cannot be struck or robbed there.
    pub(crate) hall: Option<PlaceId>,
    /// Names its founder has invited, lower-cased. Whoever joins the
    /// world under one of them arrives here.
    pub(crate) invited: Vec<String>,
}

impl Zone {
    /// A zone at full vitality, made of the `places` places of the
    /// world's map from `first` on, with its haven at `anchor`.
    pub(crate) const fn new(first: PlaceId, places: usize, anchor: PlaceId, founder: Option<CharacterId>) -> Self {
        Self {
            first: first.get(),
            places: places as u64,
            anchor,
            vitality: Meter::full(0, 100),
            corruption: Meter::empty(0, 100),
            board: Board::new(),
            shrine: Vec::new(),
            founder,
            name: None,
            house: None,
            hall: None,
            invited: Vec::new(),
        }
    }

    /// Whether `place` is one of this zone's.
    pub const fn contains(&self, place: PlaceId) -> bool {
        place.get() >= self.first && place.get() < self.first + self.places
    }

    /// The ids of the zone's places, in order.
    pub fn place_ids(&self) -> impl Iterator<Item = PlaceId> {
        (self.first..self.first + self.places).map(PlaceId::new)
    }

    /// The zone's haven: where severed spirits are pulled to, where its
    /// board and shrine stand, and where its people begin their lives.
    pub const fn anchor(&self) -> PlaceId {
        self.anchor
    }

    /// Vitality, from 0 to 100.
    pub const fn vitality(&self) -> i64 {
        self.vitality.value()
    }

    /// How much of the zone's vitality deaths are costing it, 0 to 100.
    pub const fn corruption(&self) -> i64 {
        self.corruption.value()
    }

    /// The vitality band.
    pub fn health(&self) -> Health {
        VITALITY_BANDS.band_of(self.vitality.value())
    }

    /// The zone's Contract Board, which stands at its anchor.
    pub const fn board(&self) -> &Board {
        &self.board
    }

    /// The names of those of this zone who faded, oldest first.
    pub fn shrine(&self) -> &[Epitaph] {
        &self.shrine
    }

    /// Who was first here, if the zone was made for a founder.
    pub const fn founder(&self) -> Option<CharacterId> {
        self.founder
    }

    /// What the founder called the land, once they have.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Where the land's house stands, once it has been raised.
    pub const fn hall(&self) -> Option<PlaceId> {
        self.hall
    }

    /// What the founder called their house, once they have.
    pub fn house(&self) -> Option<&str> {
        self.house.as_deref()
    }

    /// Records a death in the zone, which costs it `corruption` vitality.
    pub(crate) fn mourn(&mut self, corruption: i64) {
        self.corruption.adjust(corruption);
    }

    /// Lifts `corruption` from the zone, as a rite does.
    pub(crate) fn cleanse(&mut self, corruption: i64) {
        self.corruption.adjust(-corruption);
    }

    /// Runs one tick of the zone's ecology on its part of `map`.
    ///
    /// `growth_percent` is how much of their usual regrowth the world
    /// allows its deposits; `storm_percent` is the share of their stock
    /// a storm tears away this tick, if one has come.
    pub(crate) fn tick(&mut self, map: &mut Graph<Place>, now: Tick, weather: Weather, events: &mut Vec<Event>) {
        self.grow_deposits(map, weather, events);
        if now.get().is_multiple_of(CORRUPTION_FADES_EVERY.as_ticks()) {
            self.corruption.adjust(-1);
        }
        self.update_vitality(map, events);
    }

    fn grow_deposits(&self, map: &mut Graph<Place>, weather: Weather, events: &mut Vec<Event>) {
        let growth = self.health().growth() * weather.growth_percent / 100;
        for place in self.place_ids() {
            let Some(deposit) = map.place_mut(place).and_then(|data| data.deposit.as_mut()) else { continue };
            let from = deposit.node.quality();
            deposit.node.tick(growth);
            deposit.node.scour(weather.storm_percent);
            let to = deposit.node.quality();
            if from != to {
                events.push(Event::DepositChanged { place, resource: deposit.resource, from, to });
            }
        }
    }

    /// Vitality is 100 minus the average pressure on the zone's deposits,
    /// minus the corruption left by deaths. A deposit that is pristine
    /// and at least half full is under no pressure, so careful use costs
    /// the zone nothing.
    fn update_vitality(&mut self, map: &Graph<Place>, events: &mut Vec<Event>) {
        let (pressure, deposits) = self
            .place_ids()
            .filter_map(|place| map.place(place).and_then(|data| data.deposit.as_ref()))
            .fold((0i64, 0i64), |(pressure, count), deposit| {
                (pressure + i64::from(pressure_percent(&deposit.node)), count + 1)
            });
        let penalty = if deposits == 0 { 0 } else { pressure / deposits };
        let change = self.vitality.set(100 - penalty - self.corruption.value());
        if let Some((from, to)) = VITALITY_BANDS.crossing(change) {
            events.push(Event::VitalityChanged { anchor: self.anchor, from, to });
        }
    }
}
