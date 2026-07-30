use sim_incremental_core::ContinuationToken;

use super::{CalcLimits, RequestId};

/// Bound applied to one automatic scheduler turn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AutomaticBudget {
    /// Maximum queued roots attempted in this turn.
    pub max_requests: usize,
    /// Incremental budget applied to each attempted root.
    pub limits: CalcLimits,
}

impl AutomaticBudget {
    /// Builds an explicit automatic-work bound.
    #[must_use]
    pub const fn new(max_requests: usize, limits: CalcLimits) -> Self {
        Self {
            max_requests,
            limits,
        }
    }
}

impl Default for AutomaticBudget {
    fn default() -> Self {
        Self::new(16, CalcLimits::default())
    }
}

/// Opaque explicit continuation for remaining automatic queue work.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct AutomaticContinuation {
    generation: u64,
}

impl AutomaticContinuation {
    pub(super) const fn new(generation: u64) -> Self {
        Self { generation }
    }

    /// Returns the queue generation represented by this token.
    #[must_use]
    pub const fn generation(self) -> u64 {
        self.generation
    }
}

/// One deterministic automatic queue entry suitable for persistence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueuedCalculation {
    /// Stable automatic request identity.
    pub request_id: RequestId,
    /// Canonical cell path.
    pub cell: String,
    /// Earliest wall-clock millisecond at which the entry is ready.
    pub ready_at_ms: u64,
    /// Effective policy priority.
    pub priority: i16,
    /// Stable insertion order.
    pub sequence: u64,
    /// Number of ready selections that bypassed this entry.
    pub bypasses: u8,
    /// Incremental continuation token retained after budget exhaustion.
    pub incremental_continuation: Option<ContinuationToken>,
}

/// Restartable snapshot of the deterministic automatic queue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutomaticQueueSnapshot {
    /// Queue generation.
    pub generation: u64,
    /// Next stable insertion sequence.
    pub next_sequence: u64,
    /// Queue entries in canonical cell order.
    pub entries: Vec<QueuedCalculation>,
}

/// Evidence returned by one bounded automatic scheduler turn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutomaticRun {
    /// Requests that reached a terminal success/failure/block outcome.
    pub completed: Vec<RequestId>,
    /// Requests that stopped on an incremental budget and remain queued.
    pub budget_exhausted: Vec<RequestId>,
    /// Explicit continuation when any queue work remains.
    pub continuation: Option<AutomaticContinuation>,
}

pub(super) const MAX_READY_BYPASSES: u8 = 3;
