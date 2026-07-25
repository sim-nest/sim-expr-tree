#![forbid(unsafe_code)]
//! Loadable runtime library scaffold for expression trees.

/// Returns the crate's public scaffold identity.
pub fn crate_identity() -> &'static str {
    "sim-lib-expr-tree"
}

/// Returns the lower-layer scaffold identities.
pub fn component_identities() -> [&'static str; 2] {
    [
        sim_expr_tree_core::crate_identity(),
        sim_expr_tree_calc::crate_identity(),
    ]
}

#[cfg(test)]
mod tests {
    #[test]
    fn identity_names_the_runtime_library() {
        assert_eq!(super::crate_identity(), "sim-lib-expr-tree");
        assert_eq!(
            super::component_identities(),
            ["sim-expr-tree-core", "sim-expr-tree-calc"]
        );
    }
}
