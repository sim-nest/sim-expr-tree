//! Export, projection deletion, and replay.

use crate::{
    codec,
    edit::ExpeditionBook,
    model::{EvidenceRef, EvidenceResolver},
};
use std::collections::BTreeMap;

/// Replay failure that remains visible to callers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArchiveError {
    /// Stored export is absent.
    MissingExport(String),
    /// Stored payload failed decoding.
    InvalidExport(codec::CodecError),
}

/// Typed replay result, including missing referenced evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReplayOutcome {
    /// Exact semantic state and every referenced identity are available.
    Reproduced(ExpeditionBook),
    /// State decoded exactly, but external evidence is currently missing.
    MissingEvidence {
        /// Decoded editable state.
        book: ExpeditionBook,
        /// Exact missing identities.
        missing: Vec<EvidenceRef>,
    },
}

/// In-memory archive fixture. Durable providers can store these canonical bytes.
#[derive(Clone, Debug, Default)]
pub struct Archive {
    exports: BTreeMap<String, String>,
    projections: BTreeMap<String, String>,
}

impl Archive {
    /// Stores canonical semantics and an explicitly disposable projection.
    pub fn export(&mut self, name: impl Into<String>, book: &ExpeditionBook, projection: String) {
        let name = name.into();
        self.exports.insert(name.clone(), codec::encode(book));
        self.projections.insert(name, projection);
    }
    /// Deletes only the derived projection.
    pub fn delete_projection(&mut self, name: &str) -> bool {
        self.projections.remove(name).is_some()
    }
    /// Reports whether a disposable projection exists.
    pub fn has_projection(&self, name: &str) -> bool {
        self.projections.contains_key(name)
    }
    /// Replays exact semantics and checks external identities without reattaching.
    pub fn replay(
        &self,
        name: &str,
        resolver: &dyn EvidenceResolver,
    ) -> Result<ReplayOutcome, ArchiveError> {
        let bytes = self
            .exports
            .get(name)
            .ok_or_else(|| ArchiveError::MissingExport(name.into()))?;
        let book = codec::decode(bytes).map_err(ArchiveError::InvalidExport)?;
        let mut missing = Vec::new();
        for branch in book.branches.values() {
            for r in branch
                .evidence
                .iter()
                .chain(branch.objections.values().flat_map(|o| &o.evidence))
            {
                if !resolver.exists(r) {
                    missing.push(r.clone());
                }
            }
        }
        missing.sort();
        missing.dedup();
        Ok(if missing.is_empty() {
            ReplayOutcome::Reproduced(book)
        } else {
            ReplayOutcome::MissingEvidence { book, missing }
        })
    }
}
