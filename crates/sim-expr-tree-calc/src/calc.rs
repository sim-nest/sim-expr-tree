use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use sim_expr_tree_core::{BackendKind, MountEpoch, MountResource};
use sim_incremental_core::{
    IncrementalEngine, IncrementalError, ObservationKind, SnapshotBudgets, ValueFingerprint,
};
use sim_kernel::{Cx, DefaultFactory, EagerPolicy, Expr, StrictNames, Symbol, Value};
use sim_table_core::TablePath;

use crate::ExprTreeRefPolicy;

mod eval;
use eval::{evaluate_cell, observe_runtime_context, parent_path, path_key};
mod model;
pub use model::{
    CalcError, CalcLimits, CalcQuery, CellFailure, HARD_MAX_EXPR_DEPTH, HARD_MAX_OBSERVATIONS,
    HARD_MAX_OUTPUT, HARD_MAX_QUERY_DEPTH, HARD_MAX_WORK, LastGoodValue,
};
use model::{ContextFactory, MemoOutcome, MemoValue};
mod value;

/// Incremental calculator for ordinary SIM [`Expr`] sources and [`Value`]
/// results.
pub struct ExprTreeCalc {
    state: Arc<RwLock<CalcState>>,
    engine: IncrementalEngine<CalcQuery, MemoValue>,
    context_factory: Arc<ContextFactory>,
    cancel_requested: Arc<AtomicBool>,
    next_volatile: Arc<AtomicU64>,
}

#[derive(Clone, Default)]
pub(crate) struct CalcState {
    cells: BTreeMap<String, Expr>,
    bound_names: BTreeSet<String>,
    bound_values: BTreeMap<Symbol, Value>,
    mounts: BTreeMap<String, MountState>,
    effective_policy: String,
    codec_registry_revision: u64,
    authority_ceiling: String,
    current: BTreeMap<String, Result<Value, CalcError>>,
    last_good: BTreeMap<String, Value>,
    volatile: BTreeSet<String>,
    failed_cells: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MountState {
    resource: MountResource,
    backend: BackendKind,
    epoch: MountEpoch,
}

impl ExprTreeCalc {
    /// Creates a calculator using strict eager ordinary SIM evaluation.
    #[must_use]
    pub fn new() -> Self {
        Self::with_context_factory(|| {
            Cx::new(
                Arc::new(ExprTreeRefPolicy::new(StrictNames(EagerPolicy))),
                Arc::new(DefaultFactory),
            )
        })
    }

