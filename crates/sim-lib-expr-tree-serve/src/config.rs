//! Typed projection of the standard runtime config table.

use std::path::PathBuf;

use sim_config::ConfigView;
use sim_kernel::Symbol;
use sim_run_core::RuntimeConfigState;

/// Server placement selected by the product recipe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServerPlacement {
    /// Construct and own the authoritative server in this boot session.
    InProcess,
    /// Use an already loaded site whose object implements `EvalFabric`.
    External {
        /// Site export selected from the boot runtime registry.
        site: Symbol,
    },
}

/// Runtime configuration consumed by the expression-tree serve recipe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpressionTreeServeConfig {
    /// Placement of the authoritative server.
    pub placement: ServerPlacement,
    /// Durable storage name opened by the authoritative server.
    pub storage: String,
    /// Opaque browser-visible alias for the authoritative session.
    pub browser_resource: String,
    /// Optional deterministic in-process bridge id.
    pub bridge_thread: Option<u64>,
    /// Address bound by the generic web shell.
    pub web_addr: String,
    /// Generated Atelier cache root served by the generic web shell.
    pub atelier_root: PathBuf,
    /// Compose and validate the product without binding the web listener.
    pub dry_run: bool,
}

impl Default for ExpressionTreeServeConfig {
    fn default() -> Self {
        Self {
            placement: ServerPlacement::InProcess,
            storage: "expression-tree".to_owned(),
            browser_resource: "tree".to_owned(),
            bridge_thread: None,
            web_addr: "127.0.0.1:8787".to_owned(),
            atelier_root: PathBuf::from(".sim/atelier"),
            dry_run: false,
        }
    }
}

impl ExpressionTreeServeConfig {
    /// Reads the product table from the already merged bootloader config state.
    pub fn from_runtime_config(state: &RuntimeConfigState) -> Result<Self, String> {
        let Some(table) = state.effective().dir.table(&serve_config_symbol()) else {
            return Ok(Self::default());
        };
        let view = ConfigView::new(table);
        let placement = match optional_string(&view, "placement")?.as_deref() {
            None | Some("in-process") => ServerPlacement::InProcess,
            Some("external") => ServerPlacement::External {
                site: Symbol::new(required_string(&view, "server-site")?),
            },
            Some(other) => {
                return Err(format!(
                    "placement must be \"in-process\" or \"external\", found {other:?}"
                ));
            }
        };
        let defaults = Self::default();
        let storage = optional_string(&view, "storage")?.unwrap_or(defaults.storage);
        let browser_resource =
            optional_string(&view, "browser-resource")?.unwrap_or(defaults.browser_resource);
        if storage.is_empty() {
            return Err("storage must not be empty".to_owned());
        }
        if browser_resource.is_empty() {
            return Err("browser-resource must not be empty".to_owned());
        }
        let bridge_thread = optional_positive_u64(&view, "bridge-thread")?;
        let web_addr = optional_string(&view, "web-addr")?.unwrap_or(defaults.web_addr);
        let atelier_root = optional_string(&view, "atelier-root")
            .map(|value| value.map(PathBuf::from))?
            .unwrap_or(defaults.atelier_root);
        let dry_run = optional_bool(&view, "dry-run")?.unwrap_or(defaults.dry_run);
        Ok(Self {
            placement,
            storage,
            browser_resource,
            bridge_thread,
            web_addr,
            atelier_root,
            dry_run,
        })
    }
}

/// Stable runtime config table id for the serve recipe.
pub fn serve_config_symbol() -> Symbol {
    Symbol::qualified("lib", "expr-tree-serve")
}

fn optional_string(view: &ConfigView<'_>, key: &str) -> Result<Option<String>, String> {
    match view.get(key) {
        None => Ok(None),
        Some(_) => view
            .string(key)
            .map(|value| Some(value.to_owned()))
            .ok_or_else(|| format!("{key} must be a string")),
    }
}

fn required_string(view: &ConfigView<'_>, key: &str) -> Result<String, String> {
    optional_string(view, key)?.ok_or_else(|| format!("{key} is required"))
}

fn optional_bool(view: &ConfigView<'_>, key: &str) -> Result<Option<bool>, String> {
    match view.get(key) {
        None => Ok(None),
        Some(_) => view
            .bool(key)
            .map(Some)
            .ok_or_else(|| format!("{key} must be a boolean")),
    }
}

fn optional_positive_u64(view: &ConfigView<'_>, key: &str) -> Result<Option<u64>, String> {
    match view.get(key) {
        None => Ok(None),
        Some(_) => {
            let value = view
                .i64(key)
                .ok_or_else(|| format!("{key} must be a positive integer"))?;
            u64::try_from(value)
                .ok()
                .filter(|value| *value > 0)
                .map(Some)
                .ok_or_else(|| format!("{key} must be a positive integer"))
        }
    }
}
