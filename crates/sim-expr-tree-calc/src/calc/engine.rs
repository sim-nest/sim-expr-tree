use super::attempt::*;
use super::*;

impl ExprTreeCalc {
    pub(super) fn register_cell_query(&mut self, key: String) {
        let state = Arc::clone(&self.state);
        let context_factory = Arc::clone(&self.context_factory);
        let cancel_requested = Arc::clone(&self.cancel_requested);
        let next_volatile = Arc::clone(&self.next_volatile);
        let wall_clock = Arc::clone(&self.wall_clock);
        self.engine
            .register_fn(CalcQuery::Cell(key), move |query, frame| {
                let CalcQuery::Cell(cell_key) = query else {
                    return Err(IncrementalError::UnknownQuery { key: query.clone() });
                };
                if cancel_requested.swap(false, Ordering::AcqRel) {
                    frame.cancel();
                    frame.charge_work(0)?;
                }
                observe_runtime_context(frame, cell_key)?;
                frame.observe(
                    ObservationKind::Custom("cell-source"),
                    CalcQuery::Cell(cell_key.clone()),
                )?;
                let attempt = begin_attempt(&state, &wall_clock, cell_key);
                if let Some(blocked) = blocked_by_trigger(&attempt) {
                    let memo = MemoValue::failure(blocked);
                    finish_attempt(
                        &state,
                        &wall_clock,
                        attempt,
                        outcome_for_memo(&memo),
                        Vec::new(),
                        0,
                    );
                    return Ok(memo);
                }
                if let Some(capability) = attempt.authority.first_missing_requirement() {
                    let memo = MemoValue::failure(CellFailure::RequiredCapability {
                        path: cell_key.clone(),
                        capability,
                    });
                    finish_attempt(
                        &state,
                        &wall_clock,
                        attempt,
                        outcome_for_memo(&memo),
                        Vec::new(),
                        0,
                    );
                    return Ok(memo);
                }
                let source = state
                    .read()
                    .expect("calc state poisoned")
                    .cells
                    .get(cell_key)
                    .cloned();
                let evaluated = evaluate_cell(
                    &state,
                    frame,
                    &context_factory,
                    &next_volatile,
                    cell_key,
                    source,
                    attempt.authority.capabilities().clone(),
                );
                match evaluated {
                    Ok(evaluated) => {
                        let memo = apply_cycle_policy(evaluated.memo, attempt.policy, cell_key);
                        let outcome = outcome_for_memo(&memo);
                        finish_attempt(
                            &state,
                            &wall_clock,
                            attempt,
                            outcome,
                            evaluated.effects,
                            evaluated.omitted_effects,
                        );
                        Ok(memo)
                    }
                    Err(error) => {
                        finish_attempt(
                            &state,
                            &wall_clock,
                            attempt,
                            outcome_for_incremental_error(&error),
                            Vec::new(),
                            0,
                        );
                        Err(error)
                    }
                }
            });
    }

    pub(super) fn invalidate_cell_source(&mut self, path: &TablePath, namespace_changed: bool) {
        self.engine.invalidate(&CalcQuery::Cell(path_key(path)));
        if namespace_changed {
            self.engine.invalidate(&CalcQuery::NameSlot(path_key(path)));
            self.engine
                .invalidate(&CalcQuery::Listing(path_key(&parent_path(path))));
        }
    }

    pub(super) fn invalidate_failed_cells(&mut self, failed: Vec<String>) {
        for key in failed {
            self.engine.invalidate(&CalcQuery::Cell(key));
        }
    }

