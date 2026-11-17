//! The sums behind a harvest: what reaches the pack, how hard a deposit
//! is being pushed, and what over-harvesting costs the one who does it.

use eq_nodes::{Node, Quality};

use crate::content::{TAINT_AT_FULL_PRESSURE, YIELD_LOST_AT_FULL_TAINT, YIELD_PERCENT_PER_LEVEL};
use crate::types::Health;

#[cfg(test)]
use alloc::vec::Vec;

/// Everything that decides how much of a harvest reaches the pack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Haul {
    /// Units removed from the deposit.
    pub taken: u32,
    /// The deposit's quality when the work began.
    pub quality: Quality,
    /// The zone's health.
    pub health: Health,
    /// The gatherer's level in the attribute that governs this resource.
    pub attribute_level: u32,
    /// The gatherer's taint, from 0 to 100.
    pub taint: u32,
}

impl Haul {
    /// How much reaches the pack:
    ///
    /// `taken × quality × zone health × (1 + attribute level / 100) × (1 − taint loss)`
    ///
    /// rounded down once at the end.
    pub fn amount(&self) -> u64 {
        const WHOLE: u64 = 100;
        let attribute_percent = WHOLE + u64::from(self.attribute_level) * YIELD_PERCENT_PER_LEVEL;
        let taint_percent = WHOLE - u64::from(self.taint.min(100) * YIELD_LOST_AT_FULL_TAINT) / WHOLE;
        let parts = u64::from(self.taken)
            * u64::from(self.quality.yield_percent())
            * u64::from(self.health.yield_percent())
            * attribute_percent
            * taint_percent;
        parts / WHOLE.pow(4)
    }
}

/// Taint earned by one haul from a deposit under `pressure` percent.
pub fn taint_for(pressure: u32) -> i64 {
    i64::from(pressure.min(100) * TAINT_AT_FULL_PRESSURE / 100)
}

/// How hard a deposit is being pushed, from 0 to 100: the greater of its
/// wear and its bareness.
///
/// Wear comes from taking fast. Bareness comes from never letting the
/// stock recover, however little each visitor takes. Either one means the
/// deposit is over-harvested.
pub fn pressure_percent(node: &Node) -> u32 {
    node.wear_percent().max(node.bareness_percent())
}

#[cfg(test)]
mod tests {
    use super::*;
    use eq_nodes::NodeConfig;

    const fn haul(quality: Quality, health: Health, attribute_level: u32, taint: u32) -> Haul {
        Haul { taken: 10, quality, health, attribute_level, taint }
    }

    #[test]
    fn yield_follows_quality_health_attribute_and_taint() {
        assert_eq!(haul(Quality::Pristine, Health::Balanced, 0, 0).amount(), 10);
        assert_eq!(haul(Quality::Pristine, Health::Peak, 0, 0).amount(), 12);
        assert_eq!(haul(Quality::Depleted, Health::Balanced, 0, 0).amount(), 2);
        assert_eq!(haul(Quality::Depleted, Health::Collapsed, 0, 0).amount(), 1);
        assert_eq!(haul(Quality::Pristine, Health::Balanced, 50, 0).amount(), 15);
        assert_eq!(haul(Quality::Pristine, Health::Balanced, 0, 40).amount(), 7);
        assert_eq!(haul(Quality::Pristine, Health::Balanced, 0, 100).amount(), 2);
    }

    #[test]
    fn yield_rises_with_the_attribute_and_falls_with_taint() {
        let by_level: Vec<u64> = (0..=100).map(|level| haul(Quality::Healthy, Health::Balanced, level, 0).amount()).collect();
        assert!(by_level.windows(2).all(|pair| pair[0] <= pair[1]));
        let by_taint: Vec<u64> = (0..=100).map(|taint| haul(Quality::Healthy, Health::Balanced, 0, taint).amount()).collect();
        assert!(by_taint.windows(2).all(|pair| pair[0] >= pair[1]));
    }

    #[test]
    fn taint_scales_with_pressure_and_careful_hauls_earn_none() {
        assert_eq!(taint_for(0), 0);
        assert_eq!(taint_for(50), 5);
        assert_eq!(taint_for(100), 10);
        assert_eq!(taint_for(999), 10);
    }

    #[test]
    fn pressure_is_the_worse_of_wear_and_bareness() {
        let config = NodeConfig { capacity: 100, regrow_cost: 20, strain_per_unit: 1, recovery_every: 2 };
        // Emptied gently: no wear, fully bare.
        let mut bare = Node::new(config);
        bare.harvest(100);
        assert_eq!((bare.wear_percent(), bare.bareness_percent(), pressure_percent(&bare)), (0, 100, 100));
        // Hit hard but still mostly full: worn, not bare.
        let mut worn = Node::new(NodeConfig { strain_per_unit: 20, ..config });
        worn.harvest(40);
        assert_eq!((worn.wear_percent(), worn.bareness_percent(), pressure_percent(&worn)), (73, 0, 73));
    }
}
