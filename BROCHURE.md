# sim-expr-tree

In one line: `sim-expr-tree` turns named SIM expressions into a durable,
inspectable calculation tree that remains ordinary SIM all the way through.

## What it gives you

The framework combines a finite hierarchical namespace, generated names,
heterogeneous Table/Dir mounts, inherited calculation and codec policy, and a
bounded incremental engine. Its loadable runtime library presents those
capabilities as normal `expr-tree/*` operations with Shapes, Cards,
capabilities, receipts, and standard streams.

Sources remain ordinary `Expr` values and calculated results remain ordinary
`Value` values. General-purpose codecs edit and render them; Citizen records
reconstruct durable source and policy data; live handles keep storage and
authority opaque.

## Why you will be glad

- Build tree-shaped calculated data without adding a private evaluator or
  value model.
- Mix memory, filesystem, database, read-only, and composed mounts behind one
  canonical Table path contract.
- Choose automatic, on-demand, manual, or frozen calculation per subtree.
- Inspect bounded dependency, policy, authority, and receipt evidence without
  triggering evaluation.
- Reopen a named tree and continue from its durable source and control state.
- Discover the complete framework through checked Index claims and runnable
  Lisp recipes.

## Where it fits

Use `sim-lib-expr-tree` whenever a runtime, view, service, or application needs
named calculations with durable policy and explicit storage. Higher product
surfaces delegate to this library; the kernel remains unchanged.
