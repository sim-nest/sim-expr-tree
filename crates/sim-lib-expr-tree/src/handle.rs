use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use sim_kernel::{Cx, HandleSeed, Object, ObjectCompat, Result};

use crate::runtime::TreeState;

const MAX_STORAGE_NAME_BYTES: usize = 256;

// sim-non-citizen(reason = "live writer scope and backend authority", kind = "handle", descriptor = "")
/// Opaque live expression-tree handle.
///
/// Handles are cloneable references to one writer scope. Their default object
/// expression is `core/opaque-object`; they never expose Citizen reconstruction.
#[derive(Clone)]
pub struct TreeHandle {
    pub(crate) state: Arc<Mutex<TreeState>>,
}

impl TreeHandle {
    fn new(state: Arc<Mutex<TreeState>>) -> Self {
        Self { state }
    }
}

impl Object for TreeHandle {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        let state = self
            .state
            .lock()
            .map_err(|_| sim_kernel::Error::Eval("expression-tree state poisoned".to_owned()))?;
        Ok(format!("#<expr-tree {}>", state.storage_name()))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for TreeHandle {}

pub(crate) struct TreeRuntime {
    stores: Mutex<BTreeMap<String, Arc<Mutex<TreeState>>>>,
    next_handle_seed: Arc<AtomicU64>,
}

impl TreeRuntime {
    pub(crate) fn new(first_handle_seed: HandleSeed) -> Self {
        Self {
            stores: Mutex::new(BTreeMap::new()),
            next_handle_seed: Arc::new(AtomicU64::new(first_handle_seed.0)),
        }
    }

    pub(crate) fn open(
        &self,
        cx: &Cx,
        storage_name: &str,
    ) -> std::result::Result<TreeHandle, String> {
        if storage_name.is_empty() || storage_name.len() > MAX_STORAGE_NAME_BYTES {
            return Err(format!(
                "storage name must contain 1..={MAX_STORAGE_NAME_BYTES} bytes"
            ));
        }
        let mut stores = self
            .stores
            .lock()
            .map_err(|_| "expression-tree storage registry poisoned".to_owned())?;
        let state = match stores.get(storage_name) {
            Some(state) => Arc::clone(state),
            None => {
                let state = Arc::new(Mutex::new(TreeState::new(
                    cx,
                    storage_name.to_owned(),
                    Arc::clone(&self.next_handle_seed),
                )?));
                stores.insert(storage_name.to_owned(), Arc::clone(&state));
                state
            }
        };
        Ok(TreeHandle::new(state))
    }
}

pub(crate) fn next_handle_seed(sequence: &AtomicU64) -> HandleSeed {
    let seed = sequence
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |seed| {
            seed.checked_add(1)
        })
        .expect("expression-tree handle seed space exhausted");
    HandleSeed::new(seed)
}
