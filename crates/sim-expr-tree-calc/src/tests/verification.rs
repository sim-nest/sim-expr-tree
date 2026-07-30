use super::support::*;
use super::*;

#[test]
fn verify_ordinary_expr_returns_ordinary_value_and_tracks_runtime_references() {
    let mut calc = ExprTreeCalc::new();
    calc.set_cell(path("/sheet/a"), Expr::String("A".to_owned()));
    calc.set_cell(path("/sheet/b"), explicit_ref("a"));
    calc.set_cell(path("/sheet/c"), Expr::Symbol(Symbol::new("a")));

    assert_eq!(
        value_expr(calc.verify_cell(&path("/sheet/b")).unwrap()),
        Expr::String("A".to_owned())
    );
    assert_eq!(
        value_expr(calc.verify_cell(&path("/sheet/c")).unwrap()),
        Expr::String("A".to_owned())
    );
    assert!(dependencies(&mut calc, "/sheet/b").contains(&(
        CalcQuery::Cell("/sheet/a".to_owned()),
        ObservationKind::Read,
    )));
    assert!(dependencies(&mut calc, "/sheet/c").contains(&(
        CalcQuery::Cell("/sheet/a".to_owned()),
        ObservationKind::Read,
    )));

    calc.set_cell(
        path("/sheet/quoted"),
        Expr::Quote {
            mode: sim_kernel::QuoteMode::Quote,
            expr: Box::new(Expr::Symbol(Symbol::new("future-name"))),
        },
    );
    assert_eq!(
        value_expr(calc.verify_cell(&path("/sheet/quoted")).unwrap()),
        Expr::Symbol(Symbol::new("future-name"))
    );
    assert!(
        !dependencies(&mut calc, "/sheet/quoted")
            .iter()
            .any(|(key, _)| matches!(key, CalcQuery::NameSlot(_)))
    );
}

#[test]
fn verify_lexical_binding_invalidates_prior_tree_fallback() {
    let mut calc = ExprTreeCalc::new();
    calc.set_cell(path("/sheet/value"), Expr::String("tree".to_owned()));
    calc.set_cell(path("/sheet/use-value"), Expr::Symbol(Symbol::new("value")));
    assert_eq!(
        value_expr(calc.verify_cell(&path("/sheet/use-value")).unwrap()),
        Expr::String("tree".to_owned())
    );

    let cx = strict_context();
    let bound = cx.factory().string("bound".to_owned()).unwrap();
    calc.bind_value(Symbol::new("value"), bound);
    assert_eq!(
        value_expr(calc.verify_cell(&path("/sheet/use-value")).unwrap()),
        Expr::String("bound".to_owned())
    );
}

#[test]
fn verify_macro_created_reference_is_observed_after_expansion() {
    struct RefMacro;

    impl MacroExpander for RefMacro {
        fn expand_expr(&self, _cx: &mut Cx, _phase: Phase, expr: Expr) -> sim_kernel::Result<Expr> {
            if matches!(
                expr,
                Expr::Extension { ref tag, .. } if tag.to_string() == "test/macro-input"
            ) {
                Ok(explicit_ref("a"))
            } else {
                Ok(expr)
            }
        }
    }

    let mut calc = ExprTreeCalc::with_context_factory(|| {
        let (mut cx, seat) = Cx::new_seated(
            Arc::new(ExprTreeRefPolicy::new(StrictNames(EagerPolicy))),
            Arc::new(DefaultFactory),
        );
        seat.grant(&mut cx, macro_expansion_capability_for_phase(Phase::Eval))
            .unwrap();
        cx.set_macro_expander(Arc::new(RefMacro));
        cx
    });
    calc.set_cell(path("/sheet/a"), Expr::String("macro-value".to_owned()));
    calc.set_cell(
        path("/sheet/use"),
        Expr::Extension {
            tag: Symbol::new("test/macro-input"),
            payload: Box::new(Expr::Nil),
        },
    );
    assert_eq!(
        value_expr(calc.verify_cell(&path("/sheet/use")).unwrap()),
        Expr::String("macro-value".to_owned())
    );
    assert!(dependencies(&mut calc, "/sheet/use").contains(&(
        CalcQuery::Cell("/sheet/a".to_owned()),
        ObservationKind::Read,
    )));
}

