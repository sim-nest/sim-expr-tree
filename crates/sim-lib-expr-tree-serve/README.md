# sim-lib-expr-tree-serve

Loadable product composition for the expression-tree engine, reversible view,
authoritative server, and generic web host. `ExpressionTreeRecipe` opens the
configured tree through `EvalFabric`, injects an
`ExpressionTreeWebSurfaceFactory` into `sim-web-shell`, and closes the
authoritative session when the host stops.

`ExprTreeServeLib` exports `cli/main/expr-tree` for a standard `sim-run-core`
`Bootloader`. It reads the already merged `lib/expr-tree-serve` runtime config
table. In-process placement owns an `ExpressionTreeServer`; external placement
resolves an already loaded site implementing `EvalFabric`. Both placements use
the same session and browser-transport path.

The default product host grants expression-tree read, write, and calculation
capabilities. Mount and network powers stay denied. The executable crate adds
no argument parser and constructs no ambient runtime context.
