#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Authoritative expression-tree sessions over SIM's standard server surfaces.
//!
//! [`ExpressionTreeServer`] is both a [`sim_lib_server::EvalSite`] and a
//! [`sim_kernel::EvalFabric`]. It owns bounded live tree sessions, evaluates
//! expression-tree operations with the caller's context and capabilities,
//! accepts checked [`sim_lib_view::SurfaceCodec`] operations, and exposes
//! bounded revisioned snapshots and watches. Optional server wall-clock
//! observations accompany mandatory logical ticks but never decide freshness,
//! expiry, or optimistic concurrency.

mod error;
mod model;
mod protocol;
mod server;
mod session;
mod site;

pub use error::ExpressionTreeServerError;
pub use model::{ChangeEvent, ExpressionTreeServerLimits, SessionId, WatchBatch, WatchId};
pub use server::ExpressionTreeServer;
pub use site::{
    ExpressionTreeServerLib, expr_tree_server_lib_symbol, expr_tree_server_site_symbol,
    install_expr_tree_server_lib,
};

/// Returns the crate's public server-library identity.
pub const fn crate_identity() -> &'static str {
    "sim-lib-expr-tree-server"
}

/// Returns the runtime and surface identities composed by this server.
pub fn component_identities() -> [&'static str; 2] {
    [
        sim_lib_expr_tree::crate_identity(),
        sim_lib_view_expr_tree::crate_identity(),
    ]
}

#[cfg(test)]
mod tests;
