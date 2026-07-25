use sim_table_core::is_legal_table_segment;

use crate::NamespaceError;

/// A validated finite namespace child name.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NamespaceName(String);

impl NamespaceName {
    /// Validate `value` through the canonical Table segment predicate.
    pub fn new(value: impl Into<String>) -> Result<Self, NamespaceError> {
        let value = value.into();
        if !is_legal_table_segment(&value) {
            return Err(NamespaceError::IllegalName(value));
        }
        Ok(Self(value))
    }

    /// Borrow the stored child name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for NamespaceName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Generated-name counter families are scoped per parent directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GeneratedNameKind {
    /// Durable calculation cell names.
    Cell,
    /// Durable child directory names.
    Dir,
}

impl GeneratedNameKind {
    pub(crate) fn prefix(self) -> &'static str {
        match self {
            Self::Cell => "cell",
            Self::Dir => "dir",
        }
    }
}