    /// Creates a calculator from a fresh-context factory.
    ///
    /// The factory may install loadable libraries, lexical values, functions,
    /// macros, tables, and directories. It is invoked with no calculator lock
    /// held.
    #[must_use]
    pub fn with_context_factory<F>(factory: F) -> Self
    where
        F: Fn() -> Cx + Send + Sync + 'static,
    {
        Self {
            state: Arc::new(RwLock::new(CalcState {
                effective_policy: "default".to_owned(),
                authority_ceiling: "ambient".to_owned(),
                ..CalcState::default()
            })),
            engine: IncrementalEngine::new(),
            context_factory: Arc::new(factory),
            cancel_requested: Arc::new(AtomicBool::new(false)),
            next_volatile: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Installs or replaces an ordinary expression source.
    pub fn set_cell(&mut self, path: TablePath, source: Expr) {
        let key = path_key(&path);
        let (replaced, failed) = {
            let mut state = self.state.write().expect("calc state poisoned");
            let replaced = state.cells.insert(key.clone(), source).is_some();
            state.current.remove(&key);
            state.volatile.remove(&key);
            (
                replaced,
                state.failed_cells.iter().cloned().collect::<Vec<_>>(),
            )
        };
        if !replaced {
            self.register_cell_query(key);
        }
        self.invalidate_cell_source(&path, !replaced);
        self.invalidate_failed_cells(failed);
    }

    /// Removes a cell source while retaining an explicit missing-value query.
    pub fn remove_cell(&mut self, path: &TablePath) {
        let key = path_key(path);
        let failed = {
            let mut state = self.state.write().expect("calc state poisoned");
            state.cells.remove(&key);
            state.current.remove(&key);
            state.volatile.remove(&key);
            state.failed_cells.iter().cloned().collect::<Vec<_>>()
        };
        self.register_cell_query(key);
        self.invalidate_cell_source(path, true);
        self.invalidate_failed_cells(failed);
    }

    /// Moves an ordinary source and invalidates both namespace locations.
    pub fn move_cell(&mut self, from: &TablePath, to: TablePath) {
        let from_key = path_key(from);
        let to_key = path_key(&to);
        let (moved, failed) = {
            let mut state = self.state.write().expect("calc state poisoned");
            let moved = state.cells.remove(&from_key);
            if let Some(source) = moved.clone() {
                state.cells.insert(to_key.clone(), source);
            }
            state.current.remove(&from_key);
            state.current.remove(&to_key);
            state.volatile.remove(&from_key);
            state.volatile.remove(&to_key);
            let failed = state.failed_cells.iter().cloned().collect::<Vec<_>>();
            (moved, failed)
        };
        if moved.is_some() {
            self.register_cell_query(to_key);
        }
        self.register_cell_query(from_key);
        self.invalidate_cell_source(from, true);
        self.invalidate_cell_source(&to, true);
        self.invalidate_failed_cells(failed);
    }

    /// Binds a lexical name to a diagnostic string value.
    ///
    /// This compatibility helper keeps a name ahead of tree lookup. New code
    /// should prefer [`Self::bind_value`].
    pub fn bind_name(&mut self, name: impl Into<String>) {
        let name = name.into();
        self.state
            .write()
            .expect("calc state poisoned")
            .bound_names
            .insert(name.clone());
        self.engine.invalidate(&CalcQuery::NameSlot(name));
    }

    /// Binds an arbitrary ordinary SIM value ahead of tree-name lookup.
    pub fn bind_value(&mut self, name: Symbol, value: Value) {
        self.state
            .write()
            .expect("calc state poisoned")
            .bound_values
            .insert(name.clone(), value);
        self.engine
            .invalidate(&CalcQuery::NameSlot(name.to_string()));
    }

    /// Updates effective policy observation state.
    pub fn set_effective_policy(&mut self, policy: impl Into<String>) {
        self.state
            .write()
            .expect("calc state poisoned")
            .effective_policy = policy.into();
        self.engine.invalidate(&CalcQuery::EffectivePolicy);
    }

    /// Updates the observed codec registry revision.
    pub fn set_codec_registry_revision(&mut self, revision: u64) {
        self.state
            .write()
            .expect("calc state poisoned")
            .codec_registry_revision = revision;
        self.engine.invalidate(&CalcQuery::CodecRegistry);
    }

    /// Updates the observed authority ceiling.
    pub fn set_authority_ceiling(&mut self, ceiling: impl Into<String>) {
        self.state
            .write()
            .expect("calc state poisoned")
            .authority_ceiling = ceiling.into();
        self.engine.invalidate(&CalcQuery::AuthorityCeiling);
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
        self.state
            .write()
            .expect("calc state poisoned")
            .mounts
            .insert(
                key.clone(),
                MountState {
                    resource,
                    backend,
                    epoch,
                },
            );
        self.engine.invalidate(&CalcQuery::MountEpoch(key));
    }

    /// Advances a mounted backend epoch.
    pub fn observe_mount_epoch(&mut self, path: &TablePath, epoch: MountEpoch) {
        let key = path_key(path);
        if let Some(mount) = self
            .state
            .write()
            .expect("calc state poisoned")
            .mounts
            .get_mut(&key)
        {
            mount.epoch = epoch;
        }
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
        let key = path_key(path);
        let result = self
            .engine
            .verify_with_budgets(CalcQuery::Cell(key.clone()), limits.clamped())
            .map_err(CalcError::Incremental)
            .and_then(|memo| {
                let is_volatile = memo.is_volatile();
                match memo.outcome {
                    MemoOutcome::Value(value) => Ok((value, is_volatile)),
                    MemoOutcome::Failure(failure) => Err(CalcError::Cell(failure)),
                }
            });

        let mut state = self.state.write().expect("calc state poisoned");
        match result {
            Ok((value, is_volatile)) => {
                state.current.insert(key.clone(), Ok(value.clone()));
                state.last_good.insert(key.clone(), value.clone());
                state.failed_cells.remove(&key);
                if is_volatile {
                    state.volatile.insert(key);
                } else {
                    state.volatile.remove(&key);
                }
                Ok(value)
            }
            Err(error) => {
                state.current.insert(key.clone(), Err(error.clone()));
                state.volatile.remove(&key);
                state.failed_cells.insert(key);
                Err(error)
            }
        }
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

    /// Returns the dependency observations for a cell in deterministic order.
    pub fn cell_dependencies(
        &mut self,
        path: &TablePath,
    ) -> Result<Vec<(CalcQuery, ObservationKind)>, IncrementalError<CalcQuery>> {
        let key = CalcQuery::Cell(path_key(path));
        let snapshot = self
            .engine
            .snapshot([key.clone()], SnapshotBudgets::default())?;
        Ok(snapshot
            .nodes
            .iter()
            .find(|node| node.key == key)
            .map(|node| {
                node.dependencies
                    .iter()
                    .map(|observation| (observation.key().clone(), observation.kind().clone()))
                    .collect()
            })
            .unwrap_or_default())
    }

    fn register_cell_query(&mut self, key: String) {
        let state = Arc::clone(&self.state);
        let context_factory = Arc::clone(&self.context_factory);
        let cancel_requested = Arc::clone(&self.cancel_requested);
        let next_volatile = Arc::clone(&self.next_volatile);
        self.engine
            .register_fn(CalcQuery::Cell(key), move |query, frame| {
                let CalcQuery::Cell(cell_key) = query else {
                    return Err(IncrementalError::UnknownQuery { key: query.clone() });
                };
                if cancel_requested.swap(false, Ordering::AcqRel) {
                    frame.cancel();
                    frame.charge_work(0)?;
                }
                observe_runtime_context(frame)?;
                frame.observe(
                    ObservationKind::Custom("cell-source"),
                    CalcQuery::Cell(cell_key.clone()),
                )?;
                let source = state
                    .read()
                    .expect("calc state poisoned")
                    .cells
                    .get(cell_key)
                    .cloned();
                let memo = evaluate_cell(
                    &state,
                    frame,
                    &context_factory,
                    &next_volatile,
                    cell_key,
                    source,
                )?;
                Ok(memo)
            });
    }

    fn invalidate_cell_source(&mut self, path: &TablePath, namespace_changed: bool) {
        self.engine.invalidate(&CalcQuery::Cell(path_key(path)));
        if namespace_changed {
            self.engine.invalidate(&CalcQuery::NameSlot(path_key(path)));
            self.engine
                .invalidate(&CalcQuery::Listing(path_key(&parent_path(path))));
        }
    }

    fn invalidate_failed_cells(&mut self, failed: Vec<String>) {
        for key in failed {
            self.engine.invalidate(&CalcQuery::Cell(key));
        }
    }

    #[cfg(test)]
    pub(crate) fn replace_context_factory<F>(&mut self, factory: F)
    where
        F: Fn() -> Cx + Send + Sync + 'static,
    {
        self.context_factory = Arc::new(factory);
    }

    #[cfg(test)]
    pub(crate) fn state_for_lock_probe(&self) -> Arc<RwLock<CalcState>> {
        Arc::clone(&self.state)
    }
}

impl Default for ExprTreeCalc {
    fn default() -> Self {
        Self::new()
    }
}

fn incremental_failure(
    error: IncrementalError<CalcQuery>,
) -> Result<MemoValue, IncrementalError<CalcQuery>> {
    match error {
        IncrementalError::Cycle { path } => Ok(MemoValue::failure(CellFailure::Cycle { path })),
        IncrementalError::UnknownQuery { key } => Ok(MemoValue::failure(CellFailure::Evaluation {
            message: format!("unknown dependency {key:?}"),
        })),
        IncrementalError::BudgetExceeded { .. }
        | IncrementalError::Cancelled
        | IncrementalError::UnknownContinuation { .. } => Err(error),
    }
}
