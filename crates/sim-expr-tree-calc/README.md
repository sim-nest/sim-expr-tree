# sim-expr-tree-calc

`sim-expr-tree-calc` pull-verifies ordinary `sim_kernel::Expr` cell sources and
retains ordinary `sim_kernel::Value` results. It layers expression-tree lookup
policy over `sim-incremental-core`: runtime references record cells, missing
names, lookup steps, listings, mount epochs, inherited policy, codec registry,
and authority observations.

Canonical `as_expr` projections, including Citizen/read-construct values where
available, receive stable fingerprints for value cutoff. Valid values without a
canonical projection remain ordinary values, are labelled volatile, and
conservatively count as changed. Failed recalculations commit failure memos and
make current-result reads fail while retaining a separately labelled
`last-good` value.

Calculation always runs outside the expression-tree state lock. Requested
work, observation, query-depth, expression-depth, and output limits are clamped
to hard host ceilings. The checked specimen covers dependency-first diamond
verification, changing and unchanged branches against full recomputation,
deterministic dynamic cycles and recovery, deep chains, macro and
callable/lambda evaluation, Table, Dir and opaque values, failure recovery,
cancellation, and lock-boundary probes.

Calculation policy inherits field by field from tree to ancestor directories to
cell. Automatic, on-demand, manual, and frozen triggers use the same verify,
force-roots, and force-recursive engine. Automatic mutations enter a bounded,
debounced, priority-aware queue with bounded-bypass fairness; queue snapshots
and incremental continuation tokens make unfinished work explicit and
restartable.

Opening a calculator captures one immutable `CapabilitySet` ceiling. Cell
authority can only shrink through allow intersections and accumulated denials,
and required capabilities are checked before evaluation. Each fresh evaluation
context is diminished to that effective set, while ordinary kernel effect
ledger records are summarized into the calculation receipt.

Every attempt produces a bounded receipt containing source, policy, authority,
dependency, logical tick, optional wall-clock, effect, result fingerprint,
outcome, and reason evidence. `explain` reads that evidence without evaluating.
Progress and changes flow through `sim-lib-stream-core::StreamValue` with an
explicit `BufferPolicy`, observable overflow, ordinary stream `next`, and
cancellation.
