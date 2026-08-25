//! Durable semantic model and reference boundaries.

use sim_citizen_derive::Citizen;
use std::collections::BTreeMap;

/// Opaque content-addressed identity owned by another subsystem.
#[derive(Clone, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct EvidenceRef {
    /// Evidence kind: `run`, `claim`, `source`, or `artifact`.
    pub kind: String,
    /// Exact external content key. The book never dereferences it implicitly.
    pub content_key: String,
}

impl EvidenceRef {
    /// Constructs a validated external reference.
    pub fn new(kind: impl Into<String>, content_key: impl Into<String>) -> Option<Self> {
        let value = Self {
            kind: kind.into(),
            content_key: content_key.into(),
        };
        ((!value.kind.is_empty() && !value.content_key.is_empty())
            && matches!(value.kind.as_str(), "run" | "claim" | "source" | "artifact"))
        .then_some(value)
    }
}

/// One objection retained with a branch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Objection {
    /// Stable objection id.
    pub id: String,
    /// Editable objection statement.
    pub statement: String,
    /// Immutable supporting evidence identities.
    pub evidence: Vec<EvidenceRef>,
}

/// Editable branch of meaning.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Branch {
    /// Stable branch id.
    pub id: String,
    /// Parent branch, absent only at the root.
    pub parent: Option<String>,
    /// Editorial thesis.
    pub thesis: String,
    /// Referenced evidence, sorted and deduplicated.
    pub evidence: Vec<EvidenceRef>,
    /// Objections keyed by stable id.
    pub objections: BTreeMap<String, Objection>,
    /// Proposed next play.
    pub next_play: Option<String>,
    /// Whether further edits are refused.
    pub sealed: bool,
}

/// Citizen/read-construct representation of an expedition book.
///
/// Complex branch state is held in the versioned codec payload. This record
/// provides the stable runtime identity and Shape while remaining plain data.
#[derive(Clone, Debug, Default, PartialEq, Citizen)]
#[citizen(symbol = "expedition-book/Snapshot", version = 1)]
pub struct ExpeditionBookSnapshot {
    /// Book identity.
    pub book_id: String,
    /// Optimistic edit revision.
    pub revision: u64,
    /// Versioned canonical codec payload.
    pub payload: String,
}

/// Resolves only the existence of referenced immutable evidence.
pub trait EvidenceResolver {
    /// Returns whether the exact external identity currently exists.
    fn exists(&self, reference: &EvidenceRef) -> bool;
}
