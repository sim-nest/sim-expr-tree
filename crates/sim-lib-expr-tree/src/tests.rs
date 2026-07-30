use std::sync::Arc;

use sim_citizen::run_registry_conformance_expecting;
use sim_codec::{DecodePosition, DecodedForm, Input, decode_default_with_codec};
use sim_codec_lisp::LispCodecLib;
use sim_kernel::{
    CapabilityName, Cx, DefaultFactory, EagerPolicy, Expr, ReadPolicy, Symbol, Value,
};

use super::*;

// conformance: stable runtime operations, Shapes, Cards, capabilities, Citizens, and recipes.
const CITIZENS: [&str; 2] = ["expr-tree/SourceRecord", "expr-tree/PolicyRecord"];

#[test]
fn identity_names_runtime_and_components() {
    assert_eq!(crate_identity(), "sim-lib-expr-tree");
    assert_eq!(
        component_identities(),
        ["sim-expr-tree-core", "sim-expr-tree-calc"]
    );
}

#[test]
fn manifest_exports_every_stable_operation_shape_card_and_citizen() {
    let mut cx = runtime_cx(&all_capabilities());
    let expected = [
        "open",
        "new-cell",
        "new-dir",
        "mount",
        "unmount",
        "move",
        "rename",
        "delete",
        "set-expr",
        "set-calc-policy",
        "set-codec-policy",
        "ref",
        "list",
        "calculate",
        "recalculate",
        "recalculate-recursive",
        "cancel",
        "refresh",
        "status",
        "explain",
        "watch",
    ];
    assert_eq!(
        expr_tree_operation_symbols()
            .into_iter()
            .map(|symbol| symbol.to_string())
            .collect::<Vec<_>>(),
        expected
            .iter()
            .map(|name| format!("expr-tree/{name}"))
            .collect::<Vec<_>>()
    );
    let cards = operation_cards(&mut cx).unwrap();
    assert_eq!(cards.len(), expected.len());
    for name in expected {
        let symbol = Symbol::qualified("expr-tree", name);
        let function = cx
            .registry()
            .function_by_symbol(&symbol)
            .expect("operation is linked")
            .clone();
        let callable = function.object().as_callable().expect("callable operation");
        assert!(
            callable
                .browse_args_shape(&mut cx)
                .unwrap()
                .expect("args shape")
                .object()
                .as_shape()
                .is_some()
        );
        assert!(
            callable
                .browse_result_shape(&mut cx)
                .unwrap()
                .expect("result shape")
                .object()
                .as_shape()
                .is_some()
        );
    }
}

#[test]
fn stable_lisp_operations_cover_namespace_mounts_paths_calculation_and_reopen() {
    let mut cx = runtime_cx(&all_capabilities());
    let tree = eval_lisp(&mut cx, "(expr-tree/open \"ops\")");
    bind(&mut cx, "tree", tree);

    assert_eq!(
        eval_expr(&mut cx, "(expr-tree/new-dir tree \"/\" nil)",),
        Expr::String("/dir-1".to_owned())
    );
    eval_lisp(
        &mut cx,
        "(expr-tree/new-cell tree \"/dir-1\" \"base\" \"ready\")",
    );
    assert_eq!(
        eval_expr(
            &mut cx,
            "(expr-tree/new-cell tree \"/dir-1\" nil (expr-tree/ref \"base\"))",
        ),
        Expr::String("/dir-1/cell-1".to_owned())
    );
    eval_lisp(&mut cx, "(expr-tree/new-dir tree \"/dir-1\" \"nested\")");

    for form in [
        "(expr-tree/ref tree \"cell-1\" \"/dir-1\")",
        "(expr-tree/ref tree \"../base\" \"/dir-1/nested\")",
        "(expr-tree/ref tree \"/dir-1/base\")",
    ] {
        assert_eq!(eval_expr(&mut cx, form), Expr::String("ready".to_owned()));
    }

    assert_eq!(
        eval_expr(
            &mut cx,
            "(expr-tree/mount tree \"/external\" \"database\" \"dir\" \"7\")",
        ),
        Expr::String("/external".to_owned())
    );
    eval_lisp(
        &mut cx,
        "(expr-tree/new-cell tree \"/external\" \"record\" \"db-value\")",
    );
    eval_lisp(
        &mut cx,
        "(expr-tree/mount tree \"/catalog\" \"read-only\" \"table\" \"3\")",
    );
    let denied = eval_lisp_result(
        &mut cx,
        "(expr-tree/new-cell tree \"/catalog\" \"forbidden\" \"x\")",
    )
    .unwrap_err()
    .to_string();
    assert!(denied.contains("read-only"), "{denied}");
    assert!(denied.len() < 600);
    eval_lisp(&mut cx, "(expr-tree/unmount tree \"/catalog\")");

    assert_eq!(
        eval_expr(
            &mut cx,
            "(expr-tree/rename tree \"/external/record\" \"renamed\")",
        ),
        Expr::String("/external/renamed".to_owned())
    );
    assert_eq!(
        eval_expr(
            &mut cx,
            "(expr-tree/move tree \"/external/renamed\" \"/dir-1/moved\")",
        ),
        Expr::String("/dir-1/moved".to_owned())
    );
    eval_lisp(&mut cx, "(expr-tree/delete tree \"/dir-1/moved\")");
    eval_lisp(&mut cx, "(expr-tree/unmount tree \"/external\")");

    let listing = eval_lisp(&mut cx, "(expr-tree/list tree \"/dir-1\")");
    let listing = listing.object().as_expr(&mut cx).unwrap();
    let Expr::List(rows) = listing else {
        panic!("list operation must return a list")
    };
    assert_eq!(rows.len(), 3);

    let reopened = eval_lisp(&mut cx, "(expr-tree/open \"ops\")");
    bind(&mut cx, "reopened", reopened);
    assert_eq!(
        eval_expr(&mut cx, "(expr-tree/ref reopened \"/dir-1/cell-1\")",),
        Expr::String("ready".to_owned())
    );
}

