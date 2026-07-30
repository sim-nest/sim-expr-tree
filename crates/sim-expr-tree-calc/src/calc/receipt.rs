use sim_incremental_core::ObservationKind;

use super::{AuthorityDigest, CalcError, CalcQuery, CalcTrigger, PolicyDigest};

/// Stable identity of one directed or automatic calculation request.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RequestId(u64);

impl RequestId {
    /// Creates a request id from persisted bits.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the persisted id bits.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Incremental reuse/forcing behavior of a directed request.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CalcRequestMode {
    /// Verify the roots and reuse every current memo.
    Verify,
    /// Force only the selected roots while reusing current dependencies.
    ForceRoots,
    /// Force roots and every reachable calculated dependency.
    ForceRecursive,
}

/// Why a calculation attempt ran.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CalcReason {
    /// A caller explicitly requested verification.
    DirectedVerify,
    /// A caller explicitly forced selected roots.
    DirectedForceRoots,
    /// A caller explicitly forced roots and their calculated dependency closure.
    DirectedForceRecursive,
    /// A mutation enqueued automatic work.
    AutomaticMutation,
    /// A budget-stopped request resumed through its continuation.
    Continuation,
}

impl CalcReason {
    pub(super) fn for_mode(mode: CalcRequestMode) -> Self {
        match mode {
            CalcRequestMode::Verify => Self::DirectedVerify,
            CalcRequestMode::ForceRoots => Self::DirectedForceRoots,
            CalcRequestMode::ForceRecursive => Self::DirectedForceRecursive,
        }
    }
}

/// Terminal outcome recorded for one cell attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CalcOutcome {
    /// A current value committed.
    Succeeded,
    /// A deterministic calculation failure committed.
    Failed {
        /// Stable failure text.
        message: String,
    },
    /// Policy prevented the requested cell from running.
    Blocked {
        /// Stable blocking explanation.
        message: String,
    },
    /// The request was cancelled without corrupting an earlier memo.
    Cancelled,
    /// A bounded request stopped and retained an explicit continuation.
    BudgetExhausted {
        /// Stable exhausted-budget explanation.
        message: String,
        /// Incremental continuation token bits, when supplied.
        continuation: Option<u64>,
    },
}

/// One bounded dependency observation in a calculation receipt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DependencyStamp {
    /// Observed query key.
    pub query: CalcQuery,
    /// Observation class.
    pub kind: ObservationKind,
    /// Observed revision.
    pub revision: u64,
    /// Observed value fingerprint for query reads.
    pub fingerprint: Option<u64>,
}

/// One existing kernel effect-ledger record summarized into a receipt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectStamp {
    /// Effect kind symbol.
    pub kind: String,
    /// Whether resolution aborted.
    pub aborted: bool,
}

/// Bounded immutable evidence for one calculation attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CalcReceipt {
    /// Owning request.
    pub request_id: RequestId,
    /// Canonical absolute cell path.
    pub cell: String,
    /// Source observation revision used by the attempt.
    pub source_revision: u64,
    /// Effective inherited calculation-policy digest.
    pub policy_digest: PolicyDigest,
    /// Effective diminished authority digest.
    pub authority_digest: AuthorityDigest,
    /// Direct dependency observations retained under the receipt bound.
    pub dependencies: Vec<DependencyStamp>,
    /// Number of direct dependencies omitted from `dependencies`.
    pub omitted_dependencies: usize,
    /// Digest covering the complete direct dependency list.
    pub dependency_digest: u64,
    /// Existing effect-ledger evidence retained under the receipt bound.
    pub effects: Vec<EffectStamp>,
    /// Number of effect records omitted from `effects`.
    pub omitted_effects: usize,
    /// Monotone logical tick at attempt start.
    pub started_tick: u64,
    /// Monotone logical tick at attempt finish.
    pub finished_tick: u64,
    /// Optional human wall-clock observation at attempt start.
    pub wall_started_ms: Option<u64>,
    /// Optional human wall-clock observation at attempt finish.
    pub wall_finished_ms: Option<u64>,
    /// Terminal attempt outcome.
    pub outcome: CalcOutcome,
    /// Current result fingerprint, when a memo committed.
    pub result_fingerprint: Option<u64>,
    /// Request reason.
    pub reason: CalcReason,
    /// Effective trigger in force for the attempted cell.
    pub trigger: CalcTrigger,
}

/// Non-evaluating status exposed by the explanation model.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CalcStatus {
    /// No attempt has committed or failed.
    NeverCalculated,
    /// The committed memo is current.
    Fresh,
    /// An input changed and verification has not yet established cutoff.
    MaybeStale,
    /// Automatic work is queued.
    Pending,
    /// The last attempt failed.
    Failed,
    /// The effective policy is frozen.
    Frozen,
    /// Policy or missing authority blocked the last attempt.
    Blocked,
}

/// Inspectable, non-evaluating explanation of one cell's current state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CalcExplanation {
    /// Canonical cell path.
    pub cell: String,
    /// Current non-evaluating status.
    pub status: CalcStatus,
    /// Current source revision.
    pub source_revision: u64,
    /// Current effective policy digest.
    pub policy_digest: PolicyDigest,
    /// Current effective authority digest.
    pub authority_digest: AuthorityDigest,
    /// Latest bounded receipt, when any.
    pub receipt: Option<CalcReceipt>,
    /// Stable human-readable reasons for the status.
    pub reasons: Vec<String>,
}

/// Result of one directed root in a stable multi-root request.
#[derive(Clone)]
pub struct DirectedCellResult {
    /// Canonical root path.
    pub cell: String,
    /// Current ordinary value or typed calculation error.
    pub result: Result<sim_kernel::Value, CalcError>,
}

/// Aggregate result of a directed calculation request.
#[derive(Clone)]
pub struct DirectedCalcReport {
    /// Stable request identity.
    pub request_id: RequestId,
    /// Root outcomes in canonical path order.
    pub cells: Vec<DirectedCellResult>,
}
