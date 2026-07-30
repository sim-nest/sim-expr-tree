use std::collections::{BTreeSet, HashMap};

use crate::{
    CellId, CellRecord, CodecPolicyPatch, DirId, DirRecord, EffectiveCodecPolicy,
    GeneratedNameKind, NamespaceError, NamespaceName, NodeKind, RevisionTick, SourceRecord, Stamp,
    TreeId,
};

/// Request record for creating an immutable durable cell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellCreate {
    /// Stable cell id.
    pub id: CellId,
    /// Reserved parent directory.
    pub parent: DirId,
    /// Reserved child name.
    pub name: NamespaceName,
    /// Cell node family.
    pub kind: NodeKind,
    /// Optional source provenance.
    pub source: Option<SourceRecord>,
    /// Local policy patch.
    pub policy_patch: CodecPolicyPatch,
}

impl CellCreate {
    /// Create a request with no source provenance and no local policy patch.
    pub fn new(id: CellId, parent: DirId, name: NamespaceName, kind: NodeKind) -> Self {
        Self {
            id,
            parent,
            name,
            kind,
            source: None,
            policy_patch: CodecPolicyPatch::empty(),
        }
    }

    /// Attach source provenance.
    pub fn with_source(mut self, source: SourceRecord) -> Self {
        self.source = Some(source);
        self
    }

    /// Attach a local policy patch.
    pub fn with_policy_patch(mut self, policy_patch: CodecPolicyPatch) -> Self {
        self.policy_patch = policy_patch;
        self
    }
}

/// An acquired serialized writer lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriterLane {
    epoch: u64,
}

/// Durable finite namespace records for one expression tree.
#[derive(Debug)]
pub struct Namespace {
    tree_id: TreeId,
    root_dir: DirId,
    dirs: HashMap<DirId, DirRecord>,
    cells: HashMap<CellId, CellRecord>,
    children: HashMap<(DirId, NamespaceName), NamespaceEntry>,
    reservations: BTreeSet<(DirId, NamespaceName)>,
    counters: HashMap<(DirId, GeneratedNameKind), u64>,
    current: RevisionTick,
    writer_epoch: u64,
    writer_active: bool,
}

impl Namespace {
    /// Create a namespace with one root directory record.
    pub fn new(tree_id: TreeId, root_dir: DirId) -> Self {
        let stamp = Stamp::new(RevisionTick::default(), None);
        let root = DirRecord::root(root_dir.clone(), stamp);
        Self {
            tree_id,
            root_dir: root_dir.clone(),
            dirs: HashMap::from([(root_dir, root)]),
            cells: HashMap::new(),
            children: HashMap::new(),
            reservations: BTreeSet::new(),
            counters: HashMap::new(),
            current: RevisionTick::default(),
            writer_epoch: 0,
            writer_active: false,
        }
    }

    /// The namespace tree id.
    pub fn tree_id(&self) -> &TreeId {
        &self.tree_id
    }

    /// Root directory identity.
    pub fn root_dir(&self) -> &DirId {
        &self.root_dir
    }

    /// Acquire the single serialized writer lane.
    pub fn acquire_writer(&mut self) -> Result<WriterLane, NamespaceError> {
        if self.writer_active {
            return Err(NamespaceError::WriterAlreadyActive);
        }
        self.writer_active = true;
        self.writer_epoch += 1;
        Ok(WriterLane {
            epoch: self.writer_epoch,
        })
    }

    /// Release a previously acquired writer lane.
    pub fn release_writer(&mut self, lane: WriterLane) -> Result<(), NamespaceError> {
        self.require_writer(lane)?;
        self.writer_active = false;
        Ok(())
    }

    /// Number of durable name reservations.
    pub fn reservation_count(&self) -> usize {
        self.reservations.len()
    }

    /// Number of generated-name counters.
    pub fn counter_count(&self) -> usize {
        self.counters.len()
    }

    /// Read a directory by id without allocating namespace state.
    pub fn dir(&self, id: &DirId) -> Option<&DirRecord> {
        self.dirs.get(id)
    }

    /// Read a cell by id without allocating namespace state.
    pub fn cell(&self, id: &CellId) -> Option<&CellRecord> {
        self.cells.get(id)
    }

    /// Read a named child without allocating namespace state.
    pub fn child(&self, parent: &DirId, name: &NamespaceName) -> Option<NamespaceEntry> {
        self.children.get(&(parent.clone(), name.clone())).cloned()
    }

