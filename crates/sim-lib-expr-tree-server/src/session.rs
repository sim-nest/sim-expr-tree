//! One authoritative session and its bounded surface projection.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use sim_kernel::{Expr, Symbol};
use sim_lib_expr_tree::{TreeEntryInspection, TreeEntryKind, TreeHandle};
use sim_lib_view_expr_tree::{
    ChildPage, ExpressionTreeSnapshot, FaceSnapshot, Freshness, NodeDetail, NodeSnapshot,
    ReceiptSummary, TimestampSummary,
};
use sim_value::access;

use crate::error::{ExpressionTreeServerError, ServerResult, internal};
use crate::model::{
    ChangeEvent, ExpressionTreeServerLimits, SessionId, WatchBatch, WatchId, WatchState,
};
use crate::protocol;

pub(crate) struct SessionRecord {
    pub(crate) id: SessionId,
    pub(crate) tree: TreeHandle,
    pub(crate) revision: u64,
    pub(crate) last_activity_tick: u64,
    expanded: BTreeSet<String>,
    page_sizes: BTreeMap<String, usize>,
    continuations: BTreeMap<String, (String, usize, u64)>,
    next_continuation: u64,
    source_wall: BTreeMap<String, u64>,
    result_wall: BTreeMap<String, u64>,
    changes: VecDeque<ChangeEvent>,
    dropped_changes: u64,
    watches: BTreeMap<WatchId, WatchState>,
    next_watch: u64,
}

impl SessionRecord {
    pub(crate) fn new(id: SessionId, tree: TreeHandle, tick: u64) -> Self {
        Self {
            id,
            tree,
            revision: 1,
            last_activity_tick: tick,
            expanded: BTreeSet::from(["/".to_owned()]),
            page_sizes: BTreeMap::new(),
            continuations: BTreeMap::new(),
            next_continuation: 1,
            source_wall: BTreeMap::new(),
            result_wall: BTreeMap::new(),
            changes: VecDeque::new(),
            dropped_changes: 0,
            watches: BTreeMap::new(),
            next_watch: 1,
        }
    }

    pub(crate) fn resource(&self) -> Symbol {
        self.id.resource()
    }

    pub(crate) fn snapshot(&mut self, limits: ExpressionTreeServerLimits) -> ServerResult<Expr> {
        self.continuations.clear();
        let mut remaining = limits
            .max_page_entries
            .saturating_mul(limits.max_snapshot_depth)
            .max(1);
        let nodes = self.directory_page("/", 0, limits, &mut remaining)?;
        let root = NodeSnapshot::expanded_dir(
            "/",
            "root",
            self.revision,
            if nodes.1 {
                ChildPage::Truncated {
                    nodes: nodes.0,
                    continuation: self.continuation("/", nodes.2),
                    remaining: nodes.3,
                }
            } else {
                ChildPage::Complete(nodes.0)
            },
        );
        Ok(
            ExpressionTreeSnapshot::new(Expr::Symbol(self.resource()), self.revision, vec![root])
                .to_expr(),
        )
    }

    fn directory_page(
        &mut self,
        path: &str,
        depth: usize,
        limits: ExpressionTreeServerLimits,
        remaining_budget: &mut usize,
    ) -> ServerResult<(Vec<NodeSnapshot>, bool, usize, Option<usize>)> {
        let entries = self.tree.inspect_entries(path).map_err(internal)?;
        let requested = self
            .page_sizes
            .get(path)
            .copied()
            .unwrap_or(limits.max_page_entries)
            .min(entries.len());
        let admitted = requested.min(*remaining_budget);
        *remaining_budget = remaining_budget.saturating_sub(admitted);
        let mut nodes = Vec::with_capacity(admitted);
        for entry in entries.iter().take(admitted) {
            nodes.push(self.node(entry, depth, limits, remaining_budget)?);
        }
        let truncated = admitted < entries.len();
        Ok((
            nodes,
            truncated,
            admitted,
            truncated.then_some(entries.len().saturating_sub(admitted)),
        ))
    }

