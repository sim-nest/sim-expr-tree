use sim_expr_tree_core::{CodecPolicyPatch, EffectiveCodecPolicy};
use sim_kernel::{CapabilityName, CapabilitySet};
use sim_table_core::TablePath;

use super::CalcLimits;

/// Mutation behavior for a calculated cell.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum CalcTrigger {
    /// Mutations enqueue bounded calculation work.
    #[default]
    Automatic,
    /// Work begins only when a result is requested.
    OnDemand,
    /// Work begins only when this cell is an explicitly directed root.
    Manual,
    /// The committed memo is retained and no new work is permitted.
    Frozen,
}

/// How a multi-root request responds to one failed root.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum ErrorMode {
    /// Continue to the remaining roots and report every outcome.
    #[default]
    Continue,
    /// Stop after the first failed or blocked root.
    FailFast,
}

/// How a dynamic dependency cycle is represented.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum CycleMode {
    /// Commit the deterministic cycle as a calculation failure.
    #[default]
    Fail,
    /// Commit the deterministic cycle as a blocked calculation.
    Block,
}

/// Field-by-field calculation policy override at one tree, directory, or cell.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CalcPolicyPatch {
    /// Optional trigger override.
    pub trigger: Option<CalcTrigger>,
    /// Optional multi-root error behavior override.
    pub error_mode: Option<ErrorMode>,
    /// Optional cycle behavior override.
    pub cycle_mode: Option<CycleMode>,
    /// Optional incremental-query budget override.
    pub budget: Option<CalcLimits>,
    /// Optional scheduler priority override.
    pub priority: Option<i16>,
    /// Optional automatic-work debounce override.
    pub debounce_ms: Option<u32>,
}

impl CalcPolicyPatch {
    /// Applies this patch to an already-effective parent policy.
    pub fn apply_to(&self, effective: &mut EffectiveCalcPolicy) {
        if let Some(trigger) = self.trigger {
            effective.trigger = trigger;
        }
        if let Some(error_mode) = self.error_mode {
            effective.error_mode = error_mode;
        }
        if let Some(cycle_mode) = self.cycle_mode {
            effective.cycle_mode = cycle_mode;
        }
        if let Some(budget) = self.budget {
            effective.budget = budget;
        }
        if let Some(priority) = self.priority {
            effective.priority = priority;
        }
        if let Some(debounce_ms) = self.debounce_ms {
            effective.debounce_ms = debounce_ms;
        }
    }
}

/// Fully inherited calculation policy for one cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EffectiveCalcPolicy {
    /// Effective trigger mode.
    pub trigger: CalcTrigger,
    /// Effective multi-root error behavior.
    pub error_mode: ErrorMode,
    /// Effective cycle behavior.
    pub cycle_mode: CycleMode,
    /// Effective request budget before hard host clamping.
    pub budget: CalcLimits,
    /// Effective scheduler priority. Larger values run first.
    pub priority: i16,
    /// Effective automatic-work debounce in milliseconds.
    pub debounce_ms: u32,
}

impl Default for EffectiveCalcPolicy {
    fn default() -> Self {
        Self {
            trigger: CalcTrigger::Automatic,
            error_mode: ErrorMode::Continue,
            cycle_mode: CycleMode::Fail,
            budget: CalcLimits::default(),
            priority: 0,
            debounce_ms: 0,
        }
    }
}

impl EffectiveCalcPolicy {
    /// Returns a deterministic digest of every effective field.
    #[must_use]
    pub fn digest(self) -> PolicyDigest {
        let mut digest = StableDigest::new();
        digest.write(match self.trigger {
            CalcTrigger::Automatic => b"automatic",
            CalcTrigger::OnDemand => b"on-demand",
            CalcTrigger::Manual => b"manual",
            CalcTrigger::Frozen => b"frozen",
        });
        digest.write(match self.error_mode {
            ErrorMode::Continue => b"continue",
            ErrorMode::FailFast => b"fail-fast",
        });
        digest.write(match self.cycle_mode {
            CycleMode::Fail => b"cycle-fail",
            CycleMode::Block => b"cycle-block",
        });
        digest.write(&self.budget.max_work.to_le_bytes());
        digest.write(&self.budget.max_observations.to_le_bytes());
        digest.write(&self.budget.max_query_depth.to_le_bytes());
        digest.write(&self.budget.max_output.to_le_bytes());
        digest.write(&self.priority.to_le_bytes());
        digest.write(&self.debounce_ms.to_le_bytes());
        PolicyDigest(digest.finish())
    }
}

/// Capability allow/deny/require override at one policy level.
///
/// `allow = None` inherits the current authority. `allow = Some(empty)` denies
/// every capability. Denials accumulate and can never be re-granted by a
/// descendant patch.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AuthorityPolicyPatch {
    /// Optional allow-list intersection at this level.
    pub allow: Option<CapabilitySet>,
    /// Capabilities removed at this level and below.
    pub deny: CapabilitySet,
    /// Capabilities that must remain after diminution before evaluation.
    pub required: CapabilitySet,
}

/// Fully diminished authority and requirements for one cell.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectiveAuthority {
    capabilities: CapabilitySet,
    required: CapabilitySet,
    denied: CapabilitySet,
    digest: AuthorityDigest,
}

