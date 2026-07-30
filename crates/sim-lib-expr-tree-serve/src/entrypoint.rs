//! Loadable `cli/main/expr-tree` entrypoint.

use std::sync::Arc;

use sim_kernel::{
    AbiVersion, Args, Callable, Cx, Dependency, Export, Lib, LibManifest, LibTarget, Linker,
    LoadCx, Object, ObjectCompat, Result, Symbol, Value, Version,
};
use sim_run_core::{RuntimeConfigState, cli_envelope_args, cli_main_entrypoint_symbol};

use crate::{EXPR_TREE_VERB, ExpressionTreeRecipe, ExpressionTreeServeConfig, serve_config_symbol};

/// Returns the function symbol exported for the bootloader handoff.
pub fn expr_tree_entrypoint_symbol() -> Symbol {
    cli_main_entrypoint_symbol(EXPR_TREE_VERB)
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
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let Some(envelope) = args.values().first() else {
            return Err(sim_kernel::Error::Eval(
                "missing expression-tree envelope".to_owned(),
            ));
        };
        let args = cli_envelope_args(cx, envelope)?;
        if parse_product_args(&args)? == ProductCommand::Help {
            print!("{EXPR_TREE_HELP}");
            return cx.factory().bool(true);
        }
        let config = self
            .config
            .clone()
            .map_err(|error| sim_kernel::Error::Eval(format!("expression-tree config: {error}")))?;
        ExpressionTreeRecipe::new(config).start(cx)?.serve(cx)?;
        cx.factory().bool(true)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProductCommand {
    Serve,
    Help,
}

fn parse_product_args(args: &[String]) -> Result<ProductCommand> {
    let args = if args.first().is_some_and(|arg| arg == EXPR_TREE_VERB) {
        &args[1..]
    } else {
        args
    };
    match args {
        [] => Ok(ProductCommand::Serve),
        [arg] if arg == "--help" || arg == "-h" => Ok(ProductCommand::Help),
        [arg, ..] => Err(sim_kernel::Error::Eval(format!(
            "unknown expression-tree argument: {arg}"
        ))),
    }
}

const EXPR_TREE_HELP: &str = "\
Usage: sim [BOOT OPTIONS] expr-tree
       sim --config-file PATH expr-tree

Options:
  -h, --help

Product settings are read from the lib/expr-tree-serve runtime config table.
";
