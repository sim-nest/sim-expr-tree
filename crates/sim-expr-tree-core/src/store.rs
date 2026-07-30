use std::collections::BTreeMap;

use sim_table_core::TablePath;

use crate::{CellId, DirId, EffectiveCodecPolicy};

/// Backend family named by a mounted expression-tree store descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BackendKind {
    /// Process-local memory backend.
    Memory,
    /// Filesystem-backed Table/Dir backend.
    Filesystem,
    /// Database-backed Table/Dir backend.
    Database,
    /// Read-only backend wrapper.
    ReadOnly,
    /// Already-composed mounted namespace backend.
    MountedNamespace,
}

/// Monotonic observation of a mounted backend generation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MountEpoch(u64);

impl MountEpoch {
    /// Create an epoch from a backend supplied generation.
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    /// Borrow the raw generation value.
    pub fn value(self) -> u64 {
        self.0
    }

    /// Return the next observed generation.
    pub fn next_after(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

/// Mounted target shape. Table mounts are leaves; Dir mounts can be traversed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MountResource {
    /// A mounted Table leaf.
    Table,
    /// A mounted Dir subtree.
    Dir,
}

/// Explicit mount descriptor stored outside authored source cells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountDescriptor {
    path: TablePath,
    resource: MountResource,
    backend: BackendKind,
    epoch: MountEpoch,
}

impl MountDescriptor {
    /// Create an explicit Table mount descriptor.
    pub fn table(path: TablePath, backend: BackendKind, epoch: MountEpoch) -> Self {
        Self {
            path,
            resource: MountResource::Table,
            backend,
            epoch,
        }
    }

    /// Create an explicit Dir mount descriptor.
    pub fn dir(path: TablePath, backend: BackendKind, epoch: MountEpoch) -> Self {
        Self {
            path,
            resource: MountResource::Dir,
            backend,
            epoch,
        }
    }

    /// Absolute mount path.
    pub fn path(&self) -> &TablePath {
        &self.path
    }

    /// Mounted target shape.
    pub fn resource(&self) -> MountResource {
        self.resource
    }

    /// Mounted backend family.
    pub fn backend(&self) -> BackendKind {
        self.backend
    }

    /// Last observed backend epoch.
    pub fn epoch(&self) -> MountEpoch {
        self.epoch
    }

    fn set_epoch(&mut self, epoch: MountEpoch) {
        self.epoch = epoch;
    }
}

/// Authored source-store entry. Operational and derived values have no variants here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceEntry {
    expr: String,
    codec: Option<String>,
}

impl SourceEntry {
    /// Create an authored expression entry.
    pub fn new(expr: impl Into<String>) -> Self {
        Self {
            expr: expr.into(),
            codec: None,
        }
    }

    /// Attach the source codec used to parse the authored expression.
    pub fn with_codec(mut self, codec: impl Into<String>) -> Self {
        self.codec = Some(codec.into());
        self
    }

    /// Authored expression text.
    pub fn expr(&self) -> &str {
        &self.expr
    }

    /// Optional source codec.
    pub fn codec(&self) -> Option<&str> {
        self.codec.as_deref()
    }
}

/// Control-store entry for operational state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlEntry {
    /// Durable generated-name or scheduler counter.
    Counter(u64),
    /// Effective policy snapshot.
    Policy(EffectiveCodecPolicy),
    /// UI preference kept out of authored source.
    UiPreference(String),
    /// Last observed mount backend epoch.
    MountEpoch(MountEpoch),
}

/// Derived-store entry for rebuildable calculation artifacts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DerivedEntry {
    /// Dependency graph materialization.
    Graph(String),
    /// Cached value for a calculated cell.
    CachedValue(String),
    /// Calculation receipt or scheduler evidence.
    Receipt(String),
}

/// Recoverable source/control commit staged across separate backends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingCommit {
    source_writes: BTreeMap<CellId, SourceEntry>,
    control_writes: BTreeMap<String, ControlEntry>,
    phase: CommitPhase,
}

