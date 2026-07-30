//! Ordinary expression value consumed by the expression-tree surface.

use sim_expr_tree_calc::{
    CalcOutcome, CalcReceipt, CalcStatus, EncodedFace, FaceContent, FaceDimension, FaceIssue,
};
use sim_kernel::{Expr, Symbol};
use sim_value::build;

/// The snapshot's open data tag.
pub const SNAPSHOT_TYPE: &str = "expression-tree-snapshot";

/// A source or result face outcome safe to expose to a surface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FaceState {
    /// The bounded face is complete.
    Complete,
    /// A bound stopped projection.
    Truncated {
        /// Stable bound name.
        dimension: String,
        /// Configured maximum.
        limit: usize,
        /// Observed size.
        observed: usize,
    },
    /// The value has no safe presentation projection.
    Unsupported {
        /// Bounded explanation.
        reason: String,
    },
    /// The selected codec failed closed.
    CodecFailure {
        /// Bounded codec diagnostic.
        message: String,
    },
}

/// One already bounded source or result face.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FaceSnapshot {
    content: Option<Expr>,
    codec: Option<String>,
    state: FaceState,
}

impl FaceSnapshot {
    /// Creates a complete text face.
    pub fn text(content: impl Into<String>, codec: impl Into<String>) -> Self {
        Self {
            content: Some(Expr::String(content.into())),
            codec: Some(codec.into()),
            state: FaceState::Complete,
        }
    }

    /// Creates a complete opaque byte face. Rendering exposes only its size.
    pub fn bytes(content: Vec<u8>, codec: impl Into<String>) -> Self {
        Self {
            content: Some(Expr::Bytes(content)),
            codec: Some(codec.into()),
            state: FaceState::Complete,
        }
    }

    /// Creates an explicitly truncated face.
    pub fn truncated(dimension: impl Into<String>, limit: usize, observed: usize) -> Self {
        Self {
            content: None,
            codec: None,
            state: FaceState::Truncated {
                dimension: dimension.into(),
                limit,
                observed,
            },
        }
    }

    /// Creates a face for a value with no safe projection.
    pub fn unsupported(reason: impl Into<String>) -> Self {
        Self {
            content: None,
            codec: None,
            state: FaceState::Unsupported {
                reason: reason.into(),
            },
        }
    }

    /// Creates a face whose selected codec failed closed.
    pub fn codec_failure(message: impl Into<String>) -> Self {
        Self {
            content: None,
            codec: None,
            state: FaceState::CodecFailure {
                message: message.into(),
            },
        }
    }

    /// Copies the already bounded result of the expression-tree codec policy.
    pub fn from_encoded(face: &EncodedFace) -> Self {
        let content = face.content().map(|content| match content {
            FaceContent::Text(text) => Expr::String(text.clone()),
            FaceContent::Bytes(bytes) => Expr::Bytes(bytes.clone()),
        });
        let metadata = face.metadata();
        let state = match metadata.issue() {
            FaceIssue::Complete => FaceState::Complete,
            FaceIssue::Truncated {
                dimension,
                limit,
                observed,
            } => FaceState::Truncated {
                dimension: face_dimension(*dimension).to_owned(),
                limit: *limit,
                observed: *observed,
            },
            FaceIssue::Unsupported { reason } => FaceState::Unsupported {
                reason: reason.clone(),
            },
            FaceIssue::CodecFailure { message } => FaceState::CodecFailure {
                message: message.clone(),
            },
        };
        Self {
            content,
            codec: metadata.codec().map(str::to_owned),
            state,
        }
    }

    pub(crate) fn to_expr(&self) -> Expr {
        let (state, detail) = match &self.state {
            FaceState::Complete => ("complete", Vec::new()),
            FaceState::Truncated {
                dimension,
                limit,
                observed,
            } => (
                "truncated",
                vec![
                    ("dimension", build::sym(dimension)),
                    ("limit", build::uint(*limit as u64)),
                    ("observed", build::uint(*observed as u64)),
                ],
            ),
            FaceState::Unsupported { reason } => {
                ("unsupported", vec![("reason", build::text(reason))])
            }
            FaceState::CodecFailure { message } => {
                ("codec-failure", vec![("message", build::text(message))])
            }
        };
        let mut fields = vec![
            ("state", build::sym(state)),
            ("content", self.content.clone().unwrap_or(Expr::Nil)),
            (
                "codec",
                self.codec.as_ref().map(build::text).unwrap_or(Expr::Nil),
            ),
        ];
        fields.extend(detail);
        build::map(fields)
    }
}

fn face_dimension(dimension: FaceDimension) -> &'static str {
    match dimension {
        FaceDimension::Bytes => "bytes",
        FaceDimension::Depth => "depth",
        FaceDimension::Items => "items",
    }
}

