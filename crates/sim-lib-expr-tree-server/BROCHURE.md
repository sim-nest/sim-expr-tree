# sim-lib-expr-tree-server

In one line: `sim-lib-expr-tree-server` gives expression trees a real,
loadable, authority-preserving home on SIM's standard server fabric.

## What it gives you

The crate turns the reusable expression-tree runtime and reversible outline
codec into authoritative long-lived sessions. One site answers ordinary
`EvalFabric` requests, standard `ServerFrame` requests, checked surface
operations, and generic server-backed web-session requests.

Sessions have opaque ids, optimistic revisions, logical-time idle expiry,
bounded directory continuations, cancellable bounded watches, overflow
evidence, and structured errors. Optional injected wall observations make
human timestamps useful without making clock rollback a correctness event.

## Why you will be glad

- Keep source, calculation, policy, receipts, and authority on the server.
- Reuse `EvalSite`, `EvalFabric`, `ServerFrame`, `SurfaceCodec`, and transport
  adapters instead of owning a product protocol.
- Inject an expression-tree live surface into the generic web shell without
  product JavaScript or a forked Scene interpreter.
- Share a session across reconnecting clients while isolating unrelated trees.
- Reject stale edits and diminished authority without disconnecting the client.
- Bound every session and watch lifecycle with visible overflow evidence.
- Test deterministic time and restart behavior without ambient clock reads.

## Where it fits

Use this site when a desktop, phone, CLI, or agent needs to collaborate on an
expression tree through `realize`. The browser renders and submits reversible
operations; the server remains the sole authority for durable and calculated
state.