impl PendingCommit {
    fn new(
        source_writes: BTreeMap<CellId, SourceEntry>,
        control_writes: BTreeMap<String, ControlEntry>,
    ) -> Self {
        Self {
            source_writes,
            control_writes,
            phase: CommitPhase::Prepared,
        }
    }

    /// Whether the source side of the transaction boundary was persisted.
    pub fn source_committed(&self) -> bool {
        self.phase >= CommitPhase::SourceCommitted
    }

    /// Whether the control side of the transaction boundary was persisted.
    pub fn control_committed(&self) -> bool {
        self.phase >= CommitPhase::ControlCommitted
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum CommitPhase {
    Prepared,
    SourceCommitted,
    ControlCommitted,
}

/// Store composition failures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreError {
    /// A composed expression tree must have a root Dir.
    MissingRootDir,
    /// A mount path cannot be root and must not duplicate or conflict with existing mounts.
    InvalidMount(String),
    /// A table mount was treated as a directory.
    TableMountIsLeaf(TablePath),
    /// A persisted mount descriptor is corrupt.
    CorruptMount(String),
}

/// Typed source/control/derived stores plus explicit Table/Dir mount descriptors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExprTreeStores {
    root_dir: DirId,
    source: BTreeMap<CellId, SourceEntry>,
    control: BTreeMap<String, ControlEntry>,
    derived: BTreeMap<CellId, DerivedEntry>,
    mounts: BTreeMap<String, MountDescriptor>,
}

impl ExprTreeStores {
    /// Compose a tree over an existing root directory.
    pub fn new(root_dir: DirId) -> Result<Self, StoreError> {
        if root_dir.as_str().is_empty() {
            return Err(StoreError::MissingRootDir);
        }
        Ok(Self {
            root_dir,
            source: BTreeMap::new(),
            control: BTreeMap::new(),
            derived: BTreeMap::new(),
            mounts: BTreeMap::new(),
        })
    }

    /// Reopen persisted stores, validating the mount table before accepting it.
    pub fn reopen(
        root_dir: DirId,
        source: BTreeMap<CellId, SourceEntry>,
        control: BTreeMap<String, ControlEntry>,
        derived: BTreeMap<CellId, DerivedEntry>,
        mounts: Vec<MountDescriptor>,
    ) -> Result<Self, StoreError> {
        let mut stores = Self::new(root_dir)?;
        stores.source = source;
        stores.control = control;
        stores.derived = derived;
        for descriptor in mounts {
            stores.mount(descriptor)?;
        }
        Ok(stores)
    }

    /// Root directory required by the mounted namespace owner.
    pub fn root_dir(&self) -> &DirId {
        &self.root_dir
    }

    /// Authored source entries only.
    pub fn source_entry(&self, id: &CellId) -> Option<&SourceEntry> {
        self.source.get(id)
    }

    /// Operational control entries only.
    pub fn control_entry(&self, key: &str) -> Option<&ControlEntry> {
        self.control.get(key)
    }

    /// Rebuildable derived entries only.
    pub fn derived_entry(&self, id: &CellId) -> Option<&DerivedEntry> {
        self.derived.get(id)
    }

    /// All current explicit mounts.
    pub fn mounts(&self) -> impl Iterator<Item = &MountDescriptor> {
        self.mounts.values()
    }

    /// Return a Table or Dir value to the caller without mutating the namespace.
    pub fn return_value_without_mounting(&self, _resource: MountResource) -> usize {
        self.mounts.len()
    }

