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
//! calculation receipts, explanations, failure memos, and explicitly labelled
//! last-good recovery without introducing a product-specific value enum.

mod calc;
mod policy;

pub use calc::{
    AuthorityDigest, AuthorityPolicyPatch, AutomaticBudget, AutomaticContinuation,
    AutomaticQueueSnapshot, AutomaticRun, CalcError, CalcExplanation, CalcLimits, CalcOutcome,
    CalcPolicyPatch, CalcQuery, CalcReason, CalcReceipt, CalcRequestMode, CalcStatus, CalcTrigger,
    CalcWatch, CellFailure, CycleMode, DependencyStamp, DirectedCalcReport, DirectedCellResult,
    EffectStamp, EffectiveAuthority, EffectiveCalcPolicy, ErrorMode, ExprTreeCalc,
    HARD_MAX_EXPR_DEPTH, HARD_MAX_OBSERVATIONS, HARD_MAX_OUTPUT, HARD_MAX_QUERY_DEPTH,
    HARD_MAX_WORK, LastGoodValue, PolicyDigest, QueuedCalculation, RequestId,
};
pub use policy::{EXPR_TREE_REF, ExprTreeRefPolicy};

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
