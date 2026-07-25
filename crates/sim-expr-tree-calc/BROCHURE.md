# sim-expr-tree-calc

In one line: `sim-expr-tree-calc` connects expression-tree names to dependency-aware refresh behavior.

## What it gives you

It observes expression evaluation, records which named paths were read, and uses
the incremental query substrate to dirty dependents when names are created,
renamed, moved, listed, remounted, or governed by a changed policy. The crate is
the calculation layer for the expression-tree family, not a separate language or
parallel store.

## Why you will be glad

- Recompute only the tree results whose observed dependencies changed.
- Treat explicit names, fallback lookup, bindings, and mounts as refresh inputs.
- Keep dependency tracking outside the kernel while still using SIM policy.
- Test evaluation wrappers with ordinary workspace validation.

## Where it fits

Use this crate above `sim-expr-tree-core` and below the loadable runtime and
server libraries. It is the bridge from stable namespace records to live,
dependency-aware expression-tree sessions.
