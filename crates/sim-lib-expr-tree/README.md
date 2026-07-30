# sim-lib-expr-tree

`sim-lib-expr-tree` is the loadable runtime library for the SIM expression-tree
framework. Loading `ExprTreeLib` registers the stable `expr-tree/*` function
family, argument and result Shapes, browseable operation Cards, and the
reconstructable source and policy classes.

## Runtime surface

The library exports:

- lifecycle and namespace operations: `open`, `new-cell`, `new-dir`, `mount`,
  `unmount`, `move`, `rename`, and `delete`;
- source and policy operations: `set-expr`, `set-calc-policy`, and
  `set-codec-policy`;
- reads and calculation: `ref`, `list`, `calculate`, `recalculate`,
  `recalculate-recursive`, `cancel`, `refresh`, `status`, and `explain`;
- the bounded standard stream returned by `watch`.

Each function checks a declared capability before touching a tree:
`expr-tree.read`, `expr-tree.write`, `expr-tree.calculate`, or
`expr-tree.mount`. Arity, path, collection, tree-size, and diagnostic bounds
keep the public failure surface finite. The Lisp surface preserves the final
source argument to `new-cell` and `set-expr` as an unevaluated expression, so a
source such as `(expr-tree/ref "/base")` becomes a dependency rather than an
eager call.

`DurableSourceRecord` and `DurablePolicyRecord` participate in Citizen
reconstruction. They contain authored data only. `TreeHandle` deliberately
stays opaque because it carries a live writer scope, backend access, and the
session's authority boundary.

## Use

```rust
use sim_lib_expr_tree::install_expr_tree_lib;

# fn install(cx: &mut sim_kernel::Cx) -> sim_kernel::Result<()> {
install_expr_tree_lib(cx)?;
# Ok(())
# }
```

The embedding runtime also installs `codec/lisp` and grants only the
capabilities its caller is allowed to exercise. The `finite-tree` and
`automatic-and-directed` recipes under `recipes/` are compiled into the crate
and executed verbatim by focused tests.

```sh
cargo test -p sim-lib-expr-tree
cargo run -p xtask -- check-recipes
```
