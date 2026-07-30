//! Standard `EvalSite`, `EvalFabric`, object, and loadable library surfaces.

use std::{any::Any, sync::Arc, time::Duration};

use sim_kernel::{
    AbiVersion, ClassRef, Cx, DefaultFactory, Dependency, Error, EvalFabric, EvalReply,
    EvalRequest, Export, Factory, Lib, LibManifest, LibTarget, Linker, Object, ObjectCompat,
    Result, Symbol, Value, Version,
};
use sim_lib_server::{
    EvalSite, FrameKind, ServerAddress, ServerFrame, eval_request_from_frame,
    server_frame_from_reply,
};

use crate::ExpressionTreeServer;

/// Stable library symbol for the expression-tree server.
pub fn expr_tree_server_lib_symbol() -> Symbol {
    Symbol::qualified("lib", "expr-tree-server")
}

/// Stable placement symbol exported by the expression-tree server library.
pub fn expr_tree_server_site_symbol() -> Symbol {
    Symbol::new("site/sim-lib-expr-tree-server")
}

impl EvalFabric for ExpressionTreeServer {
    fn realize(&self, cx: &mut Cx, request: EvalRequest) -> Result<EvalReply> {
        self.realize_request(cx, request)
    }
}

impl EvalSite for ExpressionTreeServer {
    fn site_kind(&self) -> &'static str {
        "expression-tree"
    }

    fn address(&self) -> &ServerAddress {
        self.address()
    }

    fn codecs(&self) -> &[Symbol] {
        self.codecs()
    }

    fn answer(&self, cx: &mut Cx, frame: ServerFrame) -> Result<ServerFrame> {
        if frame.kind != FrameKind::Request {
            return Err(Error::Eval(format!(
                "expression-tree site cannot answer frame kind {}",
                frame.kind.as_symbol()
            )));
        }
        let correlation = frame.msg_id;
        let consistency = frame.envelope.consistency;
        let reply_codec = frame
            .envelope
            .reply_codec_hint
            .clone()
            .filter(|codec| self.codecs().contains(codec))
            .unwrap_or_else(|| frame.codec.clone());
        let request = eval_request_from_frame(cx, &frame)?;
        let reply = self.realize(cx, request)?;
        let mut frame = server_frame_from_reply(cx, &reply_codec, reply, consistency)?;
        frame.correlate = correlation;
        Ok(frame)
    }

    fn answer_with_timeout(
        &self,
        cx: &mut Cx,
        frame: ServerFrame,
        timeout: Option<Duration>,
    ) -> Result<ServerFrame> {
        if timeout.is_some_and(|timeout| timeout.is_zero()) {
            return Err(Error::HostError(
                "expression-tree server request deadline elapsed".to_owned(),
            ));
        }
        self.answer(cx, frame)
    }

    fn as_eval_fabric(&self) -> Option<&dyn EvalFabric> {
        Some(self)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Object for ExpressionTreeServer {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!(
            "#<expression-tree-server {}>",
            expr_tree_server_site_symbol()
        ))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ObjectCompat for ExpressionTreeServer {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        if let Some(class) = cx
            .registry()
            .class_by_symbol(&Symbol::qualified("core", "EvalFabric"))
        {
            return Ok(class.clone());
        }
        DefaultFactory.class_stub(
            sim_kernel::CORE_EVAL_REQUEST_CLASS_ID,
            Symbol::qualified("core", "EvalFabric"),
        )
    }

    fn as_eval_fabric(&self) -> Option<&dyn EvalFabric> {
        Some(self)
    }

    fn as_table(&self, cx: &mut Cx) -> Result<Value> {
        cx.factory().table(vec![
            (
                Symbol::new("site"),
                cx.factory().symbol(expr_tree_server_site_symbol())?,
            ),
            (
                Symbol::new("max-sessions"),
                cx.factory()
                    .string(self.limits().max_sessions.to_string())?,
            ),
            (
                Symbol::new("max-idle-ticks"),
                cx.factory()
                    .string(self.limits().max_idle_ticks.to_string())?,
            ),
        ])
    }
}

/// Loadable library exporting the authoritative expression-tree site.
#[derive(Clone, Copy, Debug, Default)]
pub struct ExpressionTreeServerLib;

impl Lib for ExpressionTreeServerLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: expr_tree_server_lib_symbol(),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: vec![Dependency {
                id: sim_lib_expr_tree::expr_tree_lib_symbol(),
                minimum_version: None,
            }],
            capabilities: Vec::new(),
            exports: vec![Export::Site {
                symbol: expr_tree_server_site_symbol(),
                runtime_id: None,
            }],
        }
    }

    fn load(&self, _cx: &mut sim_kernel::LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        linker.site_value(
            expr_tree_server_site_symbol(),
            DefaultFactory.opaque(Arc::new(ExpressionTreeServer::local()))?,
        )?;
        Ok(())
    }
}

/// Installs the expression-tree server library exactly once.
pub fn install_expr_tree_server_lib(cx: &mut Cx) -> Result<()> {
    if cx.registry().lib(&expr_tree_server_lib_symbol()).is_none() {
        cx.load_lib(&ExpressionTreeServerLib)?;
    }
    Ok(())
}
