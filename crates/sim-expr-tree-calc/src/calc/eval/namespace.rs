use super::*;

pub(super) fn resolve_explicit_ref(
    state: &Arc<RwLock<CalcState>>,
    frame: &mut QueryFrame<'_, CalcQuery, MemoValue>,
    base_cell: &TablePath,
    reference: &str,
) -> Result<MemoValue, EvalAbort> {
    let reference = TablePathRef::parse(reference).map_err(|error| CellFailure::Evaluation {
        message: format!("invalid expression-tree reference {reference:?}: {error:?}"),
    })?;
    let target = parent_path(base_cell)
        .resolve(&reference)
        .map_err(|error| CellFailure::Evaluation {
            message: format!(
                "cannot resolve expression-tree reference {}: {error:?}",
                reference.to_reference_string()
            ),
        })?;
    resolve_target(state, frame, &target)
}

pub(super) fn resolve_bare_symbol(
    state: &Arc<RwLock<CalcState>>,
    frame: &mut QueryFrame<'_, CalcQuery, MemoValue>,
    caller_key: &str,
    symbol: &Symbol,
) -> Result<MemoValue, EvalAbort> {
    let name = symbol.to_string();
    let mut dir = parent_path(&parse_absolute(caller_key));
    loop {
        frame.observe_listing(CalcQuery::Listing(path_key(&dir)))?;
        let mut candidate = dir.clone();
        candidate
            .push(&name)
            .map_err(|error| CellFailure::Evaluation {
                message: format!("invalid bare cell name {name:?}: {error:?}"),
            })?;
        observe_lookup_path(frame, &candidate)?;
        if cell_exists(state, &candidate) {
            return frame
                .read(CalcQuery::Cell(path_key(&candidate)))
                .map_err(EvalAbort::from);
        }
        frame.observe_missing(CalcQuery::NameSlot(path_key(&candidate)))?;
        if dir.is_root() {
            let cx = default_value_context();
            let value = cx
                .factory()
                .string(format!("missing:{name}"))
                .map_err(|error| CellFailure::Evaluation {
                    message: error.to_string(),
                })?;
            return Ok(MemoValue::canonical(
                value,
                Expr::String(format!("missing:{name}")).canonical_key(),
            ));
        }
        dir = parent_path(&dir);
    }
}

pub(super) fn resolve_target(
    state: &Arc<RwLock<CalcState>>,
    frame: &mut QueryFrame<'_, CalcQuery, MemoValue>,
    target: &TablePath,
) -> Result<MemoValue, EvalAbort> {
    frame.observe_listing(CalcQuery::Listing(path_key(&parent_path(target))))?;
    observe_lookup_path(frame, target)?;
    observe_mount_epochs(state, frame, target)?;
    if cell_exists(state, target) {
        frame
            .read(CalcQuery::Cell(path_key(target)))
            .map_err(EvalAbort::from)
    } else {
        frame.observe_missing(CalcQuery::NameSlot(path_key(target)))?;
        let text = format!("missing:{}", path_key(target));
        let cx = default_value_context();
        let value = cx
            .factory()
            .string(text.clone())
            .map_err(|error| CellFailure::Evaluation {
                message: error.to_string(),
            })?;
        Ok(MemoValue::canonical(
            value,
            Expr::String(text).canonical_key(),
        ))
    }
}

pub(in crate::calc) fn observe_runtime_context(
    frame: &mut QueryFrame<'_, CalcQuery, MemoValue>,
    cell_key: &str,
) -> Result<(), IncrementalError<CalcQuery>> {
    frame.observe_policy(CalcQuery::EffectivePolicy(cell_key.to_owned()))?;
    frame.observe_policy(CalcQuery::AuthorityPolicy(cell_key.to_owned()))?;
    frame.observe(
        ObservationKind::Custom("codec-registry"),
        CalcQuery::CodecRegistry,
    )?;
    frame.observe_policy(CalcQuery::AuthorityCeiling)?;
    frame.observe(
        ObservationKind::Custom("force-epoch"),
        CalcQuery::ForceEpoch(cell_key.to_owned()),
    )
}

pub(super) fn observe_lookup_path(
    frame: &mut QueryFrame<'_, CalcQuery, MemoValue>,
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

pub(super) fn observe_mount_epochs(
    state: &Arc<RwLock<CalcState>>,
    frame: &mut QueryFrame<'_, CalcQuery, MemoValue>,
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

pub(super) fn install_bindings(
    state: &Arc<RwLock<CalcState>>,
    cx: &mut Cx,
) -> sim_kernel::Result<()> {
    let (bound_names, bound_values) = {
        let state = state.read().expect("calc state poisoned");
        (state.bound_names.clone(), state.bound_values.clone())
    };
    for name in bound_names {
        let value = cx.factory().string(format!("bound:{name}"))?;
        cx.env_mut().define(Symbol::new(name), value);
    }
    for (name, value) in bound_values {
        cx.env_mut().define(name, value);
    }
    Ok(())
}

pub(super) fn cell_exists(state: &Arc<RwLock<CalcState>>, path: &TablePath) -> bool {
    state
        .read()
        .expect("calc state poisoned")
        .cells
        .contains_key(&path_key(path))
}

pub(super) fn is_prefix(candidate: &TablePath, path: &TablePath) -> bool {
    candidate.segments().len() <= path.segments().len()
        && candidate
            .segments()
            .iter()
            .zip(path.segments())
            .all(|(left, right)| left == right)
}

pub(in crate::calc) fn parent_path(path: &TablePath) -> TablePath {
    TablePath::from_segments(
        path.segments()
            .iter()
            .take(path.segments().len().saturating_sub(1)),
    )
    .expect("existing path segments are valid")
}

pub(super) fn parse_absolute(path: &str) -> TablePath {
    TablePath::parse_absolute(path).expect("stored calc keys are absolute")
}

pub(in crate::calc) fn path_key(path: &TablePath) -> String {
    path.to_absolute_reference()
}

pub(super) fn default_value_context() -> Cx {
    use sim_kernel::{DefaultFactory, EagerPolicy};

    Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory))
}