/// Every non-evaluating expression-tree freshness state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Freshness {
    /// No calculation has committed.
    NeverCalculated,
    /// The current result is verified.
    Fresh,
    /// An observation changed and verification is pending.
    MaybeStale,
    /// Automatic calculation is queued.
    Pending,
    /// The latest calculation failed.
    Failed,
    /// Effective policy freezes calculation.
    Frozen,
    /// Policy or authority blocked calculation.
    Blocked,
}

impl Freshness {
    pub(crate) const fn token(self) -> &'static str {
        match self {
            Self::NeverCalculated => "never-calculated",
            Self::Fresh => "fresh",
            Self::MaybeStale => "maybe-stale",
            Self::Pending => "pending",
            Self::Failed => "failed",
            Self::Frozen => "frozen",
            Self::Blocked => "blocked",
        }
    }
}

impl From<CalcStatus> for Freshness {
    fn from(status: CalcStatus) -> Self {
        match status {
            CalcStatus::NeverCalculated => Self::NeverCalculated,
            CalcStatus::Fresh => Self::Fresh,
            CalcStatus::MaybeStale => Self::MaybeStale,
            CalcStatus::Pending => Self::Pending,
            CalcStatus::Failed => Self::Failed,
            CalcStatus::Frozen => Self::Frozen,
            CalcStatus::Blocked => Self::Blocked,
        }
    }
}

/// Optional human timestamps used only for explanation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TimestampSummary {
    /// When source was observed changing.
    pub source_changed_ms: Option<u64>,
    /// When the current result was checked.
    pub result_checked_ms: Option<u64>,
}

/// Bounded calculation-receipt facts shown in one outline row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReceiptSummary {
    /// Stable request id.
    pub request_id: u64,
    /// Outcome token.
    pub outcome: String,
    /// Retained dependency count.
    pub dependencies: usize,
    /// Omitted dependency count.
    pub omitted_dependencies: usize,
    /// Logical start tick.
    pub started_tick: u64,
    /// Logical finish tick.
    pub finished_tick: u64,
}

impl From<&CalcReceipt> for ReceiptSummary {
    fn from(receipt: &CalcReceipt) -> Self {
        Self {
            request_id: receipt.request_id.get(),
            outcome: outcome_token(&receipt.outcome).to_owned(),
            dependencies: receipt.dependencies.len(),
            omitted_dependencies: receipt.omitted_dependencies,
            started_tick: receipt.started_tick,
            finished_tick: receipt.finished_tick,
        }
    }
}

fn outcome_token(outcome: &CalcOutcome) -> &'static str {
    match outcome {
        CalcOutcome::Succeeded => "succeeded",
        CalcOutcome::Failed { .. } => "failed",
        CalcOutcome::Blocked { .. } => "blocked",
        CalcOutcome::Cancelled => "cancelled",
        CalcOutcome::BudgetExhausted { .. } => "budget-exhausted",
    }
}

/// Fetched child state for an expanded directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChildPage {
    /// A collapsed directory fetched no descendants.
    NotFetched,
    /// Every child in this page is present.
    Complete(Vec<NodeSnapshot>),
    /// The bounded page ends in a server-issued continuation.
    Truncated {
        /// Children admitted to this page.
        nodes: Vec<NodeSnapshot>,
        /// Opaque continuation token.
        continuation: String,
        /// Known omitted child count, if supplied by the source.
        remaining: Option<usize>,
    },
}

/// Expanded details for one cell row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeDetail {
    /// Bounded source face.
    pub source: FaceSnapshot,
    /// Bounded result face.
    pub result: FaceSnapshot,
    /// Current freshness.
    pub freshness: Freshness,
    /// Source revision.
    pub source_revision: u64,
    /// Result revision, if any.
    pub result_revision: Option<u64>,
    /// Optional explanatory wall observations.
    pub timestamps: TimestampSummary,
    /// Effective policy badges in stable order.
    pub policy_badges: Vec<String>,
    /// Latest bounded receipt summary.
    pub receipt: Option<ReceiptSummary>,
}

/// One finite directory or cell snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeSnapshot {
    path: String,
    name: String,
    revision: u64,
    body: NodeBody,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum NodeBody {
    Directory(ChildPage),
    Cell(Option<Box<NodeDetail>>),
}

impl NodeSnapshot {
    /// Creates a collapsed directory. No descendant payload is accepted.
    pub fn collapsed_dir(path: impl Into<String>, name: impl Into<String>, revision: u64) -> Self {
        Self {
            path: path.into(),
            name: name.into(),
            revision,
            body: NodeBody::Directory(ChildPage::NotFetched),
        }
    }

    /// Creates an expanded directory with a complete or truncated child page.
    ///
    /// `ChildPage::NotFetched` remains valid and renders an empty expanded
    /// directory, which is useful while a requested page is in flight.
    pub fn expanded_dir(
        path: impl Into<String>,
        name: impl Into<String>,
        revision: u64,
        children: ChildPage,
    ) -> Self {
        Self {
            path: path.into(),
            name: name.into(),
            revision,
            body: NodeBody::Directory(children),
        }
    }

