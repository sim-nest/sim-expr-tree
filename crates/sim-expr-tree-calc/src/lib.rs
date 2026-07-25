#![forbid(unsafe_code)]
//! Calculation layer scaffold for expression trees.

/// Returns the crate's public scaffold identity.
pub fn crate_identity() -> &'static str {
    "sim-expr-tree-calc"
}

/// Returns the core crate identity used by this scaffold dependency.
pub fn core_identity() -> &'static str {
    sim_expr_tree_core::crate_identity()
}

#[cfg(test)]
mod tests {
    #[test]
    fn identity_names_the_calc_crate() {
        assert_eq!(super::crate_identity(), "sim-expr-tree-calc");
        assert_eq!(super::core_identity(), "sim-expr-tree-core");
    }
}
