//! Public bounded server model.

use std::collections::VecDeque;

use sim_kernel::{Expr, Symbol};
use sim_value::build;

/// Opaque identity of one authoritative expression-tree session.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SessionId(pub(crate) String);

impl SessionId {
    /// Parses an opaque session resource symbol.
    pub fn from_resource(resource: &Symbol) -> Option<Self> {
        (resource.namespace.as_deref() == Some("expr-tree/session"))
            .then(|| Self(resource.name.to_string()))
    }

    /// Returns the standard resource symbol carried by surface requests.
    pub fn resource(&self) -> Symbol {
        Symbol::qualified("expr-tree/session", self.0.clone())
    }
}

/// Opaque identity of one bounded session watch.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WatchId(pub(crate) String);

/// Hard lifecycle and backpressure limits for one server.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExpressionTreeServerLimits {
    /// Maximum concurrent authoritative sessions.
    pub max_sessions: usize,
    /// Logical request ticks a session may remain idle.
    pub max_idle_ticks: u64,
    /// Maximum watches registered on one session.
    pub max_watches_per_session: usize,
    /// Maximum queued changes retained per watch and implicit change feed.
    pub watch_capacity: usize,
    /// Maximum entries rendered in one directory page.
    pub max_page_entries: usize,
    /// Maximum expanded directory depth rendered in one snapshot.
    pub max_snapshot_depth: usize,
}

impl Default for ExpressionTreeServerLimits {
    fn default() -> Self {
        Self {
            max_sessions: 128,
            max_idle_ticks: 10_000,
            max_watches_per_session: 16,
            watch_capacity: 128,
            max_page_entries: 128,
            max_snapshot_depth: 16,
        }
    }
}

impl ExpressionTreeServerLimits {
    pub(crate) fn validate(self) -> bool {
        self.max_sessions > 0
            && self.max_idle_ticks > 0
            && self.max_watches_per_session > 0
            && self.watch_capacity > 0
            && self.max_page_entries > 0
            && self.max_snapshot_depth > 0
    }
}

/// One revisioned server-side change observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangeEvent {
    /// Session resource whose snapshot changed.
    pub resource: Symbol,
    /// Session revision after the change.
    pub revision: u64,
    /// Mandatory monotone server logical tick.
    pub logical_tick: u64,
    /// Optional human wall-clock observation.
    pub wall_ms: Option<u64>,
    /// Stable change kind.
    pub kind: String,
    /// Canonical affected path, when known.
    pub path: Option<String>,
}

impl ChangeEvent {
    pub(crate) fn to_expr(&self) -> Expr {
        build::map(vec![
            ("resource", Expr::Symbol(self.resource.clone())),
            ("revision", build::uint(self.revision)),
            ("logical-tick", build::uint(self.logical_tick)),
            (
                "wall-ms",
                self.wall_ms.map(build::uint).unwrap_or(Expr::Nil),
            ),
            ("kind", build::sym(&self.kind)),
            (
                "path",
                self.path.as_ref().map(build::text).unwrap_or(Expr::Nil),
            ),
        ])
    }
}

/// One bounded watch poll with explicit overflow evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WatchBatch {
    /// Drained events in logical order.
    pub events: Vec<ChangeEvent>,
    /// Lifetime events dropped because this consumer was slow.
    pub dropped: u64,
    /// Whether the watch has been cancelled.
    pub cancelled: bool,
}

pub(crate) struct WatchState {
    pub(crate) events: VecDeque<ChangeEvent>,
    pub(crate) dropped: u64,
    pub(crate) cancelled: bool,
}

impl WatchState {
    pub(crate) fn new() -> Self {
        Self {
            events: VecDeque::new(),
            dropped: 0,
            cancelled: false,
        }
    }

    pub(crate) fn push(&mut self, event: ChangeEvent, capacity: usize) {
        if self.cancelled {
            return;
        }
        if self.events.len() == capacity {
            self.events.pop_front();
            self.dropped = self.dropped.saturating_add(1);
        }
        self.events.push_back(event);
    }
}
