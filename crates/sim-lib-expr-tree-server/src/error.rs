//! Structured expression-tree server errors.

use std::fmt;

use sim_kernel::{Error, Expr, Symbol};
use sim_value::build;

/// Stable structured error returned by the expression-tree server.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpressionTreeServerError {
    code: &'static str,
    message: String,
}

impl ExpressionTreeServerError {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        let message = message.into();
        let message = message.chars().take(512).collect();
        Self { code, message }
    }

    /// Returns the stable machine-readable error code.
    pub const fn code(&self) -> &'static str {
        self.code
    }

    /// Returns the bounded human-readable message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Projects the error into the open remote-error map understood by standard
    /// server-backed surface transports.
    pub fn to_expr(&self) -> Expr {
        build::map(vec![
            (
                "error",
                Expr::Symbol(Symbol::qualified("expr-tree-server", self.code)),
            ),
            ("message", build::text(&self.message)),
        ])
    }
}

impl fmt::Display for ExpressionTreeServerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "expr-tree-server/{}: {}",
            self.code, self.message
        )
    }
}

impl std::error::Error for ExpressionTreeServerError {}

impl From<ExpressionTreeServerError> for Error {
    fn from(error: ExpressionTreeServerError) -> Self {
        Self::HostError(error.to_string())
    }
}

pub(crate) type ServerResult<T> = std::result::Result<T, ExpressionTreeServerError>;

pub(crate) fn internal(error: impl fmt::Display) -> ExpressionTreeServerError {
    ExpressionTreeServerError::new("internal", error.to_string())
}
