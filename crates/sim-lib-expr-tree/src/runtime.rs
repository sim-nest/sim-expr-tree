mod calculation;
mod path;

use std::collections::BTreeMap;

use sim_expr_tree_calc::{CodecPolicyPatch, ExprTreeCalc};
use sim_expr_tree_core::{
    BackendKind, CellCreate, CellId, ControlEntry, DirId, ExprTreeStores, GeneratedNameKind,
    MountDescriptor, MountEpoch, MountResource, Namespace, NamespaceName, NodeKind, SourceEntry,
    TreeId, WriterLane,
};
use sim_kernel::Expr;
use sim_table_core::TablePath;

use crate::runtime_support::{debug_error, source_projection};
use path::{child_path, path_within, resolve_path, split_path};

/// Hard ceiling on authored namespace nodes in one live tree.
pub const MAX_TREE_NODES: usize = 4_096;
/// Hard ceiling on entries returned by one list operation.
pub const MAX_LIST_ITEMS: usize = 1_024;

#[derive(Clone, Debug)]
enum EntryIdentity {
    Dir(DirId),
    Cell(CellId),
}

#[derive(Clone, Debug)]
struct RuntimeCell {
    id: CellId,
    source: Expr,
    revision: u64,
}

pub(crate) struct TreeState {
    storage_name: String,
    namespace: Namespace,
    stores: ExprTreeStores,
    calc: ExprTreeCalc,
    entries: BTreeMap<String, EntryIdentity>,
    cells: BTreeMap<String, RuntimeCell>,
    next_cell_id: u64,
    next_dir_id: u64,
    source_revision: u64,
}

impl TreeState {
    pub(crate) fn new(storage_name: String) -> std::result::Result<Self, String> {
        let tree_id = TreeId::new(format!("tree:{storage_name}")).map_err(debug_error)?;
        let root_dir = DirId::new(format!("dir:{storage_name}:root")).map_err(debug_error)?;
        let namespace = Namespace::new(tree_id, root_dir.clone());
        let stores = ExprTreeStores::new(root_dir.clone()).map_err(debug_error)?;
        Ok(Self {
            storage_name,
            namespace,
            stores,
            calc: ExprTreeCalc::new(),
            entries: BTreeMap::from([("/".to_owned(), EntryIdentity::Dir(root_dir))]),
            cells: BTreeMap::new(),
            next_cell_id: 0,
            next_dir_id: 0,
            source_revision: 0,
        })
    }

    pub(crate) fn storage_name(&self) -> &str {
        &self.storage_name
    }

    pub(crate) fn new_cell(
        &mut self,
        parent: &str,
        name: Option<&str>,
        source: Expr,
    ) -> std::result::Result<String, String> {
        self.ensure_room()?;
        let parent_path = resolve_path(parent, &TablePath::root())?;
        self.ensure_writable(&parent_path)?;
        let parent_id = self.dir_id(&parent_path)?;
        self.next_cell_id = self.next_cell_id.saturating_add(1);
        let cell_id = CellId::new(format!(
            "cell:{}:{}",
            self.namespace.tree_id(),
            self.next_cell_id
        ))
        .map_err(debug_error)?;
        let reserved_name = self.with_writer(|namespace, lane| {
            let reserved =
                reserve_name(namespace, lane, &parent_id, name, GeneratedNameKind::Cell)?;
            let create = CellCreate::new(
                cell_id.clone(),
                parent_id.clone(),
                reserved.clone(),
                NodeKind::Source,
            );
            namespace.create_cell(lane, create)?;
            Ok(reserved)
        })?;
        let path = child_path(&parent_path, reserved_name.as_str())?;
        let source_projection = source_projection(&source)?;
        self.source_revision = self.source_revision.saturating_add(1);
        let revision = self.source_revision;
        let key = path.to_absolute_reference();
        self.entries
            .insert(key.clone(), EntryIdentity::Cell(cell_id.clone()));
        self.cells.insert(
            key.clone(),
            RuntimeCell {
                id: cell_id.clone(),
                source: source.clone(),
                revision,
            },
        );
        let mut pending = ExprTreeStores::prepare_source_control_commit(
            BTreeMap::from([(cell_id, SourceEntry::new(source_projection))]),
            BTreeMap::from([(
                format!("source-revision:{key}"),
                ControlEntry::Counter(revision),
            )]),
        );
        self.stores.commit_source(&mut pending);
        self.stores.commit_control(&mut pending);
        self.calc.set_cell(path, source);
        self.run_ready_automatic();
        Ok(key)
    }

