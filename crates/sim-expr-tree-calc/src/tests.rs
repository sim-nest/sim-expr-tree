//! conformance: ordinary-value incremental expression-tree calculation.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use sim_expr_tree_core::{BackendKind, MountEpoch, MountResource};
use sim_incremental_core::{BudgetKind, IncrementalError, ObservationKind};
use sim_kernel::{
    CapabilityName, CapabilitySet, Cx, DefaultFactory, EagerPolicy, Expr, MacroExpander, Phase,
    StrictNames, Symbol, capability::macro_expansion_capability_for_phase, object::RawArgs,
};
use sim_lib_stream_core::{BufferOverflowPolicy, BufferPolicy, StreamPacket};

use crate::{
    AuthorityPolicyPatch, AutomaticBudget, CalcError, CalcLimits, CalcPolicyPatch, CalcQuery,
    CalcTrigger, CellFailure, CycleMode, EXPR_TREE_REF, ErrorMode, ExprTreeCalc, ExprTreeRefPolicy,
    HARD_MAX_EXPR_DEPTH, HARD_MAX_OUTPUT, HARD_MAX_QUERY_DEPTH, core_identity, crate_identity,
};

mod support;
use support::*;

#[test]
fn identity_names_the_calc_crate() {
    assert_eq!(crate_identity(), "sim-expr-tree-calc");
    assert_eq!(core_identity(), "sim-expr-tree-core");
}

#[test]
fn policy_inherits_field_by_field_and_enforces_all_trigger_modes() {
    let mut calc = ExprTreeCalc::new();
    calc.set_tree_calc_policy(CalcPolicyPatch {
        trigger: Some(CalcTrigger::OnDemand),
        error_mode: Some(ErrorMode::FailFast),
        cycle_mode: Some(CycleMode::Block),
        budget: Some(CalcLimits::new(80, 70, 60, 50)),
        priority: Some(4),
        debounce_ms: Some(30),
    });
    calc.set_dir_calc_policy(
        path("/team"),
        CalcPolicyPatch {
            trigger: Some(CalcTrigger::Manual),
            priority: Some(8),
            ..CalcPolicyPatch::default()
        },
    );
    calc.set_cell_calc_policy(
        path("/team/frozen"),
        CalcPolicyPatch {
            trigger: Some(CalcTrigger::Frozen),
            debounce_ms: Some(90),
            ..CalcPolicyPatch::default()
        },
    );

    let inherited = calc.effective_calc_policy(&path("/team/manual"));
    assert_eq!(inherited.trigger, CalcTrigger::Manual);
    assert_eq!(inherited.error_mode, ErrorMode::FailFast);
    assert_eq!(inherited.cycle_mode, CycleMode::Block);
    assert_eq!(inherited.budget, CalcLimits::new(80, 70, 60, 50));
    assert_eq!(inherited.priority, 8);
    assert_eq!(inherited.debounce_ms, 30);
    let frozen = calc.effective_calc_policy(&path("/team/frozen"));
    assert_eq!(frozen.trigger, CalcTrigger::Frozen);
    assert_eq!(frozen.priority, 8);
    assert_eq!(frozen.debounce_ms, 90);

    calc.set_cell(path("/ondemand"), Expr::String("pull".to_owned()));
    calc.set_cell(path("/team/manual"), Expr::String("directed".to_owned()));
    calc.set_cell(path("/team/frozen"), Expr::String("retained".to_owned()));
    assert!(
        calc.automatic_queue_snapshot().entries.is_empty(),
        "on-demand, manual, and frozen cells must not enqueue automatic work"
    );
    assert_eq!(
        value_expr(calc.verify_cell(&path("/ondemand")).unwrap()),
        Expr::String("pull".to_owned())
    );
    assert_eq!(
        value_expr(calc.verify_cell(&path("/team/manual")).unwrap()),
        Expr::String("directed".to_owned())
    );
    assert!(matches!(
        calc.verify_cell(&path("/team/frozen")),
        Err(CalcError::Cell(CellFailure::Blocked { .. }))
    ));

    let mut automatic = ExprTreeCalc::new();
    automatic.set_cell(path("/auto"), Expr::String("queued".to_owned()));
    assert_eq!(
        automatic.automatic_queue_snapshot().entries[0].cell,
        "/auto"
    );
}

