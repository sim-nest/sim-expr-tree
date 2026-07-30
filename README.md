# sim-expr-tree

`sim-expr-tree` owns SIM's expression-tree framework: finite named source
trees, mixed Table/Dir storage, dependency-aware calculation, loadable runtime
operations, and the surfaces that present or serve those trees. The framework
stays outside `sim-kernel` and composes the ordinary SIM contracts for paths,
values, codecs, Shapes, Citizens, capabilities, and streams.

The repository contains:

- `sim-expr-tree-core`, which owns finite namespace records, generated names,
  store lanes, mount descriptors, and inherited codec policy;
- `sim-expr-tree-calc`, which connects those records to the reusable
  incremental engine and retains ordinary `Expr` sources and `Value` results;
- `sim-lib-expr-tree`, which exports the capability-gated `expr-tree/*`
  operation family as a host-registered runtime library;
- view, server, serve, and bootloader crates that compose the framework into
  user-facing surfaces without duplicating its engine.

The `sim-expr-tree` executable is the ready-to-run default recipe. It boots
through `sim-run-core`, dispatches the loadable `cli/main/expr-tree` entrypoint,
and composes the existing engine, reversible view, authoritative server, and
generic web shell. Standard runtime configuration selects either an in-process
server or an already loaded external `EvalFabric` site:

```toml
[lib/expr-tree-serve]
placement = "in-process"
storage = "expression-tree"
browser-resource = "tree"
web-addr = "127.0.0.1:8787"
atelier-root = ".sim/atelier"
```

Run it with the standard bootloader envelope:

```sh
cargo run -p sim-expr-tree -- --help
cargo run -p sim-expr-tree -- --config-file expr-tree.toml
```

The loadable library is the reusable entry point. It exposes a checked Lisp
surface, a Shape and Card contract for every operation, reconstructable source
and policy Citizens, and opaque live handles whose authority is never
serialized.

Start with the checked recipes:

- `finite-tree` creates generated names, resolves bare, relative, and absolute
  paths, and combines local, database Dir, and read-only Table backends.
- `automatic-and-directed` combines automatic work, manual calculation,
  receipts, cycle recovery, and reopening a named store.
- `web-session` connects the product codec and authoritative server to the
  generic desktop and phone browser stack, including reconnect and failure
  proofs plus deterministic review screenshots.

Run the repository gate with the validation command declared in the SIM
constellation manifest. For focused runtime work:

```sh
cargo test -p sim-lib-expr-tree
cargo run -p xtask -- check-recipes
```
