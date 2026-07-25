# sim-lib-expr-tree-serve

In one line: `sim-lib-expr-tree-serve` owns the loadable serve entrypoint for expression-tree sessions.

## What it gives you

It is the serving library that product commands can load when expression-tree
behavior needs to run as a session surface. Keeping this as a library leaves the
binary thin, lets bootloader dispatch own startup, and gives server-oriented
behavior a clear place to grow beside the runtime and view crates.

## Why you will be glad

- Add serving behavior without turning the binary into a custom runtime.
- Keep command dispatch compatible with the standard `sim-run` pattern.
- Share core and calculation records with non-server callers.
- Point agents to one crate when they need expression-tree serving code.

## Where it fits

Use this crate between the product binary and the server/runtime libraries. It
is the expression-tree family's loadable serve entrypoint, not a replacement for
the core namespace or calculation crates.
