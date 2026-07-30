//! EvalFabric and server-backed surface request routing.

use sim_kernel::{Cx, Error, EvalReply, EvalRequest, Expr, Symbol, Value};

use super::{
    ExpressionTreeServer, RuntimeTarget, begin_request, classify_kernel_error, execute_runtime,
    is_revision_change, is_surface_local, operation_metadata, reserved_session_mut, session_mut,
    snapshot_revision, stale, target_session,
};
use crate::error::{ExpressionTreeServerError, ServerResult};
use crate::model::SessionId;
use crate::protocol;

impl ExpressionTreeServer {
    pub(crate) fn realize_request(
        &self,
        cx: &mut Cx,
        request: EvalRequest,
    ) -> sim_kernel::Result<EvalReply> {
        cx.require_all(&request.required_capabilities)?;
        let trace_requested = request.trace;
        let result_shape = request.result_shape.clone();
        let value = match self.route_value(cx, request.expr) {
            Ok(value) => value,
            Err(error) => cx.factory().expr(error.to_expr())?,
        };
        if let Some(shape_value) = result_shape {
            let Some(shape) = shape_value.object().as_shape() else {
                return Err(Error::TypeMismatch {
                    expected: "shape",
                    found: "non-shape",
                });
            };
            let matched = shape.check_value(cx, value.clone())?;
            if !matched.accepted {
                return Err(Error::HostError(
                    "expression-tree server reply failed requested shape".to_owned(),
                ));
            }
        }
        Ok(EvalReply {
            value,
            diagnostics: cx.take_diagnostics(),
            trace: trace_requested
                .then(|| {
                    cx.factory()
                        .symbol(Symbol::qualified("expr-tree-server", "trace"))
                })
                .transpose()?,
        })
    }

    fn route_value(&self, cx: &mut Cx, expr: Expr) -> ServerResult<Value> {
        if let Some(op) = protocol::operation(&expr) {
            let result = match op.as_qualified_str().as_str() {
                "web-session/read" => {
                    let session = protocol::required_session(&expr)?;
                    self.snapshot(&session)
                }
                "web-session/realize" => {
                    let session = protocol::required_session(&expr)?;
                    let operation = protocol::required_expr(&expr, "operation")?;
                    self.commit_surface_operation(cx, &session, None, &operation)
                }
                "web-session/commit" => {
                    let session = protocol::required_session(&expr)?;
                    let operation = protocol::required_expr(&expr, "operation")?;
                    let expected = protocol::required_expr(&expr, "expected-current")?;
                    let revision = (!matches!(expected, Expr::Nil))
                        .then(|| snapshot_revision(&expected))
                        .transpose()?;
                    self.commit_surface_operation(cx, &session, revision, &operation)
                }
                "web-session/changes" => {
                    let session = match protocol::optional_resource(&expr)? {
                        Some(session) => session,
                        None => self.single_session_id()?,
                    };
                    self.drain_changes(&session)
                }
                "expr-tree-server/create" => {
                    let storage = protocol::required_string(&expr, "storage")?;
                    self.create_session(cx, &storage)
                        .map(|session| Expr::Symbol(session.resource()))
                }
                "expr-tree-server/close" | "expr-tree-server/cancel" => {
                    let session = protocol::required_session(&expr)?;
                    self.close_session(&session).map(Expr::Bool)
                }
                "expr-tree-server/intent" => {
                    let session = protocol::required_session(&expr)?;
                    let revision = protocol::uint(&expr, "revision")?;
                    let intent = protocol::required_expr(&expr, "intent")?;
                    self.apply_intent(cx, &session, revision, &intent)
                }
                _ => return self.eval_ordinary(cx, expr),
            }?;
            return cx.factory().expr(result).map_err(classify_kernel_error);
        }

        if let Some(session) = target_session(&expr) {
            return self.execute_direct_operation(cx, &session, &expr);
        }
        self.eval_ordinary(cx, expr)
    }

