//! Resource nodes that regrow, and wear out when over-harvested.
//!
//! A [`Node`] holds a stock of units that regrows over time. Every unit
//! taken adds *strain*; strain eases off slowly when the node is left
//! alone. Strain sets the node's [`Quality`], from pristine to depleted,
//! and quality sets how much of what you take is worth keeping.
//!
//! Take slowly and the node stays pristine. Strip it and it yields a
//! fifth of what it did, and grows back at a fifth of the pace.
//!
//! Because a worn node regrows slowly, nobody can hold one stripped by
//! taking each unit as it appears: there is too little to take, the
//! strain eases faster than it builds, and the node climbs back until
//! regrowth and wear balance.
//!
//! ```
//! use eq_nodes::{Node, NodeConfig, Quality};
//!
//! let mut ore = Node::new(NodeConfig {
//!     capacity: 100,
//!     regrow_cost: 20,
//!     strain_per_unit: 8,
//!     recovery_every: 2,
//! });
//!
//! let first = ore.harvest(10);
//! assert_eq!((first.taken, first.quality), (10, Quality::Pristine));
//!
//! ore.harvest(90);
//! assert_eq!(ore.quality(), Quality::Depleted);
//! assert_eq!(ore.stock(), 0);
//! ```
#![no_std]

use eq_meter::{Bands, Meter};

/// Strain is measured in tenths of a percent, so it can creep slowly.
const MAX_STRAIN: i64 = 1_000;

/// Strain below this leaves a node pristine and counts as no wear at all.
const PRISTINE_LIMIT: i64 = 250;

const QUALITY: Bands<Quality, 3> = Bands::new(
    Quality::Pristine,
    [(PRISTINE_LIMIT, Quality::Healthy), (500, Quality::Worn), (750, Quality::Depleted)],
);

/// How worn a node is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Quality {
    /// Untouched or fully recovered.
    Pristine,
    /// Showing use.
    Healthy,
    /// Over-used.
    Worn,
    /// Stripped.
    Depleted,
}

impl Quality {
    /// The share of a harvest worth keeping, as a percentage.
    pub const fn yield_percent(self) -> u32 {
        match self {
            Self::Pristine => 100,
            Self::Healthy => 80,
            Self::Worn => 50,
            Self::Depleted => 20,
        }
    }

    /// How fast the node regrows, as a percentage of its full pace.
    pub const fn regrowth_percent(self) -> u32 {
        match self {
            Self::Pristine => 100,
            Self::Healthy => 80,
            Self::Worn => 50,
            Self::Depleted => 20,
        }
    }
}

/// How a node behaves. Plain numbers, meant to be tuned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeConfig {
    /// The most units the node can hold.
    pub capacity: u32,
    /// Growth points needed to regrow one unit, at a pristine node's pace.
    pub regrow_cost: u32,
    /// Strain added per unit taken, in tenths of a percent.
    pub strain_per_unit: u32,
    /// Ticks for strain to ease by one tenth of a percent.
    pub recovery_every: u32,
}

/// What a harvest took.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Harvest {
    /// Units removed from the node.
    pub taken: u32,
    /// The node's quality when the harvest began.
    pub quality: Quality,
}

/// A source of one resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    config: NodeConfig,
    stock: u32,
    strain: Meter,
    /// In hundredths of a growth point, so wear can slow it smoothly.
    regrow_progress: u64,
    recovery_progress: u32,
}

impl Node {
    /// A full, pristine node.
    ///
    /// # Panics
    /// Panics if `regrow_cost` or `recovery_every` is zero.
    pub const fn new(config: NodeConfig) -> Self {
        assert!(config.regrow_cost > 0, "regrow_cost must be greater than zero");
        assert!(config.recovery_every > 0, "recovery_every must be greater than zero");
        Self {
            config,
            stock: config.capacity,
            strain: Meter::empty(0, MAX_STRAIN),
            regrow_progress: 0,
            recovery_progress: 0,
        }
    }

    /// Units available to take.
    pub const fn stock(&self) -> u32 {
        self.stock
    }

    /// The most units the node can hold.
    pub const fn capacity(&self) -> u32 {
        self.config.capacity
    }

    /// How strained the node is, from 0 to 100.
    pub const fn strain_percent(&self) -> u32 {
        (self.strain.value() / 10) as u32
    }

    /// How far past pristine the node is, from 0 to 100.
    ///
    /// Zero for the whole pristine range: use that leaves a node pristine
    /// is not over-use.
    pub const fn wear_percent(&self) -> u32 {
        let over = self.strain.value() - PRISTINE_LIMIT;
        if over <= 0 {
            return 0;
        }
        (over * 100 / (MAX_STRAIN - PRISTINE_LIMIT)) as u32
    }

    /// How worn the node is.
    pub fn quality(&self) -> Quality {
        QUALITY.band_of(self.strain.value())
    }

    /// Runs one tick: regrows stock by `growth` points, slowed by wear,
    /// and eases strain.
    ///
    /// `growth` lets the surroundings speed regrowth up or slow it down.
    pub fn tick(&mut self, growth: u32) {
        self.regrow(growth);
        self.recover();
    }

    /// Takes up to `wanted` units and adds the strain of taking them.
    pub fn harvest(&mut self, wanted: u32) -> Harvest {
        let quality = self.quality();
        let taken = wanted.min(self.stock);
        self.stock -= taken;
        let strain = i64::from(taken).saturating_mul(i64::from(self.config.strain_per_unit));
        self.strain.adjust(strain);
        Harvest { taken, quality }
    }