    fn node(
        &mut self,
        entry: &TreeEntryInspection,
        depth: usize,
        limits: ExpressionTreeServerLimits,
        remaining_budget: &mut usize,
    ) -> ServerResult<NodeSnapshot> {
        let revision = entry.revision.max(self.revision);
        match entry.kind {
            TreeEntryKind::Directory | TreeEntryKind::Mount => {
                if !self.expanded.contains(&entry.path) {
                    return Ok(NodeSnapshot::collapsed_dir(
                        &entry.path,
                        &entry.name,
                        revision,
                    ));
                }
                if depth.saturating_add(1) >= limits.max_snapshot_depth {
                    return Ok(NodeSnapshot::expanded_dir(
                        &entry.path,
                        &entry.name,
                        revision,
                        ChildPage::Truncated {
                            nodes: Vec::new(),
                            continuation: self.continuation(&entry.path, 0),
                            remaining: None,
                        },
                    ));
                }
                let page = self.directory_page(
                    &entry.path,
                    depth.saturating_add(1),
                    limits,
                    remaining_budget,
                )?;
                let children = if page.1 {
                    ChildPage::Truncated {
                        nodes: page.0,
                        continuation: self.continuation(&entry.path, page.2),
                        remaining: page.3,
                    }
                } else {
                    ChildPage::Complete(page.0)
                };
                Ok(NodeSnapshot::expanded_dir(
                    &entry.path,
                    &entry.name,
                    revision,
                    children,
                ))
            }
            TreeEntryKind::Cell if !self.expanded.contains(&entry.path) => Ok(
                NodeSnapshot::collapsed_cell(&entry.path, &entry.name, revision),
            ),
            TreeEntryKind::Cell => {
                let cell = self.tree.inspect_cell(&entry.path).map_err(internal)?;
                let receipt = cell.receipt.as_ref();
                let detail = NodeDetail {
                    source: FaceSnapshot::from_encoded(&cell.source),
                    result: FaceSnapshot::from_encoded(&cell.result),
                    freshness: Freshness::from(cell.status),
                    source_revision: cell.source_revision,
                    result_revision: receipt.map(|receipt| receipt.source_revision),
                    timestamps: TimestampSummary {
                        source_changed_ms: self.source_wall.get(&entry.path).copied(),
                        result_checked_ms: self
                            .result_wall
                            .get(&entry.path)
                            .copied()
                            .or_else(|| receipt.and_then(|receipt| receipt.wall_finished_ms)),
                    },
                    policy_badges: cell.policy_badges,
                    receipt: receipt.map(ReceiptSummary::from),
                };
                Ok(NodeSnapshot::expanded_cell(
                    &entry.path,
                    &entry.name,
                    revision,
                    detail,
                ))
            }
        }
    }

    fn continuation(&mut self, path: &str, offset: usize) -> String {
        let token = format!(
            "page-{}-{:016x}-{:016x}",
            self.id.0, self.revision, self.next_continuation
        );
        self.next_continuation = self.next_continuation.saturating_add(1);
        self.continuations
            .insert(token.clone(), (path.to_owned(), offset, self.revision));
        token
    }

    pub(crate) fn apply_surface_local(
        &mut self,
        operation: &Expr,
        limits: ExpressionTreeServerLimits,
    ) -> ServerResult<bool> {
        let op = protocol::operation(operation).ok_or_else(|| {
            ExpressionTreeServerError::new("invalid-operation", "surface operation has no op")
        })?;
        let tree = protocol::required_expr(operation, "tree")?;
        if tree != Expr::Symbol(self.resource()) {
            return Err(ExpressionTreeServerError::new(
                "session-mismatch",
                "surface operation targets another session",
            ));
        }
        let revision = protocol::uint(operation, "revision")?;
        if revision != self.revision {
            return Err(ExpressionTreeServerError::new(
                "stale-revision",
                format!(
                    "rendered revision {revision} does not match current revision {}",
                    self.revision
                ),
            ));
        }
        let path = match access::field(operation, "path") {
            Some(Expr::String(path)) => path.clone(),
            _ => "/".to_owned(),
        };
        match op.name.as_ref() {
            "disclose" => {
                let open = match access::field(operation, "args") {
                    Some(Expr::List(args)) => matches!(args.first(), Some(Expr::Bool(true))),
                    _ => false,
                };
                if open {
                    self.expanded.insert(path);
                } else {
                    self.expanded
                        .retain(|expanded| expanded == "/" || !under(expanded, &path));
                }
                Ok(true)
            }
            "continue" => {
                let token = match access::field(operation, "continuation") {
                    Some(Expr::String(token)) => token,
                    _ => {
                        return Err(ExpressionTreeServerError::new(
                            "invalid-continuation",
                            "continuation operation has no token",
                        ));
                    }
                };
                let Some((token_path, offset, token_revision)) = self.continuations.remove(token)
                else {
                    return Err(ExpressionTreeServerError::new(
                        "invalid-continuation",
                        "continuation token is unknown or already consumed",
                    ));
                };
                if token_revision != self.revision || token_path != path {
                    return Err(ExpressionTreeServerError::new(
                        "stale-continuation",
                        "continuation token does not match the current snapshot",
                    ));
                }
                self.page_sizes
                    .insert(path, offset.saturating_add(limits.max_page_entries));
                Ok(true)
            }
            "open-policy" => Ok(false),
            other => Err(ExpressionTreeServerError::new(
                "invalid-operation",
                format!("unsupported local surface operation {other}"),
            )),
        }
    }

