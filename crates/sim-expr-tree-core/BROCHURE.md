# sim-expr-tree-core

In one line: `sim-expr-tree-core` gives expression trees stable names, immutable identities, inherited policy, and crash-safe reservation.

## What it gives you

It owns the finite namespace records for expression-tree storage: path
references, generated-name reservation, rename and move semantics, mount epochs,
source-control stamps, typed adapters, read-only mounts, and policy inheritance.
The crate keeps table leaves as table leaves, detects corrupt state, and makes
finite reads safe without allocating new namespace state.

## Why you will be glad

- Preserve identity while names move through a tree.
- Recover cleanly from gaps created by crashes or partial writes.
- Keep policy and mount changes observable to higher calculation layers.
- Keep source/control recovery separate from safely rebuildable derived graphs.
- Reuse the standard table substrate instead of inventing a storage island.

## Where it fits

This is the substrate crate for the expression-tree family. Calculation, runtime
library, server, and view crates depend on it when they need durable names and
policy-aware tree records.
