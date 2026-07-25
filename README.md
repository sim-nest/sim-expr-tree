# sim-expr-tree

`sim-expr-tree` is the public SIM repository for the expression-tree product
family. It starts with backend-neutral finite namespace records and grows toward
calculation, runtime, surface, server, and product crates in the planned layers.

Planned components:

- `sim-expr-tree-core`: finite namespace, policy, and stored tree records.
- `sim-expr-tree-calc`: calculation and dependency observation layer.
- `sim-lib-expr-tree`: loadable runtime library.
- `sim-lib-view-expr-tree`: expression-tree surface projection library.
- `sim-lib-expr-tree-server`: server composition library.
- `sim-lib-expr-tree-serve`: serve entrypoint library.
- `sim-expr-tree`: thin bootloader binary crate.

Feature claims, routes, and specimens are tied to checked behavior.
