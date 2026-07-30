use sim_kernel::{Cx, Expr, Object, ObjectCompat, Result};

/// Unevaluated source transported through the ordinary callable boundary.
pub(crate) struct RawSource(pub(crate) Expr);

impl Object for RawSource {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<expr-tree-source>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for RawSource {
    fn as_expr(&self, _cx: &mut Cx) -> Result<Expr> {
        Ok(self.0.clone())
    }
}