    pub(super) fn execute_root(
        &mut self,
        key: String,
        request: ActiveRequest,
        limits: CalcLimits,
        continuation: Option<ContinuationToken>,
    ) -> Result<Value, CalcError> {
        self.state
            .write()
            .expect("calc state poisoned")
            .active_request = Some(request.clone());
        self.emit_progress("running", &key, request.id);
        let raw = match continuation {
            Some(token) => match self.engine.resume(token, limits.clamped()) {
                Err(IncrementalError::UnknownContinuation { token: unknown })
                    if unknown == token && self.restored_continuations.remove(&token) =>
                {
                    // Incremental continuation ids are process-local engine
                    // handles. A persisted automatic continuation still owns
                    // the same root, so after restart it safely re-enters that
                    // root and reuses every restored dependency memo that
                    // remains current.
                    self.engine
                        .verify_with_budgets(CalcQuery::Cell(key.clone()), limits.clamped())
                }
                other => other,
            },
            None => self
                .engine
                .verify_with_budgets(CalcQuery::Cell(key.clone()), limits.clamped()),
        };
        self.state
            .write()
            .expect("calc state poisoned")
            .active_request = None;

        if let Err(error) = &raw {
            self.ensure_stopped_root_attempt(&request, &key, error);
        }
        self.commit_receipts(request.id);
        self.prune_satisfied_automatic();

        let result = raw.map_err(CalcError::Incremental).and_then(|memo| {
            let is_volatile = memo.is_volatile();
            match memo.outcome {
                MemoOutcome::Value(value) => Ok((value, is_volatile)),
                MemoOutcome::Failure(failure) => Err(CalcError::Cell(failure)),
            }
        });
        let result = self.commit_current_result(&key, result);
        self.emit_progress(
            if result.is_ok() { "finished" } else { "failed" },
            &key,
            request.id,
        );
        result
    }

    pub(super) fn commit_current_result(
        &mut self,
        key: &str,
        result: Result<(Value, bool), CalcError>,
    ) -> Result<Value, CalcError> {
        let mut state = self.state.write().expect("calc state poisoned");
        match result {
            Ok((value, is_volatile)) => {
                state.current.insert(key.to_owned(), Ok(value.clone()));
                state.last_good.insert(key.to_owned(), value.clone());
                state.failed_cells.remove(key);
                if is_volatile {
                    state.volatile.insert(key.to_owned());
                } else {
                    state.volatile.remove(key);
                }
                Ok(value)
            }
            Err(error) => {
                state.current.insert(key.to_owned(), Err(error.clone()));
                state.volatile.remove(key);
                state.failed_cells.insert(key.to_owned());
                Err(error)
            }
        }
    }

    pub(super) fn commit_receipts(&mut self, request_id: RequestId) {
        let attempts = {
            let mut state = self.state.write().expect("calc state poisoned");
            let mut selected = Vec::new();
            let mut retained = Vec::new();
            for attempt in state.attempts.drain(..) {
                if attempt.request_id == request_id {
                    selected.push(attempt);
                } else {
                    retained.push(attempt);
                }
            }
            state.attempts = retained;
            selected
        };
        for attempt in attempts {
            let query = CalcQuery::Cell(attempt.cell.clone());
            let node = self
                .engine
                .snapshot(
                    [query.clone()],
                    SnapshotBudgets::new(MAX_RECEIPT_GRAPH_NODES, MAX_RECEIPT_GRAPH_EDGES),
                )
                .ok()
                .and_then(|snapshot| snapshot.nodes.into_iter().find(|node| node.key == query));
            let all_dependencies = node
                .as_ref()
                .map(|node| node.dependencies.as_slice())
                .unwrap_or_default();
            let dependency_digest = dependency_digest(all_dependencies);
            let dependencies = all_dependencies
                .iter()
                .take(MAX_RECEIPT_DEPENDENCIES)
                .map(|observation| DependencyStamp {
                    query: observation.key().clone(),
                    kind: observation.kind().clone(),
                    revision: observation.revision().get(),
                    fingerprint: observation.fingerprint().map(ValueFingerprint::get),
                })
                .collect::<Vec<_>>();
            let succeeded = matches!(attempt.outcome, CalcOutcome::Succeeded);
            let blocked = matches!(attempt.outcome, CalcOutcome::Blocked { .. });
            let receipt = CalcReceipt {
                request_id,
                cell: attempt.cell.clone(),
                source_revision: self.engine.source_revision(&query).get(),
                policy_digest: attempt.policy.digest(),
                authority_digest: attempt.authority.digest(),
                dependencies,
                omitted_dependencies: all_dependencies
                    .len()
                    .saturating_sub(MAX_RECEIPT_DEPENDENCIES),
                dependency_digest,
                effects: attempt.effects,
                omitted_effects: attempt.omitted_effects,
                started_tick: attempt.started_tick,
                finished_tick: attempt.finished_tick,
                wall_started_ms: attempt.wall_started_ms,
                wall_finished_ms: attempt.wall_finished_ms,
                outcome: attempt.outcome,
                result_fingerprint: succeeded
                    .then(|| {
                        node.and_then(|node| node.fingerprint)
                            .map(ValueFingerprint::get)
                    })
                    .flatten(),
                reason: attempt.reason,
                trigger: attempt.policy.trigger,
            };
            self.state
                .write()
                .expect("calc state poisoned")
                .receipts
                .insert(attempt.cell.clone(), receipt);
            if blocked {
                // Blocking is request-contextual (for example an automatic
                // caller reaching a manual dependency), so it must never
                // become a reusable success-equivalent memo for a later
                // directed request.
                self.engine
                    .invalidate(&CalcQuery::ForceEpoch(attempt.cell.clone()));
            }
            self.emit_progress("receipt", &attempt.cell, request_id);
        }
    }

