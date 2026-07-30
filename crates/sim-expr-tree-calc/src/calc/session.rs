use super::attempt::{intersect_limits, parse_absolute_path};
use super::*;

impl ExprTreeCalc {
    /// Replaces the tree-level codec policy patch.
    pub fn set_tree_codec_policy(&mut self, patch: CodecPolicyPatch) {
        let mut state = self.state.write().expect("calc state poisoned");
        state.tree_codec_policy = patch;
        bump_generation(&mut state.control_generation);
    }

    /// Replaces one directory-level codec policy patch.
    pub fn set_dir_codec_policy(&mut self, directory: TablePath, patch: CodecPolicyPatch) {
        let key = path_key(&directory);
        let mut state = self.state.write().expect("calc state poisoned");
        state.dir_codec_policies.insert(key, patch);
        bump_generation(&mut state.control_generation);
    }

    /// Replaces one cell-level codec policy patch.
    pub fn set_cell_codec_policy(&mut self, cell: TablePath, patch: CodecPolicyPatch) {
        let key = path_key(&cell);
        let mut state = self.state.write().expect("calc state poisoned");
        state.cell_codec_policies.insert(key, patch);
        bump_generation(&mut state.control_generation);
    }

    /// Resolves codec policy field by field from tree through ancestor
    /// directories to the selected cell.
    #[must_use]
    pub fn effective_codec_policy(&self, cell: &TablePath) -> EffectiveCodecPolicy {
        let state = self.state.read().expect("calc state poisoned");
        effective_codec_policy(
            &state.tree_codec_policy,
            &state.dir_codec_policies,
            &state.cell_codec_policies,
            &path_key(cell),
        )
    }

    /// Replaces the tree-level calculation policy patch.
    pub fn set_tree_calc_policy(&mut self, patch: CalcPolicyPatch) {
        {
            let mut state = self.state.write().expect("calc state poisoned");
            state.tree_calc_policy = patch;
            bump_generation(&mut state.control_generation);
        }
        self.invalidate_calc_policy_matching(|_| true);
        self.schedule_dirty_automatic();
    }

    /// Replaces one directory-level calculation policy patch.
    pub fn set_dir_calc_policy(&mut self, directory: TablePath, patch: CalcPolicyPatch) {
        let key = path_key(&directory);
        {
            let mut state = self.state.write().expect("calc state poisoned");
            state.dir_calc_policies.insert(key, patch);
            bump_generation(&mut state.control_generation);
        }
        self.invalidate_calc_policy_matching(|cell| is_descendant_or_same(&directory, cell));
        self.schedule_dirty_automatic();
    }

    /// Replaces one cell-level calculation policy patch.
    pub fn set_cell_calc_policy(&mut self, cell: TablePath, patch: CalcPolicyPatch) {
        let key = path_key(&cell);
        {
            let mut state = self.state.write().expect("calc state poisoned");
            state.cell_calc_policies.insert(key.clone(), patch);
            bump_generation(&mut state.control_generation);
        }
        self.engine
            .invalidate(&CalcQuery::EffectivePolicy(key.clone()));
        self.emit_change("calculation-policy", &key);
        self.schedule_dirty_automatic();
    }

    /// Resolves tree, ancestor-directory, and cell calculation policy fields.
    #[must_use]
    pub fn effective_calc_policy(&self, cell: &TablePath) -> EffectiveCalcPolicy {
        let state = self.state.read().expect("calc state poisoned");
        effective_calc_policy(
            &state.tree_calc_policy,
            &state.dir_calc_policies,
            &state.cell_calc_policies,
            &path_key(cell),
        )
    }

    /// Replaces the tree-level authority policy patch.
    pub fn set_tree_authority_policy(&mut self, patch: AuthorityPolicyPatch) {
        {
            let mut state = self.state.write().expect("calc state poisoned");
            state.tree_authority_policy = patch;
            bump_generation(&mut state.control_generation);
        }
        self.invalidate_authority_policy_matching(|_| true);
        self.schedule_dirty_automatic();
    }

    /// Replaces one directory-level authority policy patch.
    pub fn set_dir_authority_policy(&mut self, directory: TablePath, patch: AuthorityPolicyPatch) {
        let key = path_key(&directory);
        {
            let mut state = self.state.write().expect("calc state poisoned");
            state.dir_authority_policies.insert(key, patch);
            bump_generation(&mut state.control_generation);
        }
        self.invalidate_authority_policy_matching(|cell| is_descendant_or_same(&directory, cell));
        self.schedule_dirty_automatic();
    }