#[test]
fn policy_request_modes_verify_force_roots_force_recursive_and_block_dependencies() {
    let runtime = TestRuntime::default();
    *runtime.source.lock().unwrap() = "one".to_owned();
    let mut calc = runtime_calc(runtime.clone());
    install_diamond(&mut calc);
    calc.verify_cell(&path("/d")).unwrap();
    assert_eq!(
        (
            runtime.count("probe"),
            runtime.count("concat-b"),
            runtime.count("concat-c"),
            runtime.count("concat-d"),
        ),
        (1, 1, 1, 1)
    );

    calc.verify_cell(&path("/d")).unwrap();
    assert_eq!(runtime.count("concat-d"), 1, "verify reuses the root");

    calc.recalculate_cell(&path("/d")).unwrap();
    assert_eq!(
        (
            runtime.count("probe"),
            runtime.count("concat-b"),
            runtime.count("concat-c"),
            runtime.count("concat-d"),
        ),
        (1, 1, 1, 2),
        "force-roots must preserve valid dependency reuse"
    );

    calc.recalculate_recursive(&path("/d")).unwrap();
    assert_eq!(
        (
            runtime.count("probe"),
            runtime.count("concat-b"),
            runtime.count("concat-c"),
            runtime.count("concat-d"),
        ),
        (2, 2, 2, 3),
        "force-recursive must force the reachable calculated closure"
    );

    let mut blocked = ExprTreeCalc::new();
    blocked.set_cell_calc_policy(
        path("/manual"),
        CalcPolicyPatch {
            trigger: Some(CalcTrigger::Manual),
            ..CalcPolicyPatch::default()
        },
    );
    blocked.set_cell(path("/manual"), Expr::String("directed-only".to_owned()));
    blocked.set_cell(path("/auto"), explicit_ref("/manual"));
    assert!(matches!(
        blocked.verify_cell(&path("/auto")),
        Err(CalcError::Cell(CellFailure::Blocked { .. }))
    ));
    blocked.verify_cell(&path("/manual")).unwrap();
    assert_eq!(
        value_expr(blocked.verify_cell(&path("/auto")).unwrap()),
        Expr::String("directed-only".to_owned()),
        "a current manual dependency may be reused without silently forcing it"
    );

    blocked.set_cell_calc_policy(
        path("/manual"),
        CalcPolicyPatch {
            trigger: Some(CalcTrigger::Frozen),
            ..CalcPolicyPatch::default()
        },
    );
    blocked.set_cell(path("/manual"), Expr::String("new-but-frozen".to_owned()));
    assert!(matches!(
        blocked.recalculate_recursive(&path("/auto")),
        Err(CalcError::Cell(CellFailure::Blocked { .. }))
    ));
}

#[test]
fn authority_open_ceiling_is_immutable_and_cell_policy_only_diminishes() {
    let alpha = CapabilityName::new("expr-tree.test.alpha");
    let beta = CapabilityName::new("expr-tree.test.beta");
    let calls = Arc::new(AtomicUsize::new(0));
    let factory_calls = Arc::clone(&calls);
    let alpha_for_factory = alpha.clone();
    let beta_for_factory = beta.clone();
    let mut calc = ExprTreeCalc::with_context_factory(move || {
        let (mut cx, seat) = Cx::new_seated(
            Arc::new(ExprTreeRefPolicy::new(StrictNames(EagerPolicy))),
            Arc::new(DefaultFactory),
        );
        seat.grant(&mut cx, alpha_for_factory.clone()).unwrap();
        if factory_calls.fetch_add(1, Ordering::AcqRel) > 0 {
            seat.grant(&mut cx, beta_for_factory.clone()).unwrap();
        }
        cx
    });
    assert!(calc.open_time_authority().contains(&alpha));
    assert!(!calc.open_time_authority().contains(&beta));

    calc.set_tree_authority_policy(AuthorityPolicyPatch {
        allow: Some(
            CapabilitySet::new()
                .grant(alpha.clone())
                .grant(beta.clone()),
        ),
        ..AuthorityPolicyPatch::default()
    });
    calc.set_dir_authority_policy(
        path("/secure"),
        AuthorityPolicyPatch {
            allow: Some(CapabilitySet::new().grant(beta.clone())),
            required: CapabilitySet::new().grant(beta.clone()),
            ..AuthorityPolicyPatch::default()
        },
    );
    calc.set_cell(
        path("/secure/result"),
        Expr::String("must-not-escalate".to_owned()),
    );
    let diminished = calc.effective_authority(&path("/secure/result"));
    assert!(!diminished.capabilities().contains(&alpha));
    assert!(!diminished.capabilities().contains(&beta));
    assert_eq!(diminished.first_missing_requirement(), Some(beta.clone()));
    assert!(matches!(
        calc.verify_cell(&path("/secure/result")),
        Err(CalcError::Cell(CellFailure::RequiredCapability {
            capability,
            ..
        })) if capability == beta
    ));

    calc.set_dir_authority_policy(
        path("/secure"),
        AuthorityPolicyPatch {
            allow: Some(
                CapabilitySet::new()
                    .grant(alpha.clone())
                    .grant(beta.clone()),
            ),
            deny: CapabilitySet::new().grant(beta.clone()),
            required: CapabilitySet::new().grant(alpha.clone()),
        },
    );
    let diminished = calc.effective_authority(&path("/secure/result"));
    assert!(diminished.capabilities().contains(&alpha));
    assert!(!diminished.capabilities().contains(&beta));
    assert!(diminished.denied().contains(&beta));
    assert_eq!(
        value_expr(calc.verify_cell(&path("/secure/result")).unwrap()),
        Expr::String("must-not-escalate".to_owned())
    );
}

