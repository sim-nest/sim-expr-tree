use sim_kernel::{CapabilityName, Expr};
use sim_lib_intent::{Origin, intent};
use sim_lib_view::{SurfaceCodec, surface};
use sim_value::{access, build};

use crate::{
    ExpressionTreeSurfaceCodec, FaceSnapshot, Freshness, NodeDetail, NodeSnapshot, TimestampSummary,
};

use super::support::{TREE_REVISION, collect_kind, detail, expanded_cell, snapshot, target};

#[test]
fn every_freshness_state_has_visible_non_color_status_text() {
    let cases = [
        (Freshness::NeverCalculated, "Never calculated"),
        (Freshness::Fresh, "Fresh"),
        (Freshness::MaybeStale, "Maybe stale"),
        (Freshness::Pending, "Pending"),
        (Freshness::Failed, "Failed"),
        (Freshness::Frozen, "Frozen"),
        (Freshness::Blocked, "Blocked"),
    ];
    let codec = ExpressionTreeSurfaceCodec::new();
    let caps = surface::preset("desktop").expect("desktop caps");
    let mut cx = sim_kernel::testing::eager_cx();
    for (freshness, expected) in cases {
        let value = snapshot(vec![expanded_cell("/cell", "cell", freshness)]);
        let scene = codec.encode(&mut cx, &value, &caps).expect("status Scene");
        let mut badges = Vec::new();
        collect_kind(&scene, "badge", &mut badges);
        let status = badges.first().expect("status badge");
        assert_eq!(access::field_str(status, "label"), Some(expected));
        assert_eq!(
            access::field_str(status, "aria-label"),
            Some(format!("Calculation status: {}", freshness.token()).as_str())
        );
    }
}

#[test]
fn arbitrary_binary_and_truncated_or_failed_faces_remain_bounded() {
    let cases = [
        (
            FaceSnapshot::bytes(vec![0, 159, 255, 42], "codec/bin"),
            "binary face (4 bytes)",
        ),
        (
            FaceSnapshot::truncated("items", 64, 65),
            "truncated: items limit 64, observed 65",
        ),
        (
            FaceSnapshot::unsupported("opaque callable result"),
            "unsupported: opaque callable result",
        ),
        (
            FaceSnapshot::codec_failure("encoder refused extension"),
            "codec failure: encoder refused extension",
        ),
    ];
    let codec = ExpressionTreeSurfaceCodec::new();
    let caps = surface::preset("desktop").expect("desktop caps");
    let mut cx = sim_kernel::testing::eager_cx();
    for (result, expected) in cases {
        let mut cell_detail = detail(Freshness::Fresh);
        cell_detail.result = result;
        let value = snapshot(vec![NodeSnapshot::expanded_cell(
            "/arbitrary",
            "arbitrary",
            TREE_REVISION,
            cell_detail,
        )]);
        let scene = codec
            .encode(&mut cx, &value, &caps)
            .expect("bounded face Scene");
        let mut texts = Vec::new();
        collect_kind(&scene, "text", &mut texts);
        assert!(
            texts
                .iter()
                .any(|text| access::field_str(text, "text") == Some(expected)),
            "missing {expected:?} in {scene:?}"
        );
    }

    let huge = "x".repeat(20_000);
    let detail = NodeDetail {
        source: FaceSnapshot::text(huge.clone(), "codec/lisp"),
        result: FaceSnapshot::text("ok", "codec/lisp"),
        freshness: Freshness::Fresh,
        source_revision: 1,
        result_revision: Some(1),
        timestamps: TimestampSummary::default(),
        policy_badges: Vec::new(),
        receipt: None,
    };
    let value = snapshot(vec![NodeSnapshot::expanded_cell(
        "/huge",
        "huge",
        TREE_REVISION,
        detail,
    )]);
    let scene = codec
        .encode(&mut cx, &value, &caps)
        .expect("huge face is bounded");
    assert!(
        !format!("{scene:?}").contains(&huge),
        "hostile face payload never reaches the Scene"
    );
}

