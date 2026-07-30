use sim_kernel::Symbol;
use sim_lib_view::{SurfaceCodec, surface};
use sim_value::access;

use crate::{ExpressionTreeSurfaceCodec, Freshness};

use super::support::{collect_kind, expanded_cell, snapshot, symbol_field};

#[test]
fn expanded_row_renders_faces_status_revisions_times_policy_receipt_and_actions() {
    let value = snapshot(vec![expanded_cell("/solve", "solve", Freshness::Fresh)]);
    let caps = surface::preset("desktop").expect("desktop caps");
    let mut cx = sim_kernel::testing::eager_cx();
    let scene = ExpressionTreeSurfaceCodec::new()
        .encode(&mut cx, &value, &caps)
        .expect("expanded row encodes");

    let mut trees = Vec::new();
    collect_kind(&scene, "tree", &mut trees);
    assert_eq!(trees.len(), 1);
    assert_eq!(access::field_bool(trees[0], "open"), Some(true));
    assert_eq!(
        access::field_str(trees[0], "aria-label"),
        Some("solve, cell, expanded")
    );

    let mut boxes = Vec::new();
    collect_kind(&scene, "box", &mut boxes);
    assert!(
        boxes
            .iter()
            .any(|item| symbol_field(item, "role").as_deref() == Some("source"))
    );
    assert!(
        boxes
            .iter()
            .any(|item| symbol_field(item, "role").as_deref() == Some("result"))
    );

    let mut fields = Vec::new();
    collect_kind(&scene, "field", &mut fields);
    assert_eq!(fields.len(), 1, "only the source face is editable");
    assert_eq!(access::field_str(fields[0], "value"), Some("(+ 20 22)"));
    assert_eq!(
        access::field_str(fields[0], "aria-label"),
        Some("Edit source for solve")
    );

    let mut badges = Vec::new();
    collect_kind(&scene, "badge", &mut badges);
    let labels = badges
        .iter()
        .filter_map(|badge| access::field_str(badge, "label"))
        .collect::<Vec<_>>();
    assert_eq!(labels, ["Fresh", "Automatic", "codec/lisp"]);

    let mut text = Vec::new();
    collect_kind(&scene, "text", &mut text);
    let lines = text
        .iter()
        .filter_map(|line| access::field_str(line, "text"))
        .collect::<Vec<_>>();
    assert!(lines.contains(&"42"));
    assert!(lines.contains(&"source r17 · result r16"));
    assert!(lines.iter().any(|line| line.contains("1700000000100 ms")));
    assert!(lines.iter().any(|line| {
        line.contains("receipt #9: succeeded, 3 dependencies (+2 omitted), ticks 40–44")
    }));

    let mut buttons = Vec::new();
    collect_kind(&scene, "button", &mut buttons);
    let controls = buttons
        .iter()
        .filter_map(|button| access::field_str(button, "control"))
        .collect::<Vec<_>>();
    assert_eq!(
        controls,
        [
            "calculate",
            "recalculate",
            "recalculate-recursive",
            "policy",
            "explain",
            "cancel",
        ]
    );
    assert!(
        buttons
            .iter()
            .all(|button| access::field_str(button, "aria-label").is_some())
    );
}

#[test]
fn layout_is_projected_from_open_surface_caps_not_a_device_enum() {
    let value = snapshot(vec![expanded_cell("/solve", "solve", Freshness::Fresh)]);
    let codec = ExpressionTreeSurfaceCodec::new();
    let mut cx = sim_kernel::testing::eager_cx();

    let desktop = surface::preset("desktop").expect("desktop caps");
    assert_eq!(
        face_layout(&codec.encode(&mut cx, &value, &desktop).unwrap()),
        "row"
    );

    let phone = surface::preset("phone").expect("phone caps");
    assert_eq!(
        face_layout(&codec.encode(&mut cx, &value, &phone).unwrap()),
        "column"
    );

    let mut future_surface = surface::preset("phone").expect("base caps");
    future_surface.preset = Symbol::qualified("surface", "future-reader");
    future_surface.display = access::set(
        &future_surface.display,
        "density",
        sim_value::build::sym("dense"),
    );
    assert_eq!(
        face_layout(&codec.encode(&mut cx, &value, &future_surface).unwrap()),
        "row",
        "an unknown preset gets the dense projection advertised by its caps"
    );
}

fn face_layout(scene: &sim_kernel::Expr) -> String {
    let mut stacks = Vec::new();
    collect_kind(scene, "stack", &mut stacks);
    stacks
        .into_iter()
        .find(|stack| {
            access::field_str(stack, "aria-label")
                .is_some_and(|label| label.ends_with("source and result"))
        })
        .and_then(|stack| symbol_field(stack, "dir"))
        .expect("face layout stack")
}
