//! Bounded, read-only inspection of a live expression-tree handle.

use sim_expr_tree_calc::{CalcReceipt, CalcStatus, EncodedFace};
use sim_kernel::{Error, Result};

use crate::TreeHandle;

/// Kind of one immediate expression-tree entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TreeEntryKind {
    /// A finite namespace directory.
    Directory,
    /// A source cell.
    Cell,
    /// A mounted directory or table boundary.
    Mount,
}

/// Bounded identity facts for one immediate expression-tree entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeEntryInspection {
    /// Canonical absolute path.
    pub path: String,
    /// Final canonical path segment.
    pub name: String,
    /// Entry kind.
    pub kind: TreeEntryKind,
    /// Source revision for a cell, or zero for a directory.
    pub revision: u64,
}

/// Non-evaluating, already-bounded facts for one expression-tree cell.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeCellInspection {
    /// Canonical absolute path.
    pub path: String,
    /// Bounded source face produced by inherited codec policy.
    pub source: EncodedFace,
    /// Bounded result face produced by inherited codec policy.
    pub result: EncodedFace,
    /// Current non-evaluating calculation status.
    pub status: CalcStatus,
    /// Current source revision.
    pub source_revision: u64,
    /// Latest bounded receipt, when any.
    pub receipt: Option<CalcReceipt>,
    /// Stable effective-policy badges.
    pub policy_badges: Vec<String>,
}

impl TreeHandle {
    /// Returns the durable storage name without exposing the live backend.
    pub fn storage_name(&self) -> Result<String> {
        self.with_state(|state| Ok(state.storage_name().to_owned()))
    }

    /// Lists bounded immediate entry facts below `path`.
    pub fn inspect_entries(&self, path: &str) -> Result<Vec<TreeEntryInspection>> {
        self.with_state(|state| state.inspect_entries(path))
    }

    /// Returns bounded, non-evaluating facts for one cell.
    pub fn inspect_cell(&self, path: &str) -> Result<TreeCellInspection> {
        self.with_state(|state| state.inspect_cell(path))
    }

    /// Injects optional human wall-clock observations into future receipts.
    ///
    /// Logical ticks and revisions remain the only freshness authority.
    pub fn set_wall_clock<F>(&self, clock: F) -> Result<()>
    where
        F: Fn() -> Option<u64> + Send + Sync + 'static,
    {
        self.with_state(|state| {
            state.set_wall_clock(clock);
            Ok(())
        })
    }

    fn with_state<T>(
        &self,
        action: impl FnOnce(&mut crate::runtime::TreeState) -> std::result::Result<T, String>,
    ) -> Result<T> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::Eval("expression-tree state poisoned".to_owned()))?;
        action(&mut state).map_err(Error::Eval)
    }
}