    fn regrow(&mut self, growth: u32) {
        if self.stock >= self.config.capacity {
            self.regrow_progress = 0;
            return;
        }
        let pace = u64::from(self.quality().regrowth_percent());
        let cost = u64::from(self.config.regrow_cost) * 100;
        let progress = self.regrow_progress + u64::from(growth) * pace;
        let room = u64::from(self.config.capacity - self.stock);
        let grown = (progress / cost).min(room);
        // `grown` is at most `room`, which came from a u32.
        self.stock += grown as u32;
        self.regrow_progress = progress - grown * cost;
    }

    fn recover(&mut self) {
        self.recovery_progress += 1;
        if self.recovery_progress >= self.config.recovery_every {
            self.recovery_progress = 0;
            self.strain.adjust(-1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORE: NodeConfig =
        NodeConfig { capacity: 100, regrow_cost: 20, strain_per_unit: 8, recovery_every: 2 };

    fn run(node: &mut Node, ticks: u32, growth: u32) {
        for _ in 0..ticks {
            node.tick(growth);
        }
    }

    #[test]
    fn starts_full_and_pristine() {
        let node = Node::new(ORE);
        assert_eq!((node.stock(), node.capacity()), (100, 100));
        assert_eq!((node.quality(), node.strain_percent()), (Quality::Pristine, 0));
    }

    #[test]
    fn harvest_takes_no_more_than_stock() {
        let mut node = Node::new(ORE);
        assert_eq!(node.harvest(70).taken, 70);
        assert_eq!(node.harvest(70).taken, 30);
        assert_eq!(node.harvest(70).taken, 0);
    }

    #[test]
    fn quality_falls_through_every_band_as_strain_builds() {
        let mut node = Node::new(ORE);
        let mut reached = [false; 4];
        for _ in 0..10 {
            node.harvest(10);
            reached[node.quality() as usize] = true;
        }
        // 10 units add 8% strain, so no band can be skipped on the way down.
        assert_eq!(reached, [true; 4]);
        assert_eq!(node.quality(), Quality::Depleted);
    }

    #[test]
    fn harvest_reports_quality_before_the_cut() {
        let mut node = Node::new(ORE);
        let harvest = node.harvest(100);
        assert_eq!(harvest.quality, Quality::Pristine);
        assert_eq!(node.quality(), Quality::Depleted);
    }

    #[test]
    fn stock_regrows_at_the_growth_rate_and_stops_at_capacity() {
        let mut node = Node::new(ORE);
        node.harvest(10);
        run(&mut node, 20, 1);
        assert_eq!(node.stock(), 91);
        run(&mut node, 20, 2);
        assert_eq!(node.stock(), 93);
        run(&mut node, 10_000, 3);
        assert_eq!(node.stock(), 100);
    }

    #[test]
    fn no_growth_means_no_regrowth() {
        let mut node = Node::new(ORE);
        node.harvest(50);
        run(&mut node, 1_000, 0);
        assert_eq!(node.stock(), 50);
    }

    #[test]
    fn a_stripped_node_regrows_slowly_until_it_heals() {
        let mut stripped = Node::new(ORE);
        stripped.harvest(100);
        let mut emptied_gently = Node::new(ORE);
        emptied_gently.harvest(30);

        run(&mut stripped, 100, 2);
        run(&mut emptied_gently, 100, 2);
        // Same time, same growth: 2 units against 10.
        assert_eq!(stripped.stock(), 2);
        assert_eq!(emptied_gently.stock(), 70 + 10);

        run(&mut stripped, 2_000, 2);
        assert_eq!((stripped.stock(), stripped.quality()), (100, Quality::Pristine));
    }

    #[test]
    fn camping_cannot_hold_a_node_stripped() {
        let mut node = Node::new(ORE);
        node.harvest(100);
        assert_eq!(node.quality(), Quality::Depleted);

        // Take every unit the moment it appears, for a long time.
        for _ in 0..20_000 {
            node.tick(2);
            node.harvest(u32::MAX);
        }
        assert!(node.quality() <= Quality::Worn, "still {:?}", node.quality());
    }

    #[test]
    fn wear_is_zero_while_pristine_and_full_at_the_limit() {
        let mut node = Node::new(ORE);
        node.harvest(30);
        assert_eq!((node.strain_percent(), node.wear_percent()), (24, 0));
        node.harvest(10);
        assert_eq!((node.strain_percent(), node.wear_percent()), (32, 9));
        node.harvest(60);
        assert_eq!((node.strain_percent(), node.wear_percent()), (80, 73));
        let mut ruined = Node::new(NodeConfig { strain_per_unit: 10, ..ORE });
        ruined.harvest(100);
        assert_eq!(ruined.wear_percent(), 100);
    }

    #[test]
    fn gentle_harvesting_stays_pristine() {
        let mut node = Node::new(ORE);
        for _ in 0..200 {
            node.harvest(5);
            run(&mut node, 100, 2);
        }
        assert_eq!(node.quality(), Quality::Pristine);
    }

    #[test]
    fn yield_and_regrowth_shrink_with_wear() {
        let all = [Quality::Pristine, Quality::Healthy, Quality::Worn, Quality::Depleted];
        assert_eq!(all.map(Quality::yield_percent), [100, 80, 50, 20]);
        assert_eq!(all.map(Quality::regrowth_percent), [100, 80, 50, 20]);
    }

    #[test]
    fn huge_growth_does_not_overflow() {
        let mut node = Node::new(ORE);
        node.harvest(100);
        node.tick(u32::MAX);
        node.tick(u32::MAX);
        assert_eq!(node.stock(), 100);
    }
}
