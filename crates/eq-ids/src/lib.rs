//! Typed ids.
//!
//! Every kind of thing gets its own id type, so a `PlayerId` can never be
//! passed where a `PlaceId` is expected. Ids are plain `u64`s underneath:
//! cheap to copy, order and store.
//!
//! ```
//! eq_ids::define_id!(
//!     /// Identifies a player.
//!     PlayerId
//! );
//!
//! let a = PlayerId::new(7);
//! assert_eq!(a.get(), 7);
//! assert_eq!(a.to_string(), "PlayerId(7)");
//! ```
#![no_std]

/// Declares a new id type wrapping a `u64`.
#[macro_export]
macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(u64);

        impl $name {
            /// Wraps a raw value.
            pub const fn new(raw: u64) -> Self {
                Self(raw)
            }

            /// Returns the raw value.
            pub const fn get(self) -> u64 {
                self.0
            }
        }

        impl ::core::fmt::Display for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }
    };
}

/// Hands out ids in order, starting from zero.
///
/// The allocator is plain data: store it in world state and it replays
/// exactly like everything else.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Allocator {
    next: u64,
}

impl Allocator {
    /// An allocator whose first id is zero.
    pub const fn new() -> Self {
        Self { next: 0 }
    }

    /// Returns the next raw id. Wrap it with the id type's `new`.
    pub fn next_raw(&mut self) -> u64 {
        let raw = self.next;
        self.next += 1;
        raw
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    define_id!(
        /// Test id.
        PlaceId
    );

    #[test]
    fn round_trips_raw_value() {
        assert_eq!(PlaceId::new(42).get(), 42);
    }

    #[test]
    fn orders_by_raw_value() {
        assert!(PlaceId::new(1) < PlaceId::new(2));
    }

    #[test]
    fn allocator_counts_up_from_zero() {
        let mut ids = Allocator::new();
        assert_eq!(PlaceId::new(ids.next_raw()), PlaceId::new(0));
        assert_eq!(PlaceId::new(ids.next_raw()), PlaceId::new(1));
    }
}
