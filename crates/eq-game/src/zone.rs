//! A zone: its map, its deposits and its health.

use alloc::vec::Vec;

use eq_clock::Tick;
use eq_meter::Meter;
use eq_world_graph::{Graph, Passage};

use crate::content::{CORRUPTION_FADES_EVERY, VITALITY_BANDS};
use crate::event::Event;
use crate::haul::pressure_percent;
use crate::types::{Health, Place};

/// One region of the world, with its own ecology.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Zone {
    map: Graph<Place>,
    vitality: Meter,
    /// What deaths have cost the zone, fading with time.
    corruption: Meter,
}

impl Zone {
    /// A zone at full vitality.
    pub(crate) const fn new(map: Graph<Place>) -> Self {
        Self { map, vitality: Meter::full(0, 100), corruption: Meter::empty(0, 100) }
    }

    /// The zone's places and exits.
    pub const fn map(&self) -> &Graph<Place> {
        &self.map
    }

    pub(crate) const fn map_mut(&mut self) -> &mut Graph<Place> {
        &mut self.map
    }

    /// Vitality, from 0 to 100.
    pub const fn vitality(&self) -> i64 {
        self.vitality.value()
    }

    /// How much of the zone's vitality deaths are costing it, 0 to 100.
    pub const fn corruption(&self) -> i64 {
        self.corruption.value()
    }

    /// Records a death in the zone, which costs it `corruption` vitality.
    pub(crate) fn mourn(&mut self, corruption: i64) {
        self.corruption.adjust(corruption);
    }

    /// Lifts `corruption` from the zone, as a rite does.
    pub(crate) fn cleanse(&mut self, corruption: i64) {
        self.corruption.adjust(-corruption);
    }

    /// The vitality band.
    pub fn health(&self) -> Health {
        VITALITY_BANDS.band_of(self.vitality.value())
    }

    /// Runs one tick of the zone's ecology and tides.
    pub(crate) fn tick(&mut self, now: Tick, events: &mut Vec<Event>) {
        self.grow_deposits(events);
        if now.get().is_multiple_of(CORRUPTION_FADES_EVERY.as_ticks()) {
            self.corruption.adjust(-1);
        }
        self.update_vitality(events);
        self.report_passages(now, events);
    }

    fn grow_deposits(&mut self, events: &mut Vec<Event>) {
        let growth = self.health().growth();
        for (place, data) in self.map.places_mut() {
            let Some(deposit) = data.deposit.as_mut() else { continue };
            let from = deposit.node.quality();
            deposit.node.tick(growth);
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
    fn update_vitality(&mut self, events: &mut Vec<Event>) {
        let (pressure, deposits) = self
            .map
            .places()
            .filter_map(|(_, place)| place.deposit.as_ref())
            .fold((0i64, 0i64), |(pressure, count), deposit| {
                (pressure + i64::from(pressure_percent(&deposit.node)), count + 1)
            });
        let penalty = if deposits == 0 { 0 } else { pressure / deposits };
        let change = self.vitality.set(100 - penalty - self.corruption.value());
        if let Some((from, to)) = VITALITY_BANDS.crossing(change) {
            events.push(Event::VitalityChanged { from, to });
        }
    }

    /// Reports every tidal exit that opened or closed this tick.
    fn report_passages(&self, now: Tick, events: &mut Vec<Event>) {
        let Some(previous) = now.get().checked_sub(1).map(Tick::new) else { return };
        for (place, exit) in self.map.all_exits() {
            if matches!(exit.passage, Passage::Open) {
                continue;
            }
            let open = exit.passage.is_open(now);
            if open != exit.passage.is_open(previous) {
                events.push(Event::PassageChanged { place, label: exit.label.clone(), open });
            }
        }
    }
}