    pub(crate) fn new_dir(
        &mut self,
        parent: &str,
        name: Option<&str>,
    ) -> std::result::Result<String, String> {
        let parent_path = resolve_path(parent, &TablePath::root())?;
        self.create_dir(&parent_path, name, false)
    }

    fn create_dir(
        &mut self,
        parent_path: &TablePath,
        name: Option<&str>,
        mounting: bool,
    ) -> std::result::Result<String, String> {
        self.ensure_room()?;
        self.ensure_writable(parent_path)?;
        if !mounting && self.table_mount_contains(parent_path) {
            return Err(format!(
                "new-dir rejected: Table mount {parent_path} is a leaf"
            ));
        }
        let parent_id = self.dir_id(parent_path)?;
        self.next_dir_id = self.next_dir_id.saturating_add(1);
        let dir_id = DirId::new(format!(
            "dir:{}:{}",
            self.namespace.tree_id(),
            self.next_dir_id
        ))
        .map_err(debug_error)?;
        let reserved_name = self.with_writer(|namespace, lane| {
            let reserved = reserve_name(namespace, lane, &parent_id, name, GeneratedNameKind::Dir)?;
            namespace.create_dir(
                lane,
                dir_id.clone(),
                &parent_id,
                reserved.clone(),
                CodecPolicyPatch::empty(),
            )?;
            Ok(reserved)
        })?;
        let path = child_path(parent_path, reserved_name.as_str())?;
        let key = path.to_absolute_reference();
        self.entries.insert(key.clone(), EntryIdentity::Dir(dir_id));
        Ok(key)
    }

    pub(crate) fn mount(
        &mut self,
        path: &str,
        backend: BackendKind,
        resource: MountResource,
        epoch: MountEpoch,
    ) -> std::result::Result<String, String> {
        let target = resolve_path(path, &TablePath::root())?;
        if target.is_root() {
            return Err("mount rejected: root is the required root Dir".to_owned());
        }
        let (parent, name) = split_path(&target)?;
        let created = self.create_dir(&parent, Some(&name), true)?;
        let descriptor = match resource {
            MountResource::Table => MountDescriptor::table(target.clone(), backend, epoch),
            MountResource::Dir => MountDescriptor::dir(target.clone(), backend, epoch),
        };
        if let Err(error) = self.stores.mount(descriptor) {
            let _ = self.delete_empty_dir(&target);
            return Err(debug_error(error));
        }
        self.calc.mount(target, resource, backend, epoch);
        Ok(created)
    }

    pub(crate) fn unmount(&mut self, path: &str) -> std::result::Result<bool, String> {
        let target = resolve_path(path, &TablePath::root())?;
        self.ensure_empty_dir(&target)?;
        self.stores.unmount(&target).map_err(debug_error)?;
        self.calc.unmount(&target);
        self.delete_empty_dir(&target)?;
        Ok(true)
    }

    pub(crate) fn move_entry(
        &mut self,
        from: &str,
        to: &str,
    ) -> std::result::Result<String, String> {
        let from = resolve_path(from, &TablePath::root())?;
        let to = resolve_path(to, &TablePath::root())?;
        if from.is_root() || to.is_root() {
            return Err("move rejected: root cannot move or be replaced".to_owned());
        }
        self.ensure_writable(&from)?;
        let (to_parent, to_name) = split_path(&to)?;
        self.ensure_writable(&to_parent)?;
        let parent_id = self.dir_id(&to_parent)?;
        let new_name = NamespaceName::new(to_name).map_err(debug_error)?;
        let from_key = from.to_absolute_reference();
        let identity = self
            .entries
            .get(&from_key)
            .cloned()
            .ok_or_else(|| format!("missing namespace entry {from_key}"))?;
        if self
            .stores
            .mounts()
            .any(|mount| path_within(&from, mount.path()))
        {
            return Err(format!(
                "mounted subtree {from} must be unmounted before move"
            ));
        }
        if self.entries.contains_key(&to.to_absolute_reference()) {
            return Err(format!("target path already exists: {to}"));
        }
        match &identity {
            EntryIdentity::Cell(id) => {
                let id = id.clone();
                self.with_writer(|namespace, lane| {
                    namespace.move_cell(lane, &id, &parent_id, new_name)
                })?;
                self.calc.move_cell(&from, to.clone());
            }
            EntryIdentity::Dir(id) => {
                let id = id.clone();
                self.with_writer(|namespace, lane| {
                    namespace.move_dir(lane, &id, &parent_id, new_name)
                })?;
            }
        }
        self.rekey_subtree(&from, &to)?;
        Ok(to.to_absolute_reference())
    }

