# sim-lib-expr-tree-server

`sim-lib-expr-tree-server` is the authoritative server backend for live SIM
expression trees. It exports `site/sim-lib-expr-tree-server` as a loadable
placement site and implements both the standard `EvalSite` frame boundary and
the kernel `EvalFabric` realization boundary.

The server owns a bounded registry of opaque session ids and live
`TreeHandle`s. Every request runs through the caller's `Cx`, so the existing
`expr-tree/*` capability checks remain authoritative. The server also accepts
the operations produced by `ExpressionTreeSurfaceCodec` and the generic
`web-session/*` request maps used by server-backed surfaces. It does not add a
tree-specific transport or evaluator.

Each session provides:

- optimistic snapshot revisions and one-time bounded continuation tokens;
- explicit session, idle-tick, page, depth, watch, and queue limits;
- independent bounded watches with lifetime overflow evidence;
- cancellation for watches and whole sessions;
- structured remote errors that keep a connected client live;
- mandatory monotone logical ticks on changes; and
- optional injected `WallClock` observations used only as human evidence.

Wall time is never compared for freshness, optimistic concurrency, or idle
expiry. Source, policy, calculation, receipts, and live authority remain in the
server-owned expression-tree runtime. Browser and transport adapters carry
only bounded snapshots, Intents, operations, and change records.

Run the focused conformance specimen with:

```sh
cargo test -p sim-lib-expr-tree-server
```
