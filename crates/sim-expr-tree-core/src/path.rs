use sim_table_core::{TablePath, TablePathRef, TablePathRefError};

/// Resolve a canonical expression-tree namespace reference.
///
/// This crate deliberately reuses `sim-table-core` path references and does not
/// define a local path parser.
pub fn resolve_namespace_path(
    base: &TablePath,
    reference: &TablePathRef,
) -> Result<TablePath, TablePathRefError> {
    base.resolve(reference)
}
