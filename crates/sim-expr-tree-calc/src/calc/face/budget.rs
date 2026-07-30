use std::cmp::Ordering;

use sim_expr_tree_core::FaceBudget;
use sim_kernel::{Cx, Expr, Symbol, Value};

use super::{FaceDimension, FaceIssue, bounded_message};

pub(super) fn inspect_expr(expr: &Expr, budget: FaceBudget) -> Result<(), FaceIssue> {
    let mut stack = vec![(expr, 0usize)];
    let mut items = 0usize;
    let mut bytes = 0usize;
    while let Some((expr, depth)) = stack.pop() {
        if depth > budget.max_depth() {
            return Err(FaceIssue::Truncated {
                dimension: FaceDimension::Depth,
                limit: budget.max_depth(),
                observed: depth,
            });
        }
        items = items.saturating_add(1);
        if items > budget.max_items() {
            return Err(FaceIssue::Truncated {
                dimension: FaceDimension::Items,
                limit: budget.max_items(),
                observed: items,
            });
        }
        charge_expr_scalar(expr, &mut bytes, budget)?;
        push_expr_children(expr, depth.saturating_add(1), &mut stack);
    }
    Ok(())
}

pub(super) fn bounded_value_expr(
    cx: &mut Cx,
    value: &Value,
    budget: FaceBudget,
) -> Result<Expr, FaceIssue> {
    let mut tracker = FaceTracker::new(budget);
    project_value(cx, value, 0, &mut tracker)
}

fn project_value(
    cx: &mut Cx,
    value: &Value,
    depth: usize,
    tracker: &mut FaceTracker,
) -> Result<Expr, FaceIssue> {
    tracker.enter(depth)?;
    if value.object().as_callable().is_some() {
        return Err(FaceIssue::Unsupported {
            reason: "callable result has no bounded data projection".to_owned(),
        });
    }
    if let Some(list) = value.object().as_list() {
        let remaining = tracker.remaining_items();
        match list.len_cmp(cx, remaining).map_err(projection_failure)? {
            Ordering::Greater => return Err(tracker.items_truncated()),
            Ordering::Less | Ordering::Equal => {}
        }
        let values = list
            .to_vec(cx, Some(remaining))
            .map_err(projection_failure)?;
        let mut items = Vec::with_capacity(values.len());
        for value in values {
            items.push(project_value(cx, &value, depth.saturating_add(1), tracker)?);
        }
        return Ok(Expr::List(items));
    }
    if let Some(table) = value.object().as_table_impl() {
        let len = table.len(cx).map_err(projection_failure)?;
        if len > tracker.remaining_items() / 2 {
            return Err(tracker.items_truncated());
        }
        let entries = table.entries(cx).map_err(projection_failure)?;
        if entries.len() > len {
            return Err(tracker.items_truncated());
        }
        let mut projected = Vec::with_capacity(entries.len());
        for (key, value) in entries {
            tracker.enter(depth.saturating_add(1))?;
            tracker.bytes(symbol_bytes(&key))?;
            projected.push((
                Expr::Symbol(key),
                project_value(cx, &value, depth.saturating_add(1), tracker)?,
            ));
        }
        return Ok(Expr::Map(projected));
    }
    let snapshot = value.object().snapshot(cx).map_err(projection_failure)?;
    let Some(snapshot) = snapshot else {
        return Err(FaceIssue::Unsupported {
            reason: "result exposes no bounded data snapshot".to_owned(),
        });
    };
    let expr = Expr::from(snapshot);
    tracker.consume_expr_tail(&expr, depth)
}

struct FaceTracker {
    budget: FaceBudget,
    items: usize,
    bytes: usize,
}

impl FaceTracker {
    fn new(budget: FaceBudget) -> Self {
        Self {
            budget,
            items: 0,
            bytes: 0,
        }
    }

    fn enter(&mut self, depth: usize) -> Result<(), FaceIssue> {
        if depth > self.budget.max_depth() {
            return Err(FaceIssue::Truncated {
                dimension: FaceDimension::Depth,
                limit: self.budget.max_depth(),
                observed: depth,
            });
        }
        self.items = self.items.saturating_add(1);
        if self.items > self.budget.max_items() {
            return Err(self.items_truncated());
        }
        Ok(())
    }

    fn bytes(&mut self, additional: usize) -> Result<(), FaceIssue> {
        charge_bytes(&mut self.bytes, additional, self.budget)
    }

    fn remaining_items(&self) -> usize {
        self.budget.max_items().saturating_sub(self.items)
    }

