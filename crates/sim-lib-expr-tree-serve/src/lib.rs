#![forbid(unsafe_code)]
//! Serve entrypoint scaffold for expression trees.

/// Returns the crate's public scaffold identity.
pub fn crate_identity() -> &'static str {
    "sim-lib-expr-tree-serve"
}

/// Returns the server library identity used by this scaffold dependency.
pub fn server_identity() -> &'static str {
    sim_lib_expr_tree_server::crate_identity()
}

#[cfg(test)]
mod tests {
    #[test]
    fn identity_names_the_serve_library() {
        assert_eq!(super::crate_identity(), "sim-lib-expr-tree-serve");
        assert_eq!(super::server_identity(), "sim-lib-expr-tree-server");
    }
}
