//! Parsing helpers for open server and web-session request maps.

use sim_kernel::{Expr, Symbol};
use sim_value::access;

use crate::error::{ExpressionTreeServerError, ServerResult};
use crate::model::SessionId;

pub(crate) fn operation(expr: &Expr) -> Option<Symbol> {
    access::field_sym(expr, "op")
}

pub(crate) fn required_expr(expr: &Expr, name: &str) -> ServerResult<Expr> {
    access::field(expr, name).cloned().ok_or_else(|| {
        ExpressionTreeServerError::new("invalid-request", format!("missing field {name}"))
    })
}

pub(crate) fn required_string(expr: &Expr, name: &str) -> ServerResult<String> {
    match required_expr(expr, name)? {
        Expr::String(value) => Ok(value),
        _ => Err(ExpressionTreeServerError::new(
            "invalid-request",
            format!("field {name} must be a string"),
        )),
    }
}

pub(crate) fn required_session(expr: &Expr) -> ServerResult<SessionId> {
    required_resource(expr, "resource")
}

pub(crate) fn required_resource(expr: &Expr, name: &str) -> ServerResult<SessionId> {
    match required_expr(expr, name)? {
        Expr::Symbol(resource) => SessionId::from_resource(&resource).ok_or_else(|| {
            ExpressionTreeServerError::new(
                "invalid-session-id",
                format!("{resource} is not an expression-tree session resource"),
            )
        }),
        _ => Err(ExpressionTreeServerError::new(
            "invalid-session-id",
            format!("field {name} must be a session resource symbol"),
        )),
    }
}

pub(crate) fn optional_resource(expr: &Expr) -> ServerResult<Option<SessionId>> {
    match access::field(expr, "resource") {
        None | Some(Expr::Nil) => Ok(None),
        Some(Expr::Symbol(resource)) => {
            SessionId::from_resource(resource).map(Some).ok_or_else(|| {
                ExpressionTreeServerError::new(
                    "invalid-session-id",
                    format!("{resource} is not an expression-tree session resource"),
                )
            })
        }
        Some(_) => Err(ExpressionTreeServerError::new(
            "invalid-session-id",
            "resource must be a session resource symbol",
        )),
    }
}

pub(crate) fn uint(expr: &Expr, name: &str) -> ServerResult<u64> {
    let value = required_expr(expr, name)?;
    match value {
        Expr::Number(number) => number.canonical.parse().map_err(|_| {
            ExpressionTreeServerError::new(
                "invalid-request",
                format!("field {name} must be an unsigned integer"),
            )
        }),
        Expr::String(text) => text.parse().map_err(|_| {
            ExpressionTreeServerError::new(
                "invalid-request",
                format!("field {name} must be an unsigned integer"),
            )
        }),
        _ => Err(ExpressionTreeServerError::new(
            "invalid-request",
            format!("field {name} must be an unsigned integer"),
        )),
    }
}
