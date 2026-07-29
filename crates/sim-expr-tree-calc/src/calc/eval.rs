use std::sync::{Arc, RwLock, atomic::AtomicU64};

use sim_expr_tree_core::MountResource;
use sim_incremental_core::{IncrementalError, ObservationKind, QueryFrame};
use sim_kernel::{Cx, Expr, Phase, Symbol};
use sim_table_core::{TablePath, TablePathRef};

use super::{
    CalcQuery, CalcState, CellFailure, ContextFactory, HARD_MAX_EXPR_DEPTH, MemoOutcome, MemoValue,
    incremental_failure, value::canonicalize_value,
};
use crate::EXPR_TREE_REF;

enum EvalAbort {
    Cell(CellFailure),
    Incremental(IncrementalError<CalcQuery>),
}

impl From<CellFailure> for EvalAbort {
    fn from(value: CellFailure) -> Self {
        Self::Cell(value)
    }
}

impl From<IncrementalError<CalcQuery>> for EvalAbort {
    fn from(value: IncrementalError<CalcQuery>) -> Self {
        Self::Incremental(value)
    }
}

pub(super) fn evaluate_cell(
    state: &Arc<RwLock<CalcState>>,
    frame: &mut QueryFrame<'_, CalcQuery, MemoValue>,
    context_factory: &Arc<ContextFactory>,
    next_volatile: &AtomicU64,
    caller_key: &str,
    source: Option<Expr>,
) -> Result<MemoValue, IncrementalError<CalcQuery>> {
    let mut cx = context_factory();
    if let Err(error) = install_bindings(state, &mut cx) {
        return Ok(MemoValue::failure(CellFailure::Evaluation {
            message: format!("cannot install evaluation bindings: {error}"),
        }));
    }
    let Some(source) = source else {
        let value = match cx.factory().string(format!("missing:{caller_key}")) {
            Ok(value) => value,
            Err(error) => {
                return Ok(MemoValue::failure(CellFailure::Evaluation {
                    message: format!("cannot construct missing-cell value: {error}"),
                }));
            }
        };
        return canonicalize_value(frame, &mut cx, next_volatile, value);
    };

    let result = (|| {
        let expanded =
            cx.expand_macros(Phase::Eval, source)
                .map_err(|error| CellFailure::Evaluation {
                    message: error.to_string(),
                })?;
        // Expansion must happen before dependency preparation so references
        // emitted by macros are observed. The fresh per-cell context is then
        // cleared to prevent the kernel evaluator from expanding the already
        // prepared form a second time.
        cx.clear_macro_expander();
        let mut dependency_index = 0;
        let prepared = prepare_expr(
            state,
            frame,
            &mut cx,
            caller_key,
            expanded,
            0,
            &mut dependency_index,
        )?;
        let value = cx
            .eval_expr(prepared)
            .map_err(|error| CellFailure::Evaluation {
                message: error.to_string(),
            })?;
        canonicalize_value(frame, &mut cx, next_volatile, value).map_err(EvalAbort::from)
    })();

    match result {
        Ok(memo) => Ok(memo),
        Err(EvalAbort::Cell(failure)) => Ok(MemoValue::failure(failure)),
        Err(EvalAbort::Incremental(error)) => incremental_failure(error),
    }
}

