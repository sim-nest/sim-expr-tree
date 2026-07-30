use std::sync::Arc;

use sim_kernel::{Cx, Expr, MatchScore, Result, Shape, ShapeDoc, ShapeMatch, Symbol, Value};
use sim_shape::shape_value;

/// Symbol for one operation's argument-list Shape.
pub fn operation_args_shape_symbol(operation: &str) -> Symbol {
    Symbol::qualified(format!("expr-tree/{operation}"), "Args")
}

/// Symbol for one operation's result Shape.
pub fn operation_result_shape_symbol(operation: &str) -> Symbol {
    Symbol::qualified(format!("expr-tree/{operation}"), "Result")
}

pub(crate) fn argument_shape(
    operation: &'static str,
    min_args: usize,
    max_args: usize,
    detail: &'static str,
) -> Value {
    let symbol = operation_args_shape_symbol(operation);
    shape_value(
        symbol.clone(),
        Arc::new(OperationArgsShape {
            symbol,
            operation,
            min_args,
            max_args,
            detail,
        }),
    )
}

pub(crate) fn result_shape(operation: &'static str, detail: &'static str) -> Value {
    let symbol = operation_result_shape_symbol(operation);
    shape_value(
        symbol.clone(),
        Arc::new(OperationResultShape {
            symbol,
            operation,
            detail,
        }),
    )
}

struct OperationArgsShape {
    symbol: Symbol,
    operation: &'static str,
    min_args: usize,
    max_args: usize,
    detail: &'static str,
}

impl Shape for OperationArgsShape {
    fn symbol(&self) -> Option<Symbol> {
        Some(self.symbol.clone())
    }

    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        let expression = value.object().as_expr(cx)?;
        self.check_expr(cx, &expression)
    }

    fn check_expr(&self, _cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        let Expr::List(items) = expr else {
            return Ok(ShapeMatch::reject(format!(
                "expr-tree/{} arguments must be a list",
                self.operation
            )));
        };
        if (self.min_args..=self.max_args).contains(&items.len()) {
            Ok(ShapeMatch::accept(MatchScore::exact(100)))
        } else {
            Ok(ShapeMatch::reject(format!(
                "expr-tree/{} expects {}..={} arguments, found {}",
                self.operation,
                self.min_args,
                self.max_args,
                items.len()
            )))
        }
    }

    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(
            ShapeDoc::new(format!("expr-tree/{} argument list", self.operation))
                .with_detail(self.detail)
                .with_detail(format!(
                    "bounded arity {}..={}",
                    self.min_args, self.max_args
                )),
        )
    }
}

struct OperationResultShape {
    symbol: Symbol,
    operation: &'static str,
    detail: &'static str,
}

impl Shape for OperationResultShape {
    fn symbol(&self) -> Option<Symbol> {
        Some(self.symbol.clone())
    }

    fn is_total(&self) -> bool {
        true
    }

    fn check_value(&self, _cx: &mut Cx, _value: Value) -> Result<ShapeMatch> {
        Ok(ShapeMatch::accept(MatchScore::exact(1)))
    }

    fn check_expr(&self, _cx: &mut Cx, _expr: &Expr) -> Result<ShapeMatch> {
        Ok(ShapeMatch::accept(MatchScore::exact(1)))
    }

    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(
            ShapeDoc::new(format!("expr-tree/{} result", self.operation))
                .with_detail(self.detail)
                .with_detail("ordinary bounded SIM value or opaque live handle"),
        )
    }
}