    pub(super) fn ensure_stopped_root_attempt(
        &self,
        request: &ActiveRequest,
        key: &str,
        error: &IncrementalError<CalcQuery>,
    ) {
        let exists = self
            .state
            .read()
            .expect("calc state poisoned")
            .attempts
            .iter()
            .any(|attempt| attempt.request_id == request.id && attempt.cell == key);
        if exists {
            return;
        }
        let started_wall = self.wall_now();
        let finished_wall = self.wall_now();
        let mut state = self.state.write().expect("calc state poisoned");
        let policy = effective_calc_policy(
            &state.tree_calc_policy,
            &state.dir_calc_policies,
            &state.cell_calc_policies,
            key,
        );
        let authority = effective_authority(
            &state.authority_ceiling,
            &state.tree_authority_policy,
            &state.dir_authority_policies,
            &state.cell_authority_policies,
            key,
        );
        let started_tick = allocate_logical_tick(&mut state);
        let finished_tick = allocate_logical_tick(&mut state);
        state.attempts.push(AttemptDraft {
            request_id: request.id,
            cell: key.to_owned(),
            policy,
            authority,
            started_tick,
            finished_tick,
            wall_started_ms: started_wall,
            wall_finished_ms: finished_wall,
            outcome: outcome_for_incremental_error(error),
            effects: Vec::new(),
            omitted_effects: 0,
            reason: request.reason,
        });
    }

    pub(super) fn force_recursive_closure(&mut self, roots: &BTreeSet<String>) -> BTreeSet<String> {
        let mut cells = roots.clone();
        for root in roots {
            if let Ok(snapshot) = self.engine.snapshot(
                [CalcQuery::Cell(root.clone())],
                SnapshotBudgets::new(MAX_RECEIPT_GRAPH_NODES, MAX_RECEIPT_GRAPH_EDGES),
            ) {
                cells.extend(
                    snapshot
                        .nodes
                        .into_iter()
                        .filter_map(|node| match node.key {
                            CalcQuery::Cell(cell) => Some(cell),
                            _ => None,
                        }),
                );
            }
        }
        cells
    }

    pub(super) fn invalidate_calc_policy_matching(
        &mut self,
        predicate: impl Fn(&TablePath) -> bool,
    ) {
        let cells = self
            .state
            .read()
            .expect("calc state poisoned")
            .cells
            .keys()
            .filter(|cell| predicate(&parse_absolute_path(cell)))
            .cloned()
            .collect::<Vec<_>>();
        for cell in cells {
            self.engine
                .invalidate(&CalcQuery::EffectivePolicy(cell.clone()));
            self.emit_change("calculation-policy", &cell);
        }
    }

    pub(super) fn invalidate_authority_policy_matching(
        &mut self,
        predicate: impl Fn(&TablePath) -> bool,
    ) {
        let cells = self
            .state
            .read()
            .expect("calc state poisoned")
            .cells
            .keys()
            .filter(|cell| predicate(&parse_absolute_path(cell)))
            .cloned()
            .collect::<Vec<_>>();
        for cell in cells {
            self.engine
                .invalidate(&CalcQuery::AuthorityPolicy(cell.clone()));
            self.emit_change("authority-policy", &cell);
        }
    }
}
