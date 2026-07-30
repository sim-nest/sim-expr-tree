//! SurfaceCodec identity and reversible contract implementation.

use sim_kernel::{Cx, Diagnostic, Error, Expr, Result, Symbol};
use sim_lib_view::{Draft, Operation, SurfaceCaps, SurfaceCodec};

use crate::{intent, scene};

/// Stable registry id for the expression-tree reversible surface.
pub const EXPRESSION_TREE_SURFACE_CODEC_ID: &str = "surface:expression-tree";

/// Returns the expression-tree surface registry symbol.
pub fn expression_tree_surface_codec_symbol() -> Symbol {
    Symbol::new(EXPRESSION_TREE_SURFACE_CODEC_ID)
}

/// Reversible, stateless codec for a revisioned expression-tree snapshot.
#[derive(Clone, Copy, Debug, Default)]
pub struct ExpressionTreeSurfaceCodec;

impl ExpressionTreeSurfaceCodec {
    /// Builds the expression-tree surface codec.
    pub const fn new() -> Self {
        Self
    }
}

impl SurfaceCodec for ExpressionTreeSurfaceCodec {
    fn encode(&self, _cx: &mut Cx, value: &Expr, caps: &SurfaceCaps) -> Result<Expr> {
        scene::encode(value, caps)
    }

    fn decode(&self, _cx: &mut Cx, value: &Expr, submitted: &Expr) -> Result<Draft> {
        match intent::decode(value, submitted) {
            Ok(command) => Ok(Draft::clean(value.clone(), command)),
            Err(error) => Ok(Draft::rejected(
                value.clone(),
                Diagnostic::error(error.to_string())
                    .with_code(Symbol::qualified("expr-tree-view", "invalid-intent")),
            )),
        }
    }

    fn commit(&self, _cx: &mut Cx, draft: &Draft) -> Result<Operation> {
        if !draft.committable || !draft.diagnostics.is_empty() {
            return Err(Error::HostError(
                "expression-tree draft is not committable".to_owned(),
            ));
        }
        intent::commit(&draft.proposed)
    }
}
