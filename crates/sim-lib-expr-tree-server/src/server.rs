//! Bounded authoritative session registry and request routing.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
    },
};

use sim_kernel::{Cx, Env, Error, Expr, Symbol, Value};
use sim_lib_expr_tree::TreeHandle;
use sim_lib_server::{ServerAddress, SystemWallClock, WallClock};
use sim_lib_view::SurfaceCodec;
use sim_lib_view_expr_tree::ExpressionTreeSurfaceCodec;
use sim_value::access;

use crate::error::{ExpressionTreeServerError, ServerResult, internal};
use crate::model::{ExpressionTreeServerLimits, SessionId, WatchBatch, WatchId};
use crate::protocol;
use crate::session::SessionRecord;

mod route;

static NEXT_SERVER_NONCE: AtomicU64 = AtomicU64::new(1);

/// Authoritative bounded expression-tree session server.
pub struct ExpressionTreeServer {
    address: ServerAddress,
    codecs: Vec<Symbol>,
    clock: Arc<dyn WallClock>,
    limits: ExpressionTreeServerLimits,
    nonce: u64,
    registry: Mutex<Registry>,
}

struct Registry {
    sessions: BTreeMap<SessionId, SessionRecord>,
    in_flight: BTreeSet<SessionId>,
    next_session: u64,
    next_tick: u64,
}

struct RuntimeTarget {
    tree: TreeHandle,
    resource: Symbol,
}

impl RuntimeTarget {
    fn new(record: &SessionRecord) -> Self {
        Self {
            tree: record.tree.clone(),
            resource: record.resource(),
        }
    }
}

impl ExpressionTreeServer {
    /// Creates a server with explicit address, codecs, wall clock, and hard
    /// lifecycle limits.
    pub fn new(
        address: ServerAddress,
        codecs: Vec<Symbol>,
        clock: Arc<dyn WallClock>,
        limits: ExpressionTreeServerLimits,
    ) -> ServerResult<Self> {
        if codecs.is_empty() {
            return Err(ExpressionTreeServerError::new(
                "invalid-config",
                "at least one server codec is required",
            ));
        }
        if !limits.validate() {
            return Err(ExpressionTreeServerError::new(
                "invalid-config",
                "all expression-tree server limits must be nonzero",
            ));
        }
        Ok(Self {
            address,
            codecs,
            clock,
            limits,
            nonce: NEXT_SERVER_NONCE.fetch_add(1, Ordering::Relaxed),
            registry: Mutex::new(Registry {
                sessions: BTreeMap::new(),
                in_flight: BTreeSet::new(),
                next_session: 1,
                next_tick: 1,
            }),
        })
    }

    /// Creates a local server using the system wall clock and binary server
    /// frames.
    pub fn local() -> Self {
        Self::new(
            ServerAddress::Local,
            vec![Symbol::qualified("codec", "binary")],
            Arc::new(SystemWallClock),
            ExpressionTreeServerLimits::default(),
        )
        .expect("default expression-tree server configuration is valid")
    }

    /// Returns the configured server address.
    pub fn address(&self) -> &ServerAddress {
        &self.address
    }

    /// Returns the configured frame codecs.
    pub fn codecs(&self) -> &[Symbol] {
        &self.codecs
    }

    /// Returns the configured lifecycle limits.
    pub const fn limits(&self) -> ExpressionTreeServerLimits {
        self.limits
    }

    /// Creates one authoritative session, capturing the creator's runtime and
    /// immutable authority ceiling in the underlying expression tree.
    pub fn create_session(&self, cx: &mut Cx, storage_name: &str) -> ServerResult<SessionId> {
        let (id, tick) = {
            let mut registry = self.lock_registry()?;
            let tick = begin_request(&mut registry, self.limits);
            if registry.sessions.len() >= self.limits.max_sessions {
                return Err(ExpressionTreeServerError::new(
                    "session-limit",
                    format!("server session limit {} reached", self.limits.max_sessions),
                ));
            }
            let id = SessionId(format!(
                "{:016x}-{:016x}",
                self.nonce, registry.next_session
            ));
            registry.next_session = registry.next_session.saturating_add(1);
            (id, tick)
        };

        let value = cx
            .eval_expr(Expr::Call {
                operator: Box::new(Expr::Symbol(Symbol::qualified("expr-tree", "open"))),
                args: vec![Expr::String(storage_name.to_owned())],
            })
            .map_err(classify_kernel_error)?;
        let tree = value
            .object()
            .downcast_ref::<TreeHandle>()
            .cloned()
            .ok_or_else(|| {
                ExpressionTreeServerError::new(
                    "runtime-contract",
                    "expr-tree/open did not return a live TreeHandle",
                )
            })?;
        let clock = Arc::clone(&self.clock);
        tree.set_wall_clock(move || clock.now().ok().map(|time| time.unix_millis()))
            .map_err(classify_kernel_error)?;

        let mut registry = self.lock_registry()?;
        expire_idle(&mut registry, self.limits);
        if registry.sessions.len() >= self.limits.max_sessions {
            return Err(ExpressionTreeServerError::new(
                "session-limit",
                "session capacity changed while opening the tree",
            ));
        }
        registry
            .sessions
            .insert(id.clone(), SessionRecord::new(id.clone(), tree, tick));
        Ok(id)
    }