    pub(crate) fn changed(
        &mut self,
        kind: &str,
        path: Option<String>,
        tick: u64,
        wall_ms: Option<u64>,
        limits: ExpressionTreeServerLimits,
    ) {
        self.revision = self.revision.saturating_add(1);
        if let (Some(path), Some(wall_ms)) = (&path, wall_ms) {
            if is_source_change(kind) {
                self.source_wall.insert(path.clone(), wall_ms);
            }
            if is_result_change(kind) {
                self.result_wall.insert(path.clone(), wall_ms);
            }
        }
        let event = ChangeEvent {
            resource: self.resource(),
            revision: self.revision,
            logical_tick: tick,
            wall_ms,
            kind: kind.to_owned(),
            path,
        };
        push_bounded(
            &mut self.changes,
            &mut self.dropped_changes,
            event.clone(),
            limits.watch_capacity,
        );
        for watch in self.watches.values_mut() {
            watch.push(event.clone(), limits.watch_capacity);
        }
    }

    pub(crate) fn drain_changes(&mut self) -> Vec<ChangeEvent> {
        self.changes.drain(..).collect()
    }

    pub(crate) fn subscribe(
        &mut self,
        limits: ExpressionTreeServerLimits,
    ) -> ServerResult<WatchId> {
        if self.watches.len() >= limits.max_watches_per_session {
            return Err(ExpressionTreeServerError::new(
                "watch-limit",
                format!(
                    "session already has {} watches",
                    limits.max_watches_per_session
                ),
            ));
        }
        let id = WatchId(format!("watch-{}-{:016x}", self.id.0, self.next_watch));
        self.next_watch = self.next_watch.saturating_add(1);
        self.watches.insert(id.clone(), WatchState::new());
        Ok(id)
    }

    pub(crate) fn poll_watch(&mut self, watch: &WatchId, limit: usize) -> ServerResult<WatchBatch> {
        let state = self.watches.get_mut(watch).ok_or_else(|| {
            ExpressionTreeServerError::new("unknown-watch", "watch does not belong to session")
        })?;
        let count = limit.min(state.events.len());
        Ok(WatchBatch {
            events: state.events.drain(..count).collect(),
            dropped: state.dropped,
            cancelled: state.cancelled,
        })
    }

    pub(crate) fn cancel_watch(&mut self, watch: &WatchId) -> ServerResult<()> {
        let state = self.watches.get_mut(watch).ok_or_else(|| {
            ExpressionTreeServerError::new("unknown-watch", "watch does not belong to session")
        })?;
        state.cancelled = true;
        state.events.clear();
        Ok(())
    }
}

fn push_bounded(
    queue: &mut VecDeque<ChangeEvent>,
    dropped: &mut u64,
    event: ChangeEvent,
    capacity: usize,
) {
    if queue.len() == capacity {
        queue.pop_front();
        *dropped = dropped.saturating_add(1);
    }
    queue.push_back(event);
}

fn under(candidate: &str, parent: &str) -> bool {
    candidate == parent
        || candidate
            .strip_prefix(parent)
            .is_some_and(|suffix| parent == "/" || suffix.starts_with('/'))
}

fn is_source_change(kind: &str) -> bool {
    matches!(kind, "new-cell" | "set-expr" | "move" | "rename" | "delete")
}

fn is_result_change(kind: &str) -> bool {
    matches!(
        kind,
        "calculate" | "recalculate" | "recalculate-recursive" | "cancel"
    )
}
