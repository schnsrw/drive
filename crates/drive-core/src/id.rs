//! Opaque IDs. ULID under the hood — sortable by creation time, opaque to
//! the user, 26-char Crockford-base32 on the wire. See
//! [`docs/research/06-security.md`](../../docs/research/06-security.md) §1
//! for why we never derive storage keys from user input.

use serde::{Deserialize, Serialize};

macro_rules! opaque_id {
    ($name:ident, $doc:expr) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub ulid::Ulid);

        impl $name {
            #[must_use]
            pub fn new() -> Self {
                Self(ulid::Ulid::new())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl std::str::FromStr for $name {
            type Err = ulid::DecodeError;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ok(Self(ulid::Ulid::from_string(s)?))
            }
        }
    };
}

opaque_id!(FileId, "Opaque file identifier (ULID).");
opaque_id!(FolderId, "Opaque folder identifier (ULID).");

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn roundtrip_string() {
        let id = FileId::new();
        let s = id.to_string();
        let parsed = FileId::from_str(&s).unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn file_and_folder_are_distinct_types() {
        // Should not compile if uncommented:
        // let _: FileId = FolderId::new();
        let _f = FileId::new();
        let _d = FolderId::new();
    }

    #[test]
    fn serde_transparent() {
        let id = FileId::new();
        let json = serde_json::to_string(&id).unwrap();
        assert!(json.starts_with('"') && json.ends_with('"'));
        let back: FileId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }
}
