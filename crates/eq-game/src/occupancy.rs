//! Who is where.
//!
//! Answering "who can see this?" by scanning every character does not
//! scale, so the game keeps this index up to date as characters move.

use alloc::vec::Vec;

use eq_world_graph::PlaceId;

use crate::character::CharacterId;

/// The characters in each place, kept in id order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Occupancy {
    by_place: Vec<Vec<CharacterId>>,
}

impl Occupancy {
    /// An index for a map of `places` places, with nobody anywhere.
    pub(crate) fn new(places: usize) -> Self {
        Self { by_place: alloc::vec![Vec::new(); places] }
    }

    /// Everyone in `place`, in id order. Empty for an unknown place.
    pub(crate) fn at(&self, place: PlaceId) -> &[CharacterId] {
        slot(place).and_then(|index| self.by_place.get(index)).map_or(&[], Vec::as_slice)
    }

    /// Records `who` arriving in `place`. Does nothing if already there.
    pub(crate) fn enter(&mut self, place: PlaceId, who: CharacterId) {
        let Some(present) = slot(place).and_then(|index| self.by_place.get_mut(index)) else { return };
        if let Err(position) = present.binary_search(&who) {
            present.insert(position, who);
        }
    }

    /// Records `who` leaving `place`. Does nothing if they were not there.
    pub(crate) fn leave(&mut self, place: PlaceId, who: CharacterId) {
        let Some(present) = slot(place).and_then(|index| self.by_place.get_mut(index)) else { return };
        if let Ok(position) = present.binary_search(&who) {
            present.remove(position);
        }
    }
}

fn slot(place: PlaceId) -> Option<usize> {
    usize::try_from(place.get()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAMP: PlaceId = PlaceId::new(0);
    const LEDGE: PlaceId = PlaceId::new(1);

    fn ids(raw: &[u64]) -> Vec<CharacterId> {
        raw.iter().map(|&id| CharacterId::new(id)).collect()
    }

    #[test]
    fn keeps_each_place_in_id_order() {
        let mut index = Occupancy::new(2);
        for id in [5, 1, 3] {
            index.enter(CAMP, CharacterId::new(id));
        }
        assert_eq!(index.at(CAMP), ids(&[1, 3, 5]));
        assert!(index.at(LEDGE).is_empty());
    }

    #[test]
    fn moving_takes_you_out_of_one_place_and_into_another() {
        let mut index = Occupancy::new(2);
        let who = CharacterId::new(7);
        index.enter(CAMP, who);
        index.leave(CAMP, who);
        index.enter(LEDGE, who);
        assert!(index.at(CAMP).is_empty());
        assert_eq!(index.at(LEDGE), [who]);
    }

    #[test]
    fn repeats_and_unknown_places_are_harmless() {
        let mut index = Occupancy::new(1);
        let who = CharacterId::new(2);
        index.enter(CAMP, who);
        index.enter(CAMP, who);
        index.leave(LEDGE, who);
        index.enter(PlaceId::new(99), who);
        index.leave(CAMP, CharacterId::new(8));
        assert_eq!(index.at(CAMP), [who]);
        assert!(index.at(PlaceId::new(99)).is_empty());
    }
}
