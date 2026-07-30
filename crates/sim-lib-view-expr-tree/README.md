# sim-lib-view-expr-tree

`sim-lib-view-expr-tree` is the bounded, reversible `SurfaceCodec` for a
Mathematica-like expression-tree outline.

The codec consumes one ordinary revisioned snapshot value and emits only
standard SIM Scene nodes. Expanded cells show bounded source and result faces,
freshness, revisions, optional human timestamps, inherited policy badges,
receipt evidence, and actions. Expanded directories carry complete or
explicitly truncated child pages. Collapsed nodes omit their body, so the
snapshot source fetches no descendant or face payload until disclosure.

Desktop and phone arrangements are selected from open `SurfaceCaps` density
metadata. There is no device enum and no expression-tree wire or view/edit
protocol. Standard Intents decode through the same codec to existing
`expr-tree/*` operations with `expr-tree.read`, `expr-tree.write`, or
`expr-tree.calculate` requirements. Disclosure and continuation remain
revision-checked surface-session operations.

```rust
use sim_kernel::{Cx, DefaultFactory, EagerPolicy, Expr};
use sim_lib_view::{SurfaceCodec, surface};
use sim_lib_view_expr_tree::{
    ExpressionTreeSnapshot, ExpressionTreeSurfaceCodec, NodeSnapshot,
};
use std::sync::Arc;

let value = ExpressionTreeSnapshot::new(
    Expr::String("tree:demo".into()),
    1,
    vec![NodeSnapshot::collapsed_dir("/", "root", 1)],
)
.to_expr();
let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
let scene = ExpressionTreeSurfaceCodec::new().encode(
    &mut cx,
    &value,
    &surface::preset("phone").unwrap(),
)?;
# Ok::<(), sim_kernel::Error>(())
```

The authoritative server adapter is responsible for producing snapshot pages
from the live expression-tree session. This crate owns the stable presentation
and reversible intent boundary, not storage, calculation, transport, or browser
session lifetime.