#[test]
fn verify_diamond_changing_and_unchanged_branches_match_full_recomputation() {
    let runtime = TestRuntime::default();
    *runtime.source.lock().unwrap() = "one".to_owned();
    let mut calc = runtime_calc(runtime.clone());
    install_diamond(&mut calc);

    let first = value_expr(calc.verify_cell(&path("/d")).unwrap());
    assert_eq!(first, Expr::String("one-bone-c".to_owned()));
    assert_eq!(
        (
            runtime.count("probe"),
            runtime.count("concat-b"),
            runtime.count("concat-c"),
            runtime.count("concat-d"),
        ),
        (1, 1, 1, 1)
    );

    *runtime.source.lock().unwrap() = "two".to_owned();
    calc.set_cell(path("/a"), probe_expr());
    let changed = value_expr(calc.verify_cell(&path("/d")).unwrap());
    assert_eq!(changed, Expr::String("two-btwo-c".to_owned()));
    assert_eq!(
        (
            runtime.count("probe"),
            runtime.count("concat-b"),
            runtime.count("concat-c"),
            runtime.count("concat-d"),
        ),
        (2, 2, 2, 2)
    );

    let full_runtime = TestRuntime::default();
    *full_runtime.source.lock().unwrap() = "two".to_owned();
    let mut full = runtime_calc(full_runtime);
    install_diamond(&mut full);
    assert_eq!(value_expr(full.verify_cell(&path("/d")).unwrap()), changed);

    calc.set_cell(path("/a"), probe_expr());
    let unchanged = value_expr(calc.verify_cell(&path("/d")).unwrap());
    assert_eq!(unchanged, changed);
    assert_eq!(
        (
            runtime.count("probe"),
            runtime.count("concat-b"),
            runtime.count("concat-c"),
            runtime.count("concat-d"),
        ),
        (3, 2, 2, 2),
        "canonical cutoff must stop the unchanged branch at /a"
    );
}

#[test]
fn cycle_dynamic_path_is_deterministic_and_recovers() {
    let mut calc = ExprTreeCalc::new();
    calc.set_cell(path("/a"), explicit_ref("/b"));
    calc.set_cell(path("/b"), Expr::String("ready".to_owned()));
    assert_eq!(
        value_expr(calc.verify_cell(&path("/a")).unwrap()),
        Expr::String("ready".to_owned())
    );

    calc.set_cell(path("/b"), explicit_ref("/a"));
    let first = calc.verify_cell(&path("/a")).unwrap_err();
    let expected = CalcError::Cell(CellFailure::Cycle {
        path: vec![
            CalcQuery::Cell("/a".to_owned()),
            CalcQuery::Cell("/b".to_owned()),
            CalcQuery::Cell("/a".to_owned()),
        ],
    });
    assert_eq!(first, expected);
    assert_eq!(calc.verify_cell(&path("/a")).unwrap_err(), expected);

    calc.set_cell(path("/b"), Expr::String("recovered".to_owned()));
    assert_eq!(
        value_expr(calc.verify_cell(&path("/a")).unwrap()),
        Expr::String("recovered".to_owned())
    );
}

#[test]
fn cycle_hard_depth_and_output_limits_override_requested_policy() {
    let mut deep = ExprTreeCalc::new();
    for index in 0..=HARD_MAX_QUERY_DEPTH {
        let source = if index == HARD_MAX_QUERY_DEPTH {
            Expr::String("bottom".to_owned())
        } else {
            explicit_ref(&format!("/chain/{}", index + 1))
        };
        deep.set_cell(path(&format!("/chain/{index}")), source);
    }
    let generous = CalcLimits::new(usize::MAX, usize::MAX, usize::MAX, usize::MAX);
    assert!(matches!(
        deep.verify_cell_with_limits(&path("/chain/0"), generous),
        Err(CalcError::Incremental(IncrementalError::BudgetExceeded {
            kind: BudgetKind::Depth,
            limit: HARD_MAX_QUERY_DEPTH,
            ..
        }))
    ));

    let mut output = ExprTreeCalc::new();
    output.set_cell(
        path("/large"),
        Expr::String("x".repeat(HARD_MAX_OUTPUT + 1)),
    );
    assert!(matches!(
        output.verify_cell_with_limits(&path("/large"), generous),
        Err(CalcError::Incremental(IncrementalError::BudgetExceeded {
            kind: BudgetKind::Output,
            limit: HARD_MAX_OUTPUT,
            ..
        }))
    ));

    let mut nested = Expr::Nil;
    for _ in 0..=HARD_MAX_EXPR_DEPTH {
        nested = Expr::List(vec![nested]);
    }
    let mut recursion = ExprTreeCalc::new();
    recursion.set_cell(path("/nested"), nested);
    assert_eq!(
        recursion.verify_cell_with_limits(&path("/nested"), generous),
        Err(CalcError::Cell(CellFailure::ExpressionDepth {
            limit: HARD_MAX_EXPR_DEPTH,
        }))
    );
}

