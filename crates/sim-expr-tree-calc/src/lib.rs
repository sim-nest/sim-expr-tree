#![forbid(unsafe_code)]
//! Incremental calculation support for expression trees.

mod calc;
mod policy;

pub use calc::{CalcQuery, CellExpr, ExprTreeCalc};
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
