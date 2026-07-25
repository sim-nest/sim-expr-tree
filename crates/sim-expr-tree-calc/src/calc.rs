use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, RwLock},
};

use sim_expr_tree_core::{BackendKind, MountEpoch, MountResource};
use sim_incremental_core::{GraphSnapshot, IncrementalEngine, IncrementalError, SnapshotBudgets};
use sim_kernel::Symbol;
use sim_table_core::TablePath;

mod eval;
use eval::{eval_cell_expr, observe_runtime_context, parent_path, path_key};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CellExpr {
    Literal(String),
    Bare(Symbol),
    Quoted(Symbol),
    Ref {
        base: Option<TablePath>,
        reference: String,
    },
    CallableRef(String),
    MacroRef(String),
    Join(Vec<CellExpr>),
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CalcQuery {
    Cell(String),
    NameSlot(String),
    LookupStep(String),
    Listing(String),
    MountEpoch(String),
    EffectivePolicy,
    CodecRegistry,
    AuthorityCeiling,
}

pub struct ExprTreeCalc {
    state: Arc<RwLock<CalcState>>,
    engine: IncrementalEngine<CalcQuery, String>,
}

#[derive(Clone, Debug, Default)]
struct CalcState {
    cells: BTreeMap<String, CellExpr>,
    bound_names: BTreeSet<String>,
    mounts: BTreeMap<String, MountState>,
    effective_policy: String,
    codec_registry_revision: u64,
    authority_ceiling: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MountState {
    resource: MountResource,
    backend: BackendKind,
    epoch: MountEpoch,
}

impl ExprTreeCalc {
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(CalcState {
                effective_policy: "default".to_owned(),
                authority_ceiling: "ambient".to_owned(),
                ..CalcState::default()
            })),
            engine: IncrementalEngine::new(),
        }
    }

    pub fn set_cell(&mut self, path: TablePath, expr: CellExpr) {
        let key = path_key(&path);
        self.state
            .write()
            .expect("calc state poisoned")
            .cells
            .insert(key.clone(), expr);
        self.register_cell_query(key);
        self.invalidate_cell_path(&path);
    }

    pub fn remove_cell(&mut self, path: &TablePath) {
        let key = path_key(path);
        self.state
            .write()
            .expect("calc state poisoned")
            .cells
            .remove(&key);
        self.register_cell_query(key);
        self.invalidate_cell_path(path);
    }

    pub fn move_cell(&mut self, from: &TablePath, to: TablePath) {
        let from_key = path_key(from);
        let to_key = path_key(&to);
        let moved = {
            let mut state = self.state.write().expect("calc state poisoned");
            let moved = state.cells.remove(&from_key);
            if let Some(expr) = moved.clone() {
                state.cells.insert(to_key.clone(), expr);
            }
            moved
        };
        if moved.is_some() {
            self.register_cell_query(to_key);
        }
        self.register_cell_query(from_key);
        self.invalidate_cell_path(from);
        self.invalidate_cell_path(&to);
    }

    pub fn bind_name(&mut self, name: impl Into<String>) {
        self.state
            .write()
            .expect("calc state poisoned")
            .bound_names
            .insert(name.into());
    }

    pub fn set_effective_policy(&mut self, policy: impl Into<String>) {
        self.state
            .write()
            .expect("calc state poisoned")
            .effective_policy = policy.into();
        self.engine.invalidate(&CalcQuery::EffectivePolicy);
    }

    pub fn set_codec_registry_revision(&mut self, revision: u64) {
        self.state
            .write()
            .expect("calc state poisoned")
            .codec_registry_revision = revision;
        self.engine.invalidate(&CalcQuery::CodecRegistry);
    }

    pub fn set_authority_ceiling(&mut self, ceiling: impl Into<String>) {
        self.state
            .write()
            .expect("calc state poisoned")
            .authority_ceiling = ceiling.into();
        self.engine.invalidate(&CalcQuery::AuthorityCeiling);
    }

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

    pub fn verify_cell(&mut self, path: &TablePath) -> Result<String, IncrementalError<CalcQuery>> {
        self.engine.verify(CalcQuery::Cell(path_key(path)))
    }

    pub fn snapshot_cell(
        &mut self,
        path: &TablePath,
    ) -> Result<GraphSnapshot<CalcQuery, String>, IncrementalError<CalcQuery>> {
        self.engine.snapshot(
            [CalcQuery::Cell(path_key(path))],
            SnapshotBudgets::default(),
        )
    }

    fn register_cell_query(&mut self, key: String) {
        let state = Arc::clone(&self.state);
        self.engine
            .register_fn(CalcQuery::Cell(key), move |query, frame| {
                let CalcQuery::Cell(cell_key) = query else {
                    return Err(IncrementalError::UnknownQuery { key: query.clone() });
                };
                observe_runtime_context(frame)?;
                let expr = state
                    .read()
                    .expect("calc state poisoned")
                    .cells
                    .get(cell_key)
                    .cloned();
                let Some(expr) = expr else {
                    frame.observe_missing(CalcQuery::NameSlot(cell_key.clone()))?;
                    return Ok(format!("missing:{cell_key}"));
                };
                eval_cell_expr(&state, frame, cell_key, &expr)
            });
    }

    fn invalidate_cell_path(&mut self, path: &TablePath) {
        self.engine.invalidate(&CalcQuery::Cell(path_key(path)));
        self.engine.invalidate(&CalcQuery::NameSlot(path_key(path)));
        self.engine
            .invalidate(&CalcQuery::Listing(path_key(&parent_path(path))));
    }
}

impl Default for ExprTreeCalc {
    fn default() -> Self {
        Self::new()
    }
}
