#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Bootloader-loaded expression-tree product recipe.
//!
//! The crate owns no expression-tree engine, view protocol, server protocol, or
//! browser runtime. [`ExpressionTreeRecipe`] composes the existing loadable
//! engine and server with the reversible expression-tree surface and generic
//! web host. [`expr_tree_bootloader`] supplies that recipe to the standard
//! `sim-run` boot path and dispatches [`expr_tree_entrypoint_symbol`].

mod config;
mod recipe;

use std::{ffi::OsString, sync::Arc};

use sim_kernel::{
    AbiVersion, Args, Callable, CodecId, Cx, Dependency, Export, Lib, LibManifest, LibTarget,
    Linker, LoadCx, Object, ObjectCompat, Result, Symbol, Value, Version,
};
use sim_run_core::{Bootloader, RuntimeConfigState, cli_main_entrypoint_symbol};

pub use config::{ExpressionTreeServeConfig, ServerPlacement, serve_config_symbol};
pub use recipe::{ExpressionTreeProduct, ExpressionTreeRecipe};

/// Bootloader verb served by the expression-tree product.
pub const EXPR_TREE_VERB: &str = "expr-tree";

/// Host library id registered for the expression-tree product.
pub const EXPR_TREE_HOST_LIB: &str = "lib/expr-tree-serve";

/// Returns the crate's public identity.
pub const fn crate_identity() -> &'static str {
    "sim-lib-expr-tree-serve"
}

/// Returns the loadable components composed by the default product recipe.
pub const fn component_identities() -> [&'static str; 4] {
    [
        "sim-lib-expr-tree",
        "sim-lib-view-expr-tree",
        "sim-lib-expr-tree-server",
        "sim-web-shell",
    ]
}

/// Returns the function symbol exported for the bootloader handoff.
pub fn expr_tree_entrypoint_symbol() -> Symbol {
    cli_main_entrypoint_symbol(EXPR_TREE_VERB)
}

/// Builds the standard Bootloader used by the `sim-expr-tree` binary.
///
/// The host grants the ordinary read, write, and calculation powers required
/// by the default editable product. Mounting and external network access remain
/// denied unless a different trusted host composition grants them.
pub fn expr_tree_bootloader() -> Bootloader {
    Bootloader::standard()
        .host_lib("codec/lisp", lisp_boot_codec)
        .with_context(|cx| cx.set_eval_policy(Arc::new(sim_kernel::EagerPolicy)))
        .with_capability(sim_lib_expr_tree::expr_tree_read_capability())
        .with_capability(sim_lib_expr_tree::expr_tree_write_capability())
        .with_capability(sim_lib_expr_tree::expr_tree_calculate_capability())
        .host_verb_with_config(
            EXPR_TREE_VERB,
            EXPR_TREE_HOST_LIB,
            vec![serve_config_symbol()],
            |state| Box::new(ExprTreeServeLib::from_runtime_config(state)),
        )
}

/// Appends the product verb after untouched standard bootloader arguments.
///
/// The product has no private CLI options: flags such as `--config-file` remain
/// owned by `sim-run`, and the trailing verb selects `cli/main/expr-tree` after
/// those controls have been parsed.
pub fn expr_tree_boot_args<I, S>(args: I) -> Vec<OsString>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let mut args = args.into_iter().map(Into::into).collect::<Vec<_>>();
    args.push(OsString::from(EXPR_TREE_VERB));
    args
}

fn lisp_boot_codec() -> Box<dyn Lib> {
    Box::new(sim_codec_lisp::LispCodecLib::new(CodecId(1)).expect("lisp boot codec"))
}

/// Loadable library exporting the expression-tree product entrypoint.
#[derive(Clone, Debug)]
pub struct ExprTreeServeLib {
    config: std::result::Result<ExpressionTreeServeConfig, String>,
}

impl ExprTreeServeLib {
    /// Builds the serve library from already-discovered runtime configuration.
    pub fn from_runtime_config(state: &RuntimeConfigState) -> Self {
        Self {
            config: ExpressionTreeServeConfig::from_runtime_config(state),
        }
    }

    /// Builds the serve library around an explicit checked recipe config.
    pub fn new(config: ExpressionTreeServeConfig) -> Self {
        Self { config: Ok(config) }
    }
}

impl Default for ExprTreeServeLib {
    fn default() -> Self {
        Self::new(ExpressionTreeServeConfig::default())
    }
}

impl Lib for ExprTreeServeLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: serve_config_symbol(),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: vec![Dependency {
                id: Symbol::qualified("codec", "lisp"),
                minimum_version: None,
            }],
            capabilities: Vec::new(),
            exports: vec![Export::Function {
                symbol: expr_tree_entrypoint_symbol(),
                function_id: None,
            }],
        }
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        linker.function_value(
            expr_tree_entrypoint_symbol(),
            cx.factory().opaque(Arc::new(ExprTreeEntrypoint {
                config: self.config.clone(),
            }))?,
        )?;
        Ok(())
    }
}

#[derive(Clone)]
struct ExprTreeEntrypoint {
    config: std::result::Result<ExpressionTreeServeConfig, String>,
}

impl Object for ExprTreeEntrypoint {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("cli/main/expr-tree".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for ExprTreeEntrypoint {
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for ExprTreeEntrypoint {
    fn call(&self, cx: &mut Cx, _args: Args) -> Result<Value> {
        let config = self
            .config
            .clone()
            .map_err(|error| sim_kernel::Error::Eval(format!("expression-tree config: {error}")))?;
        ExpressionTreeRecipe::new(config).start(cx)?.serve(cx)?;
        cx.factory().bool(true)
    }
}

#[cfg(test)]
mod tests;