#[test]
fn verify_deep_chain_matches_full_recomputation() {
    const LAST: usize = HARD_MAX_QUERY_DEPTH - 1;

    fn install_chain(calc: &mut ExprTreeCalc, leaf: &str) {
        for index in 0..=LAST {
            let source = if index == LAST {
                Expr::String(leaf.to_owned())
            } else {
                explicit_ref(&format!("/chain/{}", index + 1))
            };
            calc.set_cell(path(&format!("/chain/{index}")), source);
        }
    }

    let mut incremental = ExprTreeCalc::new();
    install_chain(&mut incremental, "before");
    assert_eq!(
        value_expr(incremental.verify_cell(&path("/chain/0")).unwrap()),
        Expr::String("before".to_owned())
    );
    incremental.set_cell(
        path(&format!("/chain/{LAST}")),
        Expr::String("after".to_owned()),
    );
    let changed = value_expr(incremental.verify_cell(&path("/chain/0")).unwrap());

    let mut full = ExprTreeCalc::new();
    install_chain(&mut full, "after");
    assert_eq!(
        value_expr(full.verify_cell(&path("/chain/0")).unwrap()),
        changed
    );
}

#[test]
fn arbitrary_value_callable_lambda_table_dir_and_opaque_are_retained() {
    let runtime = TestRuntime::default();
    let mut calc = runtime_calc(runtime);
    calc.set_cell(
        path("/lambda-call"),
        call("lambda", vec![Expr::String("lambda-value".to_owned())]),
    );
    let table_source = Expr::Map(vec![(
        Expr::Quote {
            mode: sim_kernel::QuoteMode::Quote,
            expr: Box::new(Expr::Symbol(Symbol::new("field"))),
        },
        Expr::String("table-value".to_owned()),
    )]);
    calc.set_cell(path("/table"), table_source.clone());
    calc.set_cell(path("/callable"), call("return-lambda", vec![]));
    calc.set_cell(path("/dir"), call("return-dir", vec![]));
    calc.set_cell(path("/opaque"), call("return-opaque", vec![]));

    assert_eq!(
        value_expr(calc.verify_cell(&path("/lambda-call")).unwrap()),
        Expr::String("lambda-value".to_owned())
    );
    let table = calc.verify_cell(&path("/table")).unwrap();
    assert!(table.object().as_table_impl().is_some());
    assert_eq!(
        value_expr(table),
        Expr::Map(vec![(
            Expr::Symbol(Symbol::new("field")),
            Expr::String("table-value".to_owned()),
        )])
    );
    assert!(!calc.current_is_volatile(&path("/table")));
    let table_revision = calc.cell_revision(&path("/table")).unwrap();
    calc.set_cell(path("/table"), table_source);
    calc.verify_cell(&path("/table")).unwrap();
    assert_eq!(
        calc.cell_revision(&path("/table")).unwrap(),
        table_revision,
        "a canonical Table result must stop an unchanged branch"
    );
    let callable = calc.verify_cell(&path("/callable")).unwrap();
    assert!(callable.object().as_callable().is_some());
    assert!(calc.current_is_volatile(&path("/callable")));
    let dir = calc.verify_cell(&path("/dir")).unwrap();
    assert!(dir.object().as_dir().is_some());
    assert!(calc.current_is_volatile(&path("/dir")));
    let opaque = calc.verify_cell(&path("/opaque")).unwrap();
    assert!(opaque.object().downcast_ref::<OpaqueMarker>().is_some());
    assert!(calc.current_is_volatile(&path("/opaque")));

    let old_revision = calc.cell_revision(&path("/opaque")).unwrap();
    calc.set_cell(path("/opaque"), call("return-opaque", vec![]));
    calc.verify_cell(&path("/opaque")).unwrap();
    assert!(
        calc.cell_revision(&path("/opaque")).unwrap() > old_revision,
        "a valid noncanonical value must conservatively count as changed"
    );
}

