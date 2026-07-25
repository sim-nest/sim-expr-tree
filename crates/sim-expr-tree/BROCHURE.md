# sim-expr-tree

In one line: the `sim-expr-tree` binary crate provides the product entrypoint for expression-tree commands.

## What it gives you

It is the thin executable surface for the expression-tree family. The binary
keeps command ownership in the product repo while delegating behavior to the
loadable runtime and serve libraries, so command entrypoints can boot
through the standard SIM loader instead of constructing a private runtime.

## Why you will be glad

- Keep command naming stable while implementation lives in libraries.
- Avoid baking server or runtime behavior into a standalone binary.
- Give recipes and docs one product-level command to reference.
- Preserve the same branch and validation flow as the rest of the family.

## Where it fits

This crate sits at the edge of the expression-tree stack. Core records,
calculation, runtime exports, server sessions, and view projection stay in their
own crates; the binary is only the product command surface.
