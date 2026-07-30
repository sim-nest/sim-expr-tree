//! conformance: bounded reversible expression-tree SurfaceCodec behavior.

mod conformance;
mod intents;
mod limits;
mod rendering;
mod support;

use sim_kernel::Expr;
use sim_lib_view::{SurfaceCodec, surface};

use crate::{
    EXPRESSION_TREE_SURFACE_CODEC_ID, ExpressionTreeSnapshot, ExpressionTreeSurfaceCodec,
    NodeSnapshot, expression_tree_surface_codec_symbol,
};

#[test]
fn surface_codec_is_the_one_reversible_contract() {
    let id = expression_tree_surface_codec_symbol();
    assert_eq!(id.name.as_ref(), EXPRESSION_TREE_SURFACE_CODEC_ID);

    let snapshot = ExpressionTreeSnapshot::new(
        Expr::String("tree:test".to_owned()),
        1,
        vec![NodeSnapshot::collapsed_dir("/", "root", 1)],
    )
    .to_expr();
    let desktop = surface::preset("desktop").expect("desktop caps");
    let mut cx = sim_kernel::testing::eager_cx();
    let scene = ExpressionTreeSurfaceCodec::new()
        .encode(&mut cx, &snapshot, &desktop)
        .expect("surface codec encodes through Scene");
    sim_lib_scene::validate_scene(&scene).expect("encoded value is a standard Scene");
}