    /// Replaces one cell-level authority policy patch.
    pub fn set_cell_authority_policy(&mut self, cell: TablePath, patch: AuthorityPolicyPatch) {
        let key = path_key(&cell);
        {
            let mut state = self.state.write().expect("calc state poisoned");
            state.cell_authority_policies.insert(key.clone(), patch);
            bump_generation(&mut state.control_generation);
        }
        self.engine
            .invalidate(&CalcQuery::AuthorityPolicy(key.clone()));
        self.emit_change("authority-policy", &key);
        self.schedule_dirty_automatic();
    }

    /// Resolves the immutable ceiling through every allow/deny policy level.
    #[must_use]
    pub fn effective_authority(&self, cell: &TablePath) -> EffectiveAuthority {
        let state = self.state.read().expect("calc state poisoned");
        effective_authority(
            &state.authority_ceiling,
            &state.tree_authority_policy,
            &state.dir_authority_policies,
            &state.cell_authority_policies,
            &path_key(cell),
        )
    }

    /// Updates the observed codec registry revision.
    pub fn set_codec_registry_revision(&mut self, revision: u64) {
        {
            let mut state = self.state.write().expect("calc state poisoned");
            state.codec_registry_revision = revision;
            bump_generation(&mut state.control_generation);
        }
        self.engine.invalidate(&CalcQuery::CodecRegistry);
    }

    /// Adds or replaces a mounted backend observation.
    pub fn mount(
        &mut self,
        path: TablePath,
        resource: MountResource,
        backend: BackendKind,
        epoch: MountEpoch,
    ) {
        let key = path_key(&path);
        {
            let mut state = self.state.write().expect("calc state poisoned");
            state.mounts.insert(
                key.clone(),
                MountState {
                    resource,
                    backend,
                    epoch,
                },
            );
            bump_generation(&mut state.control_generation);
        }
        self.refresh_samples
            .insert(key.clone(), BackendRefreshSample::new(epoch));
        self.engine.invalidate(&CalcQuery::MountEpoch(key));
    }

    /// Advances a mounted backend epoch.
    pub fn observe_mount_epoch(&mut self, path: &TablePath, epoch: MountEpoch) {
        let key = path_key(path);
        {
            let mut state = self.state.write().expect("calc state poisoned");
            if let Some(mount) = state.mounts.get_mut(&key) {
                mount.epoch = epoch;
                bump_generation(&mut state.control_generation);
            }
        }
        self.refresh_samples
            .entry(key.clone())
            .and_modify(|sample| sample.epoch = epoch)
            .or_insert_with(|| BackendRefreshSample::new(epoch));
        self.engine.invalidate(&CalcQuery::MountEpoch(key));
    }

    /// Requests cancellation of the next calculation work that actually runs.
    pub fn request_cancellation(&self) {
        self.cancel_requested.store(true, Ordering::Release);
    }

    /// Pull-verifies a cell under the hard default ceilings.
    pub fn verify_cell(&mut self, path: &TablePath) -> Result<Value, CalcError> {
        self.verify_cell_with_limits(path, CalcLimits::default())
    }

    /// Pull-verifies a cell with requested limits clamped to hard ceilings.
    pub fn verify_cell_with_limits(
        &mut self,
        path: &TablePath,
        limits: CalcLimits,
    ) -> Result<Value, CalcError> {
        let mut report = self.calculate_cells([path.clone()], CalcRequestMode::Verify, limits);
        report
            .cells
            .pop()
            .map(|cell| cell.result)
            .unwrap_or_else(|| {
                Err(CalcError::NotCalculated {
                    path: path_key(path),
                })
            })
    }

    /// Forces only this root while permitting valid dependency reuse.
    pub fn recalculate_cell(&mut self, path: &TablePath) -> Result<Value, CalcError> {
        self.recalculate_cell_with_limits(path, CalcLimits::default())
    }

    /// Forces only this root under explicit hard-clamped limits.
    pub fn recalculate_cell_with_limits(
        &mut self,
        path: &TablePath,
        limits: CalcLimits,
    ) -> Result<Value, CalcError> {
        let mut report = self.calculate_cells([path.clone()], CalcRequestMode::ForceRoots, limits);
        report
            .cells
            .pop()
            .map(|cell| cell.result)
            .unwrap_or_else(|| {
                Err(CalcError::NotCalculated {
                    path: path_key(path),
                })
            })
    }

    /// Forces this root and every reachable calculated dependency.
    pub fn recalculate_recursive(&mut self, path: &TablePath) -> Result<Value, CalcError> {
        let mut report = self.calculate_cells(
            [path.clone()],
            CalcRequestMode::ForceRecursive,
            CalcLimits::default(),
        );
        report
            .cells
            .pop()
            .map(|cell| cell.result)
            .unwrap_or_else(|| {
                Err(CalcError::NotCalculated {
                    path: path_key(path),
                })
            })
    }

