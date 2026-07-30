use super::attempt::{intersect_limits, parse_absolute_path};
use super::*;

impl ExprTreeCalc {
    pub(super) fn schedule_dirty_automatic(&mut self) {
        let dirty_cells = self
            .engine
            .dirty_keys()
            .into_iter()
            .filter_map(|query| match query {
                CalcQuery::Cell(cell) => Some(cell),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        let now_ms = self.wall_now().unwrap_or(0);
        let eligible = dirty_cells
            .iter()
            .filter_map(|cell| {
                let policy = self.effective_calc_policy(&parse_absolute_path(cell));
                (policy.trigger == CalcTrigger::Automatic)
                    .then(|| (cell.clone(), policy.priority, policy.debounce_ms))
            })
            .collect::<Vec<_>>();
        let eligible_cells = eligible
            .iter()
            .map(|(cell, _, _)| cell.clone())
            .collect::<BTreeSet<_>>();
        let old_len = self.automatic_queue.len();
        self.automatic_queue
            .retain(|cell, _| eligible_cells.contains(cell));
        let mut changed = self.automatic_queue.len() != old_len;
        for (cell, priority, debounce_ms) in eligible {
            let ready_at_ms = now_ms.saturating_add(u64::from(debounce_ms));
            if let Some(queued) = self.automatic_queue.get_mut(&cell) {
                if queued.ready_at_ms != ready_at_ms || queued.priority != priority {
                    queued.ready_at_ms = ready_at_ms;
                    queued.priority = priority;
                    queued.incremental_continuation = None;
                    changed = true;
                }
                continue;
            }
            let request_id = self.allocate_request_id();
            let sequence = self.next_queue_sequence;
            self.next_queue_sequence = self.next_queue_sequence.saturating_add(1);
            self.automatic_queue.insert(
                cell.clone(),
                QueuedCalculation {
                    request_id,
                    cell: cell.clone(),
                    ready_at_ms,
                    priority,
                    sequence,
                    bypasses: 0,
                    incremental_continuation: None,
                },
            );
            self.emit_progress("queued", &cell, request_id);
            changed = true;
        }
        if changed {
            self.bump_queue_generation();
        }
    }

    pub(super) fn prune_satisfied_automatic(&mut self) {
        let dirty = self
            .engine
            .dirty_keys()
            .into_iter()
            .filter_map(|query| match query {
                CalcQuery::Cell(cell) => Some(cell),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        let old_len = self.automatic_queue.len();
        self.automatic_queue.retain(|cell, _| dirty.contains(cell));
        if self.automatic_queue.len() != old_len {
            self.bump_queue_generation();
        }
    }

    pub(super) fn run_automatic_inner(
        &mut self,
        budget: AutomaticBudget,
        now_ms: u64,
    ) -> AutomaticRun {
        let mut completed = Vec::new();
        let mut budget_exhausted = Vec::new();
        for _ in 0..budget.max_requests {
            let Some(cell) = self.select_ready_cell(now_ms) else {
                break;
            };
            let mut queued = self
                .automatic_queue
                .remove(&cell)
                .expect("selected automatic queue entry must exist");
            self.bump_queue_generation();
            let policy = self.effective_calc_policy(&parse_absolute_path(&cell));
            let limits = intersect_limits(budget.limits, policy.budget);
            let continuation = queued.incremental_continuation.take();
            let request = ActiveRequest {
                id: queued.request_id,
                reason: if continuation.is_some() {
                    CalcReason::Continuation
                } else {
                    CalcReason::AutomaticMutation
                },
                directed_cells: BTreeSet::new(),
                automatic: true,
            };
            let result = self.execute_root(cell.clone(), request, limits, continuation);
            match result {
                Err(CalcError::Incremental(error)) if error.continuation().is_some() => {
                    queued.incremental_continuation = error.continuation();
                    queued.ready_at_ms = now_ms;
                    self.automatic_queue.insert(cell, queued.clone());
                    self.bump_queue_generation();
                    budget_exhausted.push(queued.request_id);
                }
                _ => completed.push(queued.request_id),
            }
        }
        AutomaticRun {
            completed,
            budget_exhausted,
            continuation: (!self.automatic_queue.is_empty())
                .then(|| AutomaticContinuation::new(self.automatic_generation)),
        }
    }

    pub(super) fn select_ready_cell(&mut self, now_ms: u64) -> Option<String> {
        let ready = self
            .automatic_queue
            .values()
            .filter(|queued| queued.ready_at_ms <= now_ms)
            .cloned()
            .collect::<Vec<_>>();
        let selected = ready
            .iter()
            .filter(|queued| queued.bypasses >= MAX_READY_BYPASSES)
            .min_by_key(|queued| (queued.sequence, queued.cell.clone()))
            .or_else(|| {
                ready.iter().max_by(|left, right| {
                    left.priority
                        .cmp(&right.priority)
                        .then_with(|| right.sequence.cmp(&left.sequence))
                        .then_with(|| right.cell.cmp(&left.cell))
                })
            })?
            .cell
            .clone();
        for queued in self.automatic_queue.values_mut() {
            if queued.ready_at_ms > now_ms {
                continue;
            }
            if queued.cell == selected {
                queued.bypasses = 0;
            } else {
                queued.bypasses = queued.bypasses.saturating_add(1);
            }
        }
        Some(selected)
    }

    pub(super) fn allocate_request_id(&mut self) -> RequestId {
        let request_id = RequestId::new(self.next_request_id);
        self.next_request_id = self.next_request_id.saturating_add(1);
        request_id
    }

    pub(super) fn bump_queue_generation(&mut self) {
        self.automatic_generation = self.automatic_generation.saturating_add(1).max(1);
    }

    pub(super) fn wall_now(&self) -> Option<u64> {
        let clock = self
            .wall_clock
            .read()
            .expect("wall clock lock poisoned")
            .clone();
        clock()
    }

    pub(super) fn emit_change(&self, kind: &'static str, cell: &str) {
        for watch in &self.watches {
            watch.emit(
                "change",
                vec![
                    (
                        Expr::Symbol(Symbol::new("change")),
                        Expr::Symbol(Symbol::qualified("expr-tree/change", kind)),
                    ),
                    (
                        Expr::Symbol(Symbol::new("cell")),
                        Expr::String(cell.to_owned()),
                    ),
                ],
            );
        }
    }

    pub(super) fn emit_progress(&self, kind: &'static str, cell: &str, request_id: RequestId) {
        for watch in &self.watches {
            watch.emit(
                "progress",
                vec![
                    (
                        Expr::Symbol(Symbol::new("progress")),
                        Expr::Symbol(Symbol::qualified("expr-tree/progress", kind)),
                    ),
                    (
                        Expr::Symbol(Symbol::new("cell")),
                        Expr::String(cell.to_owned()),
                    ),
                    (
                        Expr::Symbol(Symbol::new("request-id")),
                        Expr::String(request_id.get().to_string()),
                    ),
                ],
            );
        }
    }
}
