use sim_kernel::CapabilityName;

/// Capability required for non-evaluating tree inspection and watches.
pub fn expr_tree_read_capability() -> CapabilityName {
    CapabilityName::new("expr-tree.read")
}

/// Capability required for namespace, source, and policy mutation.
pub fn expr_tree_write_capability() -> CapabilityName {
    CapabilityName::new("expr-tree.write")
}

/// Capability required for directed calculation, cancellation, and refresh.
pub fn expr_tree_calculate_capability() -> CapabilityName {
    CapabilityName::new("expr-tree.calculate")
}

/// Capability required to attach or remove a Table/Dir backend.
pub fn expr_tree_mount_capability() -> CapabilityName {
    CapabilityName::new("expr-tree.mount")
}
