use crate::{CellId, DirId, NamespaceName, PolicyPatch, Stamp};

/// Durable expression-tree node families.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NodeKind {
    /// A mutable source cell.
    Source,
    /// A control/configuration cell.
    Control,
    /// A calculation cell whose value is derived from dependencies.
    Derived,
    /// A mounted external table or directory.
    Mounted,
}

/// Source provenance attached to a namespace node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceRecord {
    origin: String,
    observed_at: Stamp,
}

impl SourceRecord {
    /// Create source provenance from an origin label and stamp.
    pub fn new(origin: impl Into<String>, observed_at: Stamp) -> Self {
        Self {
            origin: origin.into(),
            observed_at,
        }
    }

    /// The source origin label.
    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// The stamp associated with this source observation.
    pub fn observed_at(&self) -> Stamp {
        self.observed_at
    }
}

/// Immutable durable calculation cell record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellRecord {
    id: CellId,
    parent: DirId,
    name: NamespaceName,
    kind: NodeKind,
    source: Option<SourceRecord>,
    policy_patch: PolicyPatch,
    created_at: Stamp,
}

impl CellRecord {
    pub(crate) fn new(
        id: CellId,
        parent: DirId,
        name: NamespaceName,
        kind: NodeKind,
        source: Option<SourceRecord>,
        policy_patch: PolicyPatch,
        created_at: Stamp,
    ) -> Self {
        Self {
            id,
            parent,
            name,
            kind,
            source,
            policy_patch,
            created_at,
        }
    }

    /// Stable immutable cell id.
    pub fn id(&self) -> &CellId {
        &self.id
    }

    /// Parent directory id.
    pub fn parent(&self) -> &DirId {
        &self.parent
    }

    /// Current child name.
    pub fn name(&self) -> &NamespaceName {
        &self.name
    }

    /// Node family.
    pub fn kind(&self) -> NodeKind {
        self.kind
    }

    /// Optional source provenance.
    pub fn source(&self) -> Option<&SourceRecord> {
        self.source.as_ref()
    }

    /// Local policy patch.
    pub fn policy_patch(&self) -> &PolicyPatch {
        &self.policy_patch
    }

    /// Creation stamp.
    pub fn created_at(&self) -> Stamp {
        self.created_at
    }

    pub(crate) fn rename(&mut self, parent: DirId, name: NamespaceName) {
        self.parent = parent;
        self.name = name;
    }
}

/// Durable directory record with immutable identity and movable parent/name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirRecord {
    id: DirId,
    parent: Option<DirId>,
    name: Option<NamespaceName>,
    policy_patch: PolicyPatch,
    created_at: Stamp,
}

impl DirRecord {
    pub(crate) fn root(id: DirId, created_at: Stamp) -> Self {
        Self {
            id,
            parent: None,
            name: None,
            policy_patch: PolicyPatch::empty(),
            created_at,
        }
    }

    pub(crate) fn child(
        id: DirId,
        parent: DirId,
        name: NamespaceName,
        policy_patch: PolicyPatch,
        created_at: Stamp,
    ) -> Self {
        Self {
            id,
            parent: Some(parent),
            name: Some(name),
            policy_patch,
            created_at,
        }
    }

    /// Stable immutable directory id.
    pub fn id(&self) -> &DirId {
        &self.id
    }

    /// Parent directory id, absent for root.
    pub fn parent(&self) -> Option<&DirId> {
        self.parent.as_ref()
    }

    /// Current child name, absent for root.
    pub fn name(&self) -> Option<&NamespaceName> {
        self.name.as_ref()
    }

    /// Local policy patch.
    pub fn policy_patch(&self) -> &PolicyPatch {
        &self.policy_patch
    }

    /// Creation stamp.
    pub fn created_at(&self) -> Stamp {
        self.created_at
    }

    pub(crate) fn rename(&mut self, parent: DirId, name: NamespaceName) {
        self.parent = Some(parent);
        self.name = Some(name);
    }
}
