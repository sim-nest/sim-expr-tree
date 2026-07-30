use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
    sync::atomic::Ordering,
};

use sim_expr_tree_core::MountEpoch;
use sim_incremental_core::{
    ContinuationToken, GraphSnapshot, Observation, ObservationKind, Revision, SnapshotNode,
    ValueFingerprint,
};
use sim_kernel::{Cx, Expr, Symbol, Table, Value};

use super::*;

mod codec;
use codec::{DecodeError, decode_persisted, encode_persisted, restore_value};
mod identity;
use identity::state_identities;

/// Current durable expression-tree graph schema.
pub const GRAPH_SCHEMA_VERSION: u64 = 1;

/// Default key used by [`DerivedTableAdapter`] in the derived Table.
pub const DERIVED_SNAPSHOT_KEY: &str = "expr-tree-graph";

const MAX_PERSISTED_GRAPH_NODES: usize = 100_000;
const MAX_PERSISTED_GRAPH_EDGES: usize = 1_000_000;

/// Adapter that stores one versioned graph record in an ordinary SIM Table.
///
/// The adapter deliberately uses only `Table` operations and ordinary `Expr`
/// data. Durability, transactions, and host effects remain properties of the
/// selected backend.
pub struct DerivedTableAdapter<'a> {
    table: &'a dyn Table,
    cx: &'a mut Cx,
    key: Symbol,
}

impl<'a> DerivedTableAdapter<'a> {
    /// Targets [`DERIVED_SNAPSHOT_KEY`] in `table`.
    pub fn new(table: &'a dyn Table, cx: &'a mut Cx) -> Self {
        Self {
            table,
            cx,
            key: Symbol::new(DERIVED_SNAPSHOT_KEY),
        }
    }

    /// Targets an explicit Table key.
    pub fn with_key(table: &'a dyn Table, cx: &'a mut Cx, key: Symbol) -> Self {
        Self { table, cx, key }
    }

    fn load(&mut self) -> Result<Option<Expr>, DerivedSnapshotError> {
        if !self
            .table
            .has(self.cx, self.key.clone())
            .map_err(backend_error)?
        {
            return Ok(None);
        }
        let value = self
            .table
            .get(self.cx, self.key.clone())
            .map_err(backend_error)?;
        value
            .object()
            .as_expr(self.cx)
            .map(Some)
            .map_err(backend_error)
    }

    fn store(&mut self, expr: Expr) -> Result<(), DerivedSnapshotError> {
        let value = self.cx.factory().expr(expr).map_err(backend_error)?;
        self.table
            .set(self.cx, self.key.clone(), value)
            .map_err(backend_error)
    }

    /// Deletes the rebuildable graph record, if present.
    pub fn delete(&mut self) -> Result<(), DerivedSnapshotError> {
        self.table
            .del(self.cx, self.key.clone())
            .map(|_| ())
            .map_err(backend_error)
    }
}

/// Evidence returned after writing the current derived graph.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DerivedPersistReport {
    /// Persisted memo nodes.
    pub nodes: usize,
    /// Persisted reverse dependency edges.
    pub reverse_edges: usize,
    /// Persisted calculation receipts.
    pub receipts: usize,
    /// Persisted automatic entries carrying incremental continuations.
    pub pending_continuations: usize,
}

/// Why an open reused or conservatively rebuilt derived state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DerivedRestoreDisposition {
    /// A valid graph was rehydrated.
    Rehydrated,
    /// No derived graph existed.
    RebuiltMissing,
    /// The derived record was malformed or internally inconsistent.
    RebuiltCorrupt,
    /// The graph schema was not understood by this calculator.
    RebuiltIncompatible,
    /// Authored source or operational control state did not match the graph.
    RebuiltGenerationMismatch,
}

