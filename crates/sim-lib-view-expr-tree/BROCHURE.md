# sim-lib-view-expr-tree

In one line: `sim-lib-view-expr-tree` is a bounded reversible SurfaceCodec for expression-tree sessions.

## What it gives you

It projects one revisioned expression-tree snapshot into an expandable standard
Scene outline and decodes the matching standard Intents back into existing
`expr-tree/*` operations. A cell row can expose source and result faces,
freshness, revisions, optional human timestamps, policy badges, receipt
evidence, and actions without stringifying an unbounded runtime value.

## Why you will be glad

- Collapsed nodes fetch no descendant or face payload.
- Truncated subtrees end in visible evidence and require an explicit opaque
  continuation token.
- Desktop and phone arrangements follow open `SurfaceCaps`, not a device enum.
- Source edits, namespace mutations, calculation, cancellation, policy, and
  explanation carry the existing read/write/calculate capability requirements.
- Stale snapshot revisions and malformed or unauthorized intent paths fail
  closed before an operation is committed.

## Where it fits

Use it at the presentation edge of the expression-tree family. The server owns
the live tree and supplies bounded snapshot pages; the browser and remote bridge
continue to speak standard Scene, Intent, SurfaceCodec, and session contracts.
