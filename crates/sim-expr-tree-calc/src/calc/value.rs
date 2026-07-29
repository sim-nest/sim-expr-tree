use std::sync::atomic::{AtomicU64, Ordering};

use sim_incremental_core::{IncrementalError, QueryFrame};
use sim_kernel::{Cx, Expr, Value};

use super::{CalcQuery, HARD_MAX_EXPR_DEPTH, MemoValue};

pub(super) fn canonicalize_value(
    frame: &mut QueryFrame<'_, CalcQuery, MemoValue>,
    cx: &mut Cx,
    next_volatile: &AtomicU64,
    value: Value,
) -> Result<MemoValue, IncrementalError<CalcQuery>> {
    match value.object().as_expr(cx) {
        Ok(expr) if !is_opaque_projection(&expr) => {
            frame.charge_output(expr_weight(&expr, 0))?;
            Ok(MemoValue::canonical(value, expr.canonical_key()))
        }
        Ok(expr) => {
            frame.charge_output(expr_weight(&expr, 0))?;
            Ok(MemoValue::volatile(
                value,
                next_volatile.fetch_add(1, Ordering::Relaxed),
            ))
        }
        Err(_) => {
            frame.charge_output(1)?;
            Ok(MemoValue::volatile(
                value,
                next_volatile.fetch_add(1, Ordering::Relaxed),
            ))
        }
    }
}

fn is_opaque_projection(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Extension { tag, .. } if tag.to_string() == "core/opaque-object"
    )
}

fn expr_weight(expr: &Expr, depth: usize) -> usize {
    if depth > HARD_MAX_EXPR_DEPTH {
        return usize::MAX;
    }
    let child_depth = depth.saturating_add(1);
    let children = match expr {
        Expr::List(items) | Expr::Vector(items) | Expr::Set(items) | Expr::Block(items) => {
            items.iter().fold(0usize, |total, item| {
                total.saturating_add(expr_weight(item, child_depth))
            })
        }
        Expr::Map(entries) => entries.iter().fold(0usize, |total, (key, value)| {
            total
                .saturating_add(expr_weight(key, child_depth))
                .saturating_add(expr_weight(value, child_depth))
        }),
        Expr::Call { operator, args } => args
            .iter()
            .fold(expr_weight(operator, child_depth), |total, item| {
                total.saturating_add(expr_weight(item, child_depth))
            }),
        Expr::Infix { left, right, .. } => {
            expr_weight(left, child_depth).saturating_add(expr_weight(right, child_depth))
        }
        Expr::Prefix { arg, .. }
        | Expr::Postfix { arg, .. }
        | Expr::Quote { expr: arg, .. }
        | Expr::Extension { payload: arg, .. } => expr_weight(arg, child_depth),
        Expr::Annotated { expr, annotations } => annotations
            .iter()
            .fold(expr_weight(expr, child_depth), |total, (_, item)| {
                total.saturating_add(expr_weight(item, child_depth))
            }),
        Expr::String(value) => value.len(),
        Expr::Bytes(value) => value.len(),
        Expr::Number(number) => number.canonical.len(),
        Expr::Symbol(symbol) | Expr::Local(symbol) => symbol.to_string().len(),
        Expr::Nil | Expr::Bool(_) => 1,
    };
    1usize.saturating_add(children)
}
