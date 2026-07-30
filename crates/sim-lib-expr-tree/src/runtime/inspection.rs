//! Bounded namespace inspection for server and surface composition.

use super::TreeState;
use crate::{TreeEntryInspection, TreeEntryKind};

impl TreeState {
    pub(crate) fn inspect_entries(
        &self,
        path: &str,
    ) -> std::result::Result<Vec<TreeEntryInspection>, String> {
        self.list(path)?
            .into_iter()
            .map(|(path, kind)| {
                let revision = self.cells.get(&path).map(|cell| cell.revision).unwrap_or(0);
                let name = path
                    .rsplit('/')
                    .next()
                    .filter(|name| !name.is_empty())
                    .unwrap_or("/")
                    .to_owned();
                let kind = match kind {
                    "cell" => TreeEntryKind::Cell,
                    "mount" => TreeEntryKind::Mount,
                    _ => TreeEntryKind::Directory,
                };
                Ok(TreeEntryInspection {
                    path,
                    name,
                    kind,
                    revision,
                })
            })
            .collect()
    }
}