/// Evidence returned after opening derived state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DerivedRestoreReport {
    /// Reuse or rebuild decision.
    pub disposition: DerivedRestoreDisposition,
    /// Memo nodes accepted into the live engine.
    pub restored_nodes: usize,
    /// Restored nodes the incremental core conservatively marked dirty.
    pub recovered_dirty: usize,
    /// Restored calculation receipts.
    pub restored_receipts: usize,
    /// Restored automatic entries carrying restart continuations.
    pub pending_continuations: usize,
}

impl DerivedRestoreReport {
    fn rebuilt(disposition: DerivedRestoreDisposition) -> Self {
        Self {
            disposition,
            restored_nodes: 0,
            recovered_dirty: 0,
            restored_receipts: 0,
            pending_continuations: 0,
        }
    }
}

/// A derived Table access or snapshot export failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DerivedSnapshotError {
    /// The selected Table backend rejected an operation.
    Backend {
        /// Stable backend diagnostic.
        message: String,
    },
    /// The live graph exceeded its explicit persistence bound.
    Graph {
        /// Stable incremental-engine diagnostic.
        message: String,
    },
}

impl fmt::Display for DerivedSnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Backend { message } => write!(f, "derived Table operation failed: {message}"),
            Self::Graph { message } => write!(f, "cannot snapshot derived graph: {message}"),
        }
    }
}

impl Error for DerivedSnapshotError {}

struct PersistedCalc {
    schema: u64,
    source_generation: u64,
    control_generation: u64,
    source_identity: u64,
    control_identity: u64,
    graph: GraphSnapshot<CalcQuery, MemoValue>,
    reverse: BTreeMap<CalcQuery, BTreeSet<CalcQuery>>,
    receipts: BTreeMap<String, CalcReceipt>,
    queue: AutomaticQueueSnapshot,
    next_request_id: u64,
    next_logical_tick: u64,
    next_volatile: u64,
    refresh_samples: BTreeMap<String, BackendRefreshSample>,
    last_good: BTreeMap<String, Expr>,
}

impl ExprTreeCalc {
    /// Writes graph observations, reverse edges, memos, receipts, and scheduler
    /// continuations to the derived Table.
    pub fn persist_derived(
        &mut self,
        derived: &mut DerivedTableAdapter<'_>,
    ) -> Result<DerivedPersistReport, DerivedSnapshotError> {
        let roots = self
            .state
            .read()
            .expect("calc state poisoned")
            .cells
            .keys()
            .cloned()
            .map(CalcQuery::Cell)
            .collect::<Vec<_>>();
        let graph = self
            .engine
            .snapshot(
                roots,
                SnapshotBudgets::new(MAX_PERSISTED_GRAPH_NODES, MAX_PERSISTED_GRAPH_EDGES),
            )
            .map_err(|error| DerivedSnapshotError::Graph {
                message: error.to_string(),
            })?;
        let reverse = derive_reverse(&graph);
        let (source_identity, control_identity) =
            state_identities(&self.state, &self.context_factory)?;
        let (source_generation, control_generation, receipts, next_logical_tick, last_good_values) = {
            let state = self.state.read().expect("calc state poisoned");
            (
                state.source_generation,
                state.control_generation,
                state.receipts.clone(),
                state.next_logical_tick,
                state.last_good.clone(),
            )
        };
        let mut value_cx = (self.context_factory)();
        let last_good = last_good_values
            .into_iter()
            .filter_map(|(cell, value)| {
                value
                    .object()
                    .as_expr(&mut value_cx)
                    .ok()
                    .map(|expr| (cell, expr))
            })
            .collect();
        let queue = self.automatic_queue_snapshot();
        let pending_continuations = queue
            .entries
            .iter()
            .filter(|entry| entry.incremental_continuation.is_some())
            .count();
        let persisted = PersistedCalc {
            schema: GRAPH_SCHEMA_VERSION,
            source_generation,
            control_generation,
            source_identity,
            control_identity,
            graph,
            reverse,
            receipts,
            queue,
            next_request_id: self.next_request_id,
            next_logical_tick,
            next_volatile: self.next_volatile.load(Ordering::Acquire),
            refresh_samples: self.refresh_samples.clone(),
            last_good,
        };
        let report = DerivedPersistReport {
            nodes: persisted.graph.nodes.len(),
            reverse_edges: persisted.reverse.values().map(BTreeSet::len).sum(),
            receipts: persisted.receipts.len(),
            pending_continuations,
        };
        let mut cx = (self.context_factory)();
        let encoded = encode_persisted(&persisted, &mut cx)?;
        derived.store(encoded)?;
        Ok(report)
    }

