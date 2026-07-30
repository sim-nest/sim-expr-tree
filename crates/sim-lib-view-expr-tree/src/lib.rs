#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Bounded, reversible expression-tree outline over SIM's standard view stack.
//!
//! [`ExpressionTreeSurfaceCodec`] implements
//! [`sim_lib_view::SurfaceCodec`]. Its input is an ordinary
//! [`ExpressionTreeSnapshot`] value: collapsed nodes carry no fetched child or
//! face payload, while expanded child pages are either complete or end in an
//! explicit continuation. The codec projects that one value to desktop and
//! phone layouts from [`sim_lib_view::SurfaceCaps`], decodes standard Intents,
//! and commits them to existing `expr-tree/*` operations with their declared
//! capabilities.

mod budget;
mod codec;
mod intent;
mod model;
mod scene;

pub use codec::{
    EXPRESSION_TREE_SURFACE_CODEC_ID, ExpressionTreeSurfaceCodec,
    expression_tree_surface_codec_symbol,
};
pub use model::{
    ChildPage, ExpressionTreeSnapshot, FaceSnapshot, FaceState, Freshness, NodeDetail,
    NodeSnapshot, ReceiptSummary, TimestampSummary,
};

#[cfg(test)]
mod tests;

/// Returns the crate's public view-library identity.
pub const fn crate_identity() -> &'static str {
    "sim-lib-view-expr-tree"
}

/// Returns the runtime library identity composed by this view.
pub fn runtime_identity() -> &'static str {
    sim_lib_expr_tree::crate_identity()
}