#[test]
fn stale_revisions_and_missing_authority_fail_closed() {
    let value = snapshot(vec![expanded_cell("/solve", "solve", Freshness::Fresh)]);
    let codec = ExpressionTreeSurfaceCodec::new();
    let mut cx = sim_kernel::testing::eager_cx();
    let stale = intent(
        "edit-field",
        Origin::human(1),
        vec![
            ("target", target("/solve", TREE_REVISION - 1)),
            ("path", build::list(vec![build::text("source")])),
            ("value", build::text("stale edit")),
        ],
    );
    let rejected = codec.decode(&mut cx, &value, &stale).expect("stale decode");
    assert!(!rejected.committable);
    assert!(
        rejected.diagnostics[0]
            .message
            .contains("stale expression-tree revision")
    );

    let current = intent(
        "edit-field",
        Origin::human(2),
        vec![
            ("target", target("/solve", TREE_REVISION)),
            ("path", build::list(vec![build::text("source")])),
            ("value", build::text("current edit")),
        ],
    );
    let draft = codec
        .decode(&mut cx, &value, &current)
        .expect("current decode");
    let operation = codec.commit(&mut cx, &draft).expect("commit metadata");
    let capability = &operation.required_capabilities[0];
    assert_eq!(capability, &CapabilityName::new("expr-tree.write"));
    assert!(
        cx.require(capability).is_err(),
        "ungranted target rejects write"
    );
    cx.grant(capability.clone());
    assert!(
        cx.require(capability).is_ok(),
        "explicit grant authorizes write"
    );
}

#[test]
fn accessibility_labels_cover_every_interactive_outline_control() {
    let value = snapshot(vec![expanded_cell("/solve", "solve", Freshness::Fresh)]);
    let mut cx = sim_kernel::testing::eager_cx();
    let scene = ExpressionTreeSurfaceCodec::new()
        .encode(
            &mut cx,
            &value,
            &surface::preset("phone").expect("phone caps"),
        )
        .expect("accessible Scene");
    for kind in ["tree", "field", "button", "badge"] {
        let mut nodes = Vec::new();
        collect_kind(&scene, kind, &mut nodes);
        assert!(!nodes.is_empty(), "{kind} specimen exists");
        assert!(
            nodes
                .iter()
                .all(|node| access::field_str(node, "aria-label")
                    .is_some_and(|label| !label.is_empty())),
            "every {kind} has a non-empty accessibility label"
        );
    }
}

#[test]
fn large_trees_end_in_explicit_total_budget_evidence() {
    let nodes = (0..2_000)
        .map(|index| {
            NodeSnapshot::collapsed_cell(
                format!("/cell-{index}"),
                format!("cell-{index}"),
                TREE_REVISION,
            )
        })
        .collect();
    let value = snapshot(nodes);
    let mut cx = sim_kernel::testing::eager_cx();
    let scene = ExpressionTreeSurfaceCodec::new()
        .encode(
            &mut cx,
            &value,
            &surface::preset("phone").expect("phone caps"),
        )
        .expect("large tree truncates");
    let mut trees = Vec::new();
    collect_kind(&scene, "tree", &mut trees);
    assert!(trees.len() <= 128);
    let mut boxes = Vec::new();
    collect_kind(&scene, "box", &mut boxes);
    assert!(boxes.iter().any(|item| {
        access::field_sym(item, "role").is_some_and(|role| role.name.as_ref() == "continuation")
    }));
}

#[test]
fn malformed_non_snapshot_values_are_rejected() {
    let mut cx = sim_kernel::testing::eager_cx();
    let error = ExpressionTreeSurfaceCodec::new()
        .encode(
            &mut cx,
            &Expr::Nil,
            &surface::preset("desktop").expect("desktop caps"),
        )
        .expect_err("arbitrary non-snapshot value fails closed");
    assert!(
        error.to_string().contains("missing field type")
            || error
                .to_string()
                .contains("not an expression-tree snapshot")
    );
}