    /// Opens a valid derived graph or conservatively rebuilds it.
    ///
    /// Missing, corrupt, incompatible, and source/control-mismatched records
    /// are deleted and treated as a cache miss. Authored source and operational
    /// control state are never changed by this operation.
    pub fn restore_derived(
        &mut self,
        derived: &mut DerivedTableAdapter<'_>,
    ) -> Result<DerivedRestoreReport, DerivedSnapshotError> {
        let Some(encoded) = derived.load()? else {
            self.rebuild_derived_state();
            return Ok(DerivedRestoreReport::rebuilt(
                DerivedRestoreDisposition::RebuiltMissing,
            ));
        };
        let mut cx = (self.context_factory)();
        let persisted = match decode_persisted(&encoded, &mut cx) {
            Ok(snapshot) => snapshot,
            Err(DecodeError::Incompatible) => {
                self.rebuild_derived_state();
                derived.delete()?;
                return Ok(DerivedRestoreReport::rebuilt(
                    DerivedRestoreDisposition::RebuiltIncompatible,
                ));
            }
            Err(DecodeError::Corrupt(_)) => {
                self.rebuild_derived_state();
                derived.delete()?;
                return Ok(DerivedRestoreReport::rebuilt(
                    DerivedRestoreDisposition::RebuiltCorrupt,
                ));
            }
        };
        let (source_identity, control_identity) =
            state_identities(&self.state, &self.context_factory)?;
        let generations_match = {
            let state = self.state.read().expect("calc state poisoned");
            persisted.source_generation == state.source_generation
                && persisted.control_generation == state.control_generation
        };
        if !generations_match
            || persisted.source_identity != source_identity
            || persisted.control_identity != control_identity
        {
            self.rebuild_derived_state();
            derived.delete()?;
            return Ok(DerivedRestoreReport::rebuilt(
                DerivedRestoreDisposition::RebuiltGenerationMismatch,
            ));
        }
        if persisted.reverse != derive_reverse(&persisted.graph) {
            self.rebuild_derived_state();
            derived.delete()?;
            return Ok(DerivedRestoreReport::rebuilt(
                DerivedRestoreDisposition::RebuiltCorrupt,
            ));
        }

        let current = current_from_graph(&persisted.graph);
        let restore = match self.engine.restore_snapshot(persisted.graph) {
            Ok(report) => report,
            Err(_) => {
                self.rebuild_derived_state();
                derived.delete()?;
                return Ok(DerivedRestoreReport::rebuilt(
                    DerivedRestoreDisposition::RebuiltCorrupt,
                ));
            }
        };
        let pending_tokens = persisted
            .queue
            .entries
            .iter()
            .filter_map(|entry| entry.incremental_continuation)
            .collect::<BTreeSet<_>>();
        if self.restore_automatic_queue(persisted.queue).is_err() {
            self.rebuild_derived_state();
            derived.delete()?;
            return Ok(DerivedRestoreReport::rebuilt(
                DerivedRestoreDisposition::RebuiltCorrupt,
            ));
        }
        let restored_receipts = persisted.receipts.len();
        {
            let mut state = self.state.write().expect("calc state poisoned");
            state.receipts = persisted.receipts;
            state.next_logical_tick = persisted.next_logical_tick.max(1);
            state.current = current.0;
            state.failed_cells = current.1;
            state.volatile = current.2;
            state.last_good = restore_last_good(persisted.last_good, &mut cx);
        }
        self.next_request_id = self.next_request_id.max(persisted.next_request_id);
        self.next_volatile
            .store(persisted.next_volatile.max(1), Ordering::Release);
        self.refresh_samples = persisted.refresh_samples;
        self.restored_continuations = pending_tokens;
        Ok(DerivedRestoreReport {
            disposition: DerivedRestoreDisposition::Rehydrated,
            restored_nodes: restore.nodes,
            recovered_dirty: restore.recovered_dirty,
            restored_receipts,
            pending_continuations: self.restored_continuations.len(),
        })
    }

