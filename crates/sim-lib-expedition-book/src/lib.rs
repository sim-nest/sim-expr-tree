#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Editable expedition books which retain immutable evidence by reference.
//!
//! A book is a small semantic organ over the expression-tree framework. It
//! records editorial choices, objections and next plays, but never stores or
//! interprets run, claim, source, or artifact content. [`EvidenceResolver`]
//! makes missing external evidence an explicit replay outcome.

pub mod archive;
pub mod codec;
pub mod edit;
pub mod model;
pub mod projection;

pub use archive::{Archive, ArchiveError, ReplayOutcome};
pub use codec::{CodecError, decode, encode};
pub use edit::{EditError, ExpeditionBook};
pub use model::*;
pub use projection::{BookProjection, project};

use sim_citizen::CitizenRegistry;
use sim_kernel::{Result, Symbol};

/// Stable Citizen class symbol for durable expedition-book snapshots.
pub fn expedition_book_class_symbol() -> Symbol {
    Symbol::qualified("expedition-book", "Snapshot")
}

/// Registers the durable book Citizen and its generated Shape contract.
pub fn expedition_book_citizen_registry() -> Result<CitizenRegistry> {
    let mut registry = CitizenRegistry::new();
    registry.register::<model::ExpeditionBookSnapshot>()?;
    Ok(registry)
}

/// Stable Lisp operation names provided by book hosts.
pub fn lisp_operation_symbols() -> Vec<Symbol> {
    [
        "branch",
        "compare",
        "object",
        "choose",
        "seal",
        "reopen",
        "retain",
        "export",
        "delete-projection",
        "replay",
    ]
    .into_iter()
    .map(|name| Symbol::qualified("expedition-book", name))
    .collect()
}

#[cfg(test)]
mod tests;
