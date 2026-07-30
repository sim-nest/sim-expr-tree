#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Bootloader-loaded expression-tree product recipe.
//!
//! The crate owns no expression-tree engine, view protocol, server protocol, or
//! browser runtime. [`ExpressionTreeRecipe`] composes the existing loadable
//! engine and server with the reversible expression-tree surface and generic
//! web host. [`expr_tree_bootloader`] supplies that recipe to the standard
//! `sim-run` boot path and dispatches [`expr_tree_entrypoint_symbol`].

mod config;
mod entrypoint;
mod host;
mod recipe;

pub use config::{ExpressionTreeServeConfig, ServerPlacement, serve_config_symbol};
pub use entrypoint::{ExprTreeServeLib, expr_tree_entrypoint_symbol};
pub use host::{
    EXPR_TREE_HOST_LIB, EXPR_TREE_VERB, configure_expr_tree_session, expr_tree_boot_args,
    expr_tree_bootloader,
};
pub use recipe::{ExpressionTreeProduct, ExpressionTreeRecipe};

/// Returns the crate's public identity.
pub const fn crate_identity() -> &'static str {
    "sim-lib-expr-tree-serve"
}

/// Returns the loadable components composed by the default product recipe.
pub const fn component_identities() -> [&'static str; 4] {
    [
        "sim-lib-expr-tree",
        "sim-lib-view-expr-tree",
        "sim-lib-expr-tree-server",
        "sim-web-shell",
    ]
}

#[cfg(test)]
mod tests;