    /// Runs one stable multi-root directed request through the shared engine.
    pub fn calculate_cells(
        &mut self,
        roots: impl IntoIterator<Item = TablePath>,
        mode: CalcRequestMode,
        limits: CalcLimits,
    ) -> DirectedCalcReport {
        let roots = roots
            .into_iter()
            .map(|path| path_key(&path))
            .collect::<BTreeSet<_>>();
        let request_id = self.allocate_request_id();
        let directed_cells = match mode {
            CalcRequestMode::ForceRecursive => self.force_recursive_closure(&roots),
            CalcRequestMode::Verify | CalcRequestMode::ForceRoots => roots.clone(),
        };
        match mode {
            CalcRequestMode::Verify => {}
            CalcRequestMode::ForceRoots => {
                for root in &roots {
                    self.engine.invalidate(&CalcQuery::ForceEpoch(root.clone()));
                }
            }
            CalcRequestMode::ForceRecursive => {
                for cell in &directed_cells {
                    self.engine.invalidate(&CalcQuery::ForceEpoch(cell.clone()));
                }
            }
        }

        let request = ActiveRequest {
            id: request_id,
            reason: CalcReason::for_mode(mode),
            directed_cells,
            automatic: false,
        };
        let mut cells = Vec::new();
        for root in roots {
            let effective = self.effective_calc_policy(&parse_absolute_path(&root));
            let root_limits = intersect_limits(limits, effective.budget);
            let result = self.execute_root(root.clone(), request.clone(), root_limits, None);
            let failed = result.is_err();
            cells.push(DirectedCellResult { cell: root, result });
            if failed && effective.error_mode == ErrorMode::FailFast {
                break;
            }
        }
        DirectedCalcReport { request_id, cells }
    }

    /// Opens one standard bounded stream endpoint for progress and changes.
    pub fn watch(&mut self, buffer_policy: BufferPolicy) -> CalcWatch {
        let watch = CalcWatch::new(self.next_watch_id, buffer_policy);
        self.next_watch_id = self.next_watch_id.saturating_add(1);
        self.watches.push(watch.clone());
        watch
    }

    /// Cancels queued automatic work with this request id.
    pub fn cancel_request(&mut self, request_id: RequestId) -> bool {
        let key = self
            .automatic_queue
            .iter()
            .find_map(|(cell, queued)| (queued.request_id == request_id).then(|| cell.clone()));
        let Some(key) = key else {
            return false;
        };
        self.automatic_queue.remove(&key);
        self.bump_queue_generation();
        self.emit_progress("cancelled", &key, request_id);
        true
    }

    /// Reads only the current committed result.
    ///
    /// A failed or cancelled recalculation never falls back to last-good.
    pub fn current_cell(&self, path: &TablePath) -> Result<Value, CalcError> {
        let key = path_key(path);
        self.state
            .read()
            .expect("calc state poisoned")
            .current
            .get(&key)
            .cloned()
            .unwrap_or(Err(CalcError::NotCalculated { path: key }))
    }

    /// Returns the retained historical success, explicitly labelled
    /// `last-good`.
    #[must_use]
    pub fn last_good_cell(&self, path: &TablePath) -> Option<LastGoodValue> {
        self.state
            .read()
            .expect("calc state poisoned")
            .last_good
            .get(&path_key(path))
            .cloned()
            .map(|value| LastGoodValue { value })
    }

    /// Returns whether the current successful result is noncanonical and must
    /// therefore be treated as volatile.
    #[must_use]
    pub fn current_is_volatile(&self, path: &TablePath) -> bool {
        self.state
            .read()
            .expect("calc state poisoned")
            .volatile
            .contains(&path_key(path))
    }

    /// Returns the current incremental memo revision.
    #[must_use]
    pub fn cell_revision(&self, path: &TablePath) -> Option<u64> {
        self.engine
            .memo_revision(&CalcQuery::Cell(path_key(path)))
            .map(|revision| revision.get())
    }

    /// Returns the current incremental fingerprint.
    #[must_use]
    pub fn cell_fingerprint(&self, path: &TablePath) -> Option<ValueFingerprint> {
        self.engine
            .memo_fingerprint(&CalcQuery::Cell(path_key(path)))
    }

    /// Returns the latest bounded immutable calculation receipt.
    #[must_use]
    pub fn receipt(&self, path: &TablePath) -> Option<CalcReceipt> {
        self.state
            .read()
            .expect("calc state poisoned")
            .receipts
            .get(&path_key(path))
            .cloned()
    }

