#![forbid(unsafe_code)]
//! Core expression-tree record scaffold.

/// Current implementation posture for the core crate.
pub const SCAFFOLD_STATUS: &str = "planned";

/// Returns the crate's public scaffold identity.
pub fn crate_identity() -> &'static str {
    "sim-expr-tree-core"
}

#[cfg(test)]
mod tests {
    #[test]
    fn identity_names_the_core_crate() {
        assert_eq!(super::crate_identity(), "sim-expr-tree-core");
        assert_eq!(super::SCAFFOLD_STATUS, "planned");
    }
}
