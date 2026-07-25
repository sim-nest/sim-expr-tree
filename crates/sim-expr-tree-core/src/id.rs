use std::fmt;

use crate::error::NamespaceError;

macro_rules! id_type {
    ($name:ident, $label:literal) => {
        #[doc = concat!("Stable ", $label, " identity.")]
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            #[doc = concat!("Create a ", $label, " id from non-empty text.")]
            pub fn new(value: impl Into<String>) -> Result<Self, NamespaceError> {
                let value = value.into();
                if value.is_empty() {
                    return Err(NamespaceError::EmptyId { kind: $label });
                }
                Ok(Self(value))
            }

            #[doc = concat!("Borrow the underlying ", $label, " id text.")]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

id_type!(TreeId, "tree");
id_type!(CellId, "cell");
id_type!(DirId, "dir");