    /// Explains current state without evaluating or mutating the graph.
    #[must_use]
    pub fn explain(&self, path: &TablePath) -> CalcExplanation {
        let cell = path_key(path);
        let policy = self.effective_calc_policy(path);
        let authority = self.effective_authority(path);
        let receipt = self.receipt(path);
        let dirty = self
            .engine
            .dirty_keys()
            .contains(&CalcQuery::Cell(cell.clone()));
        let pending = self.automatic_queue.contains_key(&cell);
        let has_current = self
            .state
            .read()
            .expect("calc state poisoned")
            .current
            .contains_key(&cell);
        let status = if policy.trigger == CalcTrigger::Frozen {
            CalcStatus::Frozen
        } else if pending {
            CalcStatus::Pending
        } else if matches!(
            receipt.as_ref().map(|receipt| &receipt.outcome),
            Some(CalcOutcome::Blocked { .. })
        ) {
            CalcStatus::Blocked
        } else if matches!(
            receipt.as_ref().map(|receipt| &receipt.outcome),
            Some(CalcOutcome::Failed { .. })
        ) {
            CalcStatus::Failed
        } else if dirty && has_current {
            CalcStatus::MaybeStale
        } else if has_current {
            CalcStatus::Fresh
        } else {
            CalcStatus::NeverCalculated
        };
        let mut reasons = Vec::new();
        match status {
            CalcStatus::Frozen => reasons.push("effective trigger is frozen".to_owned()),
            CalcStatus::Pending => reasons.push("bounded automatic work is queued".to_owned()),
            CalcStatus::Blocked | CalcStatus::Failed => {
                if let Some(receipt) = &receipt {
                    match &receipt.outcome {
                        CalcOutcome::Blocked { message } | CalcOutcome::Failed { message } => {
                            reasons.push(message.clone());
                        }
                        _ => {}
                    }
                }
            }
            CalcStatus::MaybeStale => {
                reasons.push("an observed input changed and awaits verification".to_owned());
            }
            CalcStatus::Fresh => {
                reasons.push("the committed memo matches all observed revisions".to_owned());
            }
            CalcStatus::NeverCalculated => {
                reasons.push("no calculation attempt has committed".to_owned());
            }
        }
        CalcExplanation {
            cell: cell.clone(),
            status,
            source_revision: self.engine.source_revision(&CalcQuery::Cell(cell)).get(),
            policy_digest: policy.digest(),
            authority_digest: authority.digest(),
            receipt,
            reasons,
        }
    }

    /// Runs a bounded amount of ready automatic work.
    pub fn run_automatic(&mut self, budget: AutomaticBudget, now_ms: u64) -> AutomaticRun {
        self.run_automatic_inner(budget, now_ms)
    }

    /// Resumes a prior automatic queue continuation.
    pub fn continue_automatic(
        &mut self,
        continuation: AutomaticContinuation,
        budget: AutomaticBudget,
        now_ms: u64,
    ) -> Result<AutomaticRun, CalcError> {
        if continuation.generation() != self.automatic_generation {
            return Err(CalcError::UnknownAutomaticContinuation {
                generation: continuation.generation(),
            });
        }
        Ok(self.run_automatic_inner(budget, now_ms))
    }

    /// Snapshots all deterministic queue state needed for restart.
    #[must_use]
    pub fn automatic_queue_snapshot(&self) -> AutomaticQueueSnapshot {
        AutomaticQueueSnapshot {
            generation: self.automatic_generation,
            next_sequence: self.next_queue_sequence,
            entries: self.automatic_queue.values().cloned().collect(),
        }
    }

    /// Restores a deterministic queue snapshot, rejecting duplicate cells.
    pub fn restore_automatic_queue(
        &mut self,
        snapshot: AutomaticQueueSnapshot,
    ) -> Result<(), CalcError> {
        let mut restored = BTreeMap::new();
        let mut next_request_id = self.next_request_id;
        for entry in snapshot.entries {
            if !entry.cell.starts_with('/') {
                return Err(CalcError::CorruptAutomaticQueue { cell: entry.cell });
            }
            next_request_id = next_request_id.max(entry.request_id.get().saturating_add(1));
            let cell = entry.cell.clone();
            if restored.insert(cell.clone(), entry).is_some() {
                return Err(CalcError::CorruptAutomaticQueue { cell });
            }
        }
        self.automatic_queue = restored;
        self.automatic_generation = snapshot.generation.max(1);
        self.next_queue_sequence = snapshot.next_sequence.max(1);
        self.next_request_id = next_request_id;
        Ok(())
    }
}
