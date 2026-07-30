use sim_citizen::CitizenRegistry;
use sim_citizen_derive::Citizen;
use sim_kernel::{Expr, Result, Symbol};

/// Reconstructable authored source record.
///
/// This is data only. It intentionally carries no tree handle, writer lane,
/// capability set, Table/Dir backend, calculator frame, or stream endpoint.
#[derive(Clone, Debug, PartialEq, Citizen)]
#[citizen(symbol = "expr-tree/SourceRecord", version = 1)]
pub struct DurableSourceRecord {
    /// Canonical absolute cell path.
    pub path: String,
    /// Exact authored expression.
    pub source: Expr,
    /// Monotone source revision observed by the runtime.
    pub revision: u64,
}

impl Default for DurableSourceRecord {
    fn default() -> Self {
        Self {
            path: "/cell-1".to_owned(),
            source: Expr::Nil,
            revision: 0,
        }
    }
}

/// Reconstructable durable calculation and codec policy record.
///
/// Capability ceilings remain live session authority and are never encoded in
/// this record.
#[derive(Clone, Debug, Default, PartialEq, Citizen)]
#[citizen(symbol = "expr-tree/PolicyRecord", version = 1)]
pub struct DurablePolicyRecord {
    /// Canonical absolute policy owner path (`/` for tree policy).
    pub path: String,
    /// Stable trigger spelling: automatic, on-demand, manual, or frozen.
    pub calc_trigger: String,
    /// Optional installed source-codec symbol spelling.
    pub source_codec: Option<String>,
    /// Optional installed result-codec symbol spelling.
    pub result_codec: Option<String>,
}

/// Class symbol for [`DurableSourceRecord`].
pub fn durable_source_class_symbol() -> Symbol {
    Symbol::qualified("expr-tree", "SourceRecord")
}

/// Class symbol for [`DurablePolicyRecord`].
pub fn durable_policy_class_symbol() -> Symbol {
    Symbol::qualified("expr-tree", "PolicyRecord")
}

/// Builds the explicit, dead-code-elimination-safe Citizen registry.
pub fn expr_tree_citizen_registry() -> Result<CitizenRegistry> {
    let mut registry = CitizenRegistry::new();
    registry
        .register::<DurableSourceRecord>()?
        .register::<DurablePolicyRecord>()?;
    Ok(registry)
}