#[test]
fn verify_failure_memo_retains_labelled_last_good_and_recovers() {
    let runtime = TestRuntime::default();
    let mut calc = runtime_calc(runtime.clone());
    let source = call("fallible", vec![]);
    calc.set_cell(path("/unstable"), source.clone());
    assert_eq!(
        value_expr(calc.verify_cell(&path("/unstable")).unwrap()),
        Expr::String("good".to_owned())
    );

    runtime.fail.store(true, Ordering::Release);
    calc.set_cell(path("/unstable"), source.clone());
    let error = calc.verify_cell(&path("/unstable")).unwrap_err();
    assert!(matches!(
        error,
        CalcError::Cell(CellFailure::Evaluation { .. })
    ));
    assert_eq!(calc.current_cell(&path("/unstable")).unwrap_err(), error);
    let retained = calc.last_good_cell(&path("/unstable")).unwrap();
    assert_eq!(retained.label(), "last-good");
    assert_eq!(
        value_expr(retained.value().clone()),
        Expr::String("good".to_owned())
    );

    let attempts = runtime.fail_attempts.load(Ordering::Acquire);
    assert_eq!(calc.verify_cell(&path("/unstable")).unwrap_err(), error);
    assert_eq!(
        runtime.fail_attempts.load(Ordering::Acquire),
        attempts,
        "a current failure memo must not rerun user evaluation"
    );

    runtime.fail.store(false, Ordering::Release);
    calc.set_cell(path("/unstable"), source);
    assert_eq!(
        value_expr(calc.verify_cell(&path("/unstable")).unwrap()),
        Expr::String("good".to_owned())
    );
    assert!(calc.current_cell(&path("/unstable")).is_ok());
}

#[test]
fn verify_cancellation_fails_current_read_and_preserves_last_good() {
    let mut calc = ExprTreeCalc::new();
    calc.set_cell(path("/cell"), Expr::String("old".to_owned()));
    calc.verify_cell(&path("/cell")).unwrap();
    calc.set_cell(path("/cell"), Expr::String("new".to_owned()));
    calc.request_cancellation();
    assert_eq!(
        calc.verify_cell(&path("/cell")),
        Err(CalcError::Incremental(IncrementalError::Cancelled))
    );
    assert_eq!(
        calc.current_cell(&path("/cell")),
        Err(CalcError::Incremental(IncrementalError::Cancelled))
    );
    assert_eq!(
        value_expr(calc.last_good_cell(&path("/cell")).unwrap().value().clone()),
        Expr::String("old".to_owned())
    );
    let recovered = value_expr(calc.verify_cell(&path("/cell")).unwrap());
    let mut full = ExprTreeCalc::new();
    full.set_cell(path("/cell"), Expr::String("new".to_owned()));
    assert_eq!(
        value_expr(full.verify_cell(&path("/cell")).unwrap()),
        recovered
    );
}

#[test]
fn verify_no_calc_state_lock_spans_context_creation_or_sim_evaluation() {
    let mut calc = ExprTreeCalc::new();
    let state = calc.state_for_lock_probe();
    let factory_state = Arc::clone(&state);
    calc.replace_context_factory(move || {
        let guard = factory_state
            .try_write()
            .expect("calculator state lock spanned SIM context creation");
        drop(guard);
        lock_probe_context(Arc::clone(&factory_state))
    });
    calc.set_cell(path("/cell"), call("lock-probe", vec![]));
    assert_eq!(
        value_expr(calc.verify_cell(&path("/cell")).unwrap()),
        Expr::String("unlocked".to_owned())
    );
}

#[test]
fn verify_namespace_mount_policy_codec_and_authority_observations_invalidate() {
    let mut calc = ExprTreeCalc::new();
    calc.set_tree_calc_policy(CalcPolicyPatch {
        priority: Some(7),
        ..CalcPolicyPatch::default()
    });
    calc.set_codec_registry_revision(7);
    calc.set_tree_authority_policy(AuthorityPolicyPatch::default());
    calc.mount(
        path("/remote"),
        MountResource::Dir,
        BackendKind::Database,
        MountEpoch::new(1),
    );
    calc.set_cell(path("/remote/value"), Expr::String("old".to_owned()));
    calc.set_cell(path("/sheet/use-remote"), explicit_ref("/remote/value"));
    calc.verify_cell(&path("/sheet/use-remote")).unwrap();
    let deps = dependencies(&mut calc, "/sheet/use-remote");
    assert!(deps.contains(&(
        CalcQuery::EffectivePolicy("/sheet/use-remote".to_owned()),
        ObservationKind::Policy,
    )));
    assert!(deps.contains(&(
        CalcQuery::CodecRegistry,
        ObservationKind::Custom("codec-registry"),
    )));
    assert!(deps.contains(&(CalcQuery::AuthorityCeiling, ObservationKind::Policy)));
    assert!(deps.contains(&(
        CalcQuery::AuthorityPolicy("/sheet/use-remote".to_owned()),
        ObservationKind::Policy,
    )));
    assert!(deps.contains(&(
        CalcQuery::MountEpoch("/remote".to_owned()),
        ObservationKind::Epoch,
    )));

    calc.move_cell(&path("/remote/value"), path("/remote/moved"));
    assert_eq!(
        value_expr(calc.verify_cell(&path("/sheet/use-remote")).unwrap()),
        Expr::String("missing:/remote/value".to_owned())
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
