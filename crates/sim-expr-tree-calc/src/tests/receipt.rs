use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use sim_incremental_core::ValueFingerprint;

use crate::CalcStatus;

use super::support::*;
use super::*;

#[test]
fn receipt_commits_bounded_revision_authority_dependency_and_wall_clock_evidence() {
    let mut calc = ExprTreeCalc::new();
    calc.set_cell(path("/leaf"), Expr::String("leaf".to_owned()));
    calc.set_cell(path("/root"), explicit_ref("/leaf"));
    let observations = Arc::new(Mutex::new(VecDeque::from([
        Some(1_000),
        Some(1_100),
        Some(1_050),
        Some(900),
    ])));
    let clock_observations = Arc::clone(&observations);
    calc.set_wall_clock(move || {
        clock_observations
            .lock()
            .expect("clock observations poisoned")
            .pop_front()
            .flatten()
    });
    calc.verify_cell(&path("/root")).unwrap();

    let receipt = calc.receipt(&path("/root")).expect("root receipt");
    assert!(receipt.source_revision > 0);
    assert_eq!(
        receipt.policy_digest,
        calc.effective_calc_policy(&path("/root")).digest()
    );
    assert_eq!(
        receipt.authority_digest,
        calc.effective_authority(&path("/root")).digest()
    );
    assert!(receipt.dependencies.iter().any(|dependency| {
        dependency.query == CalcQuery::Cell("/leaf".to_owned())
            && dependency.kind == ObservationKind::Read
    }));
    assert_eq!(
        receipt.result_fingerprint,
        calc.cell_fingerprint(&path("/root"))
            .map(ValueFingerprint::get)
    );
    assert!(receipt.finished_tick > receipt.started_tick);
    assert_eq!(receipt.wall_started_ms, Some(1_000));
    assert_eq!(
        receipt.wall_finished_ms,
        Some(900),
        "wall-clock rollback is display evidence, never freshness authority"
    );

    let explanation = calc.explain(&path("/root"));
    assert_eq!(explanation.status, CalcStatus::Fresh);
    assert_eq!(explanation.receipt, Some(receipt));
    assert!(
        explanation
            .reasons
            .iter()
            .any(|reason| reason.contains("matches all observed revisions"))
    );

    let mut bounded = ExprTreeCalc::new();
    bounded.set_cell(
        path("/many"),
        Expr::Vector(
            (0..80)
                .map(|index| Expr::Symbol(Symbol::new(format!("missing-{index}"))))
                .collect(),
        ),
    );
    bounded.verify_cell(&path("/many")).unwrap();
    let receipt = bounded.receipt(&path("/many")).unwrap();
    assert_eq!(receipt.dependencies.len(), 64);
    assert!(receipt.omitted_dependencies > 0);
    assert_ne!(receipt.dependency_digest, 0);
}
