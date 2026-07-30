# sim-lib-expr-tree

In one line: `sim-lib-expr-tree` gives any SIM runtime a checked, loadable
expression-tree engine.

## What it gives you

Load one library and receive the stable `expr-tree/*` operation family,
per-operation Shapes and Cards, explicit read/write/calculate/mount
capabilities, durable source and policy Citizens, bounded receipts, and a
standard change stream. The same surface creates finite namespaces, composes
mixed storage, calculates dependencies, applies inherited codec policy, and
reopens named trees.

## Why you will be glad

- Keep ordinary `Expr` sources and ordinary `Value` results.
- Use one canonical path syntax for bare, relative, and absolute references.
- Retain raw Lisp source calls as dependencies instead of evaluating them on
  entry.
- Inspect operation contracts and capability requirements before invocation.
- Reconstruct durable records without serializing live handles or backend
  authority.
- Verify real workflows through the embedded, cargo-checked Lisp recipes.

## Where it fits

Compose this library above `sim-expr-tree-core` and `sim-expr-tree-calc`.
Views, servers, and product entry points load or delegate to it instead of
reimplementing namespace or calculation behavior.
