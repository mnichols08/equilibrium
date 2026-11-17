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
