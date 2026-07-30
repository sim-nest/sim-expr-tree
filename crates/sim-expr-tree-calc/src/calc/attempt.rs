use super::*;

pub(super) struct AttemptStart {
    pub(super) request: ActiveRequest,
    pub(super) policy: EffectiveCalcPolicy,
    pub(super) authority: EffectiveAuthority,
    pub(super) started_tick: u64,
    pub(super) wall_started_ms: Option<u64>,
    pub(super) cell: String,
}

pub(super) fn begin_attempt(
    state: &Arc<RwLock<CalcState>>,
    wall_clock: &Arc<RwLock<Arc<WallClock>>>,
    cell: &str,
) -> AttemptStart {
    let wall_started_ms = {
        let clock = wall_clock.read().expect("wall clock lock poisoned").clone();
        clock()
    };
    let mut state = state.write().expect("calc state poisoned");
    let request = state.active_request.clone().unwrap_or(ActiveRequest {
        id: RequestId::new(0),
        reason: CalcReason::DirectedVerify,
        directed_cells: BTreeSet::from([cell.to_owned()]),
        automatic: false,
    });
    let policy = effective_calc_policy(
        &state.tree_calc_policy,
        &state.dir_calc_policies,
        &state.cell_calc_policies,
        cell,
    );
    let authority = effective_authority(
        &state.authority_ceiling,
        &state.tree_authority_policy,
        &state.dir_authority_policies,
        &state.cell_authority_policies,
        cell,
    );
    let started_tick = allocate_logical_tick(&mut state);
    AttemptStart {
        request,
        policy,
        authority,
        started_tick,
        wall_started_ms,
        cell: cell.to_owned(),
    }
}

pub(super) fn finish_attempt(
    state: &Arc<RwLock<CalcState>>,
    wall_clock: &Arc<RwLock<Arc<WallClock>>>,
    attempt: AttemptStart,
    outcome: CalcOutcome,
    effects: Vec<EffectStamp>,
    omitted_effects: usize,
) {
    let wall_finished_ms = {
        let clock = wall_clock.read().expect("wall clock lock poisoned").clone();
        clock()
    };
    let mut state = state.write().expect("calc state poisoned");
    let finished_tick = allocate_logical_tick(&mut state);
    state.attempts.push(AttemptDraft {
        request_id: attempt.request.id,
        cell: attempt.cell,
        policy: attempt.policy,
        authority: attempt.authority,
        started_tick: attempt.started_tick,
        finished_tick,
        wall_started_ms: attempt.wall_started_ms,
        wall_finished_ms,
        outcome,
        effects,
        omitted_effects,
        reason: attempt.request.reason,
    });
}

pub(super) fn blocked_by_trigger(attempt: &AttemptStart) -> Option<CellFailure> {
    let directed = attempt.request.directed_cells.contains(&attempt.cell);
    let reason = match attempt.policy.trigger {
        CalcTrigger::Automatic => None,
        CalcTrigger::OnDemand if attempt.request.automatic => {
            Some("on-demand cells do not run from automatic mutation".to_owned())
        }
        CalcTrigger::OnDemand => None,
        CalcTrigger::Manual if attempt.request.automatic => {
            Some("manual cells require an explicit directed root".to_owned())
        }
        CalcTrigger::Manual if !directed => {
            Some("manual dependency was not an explicitly directed root".to_owned())
        }
        CalcTrigger::Manual => None,
        CalcTrigger::Frozen => Some("frozen policy forbids new calculation work".to_owned()),
    };
    reason.map(|reason| CellFailure::Blocked {
        path: attempt.cell.clone(),
        reason,
    })
}

pub(super) fn apply_cycle_policy(
    memo: MemoValue,
    policy: EffectiveCalcPolicy,
    cell: &str,
) -> MemoValue {
    if policy.cycle_mode == CycleMode::Block
        && let MemoOutcome::Failure(CellFailure::Cycle { path }) = &memo.outcome
    {
        return MemoValue::failure(CellFailure::Blocked {
            path: cell.to_owned(),
            reason: format!("dynamic dependency cycle {path:?}"),
        });
    }
    memo
}

pub(super) fn outcome_for_memo(memo: &MemoValue) -> CalcOutcome {
    match &memo.outcome {
        MemoOutcome::Value(_) => CalcOutcome::Succeeded,
        MemoOutcome::Failure(CellFailure::Blocked { reason, .. }) => CalcOutcome::Blocked {
            message: reason.clone(),
        },
        MemoOutcome::Failure(CellFailure::RequiredCapability { path, capability }) => {
            CalcOutcome::Blocked {
                message: format!("cell {path} requires capability {capability}"),
            }
        }
        MemoOutcome::Failure(failure) => CalcOutcome::Failed {
            message: failure.to_string(),
        },
    }
}

pub(super) fn outcome_for_incremental_error(error: &IncrementalError<CalcQuery>) -> CalcOutcome {
    match error {
        IncrementalError::Cancelled => CalcOutcome::Cancelled,
        IncrementalError::BudgetExceeded { continuation, .. } => CalcOutcome::BudgetExhausted {
            message: error.to_string(),
            continuation: continuation.map(ContinuationToken::get),
        },
        IncrementalError::UnknownQuery { .. }
        | IncrementalError::Cycle { .. }
        | IncrementalError::UnknownContinuation { .. } => CalcOutcome::Failed {
            message: error.to_string(),
        },
    }
}

pub(super) fn allocate_logical_tick(state: &mut CalcState) -> u64 {
    let tick = state.next_logical_tick;
    state.next_logical_tick = state.next_logical_tick.saturating_add(1);
    tick
}

pub(super) fn parse_absolute_path(path: &str) -> TablePath {
    TablePath::parse_absolute(path).expect("stored calculation paths are canonical absolute paths")
}

pub(super) const fn intersect_limits(left: CalcLimits, right: CalcLimits) -> CalcLimits {
    CalcLimits::new(
        if left.max_work < right.max_work {
            left.max_work
        } else {
            right.max_work
        },
        if left.max_observations < right.max_observations {
            left.max_observations
        } else {
            right.max_observations
        },
        if left.max_query_depth < right.max_query_depth {
            left.max_query_depth
        } else {
            right.max_query_depth
        },
        if left.max_output < right.max_output {
            left.max_output
        } else {
            right.max_output
        },
    )
}

pub(super) fn dependency_digest(
    observations: &[sim_incremental_core::Observation<CalcQuery>],
) -> u64 {
    let mut digest = 0xcbf2_9ce4_8422_2325_u64;
    for observation in observations {
        for byte in format!(
            "{:?}|{:?}|{}|{:?}",
            observation.key(),
            observation.kind(),
            observation.revision().get(),
            observation.fingerprint().map(ValueFingerprint::get)
        )
        .bytes()
        {
            digest ^= u64::from(byte);
            digest = digest.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    digest
}
