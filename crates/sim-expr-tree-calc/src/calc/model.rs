use std::{
    fmt,
    hash::{Hash, Hasher},
};

use sim_incremental_core::{IncrementalError, QueryBudgets};
use sim_kernel::{CanonicalKey, Cx, Value};

/// Absolute safety ceilings for one expression-tree calculation.
///
/// Requested limits are always clamped to these values. This keeps a persisted
/// or caller-supplied policy from turning malformed source into unbounded host
/// recursion or output.
pub const HARD_MAX_WORK: usize = 1_000_000;
pub const HARD_MAX_OBSERVATIONS: usize = 100_000;
pub const HARD_MAX_QUERY_DEPTH: usize = 64;
pub const HARD_MAX_OUTPUT: usize = 1_048_576;
pub const HARD_MAX_EXPR_DEPTH: usize = 128;

pub(super) type ContextFactory = dyn Fn() -> Cx + Send + Sync + 'static;

/// A query key in the expression-tree incremental graph.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CalcQuery {
    /// The calculated result of one cell.
    Cell(String),
    /// A name lookup slot, including a previously missing name.
    NameSlot(String),
    /// One traversed segment of a path lookup.
    LookupStep(String),
    /// A directory listing inspected during lookup.
    Listing(String),
    /// The observed epoch of a mounted backend.
    MountEpoch(String),
    /// Effective inherited calculation policy.
    EffectivePolicy,
    /// Installed source/result codec registry.
    CodecRegistry,
    /// Open-time authority ceiling.
    AuthorityCeiling,
}

/// Caller-requested limits for one verification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CalcLimits {
    /// Query executions plus explicitly charged expression nodes.
    pub max_work: usize,
    /// Dynamic dependency observations.
    pub max_observations: usize,
    /// Nested cell-query depth.
    pub max_query_depth: usize,
    /// Aggregate canonical output units.
    pub max_output: usize,
}

impl CalcLimits {
    /// Builds an explicit requested policy. Every field is hard-clamped.
    #[must_use]
    pub const fn new(
        max_work: usize,
        max_observations: usize,
        max_query_depth: usize,
        max_output: usize,
    ) -> Self {
        Self {
            max_work,
            max_observations,
            max_query_depth,
            max_output,
        }
    }

    pub(super) fn clamped(self) -> QueryBudgets {
        QueryBudgets::new(
            self.max_work.min(HARD_MAX_WORK),
            self.max_observations.min(HARD_MAX_OBSERVATIONS),
            self.max_query_depth.min(HARD_MAX_QUERY_DEPTH),
            self.max_output.min(HARD_MAX_OUTPUT),
        )
    }
}

impl Default for CalcLimits {
    fn default() -> Self {
        Self::new(
            HARD_MAX_WORK,
            HARD_MAX_OBSERVATIONS,
            HARD_MAX_QUERY_DEPTH,
            HARD_MAX_OUTPUT,
        )
    }
}

/// A memoized, deterministic calculation failure.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum CellFailure {
    /// SIM evaluation rejected the ordinary source expression.
    Evaluation {
        /// Stable diagnostic text.
        message: String,
    },
    /// Dynamic dependency evaluation entered a cycle.
    Cycle {
        /// Deterministic path with the repeated query at the end.
        path: Vec<CalcQuery>,
    },
    /// Expression nesting exceeded the non-configurable host safety ceiling.
    ExpressionDepth {
        /// Hard ceiling.
        limit: usize,
    },
}

impl fmt::Display for CellFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Evaluation { message } => write!(f, "cell evaluation failed: {message}"),
            Self::Cycle { path } => write!(f, "cell dependency cycle {path:?}"),
            Self::ExpressionDepth { limit } => {
                write!(f, "cell expression depth exceeds hard limit {limit}")
            }
        }
    }
}

/// A current-result error. Last-good data is available separately.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CalcError {
    /// No result has committed since the source last changed.
    NotCalculated {
        /// Absolute cell path.
        path: String,
    },
    /// A deterministic cell failure was committed as the current memo.
    Cell(CellFailure),
    /// Verification stopped before a current memo could commit.
    Incremental(IncrementalError<CalcQuery>),
}

impl fmt::Display for CalcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotCalculated { path } => write!(f, "cell {path} has no current result"),
            Self::Cell(failure) => failure.fmt(f),
            Self::Incremental(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for CalcError {}

/// A retained successful value explicitly labelled as historical.
#[derive(Clone)]
pub struct LastGoodValue {
    pub(super) value: Value,
}

impl LastGoodValue {
    /// The stable label callers must present beside this historical value.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        "last-good"
    }

    /// Borrows the retained ordinary SIM value.
    #[must_use]
    pub fn value(&self) -> &Value {
        &self.value
    }
}

#[derive(Clone)]
pub(super) enum MemoOutcome {
    Value(Value),
    Failure(CellFailure),
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum MemoIdentity {
    Canonical(CanonicalKey),
    Volatile(u64),
    Failure(CellFailure),
}

#[derive(Clone)]
pub(super) struct MemoValue {
    pub(super) outcome: MemoOutcome,
    identity: MemoIdentity,
}

impl MemoValue {
    pub(super) fn canonical(value: Value, key: CanonicalKey) -> Self {
        Self {
            outcome: MemoOutcome::Value(value),
            identity: MemoIdentity::Canonical(key),
        }
    }

    pub(super) fn volatile(value: Value, nonce: u64) -> Self {
        Self {
            outcome: MemoOutcome::Value(value),
            identity: MemoIdentity::Volatile(nonce),
        }
    }

    pub(super) fn failure(failure: CellFailure) -> Self {
        Self {
            outcome: MemoOutcome::Failure(failure.clone()),
            identity: MemoIdentity::Failure(failure),
        }
    }

    pub(super) fn is_volatile(&self) -> bool {
        matches!(self.identity, MemoIdentity::Volatile(_))
    }
}

impl fmt::Debug for MemoValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemoValue")
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}

impl Hash for MemoValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.identity.hash(state);
    }
}
