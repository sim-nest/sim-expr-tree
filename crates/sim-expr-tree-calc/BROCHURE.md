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
- Mix automatic, on-demand, manual, and frozen cells under inherited policy.
- Direct verification, root forcing, and recursive forcing through one engine.
- Keep authority below an immutable open-time ceiling, with effect evidence.
- Resume bounded fair automatic work from explicit queue continuations.
- Reopen a validated Table-backed graph without repeating current dependency work.
- Rebuild safely when derived state is absent, corrupt, incompatible, or stale.
- Refresh non-watch mounts explicitly from epochs, listings, and entry stamps.
- Consume progress and changes through standard bounded stream operations.
- Explain every attempt from compact revision, authority, dependency, and time receipts.
- Keep dependency tracking outside the kernel while still using SIM policy.
- Test evaluation wrappers with ordinary workspace validation.

## Where it fits

Use this crate above `sim-expr-tree-core` and below the loadable runtime and
server libraries. It is the bridge from stable namespace records to live,
dependency-aware expression-tree sessions.