    /// Reserve an explicit child name before creating the durable node.
    pub fn reserve_name(
        &mut self,
        lane: WriterLane,
        parent: &DirId,
        name: NamespaceName,
    ) -> Result<NamespaceName, NamespaceError> {
        self.require_writer(lane)?;
        self.ensure_parent(parent)?;
        self.ensure_available(parent, &name)?;
        self.reservations.insert((parent.clone(), name.clone()));
        Ok(name)
    }

    /// Reserve a generated child name before creating the durable node.
    pub fn reserve_generated_name(
        &mut self,
        lane: WriterLane,
        parent: &DirId,
        kind: GeneratedNameKind,
    ) -> Result<NamespaceName, NamespaceError> {
        self.require_writer(lane)?;
        self.ensure_parent(parent)?;
        let key = (parent.clone(), kind);
        let next_value = {
            let next = self.counters.entry(key).or_insert(0);
            *next += 1;
            *next
        };
        let name = NamespaceName::new(format!("{}-{}", kind.prefix(), next_value))?;
        if !self.is_available(parent, &name) {
            return Err(NamespaceError::CounterCorruption {
                parent: parent.clone(),
                kind,
                candidate: name,
            });
        }
        self.reservations.insert((parent.clone(), name.clone()));
        Ok(name)
    }

    /// Create a durable cell from a prior reservation.
    pub fn create_cell(
        &mut self,
        lane: WriterLane,
        request: CellCreate,
    ) -> Result<(), NamespaceError> {
        self.require_writer(lane)?;
        self.consume_reservation(&request.parent, &request.name)?;
        self.ensure_available(&request.parent, &request.name)?;
        let stamp = self.next_stamp();
        let record = CellRecord::new(
            request.id.clone(),
            request.parent.clone(),
            request.name.clone(),
            request.kind,
            request.source,
            request.policy_patch,
            stamp,
        );
        self.children.insert(
            (request.parent, request.name),
            NamespaceEntry::Cell {
                id: request.id.clone(),
                kind: request.kind,
            },
        );
        self.cells.insert(request.id, record);
        Ok(())
    }

    /// Create a durable child directory from a prior reservation.
    pub fn create_dir(
        &mut self,
        lane: WriterLane,
        id: DirId,
        parent: &DirId,
        name: NamespaceName,
        policy_patch: CodecPolicyPatch,
    ) -> Result<(), NamespaceError> {
        self.require_writer(lane)?;
        self.consume_reservation(parent, &name)?;
        self.ensure_available(parent, &name)?;
        let stamp = self.next_stamp();
        let record = DirRecord::child(
            id.clone(),
            parent.clone(),
            name.clone(),
            policy_patch,
            stamp,
        );
        self.children.insert(
            (parent.clone(), name),
            NamespaceEntry::Dir { id: id.clone() },
        );
        self.dirs.insert(id, record);
        Ok(())
    }

    /// Rename or move a cell while preserving immutable cell identity.
    pub fn move_cell(
        &mut self,
        lane: WriterLane,
        id: &CellId,
        new_parent: &DirId,
        new_name: NamespaceName,
    ) -> Result<(), NamespaceError> {
        self.require_writer(lane)?;
        self.ensure_parent(new_parent)?;
        let (old_parent, old_name, kind) = {
            let record = self
                .cells
                .get(id)
                .ok_or_else(|| NamespaceError::MissingCell(id.clone()))?;
            (
                record.parent().clone(),
                record.name().clone(),
                record.kind(),
            )
        };
        if old_parent != *new_parent || old_name != new_name {
            self.ensure_available(new_parent, &new_name)?;
        }
        self.children.remove(&(old_parent, old_name));
        self.children.insert(
            (new_parent.clone(), new_name.clone()),
            NamespaceEntry::Cell {
                id: id.clone(),
                kind,
            },
        );
        self.cells
            .get_mut(id)
            .expect("cell existence was checked above")
            .rename(new_parent.clone(), new_name);
        self.next_stamp();
        Ok(())
    }

    /// Rename or move a directory while preserving immutable directory identity.
    pub fn move_dir(
        &mut self,
        lane: WriterLane,
        id: &DirId,
        new_parent: &DirId,
        new_name: NamespaceName,
    ) -> Result<(), NamespaceError> {
        self.require_writer(lane)?;
        if id == &self.root_dir {
            return Err(NamespaceError::RootDirCannotMove);
        }
        self.ensure_parent(new_parent)?;
        if self.is_descendant(new_parent, id) {
            return Err(NamespaceError::DirMoveCycle {
                dir: id.clone(),
                new_parent: new_parent.clone(),
            });
        }
        let (old_parent, old_name) = {
            let record = self
                .dirs
                .get(id)
                .ok_or_else(|| NamespaceError::MissingDirRecord(id.clone()))?;
            (
                record
                    .parent()
                    .cloned()
                    .expect("non-root dirs have parents"),
                record.name().cloned().expect("non-root dirs have names"),
            )
        };
        if old_parent != *new_parent || old_name != new_name {
            self.ensure_available(new_parent, &new_name)?;
        }
        self.children.remove(&(old_parent, old_name));
        self.children.insert(
            (new_parent.clone(), new_name.clone()),
            NamespaceEntry::Dir { id: id.clone() },
        );
        self.dirs
            .get_mut(id)
            .expect("dir existence was checked above")
            .rename(new_parent.clone(), new_name);
        self.next_stamp();
        Ok(())
    }

