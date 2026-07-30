use sim_kernel::{Expr, Symbol};
use sim_lib_intent::{Origin, intent};
use sim_lib_view::{SurfaceCodec, surface};
use sim_value::{access, build};

use crate::{
    ChildPage, ExpressionTreeSnapshot, ExpressionTreeSurfaceCodec, Freshness, NodeSnapshot,
};

use super::support::{TREE_REVISION, collect_kind, expanded_cell, snapshot, string_field, target};

#[test]
fn collapsed_directory_does_not_inspect_descendant_payload() {
    let hostile_child = build::map(vec![
        ("node-type", build::sym("cell")),
        ("path", build::text("/hidden")),
        (
            "name",
            build::text("this descendant must never be inspected while collapsed"),
        ),
        ("revision", build::uint(TREE_REVISION)),
        ("open", Expr::Bool(true)),
        (
            "body",
            Expr::Extension {
                tag: Symbol::qualified("hostile", "descendant"),
                payload: Box::new(Expr::Nil),
            },
        ),
    ]);
    let collapsed = build::map(vec![
        ("node-type", build::sym("directory")),
        ("path", build::text("/")),
        ("name", build::text("root")),
        ("revision", build::uint(TREE_REVISION)),
        ("open", Expr::Bool(false)),
        (
            "body",
            build::map(vec![
                ("page-state", build::sym("complete")),
                ("nodes", build::list(vec![hostile_child])),
            ]),
        ),
    ]);
    let value = build::map(vec![
        (
            "type",
            Expr::Symbol(Symbol::qualified(
                "expr-tree-view",
                "expression-tree-snapshot",
            )),
        ),
        ("tree", build::text("tree:workbook")),
        ("revision", build::uint(TREE_REVISION)),
        ("nodes", build::list(vec![collapsed])),
    ]);
    let mut cx = sim_kernel::testing::eager_cx();
    let scene = ExpressionTreeSurfaceCodec::new()
        .encode(
            &mut cx,
            &value,
            &surface::preset("desktop").expect("desktop caps"),
        )
        .expect("collapsed node never touches hostile body");
    let mut trees = Vec::new();
    collect_kind(&scene, "tree", &mut trees);
    assert_eq!(trees.len(), 1);
    assert_eq!(access::field_bool(trees[0], "open"), Some(false));
}

#[test]
fn truncated_subtree_exposes_and_requires_explicit_continuation() {
    let directory = NodeSnapshot::expanded_dir(
        "/",
        "root",
        TREE_REVISION,
        ChildPage::Truncated {
            nodes: vec![expanded_cell("/shown", "shown", Freshness::Fresh)],
            continuation: "page:2:opaque".to_owned(),
            remaining: Some(41),
        },
    );
    let value =
        ExpressionTreeSnapshot::new(build::text("tree:workbook"), TREE_REVISION, vec![directory])
            .to_expr();
    let mut cx = sim_kernel::testing::eager_cx();
    let codec = ExpressionTreeSurfaceCodec::new();
    let scene = codec
        .encode(
            &mut cx,
            &value,
            &surface::preset("desktop").expect("desktop caps"),
        )
        .expect("truncated page encodes");
    let mut boxes = Vec::new();
    collect_kind(&scene, "box", &mut boxes);
    let continuations = boxes
        .into_iter()
        .filter(|item| {
            access::field_sym(item, "role").is_some_and(|role| role.name.as_ref() == "continuation")
        })
        .collect::<Vec<_>>();
    assert_eq!(continuations.len(), 1);
    assert_eq!(
        string_field(continuations[0], "label"),
        Some("More descendants require explicit continuation")
    );

    let mut buttons = Vec::new();
    collect_kind(&scene, "button", &mut buttons);
    let more = buttons
        .iter()
        .find(|button| string_field(button, "control") == Some("continue"))
        .expect("explicit Load more button");
    let continuation_target = access::field(more, "target")
        .cloned()
        .expect("continuation target");
    assert_eq!(
        access::field_str(&continuation_target, "continuation"),
        Some("page:2:opaque")
    );

    let submitted = intent(
        "tap",
        Origin::human(1),
        vec![
            ("target", continuation_target),
            ("control", build::text("continue")),
        ],
    );
    let draft = codec.decode(&mut cx, &value, &submitted).expect("decode");
    assert!(draft.committable);
    let operation = codec.commit(&mut cx, &draft).expect("commit");
    assert_eq!(
        access::field_sym(&operation.form, "op")
            .expect("continuation operation")
            .as_qualified_str(),
        "expr-tree-view/continue"
    );

    let implicit = intent(
        "tap",
        Origin::human(2),
        vec![
            ("target", target("/", TREE_REVISION)),
            ("control", build::text("continue")),
        ],
    );
    let rejected = codec.decode(&mut cx, &value, &implicit).expect("decode");
    assert!(!rejected.committable);
}

#[test]
fn typed_collapsed_directory_serializes_no_descendants() {
    let value = snapshot(vec![NodeSnapshot::collapsed_dir(
        "/",
        "root",
        TREE_REVISION,
    )]);
    let nodes = match access::field(&value, "nodes") {
        Some(Expr::List(nodes)) => nodes,
        other => panic!("snapshot nodes: {other:?}"),
    };
    assert_eq!(access::field(&nodes[0], "body"), Some(&Expr::Nil));
}
