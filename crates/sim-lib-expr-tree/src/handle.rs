use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use sim_kernel::{Cx, Object, ObjectCompat, Result};

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
}

impl TreeRuntime {
    pub(crate) fn new() -> Self {
        Self {
            stores: Mutex::new(BTreeMap::new()),
        }
    }

    pub(crate) fn open(&self, storage_name: &str) -> std::result::Result<TreeHandle, String> {
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
                let state = Arc::new(Mutex::new(TreeState::new(storage_name.to_owned())?));
                stores.insert(storage_name.to_owned(), Arc::clone(&state));
                state
            }
        };
        Ok(TreeHandle::new(state))
    }
}