#[test]
fn automatic_directed_cycle_receipts_policies_and_streams_share_one_engine() {
    let mut cx = runtime_cx(&all_capabilities());
    let tree = eval_lisp(&mut cx, "(expr-tree/open \"calculation\")");
    bind(&mut cx, "tree", tree);
    eval_lisp(
        &mut cx,
        "(expr-tree/new-cell tree \"/\" \"a\" (expr-tree/ref \"/b\"))",
    );
    eval_lisp(
        &mut cx,
        "(expr-tree/new-cell tree \"/\" \"b\" (expr-tree/ref \"/a\"))",
    );
    let cycle = eval_lisp_result(&mut cx, "(expr-tree/recalculate-recursive tree \"/a\")")
        .unwrap_err()
        .to_string();
    assert!(cycle.contains("cycle"), "{cycle}");
    assert!(cycle.len() < 600);

    eval_lisp(&mut cx, "(expr-tree/set-expr tree \"/b\" \"recovered\")");
    assert_eq!(
        eval_expr(&mut cx, "(expr-tree/recalculate tree \"/a\")"),
        Expr::String("recovered".to_owned())
    );
    let policy = eval_lisp(
        &mut cx,
        "(expr-tree/set-calc-policy tree \"/a\" \"manual\")",
    );
    assert_eq!(
        policy
            .object()
            .downcast_ref::<DurablePolicyRecord>()
            .expect("durable policy")
            .calc_trigger,
        "manual"
    );
    eval_lisp(
        &mut cx,
        "(expr-tree/set-codec-policy tree \"/a\" \"codec/lisp\")",
    );
    eval_lisp(&mut cx, "(expr-tree/calculate tree \"/a\")");

    let explanation = eval_lisp(&mut cx, "(expr-tree/explain tree \"/a\")");
    let source_record = table_value(&mut cx, &explanation, "source-record");
    let source_record = source_record
        .object()
        .downcast_ref::<DurableSourceRecord>()
        .expect("durable source record");
    assert_eq!(source_record.path, "/a");
    assert!(
        table_value(&mut cx, &explanation, "receipt")
            .object()
            .as_table_impl()
            .is_some()
    );

    let watch = eval_lisp(&mut cx, "(expr-tree/watch tree)");
    assert!(watch.object().as_sequence().is_some());
    let status = eval_lisp(&mut cx, "(expr-tree/status tree \"/a\")");
    assert_eq!(
        table_value(&mut cx, &status, "status")
            .object()
            .as_expr(&mut cx)
            .unwrap(),
        Expr::Symbol(Symbol::qualified("expr-tree/status", "fresh"))
    );
    eval_lisp(&mut cx, "(expr-tree/refresh tree)");
}

#[test]
fn capabilities_fail_closed_and_errors_are_bounded() {
    let mut cx = runtime_cx(&[expr_tree_read_capability()]);
    let tree = eval_lisp(&mut cx, "(expr-tree/open \"denied\")");
    bind(&mut cx, "tree", tree);
    let error =
        eval_lisp_result(&mut cx, "(expr-tree/new-cell tree \"/\" \"blocked\" \"x\")").unwrap_err();
    assert!(matches!(
        error,
        sim_kernel::Error::CapabilityDenied { capability }
            if capability == expr_tree_write_capability()
    ));
}

