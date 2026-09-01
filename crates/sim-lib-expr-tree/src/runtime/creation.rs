//! Live tree creation with a captured runtime and authority ceiling.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, atomic::AtomicU64},
};

use sim_expr_tree_calc::ExprTreeCalc;
use sim_expr_tree_core::{DirId, ExprTreeStores, Namespace, TreeId};
use sim_kernel::Cx;

use super::{EntryIdentity, TreeState};
use crate::handle::next_handle_seed;
use crate::runtime_support::debug_error;

impl TreeState {
    pub(crate) fn new(
        cx: &Cx,
        storage_name: String,
        handle_seeds: Arc<AtomicU64>,
    ) -> std::result::Result<Self, String> {
        let tree_id = TreeId::new(format!("tree:{storage_name}")).map_err(debug_error)?;
        let root_dir = DirId::new(format!("dir:{storage_name}:root")).map_err(debug_error)?;
        let namespace = Namespace::new(tree_id, root_dir.clone());
        let stores = ExprTreeStores::new(root_dir.clone()).map_err(debug_error)?;
        let template = Arc::new(Mutex::new(
            cx.fork_from_seed(next_handle_seed(&handle_seeds)),
        ));
        let calc_template = Arc::clone(&template);
        let calc_handle_seeds = Arc::clone(&handle_seeds);
        let calc = ExprTreeCalc::with_context_factory(move || {
            calc_template
                .lock()
                .expect("expression-tree context seed poisoned")
                .fork_from_seed(next_handle_seed(&calc_handle_seeds))
        });
        Ok(Self {
            storage_name,
            namespace,
            stores,
            calc,
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
}
