# sim-lib-view-expr-tree

In one line: `sim-lib-view-expr-tree` is the view projection library for expression-tree sessions.

## What it gives you

It owns the SurfaceCodec-facing projection for expression-tree data, so
tree sessions can become inspectable views without inventing a separate UI data
model. The crate keeps view behavior beside the rest of the expression-tree
family and leaves core naming, calculation, runtime exports, and server sessions
in their own crates.

## Why you will be glad

- Present expression-tree state through the standard SIM view/edit surface.
- Keep projection code separate from storage and server session mechanics.
- Reuse the same durable names and dependency records as other surfaces.
- Give view-related Index rows a precise owning crate.

## Where it fits

This crate sits at the presentation edge of the expression-tree family. Use it
when the task is about browsing, projecting, or editing expression-tree state
through SIM surface protocols.
