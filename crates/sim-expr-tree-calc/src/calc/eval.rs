use std::sync::{Arc, RwLock};

use sim_expr_tree_core::MountResource;
use sim_incremental_core::{IncrementalError, ObservationKind, QueryFrame};
use sim_kernel::Symbol;
use sim_table_core::{TablePath, TablePathRef};

use super::{CalcQuery, CalcState, CellExpr};

pub(super) fn eval_cell_expr(
    state: &Arc<RwLock<CalcState>>,
    frame: &mut QueryFrame<'_, CalcQuery, String>,
    caller_key: &str,
    expr: &CellExpr,
) -> Result<String, IncrementalError<CalcQuery>> {
    match expr {
        CellExpr::Literal(value) => Ok(value.clone()),
        CellExpr::Bare(symbol) => resolve_bare_symbol(state, frame, caller_key, symbol),
        CellExpr::Quoted(symbol) => Ok(symbol.to_string()),
        CellExpr::Ref { base, reference } => {
            let base = base
                .as_ref()
                .cloned()
                .unwrap_or_else(|| parse_absolute(caller_key));
            resolve_explicit_ref(state, frame, &base, reference)
        }
        CellExpr::CallableRef(reference) | CellExpr::MacroRef(reference) => {
            resolve_explicit_ref(state, frame, &parse_absolute(caller_key), reference)
        }
        CellExpr::Join(parts) => {
            let mut out = String::new();
            for part in parts {
                out.push_str(&eval_cell_expr(state, frame, caller_key, part)?);
            }
            Ok(out)
        }
    }
}

fn resolve_explicit_ref(
    state: &Arc<RwLock<CalcState>>,
    frame: &mut QueryFrame<'_, CalcQuery, String>,
    base_cell: &TablePath,
    reference: &str,
) -> Result<String, IncrementalError<CalcQuery>> {
    let reference = TablePathRef::parse(reference).map_err(|_| IncrementalError::UnknownQuery {
        key: CalcQuery::NameSlot(reference.to_owned()),
    })?;
    let target =
        parent_path(base_cell)
            .resolve(&reference)
            .map_err(|_| IncrementalError::UnknownQuery {
                key: CalcQuery::NameSlot(reference.to_reference_string()),
            })?;
    resolve_target(state, frame, &target)
}

fn resolve_bare_symbol(
    state: &Arc<RwLock<CalcState>>,
    frame: &mut QueryFrame<'_, CalcQuery, String>,
    caller_key: &str,
    symbol: &Symbol,
) -> Result<String, IncrementalError<CalcQuery>> {
    let name = symbol.to_string();
    if state
        .read()
        .expect("calc state poisoned")
        .bound_names
        .contains(&name)
    {
        return Ok(format!("bound:{name}"));
    }
    let mut dir = parent_path(&parse_absolute(caller_key));
    loop {
        frame.observe_listing(CalcQuery::Listing(path_key(&dir)))?;
        let mut candidate = dir.clone();
        candidate
            .push(&name)
            .map_err(|_| IncrementalError::UnknownQuery {
                key: CalcQuery::NameSlot(name.clone()),
            })?;
        observe_lookup_path(frame, &candidate)?;
        if cell_exists(state, &candidate) {
            return frame.read(CalcQuery::Cell(path_key(&candidate)));
        }
        frame.observe_missing(CalcQuery::NameSlot(path_key(&candidate)))?;
        if dir.is_root() {
            return Ok(format!("missing:{name}"));
        }
        dir = parent_path(&dir);
    }
}

fn resolve_target(
    state: &Arc<RwLock<CalcState>>,
    frame: &mut QueryFrame<'_, CalcQuery, String>,
    target: &TablePath,
) -> Result<String, IncrementalError<CalcQuery>> {
    frame.observe_listing(CalcQuery::Listing(path_key(&parent_path(target))))?;
    observe_lookup_path(frame, target)?;
    observe_mount_epochs(state, frame, target)?;
    if cell_exists(state, target) {
        frame.read(CalcQuery::Cell(path_key(target)))
    } else {
        frame.observe_missing(CalcQuery::NameSlot(path_key(target)))?;
        Ok(format!("missing:{}", path_key(target)))
    }
}

pub(super) fn observe_runtime_context(
    frame: &mut QueryFrame<'_, CalcQuery, String>,
) -> Result<(), IncrementalError<CalcQuery>> {
    frame.observe_policy(CalcQuery::EffectivePolicy)?;
    frame.observe(
        ObservationKind::Custom("codec-registry"),
        CalcQuery::CodecRegistry,
    )?;
    frame.observe_policy(CalcQuery::AuthorityCeiling)
}

fn observe_lookup_path(
    frame: &mut QueryFrame<'_, CalcQuery, String>,
    target: &TablePath,
) -> Result<(), IncrementalError<CalcQuery>> {
    let mut step = TablePath::root();
    frame.observe(
        ObservationKind::Custom("lookup-step"),
        CalcQuery::LookupStep(path_key(&step)),
    )?;
    for segment in target.segments() {
        step.push(segment)
            .map_err(|_| IncrementalError::UnknownQuery {
                key: CalcQuery::LookupStep(segment.clone()),
            })?;
        frame.observe(
            ObservationKind::Custom("lookup-step"),
            CalcQuery::LookupStep(path_key(&step)),
        )?;
    }
    Ok(())
}

fn observe_mount_epochs(
    state: &Arc<RwLock<CalcState>>,
    frame: &mut QueryFrame<'_, CalcQuery, String>,
    target: &TablePath,
) -> Result<(), IncrementalError<CalcQuery>> {
    let mounts = state.read().expect("calc state poisoned").mounts.clone();
    for (mount_key, mount) in mounts {
        let mount_path = parse_absolute(&mount_key);
        if (mount.resource == MountResource::Dir || mount_path == *target)
            && is_prefix(&mount_path, target)
        {
            let _backend = mount.backend;
            let _epoch = mount.epoch;
            frame.observe_epoch(CalcQuery::MountEpoch(mount_key))?;
        }
    }
    Ok(())
}

fn cell_exists(state: &Arc<RwLock<CalcState>>, path: &TablePath) -> bool {
    state
        .read()
        .expect("calc state poisoned")
        .cells
        .contains_key(&path_key(path))
}

fn is_prefix(candidate: &TablePath, path: &TablePath) -> bool {
    candidate.segments().len() <= path.segments().len()
        && candidate
            .segments()
            .iter()
            .zip(path.segments())
            .all(|(left, right)| left == right)
}

pub(super) fn parent_path(path: &TablePath) -> TablePath {
    TablePath::from_segments(
        path.segments()
            .iter()
            .take(path.segments().len().saturating_sub(1)),
    )
    .expect("existing path segments are valid")
}

fn parse_absolute(path: &str) -> TablePath {
    TablePath::parse_absolute(path).expect("stored calc keys are absolute")
}

pub(super) fn path_key(path: &TablePath) -> String {
    path.to_absolute_reference()
}
