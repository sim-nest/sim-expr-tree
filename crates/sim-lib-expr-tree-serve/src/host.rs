//! Shared standalone and standard-distribution host composition.

use std::{ffi::OsString, sync::Arc};

use sim_kernel::{CodecId, Lib};
use sim_run_core::{Bootloader, LibSourceSpec, LoadSession};

use crate::{ExprTreeServeLib, serve_config_symbol};

/// Bootloader verb served by the expression-tree product.
pub const EXPR_TREE_VERB: &str = "expr-tree";

/// Host library id registered for the expression-tree product.
pub const EXPR_TREE_HOST_LIB: &str = "lib/expr-tree-serve";

/// Installs the standard expression-tree product recipe into a boot session.
///
/// This is the one distribution composition used by both the standalone
/// `sim-expr-tree` executable and the standard `sim expr-tree` command. The
/// serve library continues to own its codec, capabilities, configuration
/// table, host factory, and entrypoint; callers only select when to install the
/// recipe.
pub fn configure_expr_tree_session(session: LoadSession) -> LoadSession {
    session
        .with_host_factory("codec/lisp", lisp_boot_codec)
        .with_context(|cx| cx.set_eval_policy(Arc::new(sim_kernel::EagerPolicy)))
        .with_capability(sim_lib_expr_tree::expr_tree_read_capability())
        .with_capability(sim_lib_expr_tree::expr_tree_write_capability())
        .with_capability(sim_lib_expr_tree::expr_tree_calculate_capability())
        .with_host_factory_with_config(EXPR_TREE_HOST_LIB, |state| {
            Box::new(ExprTreeServeLib::from_runtime_config(state))
        })
        .with_default_verb_sources(
            EXPR_TREE_VERB,
            vec![
                LibSourceSpec::Host("codec/lisp".to_owned()),
                LibSourceSpec::Host(EXPR_TREE_HOST_LIB.to_owned()),
            ],
        )
        .with_default_verb_config_libs(EXPR_TREE_VERB, vec![serve_config_symbol()])
}

/// Builds the standard Bootloader used by the `sim-expr-tree` binary.
///
/// The host grants the ordinary read, write, and calculation powers required
/// by the default editable product. Mounting and external network access remain
/// denied unless a different trusted host composition grants them.
pub fn expr_tree_bootloader() -> Bootloader {
    Bootloader::standard().configure_session(configure_expr_tree_session)
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