    fn items_truncated(&self) -> FaceIssue {
        FaceIssue::Truncated {
            dimension: FaceDimension::Items,
            limit: self.budget.max_items(),
            observed: self.budget.max_items().saturating_add(1),
        }
    }

    fn consume_expr_tail(&mut self, expr: &Expr, root_depth: usize) -> Result<Expr, FaceIssue> {
        let mut stack = Vec::new();
        push_expr_children(expr, root_depth.saturating_add(1), &mut stack);
        charge_expr_scalar(expr, &mut self.bytes, self.budget)?;
        while let Some((child, depth)) = stack.pop() {
            self.enter(depth)?;
            charge_expr_scalar(child, &mut self.bytes, self.budget)?;
            push_expr_children(child, depth.saturating_add(1), &mut stack);
        }
        Ok(expr.clone())
    }
}

fn push_expr_children<'a>(expr: &'a Expr, depth: usize, stack: &mut Vec<(&'a Expr, usize)>) {
    match expr {
        Expr::List(children)
        | Expr::Vector(children)
        | Expr::Set(children)
        | Expr::Block(children) => {
            stack.extend(children.iter().rev().map(|child| (child, depth)));
        }
        Expr::Map(entries) => {
            for (key, value) in entries.iter().rev() {
                stack.push((value, depth));
                stack.push((key, depth));
            }
        }
        Expr::Call { operator, args } => {
            stack.extend(args.iter().rev().map(|arg| (arg, depth)));
            stack.push((operator, depth));
        }
        Expr::Infix { left, right, .. } => {
            stack.push((right, depth));
            stack.push((left, depth));
        }
        Expr::Prefix { arg, .. }
        | Expr::Postfix { arg, .. }
        | Expr::Quote { expr: arg, .. }
        | Expr::Extension { payload: arg, .. } => stack.push((arg, depth)),
        Expr::Annotated { expr, annotations } => {
            stack.push((expr, depth));
            stack.extend(annotations.iter().rev().map(|(_, value)| (value, depth)));
        }
        Expr::Nil
        | Expr::Bool(_)
        | Expr::Number(_)
        | Expr::Symbol(_)
        | Expr::String(_)
        | Expr::Bytes(_)
        | Expr::Local(_) => {}
    }
}

fn charge_expr_scalar(expr: &Expr, bytes: &mut usize, budget: FaceBudget) -> Result<(), FaceIssue> {
    match expr {
        Expr::String(value) => charge_bytes(bytes, value.len(), budget),
        Expr::Bytes(value) => charge_bytes(bytes, value.len(), budget),
        Expr::Number(value) => {
            charge_bytes(bytes, symbol_bytes(&value.domain), budget)?;
            charge_bytes(bytes, value.canonical.len(), budget)
        }
        Expr::Symbol(value) | Expr::Local(value) => {
            charge_bytes(bytes, symbol_bytes(value), budget)
        }
        Expr::Infix { operator, .. }
        | Expr::Prefix { operator, .. }
        | Expr::Postfix { operator, .. } => charge_bytes(bytes, symbol_bytes(operator), budget),
        Expr::Annotated { annotations, .. } => {
            for (name, _) in annotations {
                charge_bytes(bytes, symbol_bytes(name), budget)?;
            }
            Ok(())
        }
        Expr::Extension { tag, .. } => charge_bytes(bytes, symbol_bytes(tag), budget),
        Expr::Nil
        | Expr::Bool(_)
        | Expr::List(_)
        | Expr::Vector(_)
        | Expr::Map(_)
        | Expr::Set(_)
        | Expr::Call { .. }
        | Expr::Quote { .. }
        | Expr::Block(_) => Ok(()),
    }
}

fn charge_bytes(total: &mut usize, additional: usize, budget: FaceBudget) -> Result<(), FaceIssue> {
    *total = total.saturating_add(additional);
    if *total > budget.max_bytes() {
        return Err(FaceIssue::Truncated {
            dimension: FaceDimension::Bytes,
            limit: budget.max_bytes(),
            observed: *total,
        });
    }
    Ok(())
}

fn projection_failure(error: sim_kernel::Error) -> FaceIssue {
    FaceIssue::Unsupported {
        reason: bounded_message(error.to_string()),
    }
}

fn symbol_bytes(symbol: &Symbol) -> usize {
    symbol
        .namespace
        .as_ref()
        .map_or(0, |namespace| namespace.len().saturating_add(1))
        .saturating_add(symbol.name.len())
}
