use sim_kernel::{AssocTable, Expr, Symbol, Table};

use super::*;
use crate::{
    DERIVED_SNAPSHOT_KEY, DerivedRestoreDisposition, DerivedTableAdapter, GRAPH_SCHEMA_VERSION,
};

// conformance: mixed authored, control, and derived stores survive bounded restart and recovery.

fn persist(calc: &mut ExprTreeCalc, table: &AssocTable) -> crate::DerivedPersistReport {
    let mut cx = strict_context();
    let mut derived = DerivedTableAdapter::new(table, &mut cx);
    let report = calc.persist_derived(&mut derived).unwrap();
    assert!(report.nodes > 0);
    report
}

fn restore(calc: &mut ExprTreeCalc, table: &AssocTable) -> crate::DerivedRestoreReport {
    let mut cx = strict_context();
    let mut derived = DerivedTableAdapter::new(table, &mut cx);
    calc.restore_derived(&mut derived).unwrap()
}

fn overwrite(table: &AssocTable, expr: Expr) {
    let mut cx = strict_context();
    let value = cx.factory().expr(expr).unwrap();
    table
        .set(&mut cx, Symbol::new(DERIVED_SNAPSHOT_KEY), value)
        .unwrap();
}

fn raw_snapshot(table: &AssocTable) -> Expr {
    let mut cx = strict_context();
    table
        .get(&mut cx, Symbol::new(DERIVED_SNAPSHOT_KEY))
        .unwrap()
        .object()
        .as_expr(&mut cx)
        .unwrap()
}

fn replace_schema(expr: &mut Expr, schema: u64) {
    let Expr::Map(fields) = expr else {
        panic!("derived snapshot must be a record");
    };
    let value = fields
        .iter_mut()
        .find_map(|(key, value)| {
            matches!(key, Expr::Symbol(key) if key.to_string() == "schema").then_some(value)
        })
        .expect("derived snapshot schema");
    *value = Expr::String(schema.to_string());
}

#[test]
fn restart_kill_reopen_rehydrates_without_full_reevaluation() {
    let runtime = TestRuntime::default();
    *runtime.source.lock().unwrap() = "one".to_owned();
    let table = AssocTable::new();
    let mut calc = runtime_calc(runtime.clone());
    install_diamond(&mut calc);
    assert_eq!(
        value_expr(calc.verify_cell(&path("/d")).unwrap()),
        Expr::String("one-bone-c".to_owned())
    );
    let calls_before = (
        runtime.count("probe"),
        runtime.count("concat-b"),
        runtime.count("concat-c"),
        runtime.count("concat-d"),
    );
    let original_receipt = calc.receipt(&path("/d")).unwrap();
    assert!(persist(&mut calc, &table).reverse_edges > 0);
    drop(calc);

    let mut reopened = runtime_calc(runtime.clone());
    install_diamond(&mut reopened);
    let report = restore(&mut reopened, &table);
    assert_eq!(report.disposition, DerivedRestoreDisposition::Rehydrated);
    assert!(report.restored_nodes >= 4);
    assert_eq!(
        value_expr(reopened.current_cell(&path("/d")).unwrap()),
        Expr::String("one-bone-c".to_owned()),
        "ordinary current reads are available immediately after rehydrate"
    );
    assert_eq!(reopened.receipt(&path("/d")), Some(original_receipt));
    reopened.verify_cell(&path("/d")).unwrap();
    assert_eq!(
        (
            runtime.count("probe"),
            runtime.count("concat-b"),
            runtime.count("concat-c"),
            runtime.count("concat-d"),
        ),
        calls_before,
        "an intact graph must reuse dependency work across process restart"
    );
}

