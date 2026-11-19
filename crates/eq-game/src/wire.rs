//! How ids are written when commands are saved or sent: as bare numbers.

/// Makes an id type read and write as its raw `u64`.
macro_rules! as_number {
    ($id:ty) => {
        impl serde::Serialize for $id {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_u64(self.get())
            }
        }

        impl<'de> serde::Deserialize<'de> for $id {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                <u64 as serde::Deserialize>::deserialize(deserializer).map(Self::new)
            }
        }
    };
}

pub(crate) use as_number;

/// A place as it is read back from a saved world. The game keeps a
/// place's name and description as `&'static str`: the text a world was
/// made with lives as long as the program, so text read back from a
/// saved world is kept that long too. A world is loaded once, when a
/// server starts, so this is a few dozen short strings, once.
#[derive(serde::Deserialize)]
pub(crate) struct SavedPlace {
    name: alloc::string::String,
    description: alloc::string::String,
    safety: crate::types::Safety,
    deposit: Option<crate::types::Deposit>,
    lair: Option<crate::creature::CreatureKind>,
    position: (i32, i32),
}

impl<'de> serde::Deserialize<'de> for crate::types::Place {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        SavedPlace::deserialize(deserializer).map(Self::from)
    }
}

impl From<SavedPlace> for crate::types::Place {
    fn from(saved: SavedPlace) -> Self {
        Self {
            name: saved.name.leak(),
            description: saved.description.leak(),
            safety: saved.safety,
            deposit: saved.deposit,
            lair: saved.lair,
            position: saved.position,
        }
    }
}

