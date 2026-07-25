#![forbid(unsafe_code)]
//! View library scaffold for expression trees.

/// Returns the crate's public scaffold identity.
pub fn crate_identity() -> &'static str {
    "sim-lib-view-expr-tree"
}

/// Returns the runtime library identity used by this scaffold dependency.
pub fn runtime_identity() -> &'static str {
    sim_lib_expr_tree::crate_identity()
}

#[cfg(test)]
mod tests {
    #[test]
    fn identity_names_the_view_library() {
        assert_eq!(super::crate_identity(), "sim-lib-view-expr-tree");
        assert_eq!(super::runtime_identity(), "sim-lib-expr-tree");
    }
}
