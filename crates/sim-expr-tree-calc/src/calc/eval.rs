use std::sync::{Arc, RwLock, atomic::AtomicU64};

use sim_expr_tree_core::MountResource;
use sim_incremental_core::{IncrementalError, ObservationKind, QueryFrame};
use sim_kernel::{CapabilitySet, Cx, Expr, Phase, Symbol};
use sim_table_core::{TablePath, TablePathRef};

use super::{
    CalcQuery, CalcState, CellFailure, ContextFactory, EffectStamp, HARD_MAX_EXPR_DEPTH,
    MemoOutcome, MemoValue, incremental_failure, value::canonicalize_value,
};
use crate::EXPR_TREE_REF;

mod namespace;
use namespace::{install_bindings, parse_absolute, resolve_bare_symbol, resolve_explicit_ref};
pub(super) use namespace::{observe_runtime_context, parent_path, path_key};

pub(super) const MAX_RECEIPT_EFFECTS: usize = 32;

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

pub(super) struct EvaluatedMemo {
    pub(super) memo: MemoValue,
    pub(super) effects: Vec<EffectStamp>,
    pub(super) omitted_effects: usize,
}

pub(super) fn evaluate_cell(
    state: &Arc<RwLock<CalcState>>,
    frame: &mut QueryFrame<'_, CalcQuery, MemoValue>,
    context_factory: &Arc<ContextFactory>,
    next_volatile: &AtomicU64,
    caller_key: &str,
    source: Option<Expr>,
    capabilities: CapabilitySet,
) -> Result<EvaluatedMemo, IncrementalError<CalcQuery>> {
    let mut cx = context_factory();
    if let Err(error) = install_bindings(state, &mut cx) {
        return Ok(EvaluatedMemo {
            memo: MemoValue::failure(CellFailure::Evaluation {
                message: format!("cannot install evaluation bindings: {error}"),
            }),
            effects: Vec::new(),
            omitted_effects: 0,
        });
    }
    let Some(source) = source else {
        let value = match cx.factory().string(format!("missing:{caller_key}")) {
            Ok(value) => value,
            Err(error) => {
                return Ok(EvaluatedMemo {
                    memo: MemoValue::failure(CellFailure::Evaluation {
                        message: format!("cannot construct missing-cell value: {error}"),
                    }),
                    effects: Vec::new(),
                    omitted_effects: 0,
                });
            }
        };
        return canonicalize_value(frame, &mut cx, next_volatile, value).map(|memo| {
            EvaluatedMemo {
                memo,
                effects: Vec::new(),
                omitted_effects: 0,
            }
        });
    };

    let mut result = None;
    cx.with_capabilities(capabilities, |cx| {
        result = Some((|| {
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
                cx,
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
            canonicalize_value(frame, cx, next_volatile, value).map_err(EvalAbort::from)
        })());
        Ok(())
    })
    .expect("installing diminished capabilities cannot fail");
    let result = result.expect("diminished evaluation must produce a result");
    let (effects, omitted_effects) = effect_evidence(&cx);

    match result {
        Ok(memo) => Ok(EvaluatedMemo {
            memo,
            effects,
            omitted_effects,
        }),
        Err(EvalAbort::Cell(failure)) => Ok(EvaluatedMemo {
            memo: MemoValue::failure(failure),
            effects,
            omitted_effects,
        }),
        Err(EvalAbort::Incremental(error)) => {
            incremental_failure(error).map(|memo| EvaluatedMemo {
                memo,
                effects,
                omitted_effects,
            })
        }
    }
}

fn effect_evidence(cx: &Cx) -> (Vec<EffectStamp>, usize) {
    let ledger = cx.effect_ledger();
    let total = ledger.records().len();
    let effects = ledger
        .records()
        .iter()
        .take(MAX_RECEIPT_EFFECTS)
        .filter_map(|record| {
            ledger.effect(&record.effect).map(|effect| EffectStamp {
                kind: effect.kind.to_string(),
                aborted: record.aborted,
            })
        })
        .collect();
    (effects, total.saturating_sub(MAX_RECEIPT_EFFECTS))
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