    pub(crate) fn rename_entry(
        &mut self,
        path: &str,
        name: &str,
    ) -> std::result::Result<String, String> {
        let path = resolve_path(path, &TablePath::root())?;
        let (parent, _) = split_path(&path)?;
        let target = child_path(&parent, name)?;
        self.move_entry(
            &path.to_absolute_reference(),
            &target.to_absolute_reference(),
        )
    }

    pub(crate) fn delete(&mut self, path: &str) -> std::result::Result<bool, String> {
        let path = resolve_path(path, &TablePath::root())?;
        let key = path.to_absolute_reference();
        let identity = self
            .entries
            .get(&key)
            .cloned()
            .ok_or_else(|| format!("missing namespace entry {key}"))?;
        self.ensure_writable(&path)?;
        match identity {
            EntryIdentity::Cell(id) => {
                self.with_writer(|namespace, lane| namespace.delete_cell(lane, &id))?;
                self.calc.remove_cell(&path);
                self.stores.remove_source(&id);
                self.entries.remove(&key);
                self.cells.remove(&key);
            }
            EntryIdentity::Dir(_) => {
                self.ensure_empty_dir(&path)?;
                if self.mount_at(&path).is_some() {
                    return Err(format!("mounted path {path} must be unmounted first"));
                }
                self.delete_empty_dir(&path)?;
            }
        }
        Ok(true)
    }

    pub(crate) fn set_expr(
        &mut self,
        path: &str,
        source: Expr,
    ) -> std::result::Result<String, String> {
        let path = resolve_path(path, &TablePath::root())?;
        let key = path.to_absolute_reference();
        self.ensure_writable(&path)?;
        let projection = source_projection(&source)?;
        let cell = self
            .cells
            .get_mut(&key)
            .ok_or_else(|| format!("not a cell: {key}"))?;
        self.source_revision = self.source_revision.saturating_add(1);
        cell.source = source.clone();
        cell.revision = self.source_revision;
        let mut pending = ExprTreeStores::prepare_source_control_commit(
            BTreeMap::from([(cell.id.clone(), SourceEntry::new(projection))]),
            BTreeMap::from([(
                format!("source-revision:{key}"),
                ControlEntry::Counter(cell.revision),
            )]),
        );
        self.stores.commit_source(&mut pending);
        self.stores.commit_control(&mut pending);
        self.calc.set_cell(path, source);
        self.run_ready_automatic();
        Ok(key)
    }

    pub(crate) fn list(
        &self,
        path: &str,
    ) -> std::result::Result<Vec<(String, &'static str)>, String> {
        let path = resolve_path(path, &TablePath::root())?;
        self.dir_id(&path)?;
        let mut rows = self
            .entries
            .iter()
            .filter_map(|(key, identity)| {
                let candidate = TablePath::parse_absolute(key).ok()?;
                let (parent, _) = split_path(&candidate).ok()?;
                if parent != path {
                    return None;
                }
                let kind = match identity {
                    EntryIdentity::Cell(_) => "cell",
                    EntryIdentity::Dir(_) if self.mount_at(&candidate).is_some() => "mount",
                    EntryIdentity::Dir(_) => "dir",
                };
                Some((key.clone(), kind))
            })
            .collect::<Vec<_>>();
        rows.sort();
        if rows.len() > MAX_LIST_ITEMS {
            return Err(format!(
                "list exceeds hard item limit {MAX_LIST_ITEMS} at {path}"
            ));
        }
        Ok(rows)
    }

    fn ensure_room(&self) -> std::result::Result<(), String> {
        if self.entries.len() >= MAX_TREE_NODES {
            Err(format!("tree node limit {MAX_TREE_NODES} reached"))
        } else {
            Ok(())
        }
    }

