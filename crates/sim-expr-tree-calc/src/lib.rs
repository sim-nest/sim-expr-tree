#![forbid(unsafe_code)]
//! Bounded incremental calculation for ordinary expression-tree values.
//!
//! Cell sources are ordinary [`sim_kernel::Expr`] graphs evaluated through a
//! fresh [`sim_kernel::Cx`]; successful results remain ordinary
//! [`sim_kernel::Value`] handles. The calculator adds dynamic tree-reference
//! observation, dependency-first pull verification, deterministic cycles,
//! canonical fingerprint cutoff, conservative volatility for noncanonical
//! values, inherited triggers, directed force modes, immutable authority
//! diminution, restartable automatic queues, bounded standard progress streams,
//! versioned Table-backed graph snapshots, explicit non-watch backend refresh,
//! calculation receipts, explanations, failure memos, and explicitly labelled
//! last-good recovery without introducing a product-specific value enum.

mod calc;
mod policy;

pub use calc::{
    AuthorityDigest, AuthorityPolicyPatch, AutomaticBudget, AutomaticContinuation,
    AutomaticQueueSnapshot, AutomaticRun, BackendRefreshSample, CalcError, CalcExplanation,
    CalcLimits, CalcOutcome, CalcPolicyPatch, CalcQuery, CalcReason, CalcReceipt, CalcRequestMode,
    CalcStatus, CalcTrigger, CalcWatch, CellFailure, CycleMode, DERIVED_SNAPSHOT_KEY,
    DependencyStamp, DerivedPersistReport, DerivedRestoreDisposition, DerivedRestoreReport,
    DerivedSnapshotError, DerivedTableAdapter, DirectedCalcReport, DirectedCellResult, EffectStamp,
    EffectiveAuthority, EffectiveCalcPolicy, EncodedFace, ErrorMode, ExprTreeCalc, FaceContent,
    FaceDimension, FaceIssue, FaceMetadata, FacePosition, GRAPH_SCHEMA_VERSION,
    HARD_MAX_EXPR_DEPTH, HARD_MAX_OBSERVATIONS, HARD_MAX_OUTPUT, HARD_MAX_QUERY_DEPTH,
    HARD_MAX_WORK, LastGoodValue, MountRefreshSource, PolicyDigest, QueuedCalculation,
    RefreshError, RefreshReport, RequestId, SourceEditOutcome,
};
pub use policy::{EXPR_TREE_REF, ExprTreeRefPolicy};
pub use sim_expr_tree_core::{CodecPolicyPatch, EffectiveCodecPolicy, FaceBudget};

/// Returns the crate's public scaffold identity.
pub fn crate_identity() -> &'static str {
    "sim-expr-tree-calc"
}

/// Returns the core crate identity used by this scaffold dependency.
pub fn core_identity() -> &'static str {
    sim_expr_tree_core::crate_identity()
}

#[cfg(test)]
mod tests;
