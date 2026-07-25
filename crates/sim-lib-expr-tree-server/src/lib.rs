#![forbid(unsafe_code)]
//! Server composition scaffold for expression trees.

/// Returns the crate's public scaffold identity.
pub fn crate_identity() -> &'static str {
    "sim-lib-expr-tree-server"
}

/// Returns the scaffold identities this server will compose.
pub fn component_identities() -> [&'static str; 2] {
    [
        sim_lib_expr_tree::crate_identity(),
        sim_lib_view_expr_tree::crate_identity(),
    ]
}

#[cfg(test)]
mod tests {
    #[test]
    fn identity_names_the_server_library() {
        assert_eq!(super::crate_identity(), "sim-lib-expr-tree-server");
        assert_eq!(
            super::component_identities(),
            ["sim-lib-expr-tree", "sim-lib-view-expr-tree"]
        );
    }
}