    /// Returns the current bounded snapshot for a session.
    pub fn snapshot(&self, session: &SessionId) -> ServerResult<Expr> {
        let mut registry = self.lock_registry()?;
        let tick = begin_request(&mut registry, self.limits);
        let record = session_mut(&mut registry, session)?;
        record.last_activity_tick = tick;
        record.snapshot(self.limits)
    }

    /// Returns the current optimistic revision.
    pub fn revision(&self, session: &SessionId) -> ServerResult<u64> {
        let mut registry = self.lock_registry()?;
        let tick = begin_request(&mut registry, self.limits);
        let record = session_mut(&mut registry, session)?;
        record.last_activity_tick = tick;
        Ok(record.revision)
    }

    /// Decodes and commits one standard Intent through the existing
    /// expression-tree `SurfaceCodec`.
    pub fn apply_intent(
        &self,
        cx: &mut Cx,
        session: &SessionId,
        expected_revision: u64,
        intent: &Expr,
    ) -> ServerResult<Expr> {
        let snapshot = self.snapshot(session)?;
        let current = snapshot_revision(&snapshot)?;
        if current != expected_revision {
            return Err(stale(expected_revision, current));
        }
        let codec = ExpressionTreeSurfaceCodec::new();
        let draft = codec
            .decode(cx, &snapshot, intent)
            .map_err(classify_kernel_error)?;
        let operation = codec.commit(cx, &draft).map_err(classify_kernel_error)?;
        cx.require_all(&operation.required_capabilities)
            .map_err(classify_kernel_error)?;
        self.commit_surface_operation(cx, session, Some(expected_revision), &operation.form)
    }

    /// Closes and removes one authoritative session.
    pub fn close_session(&self, session: &SessionId) -> ServerResult<bool> {
        let mut registry = self.lock_registry()?;
        begin_request(&mut registry, self.limits);
        if registry.in_flight.contains(session) {
            return Err(session_busy());
        }
        Ok(registry.sessions.remove(session).is_some())
    }

    /// Subscribes one bounded independent watch.
    pub fn subscribe(&self, session: &SessionId) -> ServerResult<WatchId> {
        let mut registry = self.lock_registry()?;
        let tick = begin_request(&mut registry, self.limits);
        let record = session_mut(&mut registry, session)?;
        record.last_activity_tick = tick;
        record.subscribe(self.limits)
    }

    /// Drains at most `limit` changes from one watch.
    pub fn poll_watch(
        &self,
        session: &SessionId,
        watch: &WatchId,
        limit: usize,
    ) -> ServerResult<WatchBatch> {
        let mut registry = self.lock_registry()?;
        let tick = begin_request(&mut registry, self.limits);
        let record = session_mut(&mut registry, session)?;
        record.last_activity_tick = tick;
        record.poll_watch(watch, limit.min(self.limits.watch_capacity))
    }

    /// Cancels a watch idempotently with respect to future event delivery.
    pub fn cancel_watch(&self, session: &SessionId, watch: &WatchId) -> ServerResult<()> {
        let mut registry = self.lock_registry()?;
        let tick = begin_request(&mut registry, self.limits);
        let record = session_mut(&mut registry, session)?;
        record.last_activity_tick = tick;
        record.cancel_watch(watch)
    }

    /// Advances the server's mandatory logical lifecycle clock and expires idle
    /// sessions. No wall-clock value participates in the comparison.
    pub fn maintenance_tick(&self, steps: u64) -> ServerResult<usize> {
        let mut registry = self.lock_registry()?;
        registry.next_tick = registry.next_tick.saturating_add(steps);
        let before = registry.sessions.len();
        expire_idle(&mut registry, self.limits);
        Ok(before.saturating_sub(registry.sessions.len()))
    }

    fn wall_observation(&self) -> Option<u64> {
        self.clock.now().ok().map(|time| time.unix_millis())
    }

    fn lock_registry(&self) -> ServerResult<MutexGuard<'_, Registry>> {
        self.registry.lock().map_err(internal)
    }

    #[cfg(test)]
    pub(crate) fn registry_is_unlocked_for_test(&self) -> bool {
        self.registry.try_lock().is_ok()
    }
}

impl Default for ExpressionTreeServer {
    fn default() -> Self {
        Self::local()
    }
}

fn begin_request(registry: &mut Registry, limits: ExpressionTreeServerLimits) -> u64 {
    let tick = registry.next_tick;
    registry.next_tick = registry.next_tick.saturating_add(1);
    expire_idle(registry, limits);
    tick
}

fn expire_idle(registry: &mut Registry, limits: ExpressionTreeServerLimits) {
    let now = registry.next_tick;
    let Registry {
        sessions,
        in_flight,
        ..
    } = registry;
    sessions.retain(|id, session| {
        in_flight.contains(id)
            || now.saturating_sub(session.last_activity_tick) <= limits.max_idle_ticks
    });
}

