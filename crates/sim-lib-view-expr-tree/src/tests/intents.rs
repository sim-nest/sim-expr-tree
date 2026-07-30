use sim_kernel::Expr;
use sim_lib_intent::{Origin, intent};
use sim_lib_view::{Operation, SurfaceCodec};
use sim_value::{access, build};

use crate::{ExpressionTreeSurfaceCodec, Freshness};

use super::support::{TREE_REVISION, expanded_cell, snapshot, target, target_with_request};

#[test]
fn standard_intents_compile_every_expression_tree_action() {
    let base = snapshot(vec![expanded_cell("/solve", "solve", Freshness::Fresh)]);
    let cases = vec![
        (
            intent(
                "tree-disclosure",
                Origin::human(1),
                vec![
                    ("target", target("/solve", TREE_REVISION)),
                    ("open", Expr::Bool(false)),
                ],
            ),
            "disclose",
            "expr-tree.read",
        ),
        (
            intent(
                "edit-field",
                Origin::human(2),
                vec![
                    ("target", target("/solve", TREE_REVISION)),
                    ("path", build::list(vec![build::text("source")])),
                    ("value", build::text("(+ 1 2)")),
                ],
            ),
            "set-expr",
            "expr-tree.write",
        ),
        (
            intent(
                "create",
                Origin::human(3),
                vec![
                    ("class", build::sym("cell")),
                    ("at", target("/", TREE_REVISION)),
                    (
                        "args",
                        build::list(vec![build::text("new-cell"), build::text("source")]),
                    ),
                ],
            ),
            "new-cell",
            "expr-tree.write",
        ),
        (
            intent(
                "create",
                Origin::human(4),
                vec![
                    ("class", build::sym("directory")),
                    ("at", target("/", TREE_REVISION)),
                    ("args", build::list(vec![build::text("new-dir")])),
                ],
            ),
            "new-dir",
            "expr-tree.write",
        ),
        (
            intent(
                "move",
                Origin::human(5),
                vec![
                    ("node", target("/solve", TREE_REVISION)),
                    ("at", target("/archive/solve", TREE_REVISION)),
                ],
            ),
            "move",
            "expr-tree.write",
        ),
        (
            intent(
                "delete",
                Origin::human(6),
                vec![(
                    "targets",
                    build::list(vec![target("/solve", TREE_REVISION)]),
                )],
            ),
            "delete",
            "expr-tree.write",
        ),
        (
            invoke("/solve", "calculate"),
            "calculate",
            "expr-tree.calculate",
        ),
        (
            invoke("/solve", "recalculate"),
            "recalculate",
            "expr-tree.calculate",
        ),
        (
            invoke("/solve", "recalculate-recursive"),
            "recalculate-recursive",
            "expr-tree.calculate",
        ),
        (
            intent(
                "cancel",
                Origin::human(10),
                vec![
                    ("pane", build::text("main")),
                    ("target", target_with_request("/solve", TREE_REVISION, 9)),
                ],
            ),
            "cancel",
            "expr-tree.calculate",
        ),
        (
            intent(
                "set-param",
                Origin::human(11),
                vec![
                    ("target", target("/solve", TREE_REVISION)),
                    ("param", build::sym("calc-policy")),
                    ("value", build::sym("manual")),
                ],
            ),
            "set-calc-policy",
            "expr-tree.write",
        ),
        (
            intent(
                "set-param",
                Origin::human(12),
                vec![
                    ("target", target("/solve", TREE_REVISION)),
                    ("param", build::sym("codec-policy")),
                    ("value", build::text("codec/json")),
                ],
            ),
            "set-codec-policy",
            "expr-tree.write",
        ),
        (invoke("/solve", "explain"), "explain", "expr-tree.read"),
    ];

    for (submitted, expected_action, expected_capability) in cases {
        let operation = decode_commit(&base, &submitted);
        assert_eq!(operation_name(&operation), expected_action);
        assert_eq!(
            operation.required_capabilities.as_slice(),
            [sim_kernel::CapabilityName::new(expected_capability)]
        );
    }
}

#[test]
fn scene_tap_controls_decode_through_the_same_surface_contract() {
    let base = snapshot(vec![expanded_cell("/solve", "solve", Freshness::Pending)]);
    for (control, action, capability) in [
        ("calculate", "calculate", "expr-tree.calculate"),
        ("recalculate", "recalculate", "expr-tree.calculate"),
        (
            "recalculate-recursive",
            "recalculate-recursive",
            "expr-tree.calculate",
        ),
        ("policy", "open-policy", "expr-tree.read"),
        ("explain", "explain", "expr-tree.read"),
        ("cancel", "cancel", "expr-tree.calculate"),
    ] {
        let submitted = intent(
            "tap",
            Origin::human(20),
            vec![
                ("target", target_with_request("/solve", TREE_REVISION, 9)),
                ("control", build::text(control)),
            ],
        );
        let operation = decode_commit(&base, &submitted);
        assert_eq!(operation_name(&operation), action);
        assert_eq!(
            operation.required_capabilities,
            vec![sim_kernel::CapabilityName::new(capability)]
        );
    }
}

fn invoke(path: &str, op: &str) -> Expr {
    intent(
        "invoke",
        Origin::human(8),
        vec![
            ("target", target(path, TREE_REVISION)),
            ("op", build::sym(op)),
            ("args", build::list(Vec::new())),
        ],
    )
}

fn decode_commit(base: &Expr, submitted: &Expr) -> Operation {
    let mut cx = sim_kernel::testing::eager_cx();
    let codec = ExpressionTreeSurfaceCodec::new();
    let draft = codec
        .decode(&mut cx, base, submitted)
        .expect("decode result");
    assert!(
        draft.committable,
        "Intent was rejected: {:?}",
        draft.diagnostics
    );
    codec.commit(&mut cx, &draft).expect("commit operation")
}

fn operation_name(operation: &Operation) -> String {
    match &operation.form {
        Expr::Call { operator, .. } => match operator.as_ref() {
            Expr::Symbol(symbol) => symbol.name.to_string(),
            _ => panic!("runtime operation has non-symbol operator"),
        },
        form @ Expr::Map(_) => access::field_sym(form, "op")
            .map(|symbol| symbol.name.to_string())
            .expect("local operation name"),
        other => panic!("unexpected operation form {other:?}"),
    }
}
