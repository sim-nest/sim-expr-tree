#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Loadable expression-tree runtime and Lisp surface.
//!
//! [`ExprTreeLib`] installs the stable `expr-tree/*` operation family, one
//! argument and result [`sim_kernel::Shape`] contract for every operation, and
//! browseable [`sim_kernel::card::Card`] projections. Calls retain ordinary
//! SIM expressions and values while composing the finite namespace,
//! mixed-backend store, bounded incremental calculator, inherited codec
//! policy, standard progress streams, and Citizen reconstruction records.
//!
//! Live [`TreeHandle`] values remain opaque runtime authority: only
//! [`DurableSourceRecord`] and [`DurablePolicyRecord`] participate in
//! Citizen/read-construct.

mod capability;
mod citizen;
mod dispatch;
mod handle;
mod inspect;
mod operation;
mod parse;
mod projection;
mod runtime;
mod runtime_support;
mod shape;
mod source;

pub use capability::{
    expr_tree_calculate_capability, expr_tree_mount_capability, expr_tree_read_capability,
    expr_tree_write_capability,
};
pub use citizen::{
    DurablePolicyRecord, DurableSourceRecord, durable_policy_class_symbol,
    durable_source_class_symbol, expr_tree_citizen_registry,
};
pub use handle::TreeHandle;
pub use inspect::{TreeCellInspection, TreeEntryInspection, TreeEntryKind};
pub use operation::{
    ExprTreeLib, expr_tree_exports, expr_tree_lib_symbol, expr_tree_operation_cards_symbol,
    expr_tree_operation_symbols, install_expr_tree_lib,
};
pub use projection::operation_cards;
pub use runtime::{MAX_LIST_ITEMS, MAX_TREE_NODES};
pub use shape::{operation_args_shape_symbol, operation_result_shape_symbol};

/// Cookbook recipes embedded with the loadable library.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

/// Returns the crate's public runtime-library identity.
pub fn crate_identity() -> &'static str {
    "sim-lib-expr-tree"
}

/// Returns the lower-layer identities composed by this library.
pub fn component_identities() -> [&'static str; 2] {
    [
        sim_expr_tree_core::crate_identity(),
        sim_expr_tree_calc::crate_identity(),
    ]
}

#[cfg(test)]
mod tests;
