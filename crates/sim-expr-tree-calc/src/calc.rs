use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use sim_expr_tree_core::{BackendKind, MountEpoch, MountResource};
use sim_incremental_core::{
    ContinuationToken, IncrementalEngine, IncrementalError, ObservationKind, SnapshotBudgets,
    ValueFingerprint,
};
use sim_kernel::{
    CapabilitySet, Cx, DefaultFactory, EagerPolicy, Expr, StrictNames, Symbol, Value,
};
use sim_lib_stream_core::BufferPolicy;
use sim_table_core::TablePath;

use crate::ExprTreeRefPolicy;

mod attempt;
mod engine;
mod eval;
use eval::{evaluate_cell, observe_runtime_context, parent_path, path_key};
mod model;
pub use model::{
    CalcError, CalcLimits, CalcQuery, CellFailure, HARD_MAX_EXPR_DEPTH, HARD_MAX_OBSERVATIONS,
    HARD_MAX_OUTPUT, HARD_MAX_QUERY_DEPTH, HARD_MAX_WORK, LastGoodValue,
};
use model::{ContextFactory, MemoOutcome, MemoValue};
mod policy;
pub use policy::{
    AuthorityDigest, AuthorityPolicyPatch, CalcPolicyPatch, CalcTrigger, CycleMode,
    EffectiveAuthority, EffectiveCalcPolicy, ErrorMode, PolicyDigest,
};
use policy::{effective_authority, effective_calc_policy, is_descendant_or_same};
mod receipt;
pub use receipt::{
    CalcExplanation, CalcOutcome, CalcReason, CalcReceipt, CalcRequestMode, CalcStatus,
    DependencyStamp, DirectedCalcReport, DirectedCellResult, EffectStamp, RequestId,
};
mod scheduler;
use scheduler::MAX_READY_BYPASSES;
pub use scheduler::{
    AutomaticBudget, AutomaticContinuation, AutomaticQueueSnapshot, AutomaticRun, QueuedCalculation,
};
mod scheduling;
mod session;
mod value;
mod watch;
pub use watch::CalcWatch;

const MAX_RECEIPT_DEPENDENCIES: usize = 64;
const MAX_RECEIPT_GRAPH_NODES: usize = 4_096;
const MAX_RECEIPT_GRAPH_EDGES: usize = 65_536;

type WallClock = dyn Fn() -> Option<u64> + Send + Sync + 'static;

/// Incremental calculator for ordinary SIM [`Expr`] sources and [`Value`]
/// results.
pub struct ExprTreeCalc {
    state: Arc<RwLock<CalcState>>,
    engine: IncrementalEngine<CalcQuery, MemoValue>,
    context_factory: Arc<ContextFactory>,
    cancel_requested: Arc<AtomicBool>,
    next_volatile: Arc<AtomicU64>,
    wall_clock: Arc<RwLock<Arc<WallClock>>>,
    next_request_id: u64,
    automatic_queue: BTreeMap<String, QueuedCalculation>,
    automatic_generation: u64,
    next_queue_sequence: u64,
    watches: Vec<CalcWatch>,
    next_watch_id: u64,
}

#[derive(Default)]
pub(crate) struct CalcState {
    cells: BTreeMap<String, Expr>,
    bound_names: BTreeSet<String>,
    bound_values: BTreeMap<Symbol, Value>,
    mounts: BTreeMap<String, MountState>,
    codec_registry_revision: u64,
    tree_calc_policy: CalcPolicyPatch,
    dir_calc_policies: BTreeMap<String, CalcPolicyPatch>,
    cell_calc_policies: BTreeMap<String, CalcPolicyPatch>,
    authority_ceiling: CapabilitySet,
    tree_authority_policy: AuthorityPolicyPatch,
    dir_authority_policies: BTreeMap<String, AuthorityPolicyPatch>,
    cell_authority_policies: BTreeMap<String, AuthorityPolicyPatch>,
    active_request: Option<ActiveRequest>,
    attempts: Vec<AttemptDraft>,
    receipts: BTreeMap<String, CalcReceipt>,
    next_logical_tick: u64,
    current: BTreeMap<String, Result<Value, CalcError>>,
    last_good: BTreeMap<String, Value>,
    volatile: BTreeSet<String>,
    failed_cells: BTreeSet<String>,
}

#[derive(Clone)]
struct ActiveRequest {
    id: RequestId,
    reason: CalcReason,
    directed_cells: BTreeSet<String>,
    automatic: bool,
}

struct AttemptDraft {
    request_id: RequestId,
    cell: String,
    policy: EffectiveCalcPolicy,
    authority: EffectiveAuthority,
    started_tick: u64,
    finished_tick: u64,
    wall_started_ms: Option<u64>,
    wall_finished_ms: Option<u64>,
    outcome: CalcOutcome,
    effects: Vec<EffectStamp>,
    omitted_effects: usize,
    reason: CalcReason,
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
        let context_factory: Arc<ContextFactory> = Arc::new(factory);
        let open_time_authority = context_factory().capabilities().clone();
        Self {
            state: Arc::new(RwLock::new(CalcState {
                authority_ceiling: open_time_authority,
                next_logical_tick: 1,
                ..CalcState::default()
            })),
            engine: IncrementalEngine::new(),
            context_factory,
            cancel_requested: Arc::new(AtomicBool::new(false)),
            next_volatile: Arc::new(AtomicU64::new(1)),
            wall_clock: Arc::new(RwLock::new(Arc::new(|| None))),
            next_request_id: 1,
            automatic_queue: BTreeMap::new(),
            automatic_generation: 1,
            next_queue_sequence: 1,
            watches: Vec::new(),
            next_watch_id: 1,
        }
    }

    /// Replaces the optional human wall-clock observation source.
    ///
    /// Logical ticks and revisions remain the only freshness authority.
    pub fn set_wall_clock<F>(&mut self, clock: F)
    where
        F: Fn() -> Option<u64> + Send + Sync + 'static,
    {
        *self.wall_clock.write().expect("wall clock lock poisoned") = Arc::new(clock);
    }

    /// Returns the immutable capability ceiling captured when this tree opened.
    #[must_use]
    pub fn open_time_authority(&self) -> CapabilitySet {
        self.state
            .read()
            .expect("calc state poisoned")
            .authority_ceiling
            .clone()
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
            self.register_cell_query(key.clone());
        }
        self.invalidate_cell_source(&path, !replaced);
        self.invalidate_failed_cells(failed);
        self.emit_change("source-set", &key);
        self.schedule_dirty_automatic();
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
        self.register_cell_query(key.clone());
        self.invalidate_cell_source(path, true);
        self.invalidate_failed_cells(failed);
        self.emit_change("source-removed", &key);
        self.schedule_dirty_automatic();
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
            self.register_cell_query(to_key.clone());
        }
        self.register_cell_query(from_key.clone());
        self.invalidate_cell_source(from, true);
        self.invalidate_cell_source(&to, true);
        self.invalidate_failed_cells(failed);
        self.emit_change("source-moved-from", &from_key);
        self.emit_change("source-moved-to", &to_key);
        self.schedule_dirty_automatic();
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