fn session_mut<'a>(
    registry: &'a mut Registry,
    session: &SessionId,
) -> ServerResult<&'a mut SessionRecord> {
    if registry.in_flight.contains(session) {
        return Err(session_busy());
    }
    reserved_session_mut(registry, session)
}

fn reserved_session_mut<'a>(
    registry: &'a mut Registry,
    session: &SessionId,
) -> ServerResult<&'a mut SessionRecord> {
    registry.sessions.get_mut(session).ok_or_else(|| {
        ExpressionTreeServerError::new(
            "unknown-session",
            "session is absent, expired, cancelled, or belongs to another server",
        )
    })
}

fn session_busy() -> ExpressionTreeServerError {
    ExpressionTreeServerError::new(
        "session-busy",
        "another operation is already evaluating for this session",
    )
}

fn execute_runtime(cx: &mut Cx, target: &RuntimeTarget, operation: &Expr) -> ServerResult<Value> {
    validate_runtime_target(target, operation)?;
    let tree = cx
        .factory()
        .opaque(Arc::new(target.tree.clone()))
        .map_err(classify_kernel_error)?;
    let mut env = Env::child(Arc::new(cx.env().clone()));
    env.define(target.resource.clone(), tree);
    cx.with_env(env, |cx| cx.eval_expr(operation.clone()))
        .map_err(classify_kernel_error)
}

fn validate_runtime_target(target: &RuntimeTarget, operation: &Expr) -> ServerResult<()> {
    let Expr::Call { operator, args } = operation else {
        return Err(ExpressionTreeServerError::new(
            "invalid-operation",
            "surface operation must be a local map or expression-tree call",
        ));
    };
    let Expr::Symbol(operator) = operator.as_ref() else {
        return Err(ExpressionTreeServerError::new(
            "invalid-operation",
            "runtime operation must name an expression-tree function",
        ));
    };
    if operator.namespace.as_deref() != Some("expr-tree") {
        return Err(ExpressionTreeServerError::new(
            "invalid-operation",
            "runtime operation is outside the expression-tree family",
        ));
    }
    if !matches!(args.first(), Some(Expr::Symbol(resource)) if resource == &target.resource) {
        return Err(ExpressionTreeServerError::new(
            "session-mismatch",
            "runtime operation targets another expression-tree session",
        ));
    }
    Ok(())
}

fn operation_metadata(operation: &Expr) -> ServerResult<(String, Option<String>)> {
    if let Some(op) = protocol::operation(operation) {
        let path = access::field_str(operation, "path").map(str::to_owned);
        return Ok((op.name.to_string(), path));
    }
    let Expr::Call { operator, args } = operation else {
        return Err(ExpressionTreeServerError::new(
            "invalid-operation",
            "operation is neither a map nor a call",
        ));
    };
    let Expr::Symbol(operator) = operator.as_ref() else {
        return Err(ExpressionTreeServerError::new(
            "invalid-operation",
            "operation call has a non-symbol operator",
        ));
    };
    let path = args.get(1).and_then(|arg| match arg {
        Expr::String(path) => Some(path.clone()),
        _ => None,
    });
    Ok((operator.name.to_string(), path))
}

fn is_surface_local(operation: &Expr) -> bool {
    protocol::operation(operation)
        .is_some_and(|op| op.namespace.as_deref() == Some("expr-tree-view"))
}

fn is_revision_change(kind: &str) -> bool {
    !matches!(kind, "ref" | "list" | "status" | "explain" | "open-policy")
}

fn target_session(expr: &Expr) -> Option<SessionId> {
    let Expr::Call { operator, args } = expr else {
        return None;
    };
    let Expr::Symbol(operator) = operator.as_ref() else {
        return None;
    };
    if operator.namespace.as_deref() != Some("expr-tree") {
        return None;
    }
    match args.first() {
        Some(Expr::Symbol(resource)) => SessionId::from_resource(resource),
        _ => None,
    }
}

fn snapshot_revision(snapshot: &Expr) -> ServerResult<u64> {
    protocol::uint(snapshot, "revision").map_err(|_| {
        ExpressionTreeServerError::new(
            "invalid-expected-revision",
            "expected-current is not an expression-tree snapshot",
        )
    })
}

fn stale(expected: u64, current: u64) -> ExpressionTreeServerError {
    ExpressionTreeServerError::new(
        "stale-revision",
        format!("expected revision {expected}, current revision is {current}"),
    )
}

fn classify_kernel_error(error: Error) -> ExpressionTreeServerError {
    match error {
        Error::CapabilityDenied { capability } => ExpressionTreeServerError::new(
            "authority-denied",
            format!("caller lacks capability {capability}"),
        ),
        Error::TrustDenied { capability, .. } => ExpressionTreeServerError::new(
            "trust-denied",
            format!("caller trust does not permit capability {capability}"),
        ),
        other => ExpressionTreeServerError::new("operation-failed", other.to_string()),
    }
}