    /// Creates a collapsed cell. Source, result, and receipt faces are absent.
    pub fn collapsed_cell(path: impl Into<String>, name: impl Into<String>, revision: u64) -> Self {
        Self {
            path: path.into(),
            name: name.into(),
            revision,
            body: NodeBody::Cell(None),
        }
    }

    /// Creates an expanded cell with bounded details.
    pub fn expanded_cell(
        path: impl Into<String>,
        name: impl Into<String>,
        revision: u64,
        detail: NodeDetail,
    ) -> Self {
        Self {
            path: path.into(),
            name: name.into(),
            revision,
            body: NodeBody::Cell(Some(Box::new(detail))),
        }
    }

    pub(crate) fn to_expr(&self) -> Expr {
        let (node_type, open, body) = match &self.body {
            NodeBody::Directory(children) => (
                "directory",
                !matches!(children, ChildPage::NotFetched),
                children_expr(children),
            ),
            NodeBody::Cell(detail) => (
                "cell",
                detail.is_some(),
                detail
                    .as_ref()
                    .map(|detail| detail_expr(detail))
                    .unwrap_or(Expr::Nil),
            ),
        };
        build::map(vec![
            ("node-type", build::sym(node_type)),
            ("path", build::text(&self.path)),
            ("name", build::text(&self.name)),
            ("revision", build::uint(self.revision)),
            ("open", Expr::Bool(open)),
            ("body", body),
        ])
    }
}

fn children_expr(children: &ChildPage) -> Expr {
    match children {
        ChildPage::NotFetched => Expr::Nil,
        ChildPage::Complete(nodes) => build::map(vec![
            ("page-state", build::sym("complete")),
            (
                "nodes",
                build::list(nodes.iter().map(NodeSnapshot::to_expr).collect()),
            ),
        ]),
        ChildPage::Truncated {
            nodes,
            continuation,
            remaining,
        } => build::map(vec![
            ("page-state", build::sym("truncated")),
            (
                "nodes",
                build::list(nodes.iter().map(NodeSnapshot::to_expr).collect()),
            ),
            ("continuation", build::text(continuation)),
            (
                "remaining",
                remaining
                    .map(|value| build::uint(value as u64))
                    .unwrap_or(Expr::Nil),
            ),
        ]),
    }
}

fn detail_expr(detail: &NodeDetail) -> Expr {
    build::map(vec![
        ("source", detail.source.to_expr()),
        ("result", detail.result.to_expr()),
        ("freshness", build::sym(detail.freshness.token())),
        ("source-revision", build::uint(detail.source_revision)),
        (
            "result-revision",
            detail.result_revision.map(build::uint).unwrap_or(Expr::Nil),
        ),
        (
            "source-changed-ms",
            detail
                .timestamps
                .source_changed_ms
                .map(build::uint)
                .unwrap_or(Expr::Nil),
        ),
        (
            "result-checked-ms",
            detail
                .timestamps
                .result_checked_ms
                .map(build::uint)
                .unwrap_or(Expr::Nil),
        ),
        (
            "policy-badges",
            build::list(detail.policy_badges.iter().map(build::text).collect()),
        ),
        (
            "receipt",
            detail
                .receipt
                .as_ref()
                .map(receipt_expr)
                .unwrap_or(Expr::Nil),
        ),
    ])
}

fn receipt_expr(receipt: &ReceiptSummary) -> Expr {
    build::map(vec![
        ("request-id", build::uint(receipt.request_id)),
        ("outcome", build::sym(&receipt.outcome)),
        ("dependencies", build::uint(receipt.dependencies as u64)),
        (
            "omitted-dependencies",
            build::uint(receipt.omitted_dependencies as u64),
        ),
        ("started-tick", build::uint(receipt.started_tick)),
        ("finished-tick", build::uint(receipt.finished_tick)),
    ])
}

/// One revisioned, finite surface snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpressionTreeSnapshot {
    tree: Expr,
    revision: u64,
    nodes: Vec<NodeSnapshot>,
}

impl ExpressionTreeSnapshot {
    /// Creates one snapshot over an authoritative tree target.
    pub fn new(tree: Expr, revision: u64, nodes: Vec<NodeSnapshot>) -> Self {
        Self {
            tree,
            revision,
            nodes,
        }
    }

    /// Encodes the snapshot as an ordinary open SIM map.
    pub fn to_expr(&self) -> Expr {
        build::map(vec![
            (
                "type",
                Expr::Symbol(Symbol::qualified("expr-tree-view", SNAPSHOT_TYPE)),
            ),
            ("tree", self.tree.clone()),
            ("revision", build::uint(self.revision)),
            (
                "nodes",
                build::list(self.nodes.iter().map(NodeSnapshot::to_expr).collect()),
            ),
        ])
    }
}