    fn entry(&self, path: &TablePath) -> std::result::Result<EntryIdentity, String> {
        self.entries
            .get(&path.to_absolute_reference())
            .cloned()
            .ok_or_else(|| format!("missing namespace entry {path}"))
    }

    fn dir_id(&self, path: &TablePath) -> std::result::Result<DirId, String> {
        match self.entry(path)? {
            EntryIdentity::Dir(id) => Ok(id),
            EntryIdentity::Cell(_) => Err(format!("not a directory: {path}")),
        }
    }

    fn cell(&self, path: &TablePath) -> std::result::Result<&RuntimeCell, String> {
        self.cells
            .get(&path.to_absolute_reference())
            .ok_or_else(|| format!("not a cell: {path}"))
    }

    fn with_writer<T>(
        &mut self,
        action: impl FnOnce(
            &mut Namespace,
            WriterLane,
        ) -> std::result::Result<T, sim_expr_tree_core::NamespaceError>,
    ) -> std::result::Result<T, String> {
        let lane = self.namespace.acquire_writer().map_err(debug_error)?;
        let value = action(&mut self.namespace, lane);
        let released = self.namespace.release_writer(lane);
        match (value, released) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), _) | (_, Err(error)) => Err(debug_error(error)),
        }
    }

    fn delete_empty_dir(&mut self, path: &TablePath) -> std::result::Result<(), String> {
        let key = path.to_absolute_reference();
        let id = self.dir_id(path)?;
        self.with_writer(|namespace, lane| namespace.delete_dir(lane, &id))?;
        self.entries.remove(&key);
        Ok(())
    }

    fn ensure_empty_dir(&self, path: &TablePath) -> std::result::Result<(), String> {
        let prefix = format!("{}/", path.to_absolute_reference().trim_end_matches('/'));
        if self
            .entries
            .keys()
            .any(|entry| entry != &path.to_absolute_reference() && entry.starts_with(&prefix))
        {
            Err(format!("directory is not empty: {path}"))
        } else {
            Ok(())
        }
    }

    fn rekey_subtree(
        &mut self,
        from: &TablePath,
        to: &TablePath,
    ) -> std::result::Result<(), String> {
        let from_key = from.to_absolute_reference();
        let to_key = to.to_absolute_reference();
        let descendants = self
            .entries
            .keys()
            .filter(|key| {
                *key == &from_key
                    || key
                        .strip_prefix(&from_key)
                        .is_some_and(|tail| tail.starts_with('/'))
            })
            .cloned()
            .collect::<Vec<_>>();
        if descendants.len() > MAX_TREE_NODES {
            return Err("move subtree exceeds tree node limit".to_owned());
        }
        for old in descendants {
            let suffix = old.strip_prefix(&from_key).expect("selected prefix");
            let new = format!("{to_key}{suffix}");
            let identity = self.entries.remove(&old).expect("selected entry");
            if let Some(cell) = self.cells.remove(&old) {
                let old_path = TablePath::parse_absolute(&old).map_err(debug_error)?;
                let new_path = TablePath::parse_absolute(&new).map_err(debug_error)?;
                if old != from_key {
                    self.calc.move_cell(&old_path, new_path);
                }
                self.cells.insert(new.clone(), cell);
            }
            self.entries.insert(new, identity);
        }
        Ok(())
    }

    fn ensure_writable(&self, path: &TablePath) -> std::result::Result<(), String> {
        for mount in self.stores.mounts() {
            if path_within(mount.path(), path) && mount.backend() == BackendKind::ReadOnly {
                return Err(format!("mounted backend is read-only at {}", mount.path()));
            }
        }
        Ok(())
    }

    fn table_mount_contains(&self, path: &TablePath) -> bool {
        self.stores.mounts().any(|mount| {
            mount.resource() == MountResource::Table && path_within(mount.path(), path)
        })
    }

    fn mount_at(&self, path: &TablePath) -> Option<&MountDescriptor> {
        self.stores.mounts().find(|mount| mount.path() == path)
    }
}

fn reserve_name(
    namespace: &mut Namespace,
    lane: WriterLane,
    parent: &DirId,
    name: Option<&str>,
    generated: GeneratedNameKind,
) -> std::result::Result<NamespaceName, sim_expr_tree_core::NamespaceError> {
    match name {
        Some(name) => namespace.reserve_name(lane, parent, NamespaceName::new(name)?),
        None => namespace.reserve_generated_name(lane, parent, generated),
    }
}