#[test]
fn authority_effect_ledger_evidence_is_preserved_in_receipt() {
    let effect_capability = CapabilityName::new("expr-tree.test.effect");
    let capability_for_factory = effect_capability.clone();
    let mut calc =
        ExprTreeCalc::with_context_factory(move || effect_context(capability_for_factory.clone()));
    calc.set_tree_authority_policy(AuthorityPolicyPatch {
        required: CapabilitySet::new().grant(effect_capability.clone()),
        ..AuthorityPolicyPatch::default()
    });
    calc.set_cell(path("/effect"), call("effectful", Vec::new()));
    assert_eq!(
        value_expr(calc.verify_cell(&path("/effect")).unwrap()),
        Expr::String("effect-ok".to_owned())
    );
    let receipt = calc.receipt(&path("/effect")).unwrap();
    assert_eq!(receipt.effects.len(), 1);
    assert_eq!(receipt.effects[0].kind, "expr-tree/test-effect");
    assert!(!receipt.effects[0].aborted);
    assert_eq!(receipt.omitted_effects, 0);

    calc.set_cell_authority_policy(
        path("/effect"),
        AuthorityPolicyPatch {
            deny: CapabilitySet::new().grant(effect_capability.clone()),
            required: CapabilitySet::new().grant(effect_capability.clone()),
            ..AuthorityPolicyPatch::default()
        },
    );
    calc.set_cell(path("/effect"), call("effectful", Vec::new()));
    assert!(matches!(
        calc.verify_cell(&path("/effect")),
        Err(CalcError::Cell(CellFailure::RequiredCapability {
            capability,
            ..
        })) if capability == effect_capability
    ));
    assert!(calc.receipt(&path("/effect")).unwrap().effects.is_empty());
}

#[test]
fn automatic_scheduler_honors_debounce_priority_fairness_cancellation_and_restart() {
    let mut calc = ExprTreeCalc::new();
    calc.set_cell_calc_policy(
        path("/low"),
        CalcPolicyPatch {
            priority: Some(-10),
            ..CalcPolicyPatch::default()
        },
    );
    calc.set_cell_calc_policy(
        path("/high"),
        CalcPolicyPatch {
            priority: Some(10),
            ..CalcPolicyPatch::default()
        },
    );
    calc.set_cell_calc_policy(
        path("/later"),
        CalcPolicyPatch {
            priority: Some(100),
            debounce_ms: Some(100),
            ..CalcPolicyPatch::default()
        },
    );
    calc.set_cell(path("/low"), Expr::String("low".to_owned()));
    calc.set_cell(path("/high"), Expr::String("high-0".to_owned()));
    calc.set_cell(path("/later"), Expr::String("later".to_owned()));

    let turn = AutomaticBudget::new(1, CalcLimits::default());
    let first = calc.run_automatic(turn, 0);
    assert!(first.continuation.is_some());
    assert_eq!(
        value_expr(calc.current_cell(&path("/high")).unwrap()),
        Expr::String("high-0".to_owned()),
        "the highest ready priority must run first"
    );
    assert!(matches!(
        calc.current_cell(&path("/later")),
        Err(CalcError::NotCalculated { .. })
    ));

    for revision in 1..=2 {
        calc.set_cell(path("/high"), Expr::String(format!("high-{revision}")));
        calc.run_automatic(turn, 0);
    }
    calc.set_cell(path("/high"), Expr::String("high-3".to_owned()));
    calc.run_automatic(turn, 0);
    assert_eq!(
        value_expr(calc.current_cell(&path("/low")).unwrap()),
        Expr::String("low".to_owned()),
        "a ready low-priority cell must run after bounded bypasses"
    );

    let before_debounce = calc.run_automatic(AutomaticBudget::default(), 99);
    assert!(before_debounce.continuation.is_some());
    assert!(matches!(
        calc.current_cell(&path("/later")),
        Err(CalcError::NotCalculated { .. })
    ));
    calc.run_automatic(AutomaticBudget::default(), 100);
    assert_eq!(
        value_expr(calc.current_cell(&path("/later")).unwrap()),
        Expr::String("later".to_owned())
    );

    calc.set_cell(path("/cancel"), Expr::String("cancelled".to_owned()));
    let cancel_id = calc
        .automatic_queue_snapshot()
        .entries
        .iter()
        .find(|entry| entry.cell == "/cancel")
        .unwrap()
        .request_id;
    assert!(calc.cancel_request(cancel_id));
    assert!(
        !calc
            .automatic_queue_snapshot()
            .entries
            .iter()
            .any(|entry| entry.cell == "/cancel")
    );

    calc.set_cell(path("/restart"), Expr::String("restored".to_owned()));
    let snapshot = calc.automatic_queue_snapshot();
    let mut restored = ExprTreeCalc::new();
    restored.set_cell(path("/restart"), Expr::String("restored".to_owned()));
    restored.restore_automatic_queue(snapshot.clone()).unwrap();
    assert_eq!(restored.automatic_queue_snapshot(), snapshot);
    restored.run_automatic(AutomaticBudget::default(), 100);
    assert_eq!(
        value_expr(restored.current_cell(&path("/restart")).unwrap()),
        Expr::String("restored".to_owned())
    );
}

