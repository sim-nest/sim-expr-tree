# sim-lib-expr-tree-server

In one line: `sim-lib-expr-tree-server` is the session-oriented server library for expression-tree work.

## What it gives you

It provides the public home for expression-tree server sessions, separate from
the core records and the product binary. That boundary lets served state,
connection handling, and collaboration flows evolve as loadable behavior
while continuing to reuse the same namespace, policy, and calculation crates.

## Why you will be glad

- Keep server behavior out of the kernel and out of the thin binary.
- Share expression-tree identity and policy with local runtime callers.
- Give session tests a stable crate boundary.
- Make served views discoverable through the Index and brochure lanes.

## Where it fits

This crate sits above `sim-expr-tree-core` and `sim-expr-tree-calc`, beside the
runtime library, and below the serve entrypoint. Use it when the work is about
long-lived expression-tree sessions.
