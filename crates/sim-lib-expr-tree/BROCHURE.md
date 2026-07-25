# sim-lib-expr-tree

In one line: `sim-lib-expr-tree` is the loadable runtime library for expression-tree operations.

## What it gives you

It gives the expression-tree family a runtime crate where callable exports,
library claims, and object-facing behavior can live without moving core records
into the kernel. The library is intentionally separate from the server and view
crates, so local runtime use can stay small while served and projected surfaces
compose it.

## Why you will be glad

- Load expression-tree behavior as an ordinary SIM library.
- Keep reusable runtime exports independent of command and server code.
- Share the same core namespace and calculation substrate as every surface.
- Give framework rows a clear owner for callable behavior.

## Where it fits

Use this crate above `sim-expr-tree-core` and `sim-expr-tree-calc` when a SIM
runtime needs expression-tree operations. Server, serve, view, and binary crates
should delegate here instead of duplicating runtime behavior.
