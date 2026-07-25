# sim-expr-tree

`sim-expr-tree` is the public SIM repository reserved for the expression-tree
product family. This initial scaffold declares the ownership boundary and crate
layout without shipping domain behavior.

Planned components:

- `sim-expr-tree-core`: finite namespace and stored tree records.
- `sim-expr-tree-calc`: calculation and dependency observation layer.
- `sim-lib-expr-tree`: loadable runtime library.
- `sim-lib-view-expr-tree`: expression-tree surface projection library.
- `sim-lib-expr-tree-server`: server composition library.
- `sim-lib-expr-tree-serve`: serve entrypoint library.
- `sim-expr-tree`: thin bootloader binary crate.

Feature claims, routes, and specimens are added only when checked behavior lands.
