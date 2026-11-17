//! Point pools with two caps: one per entry and one for the whole pool.
//!
//! This is the shape of a skill budget: each skill tops out at 100, and all
//! combat skills together top out at 300. Raising one skill past the shared
//! cap means lowering another first.
//!
//! ```
//! use eq_ledger::Ledger;
//!
//! // Four skills, 100 each, 300 between them.
//! let mut combat: Ledger<4> = Ledger::new(100, 300);
//! combat.grant(0, 100);
//! combat.grant(1, 100);
//! combat.grant(2, 90);
//!
//! let grant = combat.grant(3, 25);
//! assert_eq!((grant.granted, grant.refused), (10, 15));
//! assert_eq!(combat.remaining(), 0);
//! ```
#![no_std]

/// `N` entries of points, each capped, with a cap on their total.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ledger<const N: usize> {
    points: [u32; N],
    entry_cap: u32,
    pool_cap: u32,
}

/// The outcome of trying to add points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grant {
    /// Points actually added.
    pub granted: u32,
    /// Points turned away because a cap was reached.
    pub refused: u32,
}

impl<const N: usize> Ledger<N> {
    /// An empty ledger where each entry holds at most `entry_cap` and all
    /// entries together hold at most `pool_cap`.
    pub const fn new(entry_cap: u32, pool_cap: u32) -> Self {
        Self { points: [0; N], entry_cap, pool_cap }
    }

    /// Points held by `entry`.
    ///
    /// # Panics
    /// Panics if `entry >= N`.
    pub const fn get(&self, entry: usize) -> u32 {
        self.points[entry]
    }

    /// Points held across all entries.
    pub fn total(&self) -> u32 {
        // Never overflows: `grant` keeps the total at or below `pool_cap`.
        self.points.iter().sum()
    }

    /// Points the pool can still take before reaching its cap.
    pub fn remaining(&self) -> u32 {
        self.pool_cap - self.total()
    }

    /// The most any single entry can hold.
    pub const fn entry_cap(&self) -> u32 {
        self.entry_cap
    }

    /// The most all entries together can hold.
    pub const fn pool_cap(&self) -> u32 {
        self.pool_cap
    }

    /// Adds up to `amount` points to `entry`, stopping at whichever cap is
    /// reached first.
    ///
    /// # Panics
    /// Panics if `entry >= N`.
    pub fn grant(&mut self, entry: usize, amount: u32) -> Grant {
        let room_in_entry = self.entry_cap.saturating_sub(self.points[entry]);
        let granted = amount.min(room_in_entry).min(self.remaining());
        self.points[entry] += granted;
        Grant { granted, refused: amount - granted }
    }

    /// Takes up to `amount` points from `entry` and returns how many were
    /// taken.
    ///
    /// # Panics
    /// Panics if `entry >= N`.
    pub fn remove(&mut self, entry: usize, amount: u32) -> u32 {
        let removed = amount.min(self.points[entry]);
        self.points[entry] -= removed;
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_empty() {
        let ledger: Ledger<3> = Ledger::new(100, 300);
        assert_eq!((ledger.total(), ledger.remaining()), (0, 300));
        assert_eq!((ledger.entry_cap(), ledger.pool_cap()), (100, 300));
    }

    #[test]
    fn grant_stops_at_entry_cap() {
        let mut ledger: Ledger<3> = Ledger::new(100, 300);
        assert_eq!(ledger.grant(0, 130), Grant { granted: 100, refused: 30 });
        assert_eq!(ledger.get(0), 100);
        assert_eq!(ledger.grant(0, 1), Grant { granted: 0, refused: 1 });
    }

    #[test]
    fn grant_stops_at_pool_cap() {
        let mut ledger: Ledger<4> = Ledger::new(100, 250);
        ledger.grant(0, 100);
        ledger.grant(1, 100);
        assert_eq!(ledger.grant(2, 80), Grant { granted: 50, refused: 30 });
        assert_eq!(ledger.grant(3, 5), Grant { granted: 0, refused: 5 });
        assert_eq!(ledger.total(), 250);
    }

    #[test]
    fn removing_frees_room_for_other_entries() {
        let mut ledger: Ledger<2> = Ledger::new(100, 100);
        ledger.grant(0, 100);
        assert_eq!(ledger.grant(1, 40).granted, 0);
        assert_eq!(ledger.remove(0, 40), 40);
        assert_eq!(ledger.grant(1, 40).granted, 40);
    }

    #[test]
    fn remove_takes_no_more_than_is_held() {
        let mut ledger: Ledger<1> = Ledger::new(100, 100);
        ledger.grant(0, 10);
        assert_eq!(ledger.remove(0, 50), 10);
        assert_eq!(ledger.get(0), 0);
    }

    #[test]
    fn extreme_amounts_do_not_overflow() {
        let mut ledger: Ledger<2> = Ledger::new(u32::MAX, u32::MAX);
        ledger.grant(0, u32::MAX);
        assert_eq!(ledger.grant(1, u32::MAX), Grant { granted: 0, refused: u32::MAX });
        assert_eq!(ledger.total(), u32::MAX);
    }
}
