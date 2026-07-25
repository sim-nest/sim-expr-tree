//! conformance: dynamic expression-tree reference observation.

use std::{collections::BTreeSet, sync::Arc};

use sim_expr_tree_core::{BackendKind, MountEpoch, MountResource};
use sim_incremental_core::ObservationKind;
use sim_kernel::{Cx, DefaultFactory, EagerPolicy, Expr, StrictNames, Symbol, object::RawArgs};
use sim_table_core::TablePath;

use crate::{
    CalcQuery, CellExpr, EXPR_TREE_REF, ExprTreeCalc, ExprTreeRefPolicy, core_identity,
    crate_identity,
};

#[test]
fn identity_names_the_calc_crate() {
    assert_eq!(crate_identity(), "sim-expr-tree-calc");
    assert_eq!(core_identity(), "sim-expr-tree-core");
}

#[test]
fn references_created_by_callables_and_macros_are_runtime_dependencies() {
    let mut calc = ExprTreeCalc::new();
    calc.set_cell(path("/sheet/a"), CellExpr::Literal("1".to_owned()));
    calc.set_cell(path("/sheet/b"), CellExpr::CallableRef("a".to_owned()));
    calc.set_cell(path("/sheet/c"), CellExpr::MacroRef("a".to_owned()));
    assert_eq!(calc.verify_cell(&path("/sheet/b")).unwrap(), "1");
    assert_eq!(calc.verify_cell(&path("/sheet/c")).unwrap(), "1");
    let deps = dependencies(&calc.snapshot_cell(&path("/sheet/b")).unwrap(), "/sheet/b");
    assert!(deps.contains(&(
        CalcQuery::Cell("/sheet/a".to_owned()),
        ObservationKind::Read
    )));
    assert!(deps.iter().any(|(key, kind)| {
        matches!(key, CalcQuery::LookupStep(_))
            && matches!(kind, ObservationKind::Custom("lookup-step"))
    }));
}

#[test]
fn missing_then_created_names_dirty_dependents() {
    let mut calc = ExprTreeCalc::new();
    calc.set_cell(path("/sheet/use-missing"), ref_expr("later"));
    assert_eq!(
        calc.verify_cell(&path("/sheet/use-missing")).unwrap(),
        "missing:/sheet/later"
    );
    assert!(
        dependencies(
            &calc.snapshot_cell(&path("/sheet/use-missing")).unwrap(),
            "/sheet/use-missing"
        )
        .contains(&(
            CalcQuery::NameSlot("/sheet/later".to_owned()),
            ObservationKind::Missing
        ))
    );
    calc.set_cell(
        path("/sheet/later"),
        CellExpr::Literal("created".to_owned()),
    );
    assert_eq!(
        calc.verify_cell(&path("/sheet/use-missing")).unwrap(),
        "created"
    );
}

#[test]
fn fallback_lookup_and_binding_precedence_are_observed() {
    let mut calc = ExprTreeCalc::new();
    calc.set_cell(path("/total"), CellExpr::Literal("root".to_owned()));
    calc.set_cell(path("/sheet/row/use-total"), bare("total"));
    calc.set_cell(path("/sheet/row/use-bound"), bare("local"));
    calc.bind_name("local");
    assert_eq!(
        calc.verify_cell(&path("/sheet/row/use-total")).unwrap(),
        "root"
    );
    assert_eq!(
        calc.verify_cell(&path("/sheet/row/use-bound")).unwrap(),
        "bound:local"
    );
    let deps = dependencies(
        &calc.snapshot_cell(&path("/sheet/row/use-total")).unwrap(),
        "/sheet/row/use-total",
    );
    assert!(deps.contains(&(
        CalcQuery::NameSlot("/sheet/row/total".to_owned()),
        ObservationKind::Missing
    )));
    assert!(deps.contains(&(
        CalcQuery::NameSlot("/sheet/total".to_owned()),
        ObservationKind::Missing
    )));
    assert!(deps.contains(&(CalcQuery::Cell("/total".to_owned()), ObservationKind::Read)));
    assert!(
        !dependencies(
            &calc.snapshot_cell(&path("/sheet/row/use-bound")).unwrap(),
            "/sheet/row/use-bound"
        )
        .iter()
        .any(|(key, _)| matches!(key, CalcQuery::NameSlot(_)))
    );
}