impl EffectiveAuthority {
    /// Derives cell authority from the immutable open-time ceiling.
    #[must_use]
    pub fn derive(
        ceiling: &CapabilitySet,
        patches: impl IntoIterator<Item = AuthorityPolicyPatch>,
    ) -> Self {
        let mut capabilities = ceiling.clone();
        let mut required = CapabilitySet::new();
        let mut denied = CapabilitySet::new();
        for patch in patches {
            if let Some(allow) = patch.allow {
                capabilities = capabilities.intersect(&allow);
            }
            for capability in patch.deny.iter().cloned() {
                denied.insert(capability);
            }
            for capability in patch.required.iter().cloned() {
                required.insert(capability);
            }
            capabilities = without_denied(&capabilities, &denied);
        }
        let digest = authority_digest(&capabilities);
        Self {
            capabilities,
            required,
            denied,
            digest,
        }
    }

    /// Returns the active diminished capability set.
    #[must_use]
    pub fn capabilities(&self) -> &CapabilitySet {
        &self.capabilities
    }

    /// Returns the accumulated required capability set.
    #[must_use]
    pub fn required(&self) -> &CapabilitySet {
        &self.required
    }

    /// Returns the accumulated denial set.
    #[must_use]
    pub fn denied(&self) -> &CapabilitySet {
        &self.denied
    }

    /// Returns the deterministic digest of the active authority.
    #[must_use]
    pub const fn digest(&self) -> AuthorityDigest {
        self.digest
    }

    /// Returns the first missing requirement in stable name order.
    #[must_use]
    pub fn first_missing_requirement(&self) -> Option<CapabilityName> {
        self.required
            .iter()
            .find(|capability| !self.capabilities.contains(capability))
            .cloned()
    }
}

/// Compact deterministic calculation-policy digest.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PolicyDigest(u64);

impl PolicyDigest {
    pub(super) const fn from_persisted(value: u64) -> Self {
        Self(value)
    }

    /// Returns the digest bits.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Compact deterministic effective-authority digest.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AuthorityDigest(u64);

impl AuthorityDigest {
    pub(super) const fn from_persisted(value: u64) -> Self {
        Self(value)
    }

    /// Returns the digest bits.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

pub(super) fn effective_calc_policy(
    tree: &CalcPolicyPatch,
    directories: &std::collections::BTreeMap<String, CalcPolicyPatch>,
    cells: &std::collections::BTreeMap<String, CalcPolicyPatch>,
    cell: &str,
) -> EffectiveCalcPolicy {
    let mut effective = EffectiveCalcPolicy::default();
    tree.apply_to(&mut effective);
    for ancestor in ancestor_directories(cell) {
        if let Some(patch) = directories.get(&ancestor) {
            patch.apply_to(&mut effective);
        }
    }
    if let Some(patch) = cells.get(cell) {
        patch.apply_to(&mut effective);
    }
    effective
}

pub(super) fn effective_authority(
    ceiling: &CapabilitySet,
    tree: &AuthorityPolicyPatch,
    directories: &std::collections::BTreeMap<String, AuthorityPolicyPatch>,
    cells: &std::collections::BTreeMap<String, AuthorityPolicyPatch>,
    cell: &str,
) -> EffectiveAuthority {
    let mut patches = vec![tree.clone()];
    patches.extend(
        ancestor_directories(cell)
            .into_iter()
            .filter_map(|ancestor| directories.get(&ancestor).cloned()),
    );
    if let Some(patch) = cells.get(cell) {
        patches.push(patch.clone());
    }
    EffectiveAuthority::derive(ceiling, patches)
}

pub(super) fn effective_codec_policy(
    tree: &CodecPolicyPatch,
    directories: &std::collections::BTreeMap<String, CodecPolicyPatch>,
    cells: &std::collections::BTreeMap<String, CodecPolicyPatch>,
    cell: &str,
) -> EffectiveCodecPolicy {
    let mut patches = vec![tree.clone()];
    patches.extend(
        ancestor_directories(cell)
            .into_iter()
            .filter_map(|ancestor| directories.get(&ancestor).cloned()),
    );
    if let Some(patch) = cells.get(cell) {
        patches.push(patch.clone());
    }
    EffectiveCodecPolicy::derive(patches)
}

pub(super) fn is_descendant_or_same(directory: &TablePath, cell: &TablePath) -> bool {
    let directory_segments = directory.segments();
    let cell_segments = cell.segments();
    directory_segments.len() <= cell_segments.len()
        && directory_segments
            .iter()
            .zip(cell_segments)
            .all(|(left, right)| left == right)
}

fn ancestor_directories(cell: &str) -> Vec<String> {
    let mut ancestors = vec!["/".to_owned()];
    let mut segments = cell
        .trim_start_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    segments.pop();
    let mut current = String::new();
    for segment in segments {
        current.push('/');
        current.push_str(segment);
        ancestors.push(current.clone());
    }
    ancestors
}

fn without_denied(capabilities: &CapabilitySet, denied: &CapabilitySet) -> CapabilitySet {
    capabilities
        .iter()
        .filter(|capability| !denied.contains(capability))
        .cloned()
        .fold(CapabilitySet::new(), CapabilitySet::grant)
}

fn authority_digest(capabilities: &CapabilitySet) -> AuthorityDigest {
    let mut digest = StableDigest::new();
    for capability in capabilities.iter() {
        digest.write(capability.as_str().as_bytes());
        digest.write(&[0]);
    }
    AuthorityDigest(digest.finish())
}

struct StableDigest(u64);

impl StableDigest {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    fn new() -> Self {
        Self(Self::OFFSET)
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(Self::PRIME);
        }
    }

    fn finish(self) -> u64 {
        self.0
    }
}
