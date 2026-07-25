#![forbid(unsafe_code)]
//! Serve entrypoint scaffold for expression trees.

use std::{ffi::OsString, io::Write, sync::Arc};

use sim_kernel::{
    AbiVersion, Args, Callable, CodecId, Cx, Error, Export, Lib, LibManifest, LibTarget, Linker,
    LoadCx, Object, ObjectCompat, Result, Symbol, Value, Version,
};
use sim_run_core::{Bootloader, cli_main_entrypoint_symbol};

/// Bootloader verb served by the expression-tree binary.
pub const EXPR_TREE_VERB: &str = "expr-tree";

/// Host library id registered for the expression-tree verb.
pub const EXPR_TREE_HOST_LIB: &str = "lib/expr-tree";

/// Returns the crate's public scaffold identity.
pub fn crate_identity() -> &'static str {
    "sim-lib-expr-tree-serve"
}

/// Returns the server library identity used by this scaffold dependency.
pub fn server_identity() -> &'static str {
    sim_lib_expr_tree_server::crate_identity()
}

/// Returns the function symbol exported for the bootloader handoff.
pub fn expr_tree_entrypoint_symbol() -> Symbol {
    cli_main_entrypoint_symbol(EXPR_TREE_VERB)
}

/// Builds the Bootloader used by the `sim-expr-tree` binary.
pub fn expr_tree_bootloader() -> Bootloader {
    Bootloader::standard()
        .host_lib("codec/lisp", lisp_boot_codec)
        .host_verb(EXPR_TREE_VERB, EXPR_TREE_HOST_LIB, || {
            Box::new(ExprTreeCliLib::new())
        })
}

/// Normalizes process args so `sim-expr-tree ARGS...` boots the expression-tree verb.
pub fn expr_tree_boot_args<I, S>(args: I) -> Vec<OsString>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let mut boot_args = vec![
        OsString::from("sim-expr-tree"),
        OsString::from("--codec"),
        OsString::from("lisp"),
        OsString::from(EXPR_TREE_VERB),
    ];
    boot_args.extend(args.into_iter().map(Into::into).skip(1));
    boot_args
}

fn lisp_boot_codec() -> Box<dyn Lib> {
    Box::new(sim_codec_lisp::LispCodecLib::new(CodecId(1)).expect("lisp boot codec"))
}

/// Loadable expression-tree command library.
#[derive(Clone, Debug, Default)]
pub struct ExprTreeCliLib;

impl ExprTreeCliLib {
    /// Creates an expression-tree command library instance.
    pub fn new() -> Self {
        Self
    }
}

impl Lib for ExprTreeCliLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: Symbol::qualified("lib", "expr-tree"),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
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
            cx.factory().opaque(Arc::new(ExprTreeCliEntrypoint))?,
        )?;
        Ok(())
    }
}

#[derive(Clone)]
struct ExprTreeCliEntrypoint;

impl Object for ExprTreeCliEntrypoint {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("cli/main/expr-tree".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for ExprTreeCliEntrypoint {
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for ExprTreeCliEntrypoint {
    fn call(&self, cx: &mut Cx, _args: Args) -> Result<Value> {
        writeln!(std::io::stdout(), "{}", crate_identity())
            .map_err(|err| Error::Eval(format!("write stdout: {err}")))?;
        cx.factory().bool(true)
    }
}

#[cfg(test)]
mod tests {
    use super::{EXPR_TREE_VERB, expr_tree_boot_args};

    #[test]
    fn identity_names_the_serve_library() {
        assert_eq!(super::crate_identity(), "sim-lib-expr-tree-serve");
        assert_eq!(super::server_identity(), "sim-lib-expr-tree-server");
    }

    #[test]
    fn boot_args_inject_the_expr_tree_verb() {
        let args = expr_tree_boot_args(["sim-expr-tree", "--help"]);

        assert_eq!(args[1], "--codec");
        assert_eq!(args[2], "lisp");
        assert_eq!(args[3], EXPR_TREE_VERB);
        assert_eq!(args[4], "--help");
    }
}