    fn eval_ordinary(&self, cx: &mut Cx, expr: Expr) -> ServerResult<Value> {
        cx.eval_expr(expr).map_err(classify_kernel_error)
    }

    pub(super) fn commit_surface_operation(
        &self,
        cx: &mut Cx,
        session: &SessionId,
        expected_revision: Option<u64>,
        operation: &Expr,
    ) -> ServerResult<Expr> {
        let wall = self.wall_observation();
        let (kind, path) = operation_metadata(operation)?;
        if is_surface_local(operation) {
            let mut registry = self.lock_registry()?;
            let tick = begin_request(&mut registry, self.limits);
            let record = session_mut(&mut registry, session)?;
            record.last_activity_tick = tick;
            if let Some(expected) = expected_revision
                && expected != record.revision
            {
                return Err(stale(expected, record.revision));
            }
            if record.apply_surface_local(operation, self.limits)? {
                record.changed(&kind, path, tick, wall, self.limits);
            }
            return record.snapshot(self.limits);
        } else {
            let (target, tick) = self.reserve_runtime(session, expected_revision)?;
            let result = execute_runtime(cx, &target, operation);
            let mut registry = self.lock_registry()?;
            if !registry.in_flight.remove(session) {
                return Err(ExpressionTreeServerError::new(
                    "internal",
                    "runtime operation lost its session reservation",
                ));
            }
            let record = reserved_session_mut(&mut registry, session)?;
            result?;
            if is_revision_change(&kind) {
                record.changed(&kind, path, tick, wall, self.limits);
            }
            return record.snapshot(self.limits);
        }
    }

    fn execute_direct_operation(
        &self,
        cx: &mut Cx,
        session: &SessionId,
        operation: &Expr,
    ) -> ServerResult<Value> {
        let wall = self.wall_observation();
        let (kind, path) = operation_metadata(operation)?;
        let (target, tick) = self.reserve_runtime(session, None)?;
        let result = execute_runtime(cx, &target, operation);
        let mut registry = self.lock_registry()?;
        if !registry.in_flight.remove(session) {
            return Err(ExpressionTreeServerError::new(
                "internal",
                "runtime operation lost its session reservation",
            ));
        }
        let record = reserved_session_mut(&mut registry, session)?;
        let value = result?;
        if is_revision_change(&kind) {
            record.changed(&kind, path, tick, wall, self.limits);
        }
        Ok(value)
    }

    fn reserve_runtime(
        &self,
        session: &SessionId,
        expected_revision: Option<u64>,
    ) -> ServerResult<(RuntimeTarget, u64)> {
        let mut registry = self.lock_registry()?;
        let tick = begin_request(&mut registry, self.limits);
        let target = {
            let record = session_mut(&mut registry, session)?;
            record.last_activity_tick = tick;
            if let Some(expected) = expected_revision
                && expected != record.revision
            {
                return Err(stale(expected, record.revision));
            }
            RuntimeTarget::new(record)
        };
        registry.in_flight.insert(session.clone());
        Ok((target, tick))
    }

    fn drain_changes(&self, session: &SessionId) -> ServerResult<Expr> {
        let mut registry = self.lock_registry()?;
        let tick = begin_request(&mut registry, self.limits);
        let record = session_mut(&mut registry, session)?;
        record.last_activity_tick = tick;
        Ok(Expr::List(
            record
                .drain_changes()
                .iter()
                .map(crate::model::ChangeEvent::to_expr)
                .collect(),
        ))
    }

    fn single_session_id(&self) -> ServerResult<SessionId> {
        let registry = self.lock_registry()?;
        if registry.sessions.len() != 1 {
            return Err(ExpressionTreeServerError::new(
                "ambiguous-session",
                "changes without a resource require exactly one live session",
            ));
        }
        registry
            .sessions
            .keys()
            .next()
            .cloned()
            .ok_or_else(|| ExpressionTreeServerError::new("unknown-session", "no live session"))
    }
}
