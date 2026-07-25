#![forbid(unsafe_code)]
//! Backend-neutral expression-tree namespace records.

mod error;
mod id;
mod name;
mod namespace;
mod node;
mod path;
mod policy;
mod stamp;

pub use error::NamespaceError;
pub use id::{CellId, DirId, TreeId};
pub use name::{GeneratedNameKind, NamespaceName};
pub use namespace::{CellCreate, Namespace, NamespaceEntry, WriterLane};
pub use node::{CellRecord, DirRecord, NodeKind, SourceRecord};
pub use path::resolve_namespace_path;
pub use policy::{CodecPolicy, EffectivePolicy, PolicyPatch};
pub use stamp::{RevisionTick, Stamp, WallTimeMs};

/// Returns the crate's public identity.
pub fn crate_identity() -> &'static str {
    "sim-expr-tree-core"
}

#[cfg(test)]
mod tests;