#[test]
fn durable_records_are_conformant_citizens_but_live_handles_are_opaque() {
    let registry = expr_tree_citizen_registry().unwrap();
    registry.ensure_contains_symbols(&CITIZENS).unwrap();
    let mut conformance_cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    run_registry_conformance_expecting(&mut conformance_cx, &registry, &CITIZENS).unwrap();

    let mut cx = runtime_cx(&all_capabilities());
    let tree = eval_lisp(&mut cx, "(expr-tree/open \"opaque\")");
    let Expr::Extension { tag, .. } = tree.object().as_expr(&mut cx).unwrap() else {
        panic!("live handle must remain opaque")
    };
    assert_eq!(tag, Symbol::qualified("core", "opaque-object"));
}

#[test]
fn recipe_finite_tree_runs_checked_lisp_surface() {
    let mut cx = recipe_cx();
    let result = eval_lisp(
        &mut cx,
        include_str!("../recipes/01-basics/finite-tree/setup.siml"),
    );
    let Expr::List(root_entries) = result.object().as_expr(&mut cx).unwrap() else {
        panic!("finite-tree recipe must return the bounded root listing")
    };
    assert_eq!(root_entries.len(), 3);

    let tree = eval_lisp(&mut cx, "(expr-tree/open \"recipe-finite-tree\")");
    bind(&mut cx, "finite-tree", tree);
    assert_eq!(
        eval_expr(&mut cx, "(expr-tree/ref finite-tree \"/dir-1/cell-1\")",),
        Expr::String("ordinary-value".to_owned())
    );
    assert_eq!(
        eval_expr(
            &mut cx,
            "(expr-tree/ref finite-tree \"/measurements/trial-0001\")",
        ),
        Expr::String("measured-value".to_owned())
    );
}

#[test]
fn recipe_automatic_and_directed_runs_checked_lisp_surface() {
    let mut cx = recipe_cx();
    let explanation = eval_lisp(
        &mut cx,
        include_str!("../recipes/02-calculation/automatic-and-directed/setup.siml"),
    );
    assert_eq!(
        table_value(&mut cx, &explanation, "status")
            .object()
            .as_expr(&mut cx)
            .unwrap(),
        Expr::Symbol(Symbol::qualified("expr-tree/status", "fresh"))
    );
    assert!(
        table_value(&mut cx, &explanation, "receipt")
            .object()
            .as_table_impl()
            .is_some()
    );

    let tree = eval_lisp(
        &mut cx,
        "(expr-tree/open \"recipe-automatic-and-directed\")",
    );
    bind(&mut cx, "calculation-tree", tree);
    assert_eq!(
        eval_expr(&mut cx, "(expr-tree/ref calculation-tree \"/manual\")",),
        Expr::String("automatic-value".to_owned())
    );
    assert_eq!(
        eval_expr(&mut cx, "(expr-tree/ref calculation-tree \"/cycle-a\")",),
        Expr::String("recovered".to_owned())
    );
}

fn runtime_cx(capabilities: &[CapabilityName]) -> Cx {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    let codec = LispCodecLib::new(cx.registry_mut().fresh_codec_id()).unwrap();
    cx.load_lib(&codec).unwrap();
    install_expr_tree_lib(&mut cx).unwrap();
    for capability in capabilities {
        cx.grant(capability.clone());
    }
    cx
}

fn recipe_cx() -> Cx {
    runtime_cx(&all_capabilities())
}

fn all_capabilities() -> [CapabilityName; 4] {
    [
        expr_tree_read_capability(),
        expr_tree_write_capability(),
        expr_tree_calculate_capability(),
        expr_tree_mount_capability(),
    ]
}

fn eval_lisp(cx: &mut Cx, source: &str) -> Value {
    eval_lisp_result(cx, source).unwrap()
}

fn eval_lisp_result(cx: &mut Cx, source: &str) -> sim_kernel::Result<Value> {
    let decoded = decode_default_with_codec(
        cx,
        &Symbol::qualified("codec", "lisp"),
        Input::Text(source.to_owned()),
        ReadPolicy::default(),
        DecodePosition::Eval,
    )?;
    let expression = match decoded {
        DecodedForm::Term(term) => Expr::from(term),
        DecodedForm::Datum(datum) => Expr::from(datum),
    };
    cx.eval_expr(expression)
}

fn eval_expr(cx: &mut Cx, source: &str) -> Expr {
    eval_lisp(cx, source).object().as_expr(cx).unwrap()
}

fn bind(cx: &mut Cx, name: &str, value: Value) {
    cx.env_mut().define(Symbol::new(name), value);
}

fn table_value(cx: &mut Cx, table: &Value, name: &str) -> Value {
    table
        .object()
        .as_table_impl()
        .expect("expected table value")
        .get(cx, Symbol::new(name))
        .unwrap()
}