#[allow(clippy::too_many_arguments)]
fn prepare_expr(
    state: &Arc<RwLock<CalcState>>,
    frame: &mut QueryFrame<'_, CalcQuery, MemoValue>,
    cx: &mut Cx,
    caller_key: &str,
    expr: Expr,
    depth: usize,
    dependency_index: &mut usize,
) -> Result<Expr, EvalAbort> {
    if depth > HARD_MAX_EXPR_DEPTH {
        return Err(CellFailure::ExpressionDepth {
            limit: HARD_MAX_EXPR_DEPTH,
        }
        .into());
    }
    frame.charge_work(1)?;
    let child_depth = depth.saturating_add(1);
    match expr {
        Expr::Symbol(symbol) => {
            frame.observe(
                ObservationKind::Custom("lexical-binding"),
                CalcQuery::NameSlot(symbol.to_string()),
            )?;
            if cx.symbol_is_bound(&symbol) {
                Ok(Expr::Symbol(symbol))
            } else {
                let value = resolve_bare_symbol(state, frame, caller_key, &symbol)?;
                bind_dependency(cx, value, dependency_index)
            }
        }
        Expr::Call { operator, args } if matches!(operator.as_ref(), Expr::Symbol(symbol) if symbol.to_string() == EXPR_TREE_REF) =>
        {
            let reference = explicit_reference(&args)?;
            let value =
                resolve_explicit_ref(state, frame, &parse_absolute(caller_key), &reference)?;
            bind_dependency(cx, value, dependency_index)
        }
        Expr::Call { operator, args } => Ok(Expr::Call {
            operator: Box::new(prepare_expr(
                state,
                frame,
                cx,
                caller_key,
                *operator,
                child_depth,
                dependency_index,
            )?),
            args: prepare_items(
                state,
                frame,
                cx,
                caller_key,
                args,
                child_depth,
                dependency_index,
            )?,
        }),
        Expr::List(items) => Ok(Expr::List(prepare_items(
            state,
            frame,
            cx,
            caller_key,
            items,
            child_depth,
            dependency_index,
        )?)),
        Expr::Vector(items) => Ok(Expr::Vector(prepare_items(
            state,
            frame,
            cx,
            caller_key,
            items,
            child_depth,
            dependency_index,
        )?)),
        Expr::Set(items) => Ok(Expr::Set(prepare_items(
            state,
            frame,
            cx,
            caller_key,
            items,
            child_depth,
            dependency_index,
        )?)),
        Expr::Block(items) => Ok(Expr::Block(prepare_items(
            state,
            frame,
            cx,
            caller_key,
            items,
            child_depth,
            dependency_index,
        )?)),
        Expr::Map(entries) => Ok(Expr::Map(
            entries
                .into_iter()
                .map(|(key, value)| {
                    Ok((
                        prepare_expr(
                            state,
                            frame,
                            cx,
                            caller_key,
                            key,
                            child_depth,
                            dependency_index,
                        )?,
                        prepare_expr(
                            state,
                            frame,
                            cx,
                            caller_key,
                            value,
                            child_depth,
                            dependency_index,
                        )?,
                    ))
                })
                .collect::<Result<Vec<_>, EvalAbort>>()?,
        )),
        Expr::Annotated { expr, annotations } => Ok(Expr::Annotated {
            expr: Box::new(prepare_expr(
                state,
                frame,
                cx,
                caller_key,
                *expr,
                child_depth,
                dependency_index,
            )?),
            annotations,
        }),
        // These forms are data under the kernel evaluator. References inside
        // them must not become dependencies until a loaded evaluator executes
        // them.
        Expr::Quote { .. }
        | Expr::Extension { .. }
        | Expr::Infix { .. }
        | Expr::Prefix { .. }
        | Expr::Postfix { .. }
        | Expr::Local(_)
        | Expr::Nil
        | Expr::Bool(_)
        | Expr::Number(_)
        | Expr::String(_)
        | Expr::Bytes(_) => Ok(expr),
    }
}

#[allow(clippy::too_many_arguments)]
fn prepare_items(
    state: &Arc<RwLock<CalcState>>,
    frame: &mut QueryFrame<'_, CalcQuery, MemoValue>,
    cx: &mut Cx,
    caller_key: &str,
    items: Vec<Expr>,
    depth: usize,
    dependency_index: &mut usize,
) -> Result<Vec<Expr>, EvalAbort> {
    items
        .into_iter()
        .map(|item| prepare_expr(state, frame, cx, caller_key, item, depth, dependency_index))
        .collect()
}

fn bind_dependency(
    cx: &mut Cx,
    memo: MemoValue,
    dependency_index: &mut usize,
) -> Result<Expr, EvalAbort> {
    match memo.outcome {
        MemoOutcome::Value(value) => {
            let name = Symbol::qualified(
                "expr-tree-dependency",
                format!("value-{}", *dependency_index),
            );
            *dependency_index = dependency_index.saturating_add(1);
            cx.env_mut().define(name.clone(), value);
            Ok(Expr::Symbol(name))
        }
        MemoOutcome::Failure(failure) => Err(failure.into()),
    }
}

fn explicit_reference(args: &[Expr]) -> Result<String, CellFailure> {
    match args {
        [Expr::String(reference)] => Ok(reference.clone()),
        [Expr::Symbol(reference)] => Ok(reference.to_string()),
        _ => Err(CellFailure::Evaluation {
            message: "expr-tree/ref requires exactly one string or symbol reference".to_owned(),
        }),
    }
}

fn resolve_explicit_ref(
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

fn resolve_bare_symbol(
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

fn resolve_target(
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

pub(super) fn observe_runtime_context(
    frame: &mut QueryFrame<'_, CalcQuery, MemoValue>,
) -> Result<(), IncrementalError<CalcQuery>> {
    frame.observe_policy(CalcQuery::EffectivePolicy)?;
    frame.observe(
        ObservationKind::Custom("codec-registry"),
        CalcQuery::CodecRegistry,
    )?;
    frame.observe_policy(CalcQuery::AuthorityCeiling)
}

fn observe_lookup_path(
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

fn observe_mount_epochs(
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

fn install_bindings(state: &Arc<RwLock<CalcState>>, cx: &mut Cx) -> sim_kernel::Result<()> {
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

fn default_value_context() -> Cx {
    use sim_kernel::{DefaultFactory, EagerPolicy};

    Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory))
}