#[test]
fn automatic_budget_exhaustion_returns_and_resumes_explicit_continuation() {
    let mut calc = ExprTreeCalc::new();
    calc.set_cell_calc_policy(
        path("/root"),
        CalcPolicyPatch {
            priority: Some(10),
            ..CalcPolicyPatch::default()
        },
    );
    calc.set_cell(path("/leaf"), Expr::String("leaf".to_owned()));
    calc.set_cell(path("/root"), explicit_ref("/leaf"));
    let stopped = calc.run_automatic(AutomaticBudget::new(1, CalcLimits::new(1, 100, 10, 100)), 0);
    assert_eq!(stopped.budget_exhausted.len(), 1);
    let continuation = stopped.continuation.expect("queue work must remain");
    let resumed = calc
        .continue_automatic(
            continuation,
            AutomaticBudget::new(1, CalcLimits::default()),
            0,
        )
        .unwrap();
    assert_eq!(resumed.completed.len(), 1);
    assert_eq!(
        value_expr(calc.current_cell(&path("/root")).unwrap()),
        Expr::String("leaf".to_owned())
    );
}

#[test]
fn stream_progress_changes_are_bounded_observable_and_cancellable() {
    let mut calc = ExprTreeCalc::new();
    let events = calc
        .watch(BufferPolicy::bounded_with_overflow(32, BufferOverflowPolicy::DropNewest).unwrap());
    let overflow = calc
        .watch(BufferPolicy::bounded_with_overflow(1, BufferOverflowPolicy::DropNewest).unwrap());
    assert_eq!(events.stream().metadata().buffer().capacity(), 32);
    assert_eq!(overflow.stream().metadata().buffer().capacity(), 1);

    calc.set_cell(path("/watched"), Expr::String("value".to_owned()));
    calc.run_automatic(AutomaticBudget::default(), 0);
    assert!(
        overflow.overflow_evidence() > 0,
        "a full endpoint must retain explicit overflow evidence"
    );
    assert!(overflow.stream().stats().unwrap().dropped_newest > 0);

    let mut packet_kinds = Vec::new();
    while let Some(item) = events.next().unwrap() {
        let StreamPacket::Data(data) = item.packet() else {
            panic!("expression-tree watches must use standard data packets");
        };
        let Expr::Map(fields) = &data.payload else {
            panic!("watch payload must be an ordinary expression map");
        };
        let kind = fields.iter().find_map(|(key, value)| match (key, value) {
            (Expr::Symbol(key), Expr::Symbol(value)) if key.to_string() == "kind" => {
                Some(value.to_string())
            }
            _ => None,
        });
        if let Some(kind) = kind {
            packet_kinds.push(kind);
        }
    }
    assert!(packet_kinds.iter().any(|kind| kind == "expr-tree/change"));
    assert!(packet_kinds.iter().any(|kind| kind == "expr-tree/progress"));

    overflow.cancel().unwrap();
    assert!(overflow.stream().stats().unwrap().cancelled);
    assert!(overflow.next().unwrap().is_none());
}

mod receipt;

mod verification;