    fn rebuild_derived_state(&mut self) {
        self.engine = IncrementalEngine::new();
        let (cells, mount_epochs) = {
            let state = self.state.read().expect("calc state poisoned");
            (
                state.cells.keys().cloned().collect::<Vec<_>>(),
                state
                    .mounts
                    .iter()
                    .map(|(path, mount)| (path.clone(), mount.epoch))
                    .collect::<Vec<_>>(),
            )
        };
        {
            let mut state = self.state.write().expect("calc state poisoned");
            state.active_request = None;
            state.attempts.clear();
            state.receipts.clear();
            state.next_logical_tick = 1;
            state.current.clear();
            state.last_good.clear();
            state.volatile.clear();
            state.failed_cells.clear();
        }
        self.next_request_id = 1;
        self.automatic_queue.clear();
        self.automatic_generation = 1;
        self.next_queue_sequence = 1;
        self.next_volatile.store(1, Ordering::Release);
        self.restored_continuations.clear();
        self.refresh_samples = mount_epochs
            .into_iter()
            .map(|(path, epoch)| (path, BackendRefreshSample::new(epoch)))
            .collect();
        for cell in cells {
            self.register_cell_query(cell);
        }
        self.schedule_dirty_automatic();
    }
}

fn backend_error(error: impl fmt::Display) -> DerivedSnapshotError {
    DerivedSnapshotError::Backend {
        message: error.to_string(),
    }
}

fn derive_reverse(
    graph: &GraphSnapshot<CalcQuery, MemoValue>,
) -> BTreeMap<CalcQuery, BTreeSet<CalcQuery>> {
    let mut reverse = BTreeMap::new();
    for node in &graph.nodes {
        for observation in &node.dependencies {
            reverse
                .entry(observation.key().clone())
                .or_insert_with(BTreeSet::new)
                .insert(node.key.clone());
        }
    }
    reverse
}

type CurrentState = (
    BTreeMap<String, Result<Value, CalcError>>,
    BTreeSet<String>,
    BTreeSet<String>,
);

fn current_from_graph(graph: &GraphSnapshot<CalcQuery, MemoValue>) -> CurrentState {
    let mut current = BTreeMap::new();
    let mut failed = BTreeSet::new();
    let mut volatile = BTreeSet::new();
    for node in &graph.nodes {
        let CalcQuery::Cell(cell) = &node.key else {
            continue;
        };
        let Some(memo) = &node.value else {
            continue;
        };
        match &memo.outcome {
            MemoOutcome::Value(value) => {
                current.insert(cell.clone(), Ok(value.clone()));
                if memo.is_volatile() {
                    volatile.insert(cell.clone());
                }
            }
            MemoOutcome::Failure(failure) => {
                current.insert(cell.clone(), Err(CalcError::Cell(failure.clone())));
                failed.insert(cell.clone());
            }
        }
    }
    (current, failed, volatile)
}

fn restore_last_good(values: BTreeMap<String, Expr>, cx: &mut Cx) -> BTreeMap<String, Value> {
    values
        .into_iter()
        .filter_map(|(cell, expr)| restore_value(cx, expr).ok().map(|(value, _)| (cell, value)))
        .collect()
}