#[test]
fn relative_absolute_and_explicit_base_references_resolve() {
    let mut calc = ExprTreeCalc::new();
    calc.set_cell(path("/sheet/a"), CellExpr::Literal("A".to_owned()));
    calc.set_cell(path("/other/a"), CellExpr::Literal("B".to_owned()));
    calc.set_cell(path("/sheet/relative"), ref_expr("a"));
    calc.set_cell(path("/sheet/absolute"), ref_expr("/other/a"));
    calc.set_cell(
        path("/sheet/explicit-base"),
        CellExpr::Ref {
            base: Some(path("/other/base")),
            reference: "a".to_owned(),
        },
    );
    assert_eq!(calc.verify_cell(&path("/sheet/relative")).unwrap(), "A");
    assert_eq!(calc.verify_cell(&path("/sheet/absolute")).unwrap(), "B");
    assert_eq!(
        calc.verify_cell(&path("/sheet/explicit-base")).unwrap(),
        "B"
    );
}

#[test]
fn rename_move_listing_policy_codec_authority_and_mount_epochs_are_dependencies() {
    let mut calc = ExprTreeCalc::new();
    calc.set_effective_policy("strict");
    calc.set_codec_registry_revision(7);
    calc.set_authority_ceiling("tenant");
    calc.mount(
        path("/remote"),
        MountResource::Dir,
        BackendKind::Database,
        MountEpoch::new(1),
    );
    calc.set_cell(path("/remote/value"), CellExpr::Literal("old".to_owned()));
    calc.set_cell(path("/sheet/use-remote"), ref_expr("/remote/value"));
    assert_eq!(calc.verify_cell(&path("/sheet/use-remote")).unwrap(), "old");
    let deps = dependencies(
        &calc.snapshot_cell(&path("/sheet/use-remote")).unwrap(),
        "/sheet/use-remote",
    );
    assert!(deps.contains(&(CalcQuery::EffectivePolicy, ObservationKind::Policy)));
    assert!(deps.contains(&(
        CalcQuery::CodecRegistry,
        ObservationKind::Custom("codec-registry")
    )));
    assert!(deps.contains(&(CalcQuery::AuthorityCeiling, ObservationKind::Policy)));
    assert!(deps.contains(&(
        CalcQuery::MountEpoch("/remote".to_owned()),
        ObservationKind::Epoch
    )));
    calc.move_cell(&path("/remote/value"), path("/remote/moved"));
    assert_eq!(
        calc.verify_cell(&path("/sheet/use-remote")).unwrap(),
        "missing:/remote/value"
    );
    calc.observe_mount_epoch(&path("/remote"), MountEpoch::new(2));
    assert_eq!(
        calc.verify_cell(&path("/sheet/use-remote")).unwrap(),
        "missing:/remote/value"
    );
}

#[test]
fn symbols_that_are_not_evaluated_do_not_observe_names() {
    let mut calc = ExprTreeCalc::new();
    calc.set_cell(
        path("/sheet/data"),
        CellExpr::Quoted(Symbol::new("future-name")),
    );
    assert_eq!(
        calc.verify_cell(&path("/sheet/data")).unwrap(),
        "future-name"
    );
    assert!(
        !dependencies(
            &calc.snapshot_cell(&path("/sheet/data")).unwrap(),
            "/sheet/data"
        )
        .iter()
        .any(|(key, _)| matches!(key, CalcQuery::NameSlot(_)))
    );
}

#[test]
fn eval_policy_wrapper_delegates_and_reserves_reference_calls() {
    let policy = ExprTreeRefPolicy::new(StrictNames(EagerPolicy));
    let mut cx = Cx::new(Arc::new(policy), Arc::new(DefaultFactory));
    let value = cx.resolve_unbound_call(
        Symbol::new(EXPR_TREE_REF),
        vec![Expr::String("/sheet/a".to_owned())],
    );
    assert_eq!(
        value.unwrap().object().display(&mut cx).unwrap(),
        "expr-tree/ref:/sheet/a"
    );
    let raw = RawArgs::new(vec![Expr::String("kept".to_owned())]);
    let eval_policy = cx.eval_policy_ref();
    let prepared = eval_policy.prepare_call_args(&mut cx, raw, &[]).unwrap();
    assert_eq!(prepared.values().len(), 1);
}

fn ref_expr(reference: &str) -> CellExpr {
    CellExpr::Ref {
        base: None,
        reference: reference.to_owned(),
    }
}

fn bare(name: &str) -> CellExpr {
    CellExpr::Bare(Symbol::new(name))
}

fn path(input: &str) -> TablePath {
    TablePath::parse_absolute(input).unwrap()
}

fn dependencies(
    snapshot: &sim_incremental_core::GraphSnapshot<CalcQuery, String>,
    key: &str,
) -> BTreeSet<(CalcQuery, ObservationKind)> {
    snapshot
        .nodes
        .iter()
        .find(|node| node.key == CalcQuery::Cell(key.to_owned()))
        .unwrap()
        .dependencies
        .iter()
        .map(|observation| (observation.key().clone(), observation.kind().clone()))
        .collect()
}