#[test]
fn restart_corruption_schema_mismatch_and_generation_mismatch_rebuild_safely() {
    let runtime = TestRuntime::default();
    *runtime.source.lock().unwrap() = "old".to_owned();

    let corrupt_table = AssocTable::new();
    let mut original = runtime_calc(runtime.clone());
    install_diamond(&mut original);
    original.verify_cell(&path("/d")).unwrap();
    persist(&mut original, &corrupt_table);
    overwrite(&corrupt_table, Expr::String("not-a-graph".to_owned()));
    let mut reopened = runtime_calc(runtime.clone());
    install_diamond(&mut reopened);
    assert_eq!(
        restore(&mut reopened, &corrupt_table).disposition,
        DerivedRestoreDisposition::RebuiltCorrupt
    );
    assert_eq!(
        value_expr(reopened.verify_cell(&path("/d")).unwrap()),
        Expr::String("old-bold-c".to_owned())
    );

    let schema_table = AssocTable::new();
    persist(&mut original, &schema_table);
    let mut incompatible = raw_snapshot(&schema_table);
    replace_schema(&mut incompatible, GRAPH_SCHEMA_VERSION + 1);
    overwrite(&schema_table, incompatible);
    let mut reopened = runtime_calc(runtime.clone());
    install_diamond(&mut reopened);
    assert_eq!(
        restore(&mut reopened, &schema_table).disposition,
        DerivedRestoreDisposition::RebuiltIncompatible
    );

    let mismatch_table = AssocTable::new();
    persist(&mut original, &mismatch_table);
    let mut changed = runtime_calc(runtime.clone());
    install_diamond(&mut changed);
    changed.set_cell(path("/a"), Expr::String("authored-change".to_owned()));
    assert_eq!(
        restore(&mut changed, &mismatch_table).disposition,
        DerivedRestoreDisposition::RebuiltGenerationMismatch
    );
    assert_eq!(
        value_expr(changed.verify_cell(&path("/a")).unwrap()),
        Expr::String("authored-change".to_owned()),
        "rebuild must never replace mismatched authored source"
    );

    let identity_table = AssocTable::new();
    let mut identity_original = ExprTreeCalc::new();
    identity_original.set_cell(path("/value"), Expr::String("old".to_owned()));
    identity_original.verify_cell(&path("/value")).unwrap();
    persist(&mut identity_original, &identity_table);
    let mut same_generation_different_source = ExprTreeCalc::new();
    same_generation_different_source.set_cell(path("/value"), Expr::String("new".to_owned()));
    assert_eq!(
        restore(&mut same_generation_different_source, &identity_table).disposition,
        DerivedRestoreDisposition::RebuiltGenerationMismatch,
        "content identity must reject different source even at the same generation"
    );
    assert_eq!(
        value_expr(
            same_generation_different_source
                .verify_cell(&path("/value"))
                .unwrap()
        ),
        Expr::String("new".to_owned())
    );
}

#[test]
fn restart_missing_or_deleted_derived_store_loses_only_performance() {
    let table = AssocTable::new();
    let mut missing = ExprTreeCalc::new();
    missing.set_cell(path("/value"), Expr::String("source".to_owned()));
    assert_eq!(
        restore(&mut missing, &table).disposition,
        DerivedRestoreDisposition::RebuiltMissing
    );
    assert_eq!(
        value_expr(missing.verify_cell(&path("/value")).unwrap()),
        Expr::String("source".to_owned())
    );

    persist(&mut missing, &table);
    {
        let mut cx = strict_context();
        let mut derived = DerivedTableAdapter::new(&table, &mut cx);
        derived.delete().unwrap();
    }
    let mut reopened = ExprTreeCalc::new();
    reopened.set_cell(path("/value"), Expr::String("source".to_owned()));
    assert_eq!(
        restore(&mut reopened, &table).disposition,
        DerivedRestoreDisposition::RebuiltMissing
    );
    assert_eq!(
        value_expr(reopened.verify_cell(&path("/value")).unwrap()),
        Expr::String("source".to_owned())
    );
}

#[test]
fn restart_interrupted_automatic_continuation_resumes_after_reopen() {
    let table = AssocTable::new();
    let mut calc = ExprTreeCalc::new();
    calc.set_cell(path("/leaf"), Expr::String("leaf".to_owned()));
    calc.set_cell(path("/root"), explicit_ref("/leaf"));
    let stopped = calc.run_automatic(AutomaticBudget::new(1, CalcLimits::new(1, 100, 10, 100)), 0);
    assert_eq!(stopped.budget_exhausted.len(), 1);
    assert_eq!(persist(&mut calc, &table).pending_continuations, 1);

    let mut reopened = ExprTreeCalc::new();
    reopened.set_cell(path("/leaf"), Expr::String("leaf".to_owned()));
    reopened.set_cell(path("/root"), explicit_ref("/leaf"));
    let report = restore(&mut reopened, &table);
    assert_eq!(report.disposition, DerivedRestoreDisposition::Rehydrated);
    assert_eq!(report.pending_continuations, 1);
    let resumed = reopened.run_automatic(AutomaticBudget::default(), 0);
    assert!(!resumed.completed.is_empty());
    assert_eq!(
        value_expr(reopened.current_cell(&path("/root")).unwrap()),
        Expr::String("leaf".to_owned())
    );
}