    /// Explicitly mount a Table or Dir descriptor.
    pub fn mount(&mut self, descriptor: MountDescriptor) -> Result<(), StoreError> {
        validate_mount(&descriptor)?;
        let key = mount_key(descriptor.path());
        if self.mounts.contains_key(&key) {
            return Err(StoreError::InvalidMount(format!(
                "duplicate mount point {}",
                descriptor.path()
            )));
        }
        for existing in self.mounts.values() {
            if is_prefix(existing.path(), descriptor.path())
                && existing.resource() == MountResource::Table
                && existing.path() != descriptor.path()
            {
                return Err(StoreError::TableMountIsLeaf(existing.path().clone()));
            }
            if is_prefix(descriptor.path(), existing.path())
                && descriptor.resource() == MountResource::Table
            {
                return Err(StoreError::InvalidMount(format!(
                    "table mount {} would parent existing mount {}",
                    descriptor.path(),
                    existing.path()
                )));
            }
        }
        self.mounts.insert(key, descriptor);
        Ok(())
    }

    /// Record a mounted backend epoch in the control store.
    pub fn observe_mount_epoch(
        &mut self,
        path: &TablePath,
        epoch: MountEpoch,
    ) -> Result<(), StoreError> {
        let mount = self
            .mounts
            .get_mut(&mount_key(path))
            .ok_or_else(|| StoreError::CorruptMount(format!("missing mount {}", path)))?;
        mount.set_epoch(epoch);
        self.control.insert(
            format!("mount-epoch:{}", path),
            ControlEntry::MountEpoch(epoch),
        );
        Ok(())
    }

    /// Prepare a recoverable source/control commit.
    ///
    /// The transaction boundary is exactly source plus control. Recovery replays the
    /// same pending record until both sides are durable. Derived entries are
    /// rebuildable and are not part of this boundary.
    pub fn prepare_source_control_commit(
        source_writes: BTreeMap<CellId, SourceEntry>,
        control_writes: BTreeMap<String, ControlEntry>,
    ) -> PendingCommit {
        PendingCommit::new(source_writes, control_writes)
    }

    /// Persist the source side of a prepared commit.
    pub fn commit_source(&mut self, pending: &mut PendingCommit) {
        self.source.extend(pending.source_writes.clone());
        pending.phase = CommitPhase::SourceCommitted;
    }

    /// Persist the control side of a source-committed commit.
    pub fn commit_control(&mut self, pending: &mut PendingCommit) {
        self.control.extend(pending.control_writes.clone());
        pending.phase = CommitPhase::ControlCommitted;
    }

    /// Finish or replay a partially persisted source/control commit.
    pub fn recover_commit(&mut self, pending: &mut PendingCommit) {
        if !pending.source_committed() {
            self.commit_source(pending);
        }
        if !pending.control_committed() {
            self.commit_control(pending);
        }
    }

    /// Store a rebuildable derived value outside source entries.
    pub fn put_derived(&mut self, id: CellId, entry: DerivedEntry) {
        self.derived.insert(id, entry);
    }

    /// Store an operational control value outside source entries.
    pub fn put_control(&mut self, key: impl Into<String>, entry: ControlEntry) {
        self.control.insert(key.into(), entry);
    }
}

fn validate_mount(descriptor: &MountDescriptor) -> Result<(), StoreError> {
    if descriptor.path().is_root() {
        return Err(StoreError::InvalidMount(
            "root is supplied as the required root Dir, not as a mount".to_owned(),
        ));
    }
    if descriptor.backend() == BackendKind::ReadOnly && descriptor.resource() == MountResource::Dir
    {
        return Ok(());
    }
    Ok(())
}

fn is_prefix(candidate: &TablePath, path: &TablePath) -> bool {
    let candidate_segments = segments(candidate);
    let path_segments = segments(path);
    candidate_segments.len() <= path_segments.len()
        && candidate_segments
            .iter()
            .zip(path_segments.iter())
            .all(|(left, right)| left == right)
}

fn segments(path: &TablePath) -> Vec<&str> {
    path.segments().iter().map(String::as_str).collect()
}

fn mount_key(path: &TablePath) -> String {
    path.to_absolute_reference()
}

#[cfg(test)]
pub(crate) fn source_keys_for_test(stores: &ExprTreeStores) -> std::collections::BTreeSet<CellId> {
    stores.source.keys().cloned().collect()
}
