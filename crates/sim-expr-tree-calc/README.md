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