    /// Resolve the inherited policy for a directory.
    pub fn effective_dir_policy(&self, id: &DirId) -> Result<EffectiveCodecPolicy, NamespaceError> {
        let mut lineage = Vec::new();
        let mut cursor = id;
        loop {
            let dir = self
                .dirs
                .get(cursor)
                .ok_or_else(|| NamespaceError::MissingDirRecord(cursor.clone()))?;
            lineage.push(dir);
            match dir.parent() {
                Some(parent) => cursor = parent,
                None => break,
            }
        }

        let mut effective = EffectiveCodecPolicy::empty();
        for dir in lineage.into_iter().rev() {
            dir.policy_patch().apply_to(&mut effective);
        }
        Ok(effective)
    }

    /// Resolve the inherited policy for a cell, applying the cell patch last.
    pub fn effective_cell_policy(
        &self,
        id: &CellId,
    ) -> Result<EffectiveCodecPolicy, NamespaceError> {
        let cell = self
            .cells
            .get(id)
            .ok_or_else(|| NamespaceError::MissingCell(id.clone()))?;
        let mut effective = self.effective_dir_policy(cell.parent())?;
        cell.policy_patch().apply_to(&mut effective);
        Ok(effective)
    }

    /// Test-only recovery hook for corrupt persisted counters.
    #[cfg(test)]
    pub(crate) fn set_counter_for_test(
        &mut self,
        parent: DirId,
        kind: GeneratedNameKind,
        value: u64,
    ) {
        self.counters.insert((parent, kind), value);
    }

    fn require_writer(&self, lane: WriterLane) -> Result<(), NamespaceError> {
        if self.writer_active && lane.epoch == self.writer_epoch {
            Ok(())
        } else {
            Err(NamespaceError::InvalidWriterLane)
        }
    }

    fn ensure_parent(&self, parent: &DirId) -> Result<(), NamespaceError> {
        if self.dirs.contains_key(parent) {
            Ok(())
        } else {
            Err(NamespaceError::MissingDir(parent.clone()))
        }
    }

    fn is_available(&self, parent: &DirId, name: &NamespaceName) -> bool {
        !self.children.contains_key(&(parent.clone(), name.clone()))
            && !self.reservations.contains(&(parent.clone(), name.clone()))
    }

    fn ensure_available(&self, parent: &DirId, name: &NamespaceName) -> Result<(), NamespaceError> {
        if self.is_available(parent, name) {
            Ok(())
        } else {
            Err(NamespaceError::NameCollision {
                parent: parent.clone(),
                name: name.clone(),
            })
        }
    }

    fn consume_reservation(
        &mut self,
        parent: &DirId,
        name: &NamespaceName,
    ) -> Result<(), NamespaceError> {
        if self.reservations.remove(&(parent.clone(), name.clone())) {
            Ok(())
        } else {
            Err(NamespaceError::MissingReservation {
                parent: parent.clone(),
                name: name.clone(),
            })
        }
    }

    fn next_stamp(&mut self) -> Stamp {
        self.current = self.current.next_after();
        Stamp::new(self.current, None)
    }

    fn is_descendant(&self, candidate: &DirId, ancestor: &DirId) -> bool {
        let mut cursor = Some(candidate);
        while let Some(id) = cursor {
            if id == ancestor {
                return true;
            }
            cursor = self.dirs.get(id).and_then(DirRecord::parent);
        }
        false
    }
}

/// A named child entry in the finite namespace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NamespaceEntry {
    /// Child directory.
    Dir {
        /// Directory identity.
        id: DirId,
    },
    /// Child cell.
    Cell {
        /// Cell identity.
        id: CellId,
        /// Cell kind.
        kind: NodeKind,
    },
}

impl Namespace {
    /// Convenience policy patch for tests and callers that need only a codec.
    pub fn codec_patch(codec: impl Into<String>) -> CodecPolicyPatch {
        CodecPolicyPatch::set_codec(codec)
    }
}
