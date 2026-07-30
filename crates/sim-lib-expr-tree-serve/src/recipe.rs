//! Composition and lifecycle for the expression-tree product recipe.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

use sim_kernel::{
    Consistency, Cx, Error, EvalFabric, EvalMode, EvalReply, EvalRequest, Expr, Result, Symbol,
    Value,
};
use sim_lib_expr_tree_server::{
    ExpressionTreeServer, ExpressionTreeServerLib, ExpressionTreeServerLimits,
    ExpressionTreeWebSurfaceFactory,
};
use sim_lib_server::{
    EvalSite, FrameKind, LoopbackTransportEndpoint, ServerAddress, ServerFrame, SystemWallClock,
    eval_request_from_frame, register_loopback_transport_endpoint, server_frame_from_reply,
};
use sim_web_shell::{ServeConfig, serve_with_surface_factory};

use crate::{ExpressionTreeServeConfig, ServerPlacement};

static NEXT_BRIDGE_THREAD: AtomicU64 = AtomicU64::new(10_000);

/// Default product recipe over loadable engine, view, server, and web-host parts.
#[derive(Clone, Debug)]
pub struct ExpressionTreeRecipe {
    config: ExpressionTreeServeConfig,
}

impl ExpressionTreeRecipe {
    /// Creates a recipe from checked runtime configuration.
    pub fn new(config: ExpressionTreeServeConfig) -> Self {
        Self { config }
    }

    /// Composes the configured product inside the bootloader-provided context.
    pub fn start(self, cx: &mut Cx) -> Result<ExpressionTreeProduct> {
        sim_lib_server::install_server_lib(cx)?;
        sim_lib_expr_tree::install_expr_tree_lib(cx)?;

        let bridge_thread = self
            .config
            .bridge_thread
            .unwrap_or_else(|| NEXT_BRIDGE_THREAD.fetch_add(1, Ordering::Relaxed));
        let bridge_address = ServerAddress::InProcess {
            thread: bridge_thread,
        };
        let (fabric, local_server): (Arc<dyn EvalFabric>, Option<Arc<ExpressionTreeServer>>) =
            match &self.config.placement {
                ServerPlacement::InProcess => {
                    let server = Arc::new(
                        ExpressionTreeServer::new(
                            bridge_address.clone(),
                            vec![Symbol::qualified("codec", "lisp")],
                            Arc::new(SystemWallClock),
                            ExpressionTreeServerLimits::default(),
                        )
                        .map_err(Error::from)?,
                    );
                    cx.load_lib(&ExpressionTreeServerLib::new(server.clone()))?;
                    (server.clone(), Some(server))
                }
                ServerPlacement::External { site } => {
                    let value = cx.registry().site_by_symbol(site).cloned().ok_or_else(|| {
                        Error::Eval(format!(
                            "configured expression-tree EvalFabric site {site} is not loaded"
                        ))
                    })?;
                    if value.object().as_eval_fabric().is_none() {
                        return Err(Error::TypeMismatch {
                            expected: "eval fabric site",
                            found: "non-fabric site",
                        });
                    }
                    (Arc::new(LoadedFabric { value }), None)
                }
            };

        let bridge_site = Arc::new(ProductFabricSite::new(
            bridge_address.clone(),
            vec![Symbol::qualified("codec", "lisp")],
            fabric.clone(),
        ));
        let endpoint = register_loopback_transport_endpoint(bridge_address.clone(), bridge_site)?;
        let resource = create_session(fabric.as_ref(), cx, &self.config.storage)?;
        let seed = Arc::new(Mutex::new(cx.fork_from_seed()));

        Ok(ExpressionTreeProduct {
            config: self.config,
            bridge_address,
            resource,
            fabric,
            seed,
            endpoint: Some(endpoint),
            local_server,
            shutdown: false,
        })
    }
}

/// Running expression-tree product composition.
pub struct ExpressionTreeProduct {
    config: ExpressionTreeServeConfig,
    bridge_address: ServerAddress,
    resource: Symbol,
    fabric: Arc<dyn EvalFabric>,
    seed: Arc<Mutex<Cx>>,
    endpoint: Option<LoopbackTransportEndpoint>,
    local_server: Option<Arc<ExpressionTreeServer>>,
    shutdown: bool,
}

impl ExpressionTreeProduct {
    /// Returns the authoritative session resource opened from configured storage.
    pub fn resource(&self) -> &Symbol {
        &self.resource
    }

    /// Returns whether this recipe owns the authoritative server in-process.
    pub fn owns_server(&self) -> bool {
        self.local_server.is_some()
    }

    /// Returns the effective checked recipe config.
    pub fn config(&self) -> &ExpressionTreeServeConfig {
        &self.config
    }

    /// Builds the injected generic-web-host surface factory.
    pub fn surface_factory(&self) -> ExpressionTreeWebSurfaceFactory {
        let seed = self.seed.clone();
        ExpressionTreeWebSurfaceFactory::new(
            format!("in-process:{}", bridge_thread(&self.bridge_address)),
            self.bridge_address.clone(),
            self.config.browser_resource.clone(),
            self.resource.clone(),
            move || {
                seed.lock()
                    .map_err(|_| Error::PoisonedLock("expression-tree browser context seed"))
                    .map(|seed| seed.fork_from_seed())
            },
        )
    }

    /// Runs the generic web host and closes the authoritative session on exit.
    pub fn serve(mut self, cx: &mut Cx) -> Result<()> {
        let web = ServeConfig {
            addr: self.config.web_addr.clone(),
            atelier_root: self.config.atelier_root.clone(),
            dry_run: self.config.dry_run,
            cookbook: None,
        };
        let serve_result = serve_with_surface_factory(cx, &web, Box::new(self.surface_factory()))
            .map_err(|error| Error::HostError(format!("expression-tree web host: {error}")));
        let shutdown_result = self.shutdown(cx);
        match (serve_result, shutdown_result) {
            (Err(error), _) => Err(error),
            (Ok(()), result) => result,
        }
    }

    /// Closes the authoritative session and removes the in-process bridge.
    pub fn shutdown(&mut self, cx: &mut Cx) -> Result<()> {
        if self.shutdown {
            return Ok(());
        }
        close_session(self.fabric.as_ref(), cx, &self.resource)?;
        self.endpoint.take();
        self.shutdown = true;
        Ok(())
    }

    /// Returns whether graceful shutdown completed.
    pub const fn is_shutdown(&self) -> bool {
        self.shutdown
    }
}

impl Drop for ExpressionTreeProduct {
    fn drop(&mut self) {
        self.endpoint.take();
    }
}

#[derive(Clone)]
struct LoadedFabric {
    value: Value,
}

impl EvalFabric for LoadedFabric {
    fn realize(&self, cx: &mut Cx, request: EvalRequest) -> Result<EvalReply> {
        self.value
            .object()
            .as_eval_fabric()
            .ok_or(Error::TypeMismatch {
                expected: "eval fabric site",
                found: "non-fabric site",
            })?
            .realize(cx, request)
    }
}

struct ProductFabricSite {
    address: ServerAddress,
    codecs: Vec<Symbol>,
    fabric: Arc<dyn EvalFabric>,
}

impl ProductFabricSite {
    fn new(address: ServerAddress, codecs: Vec<Symbol>, fabric: Arc<dyn EvalFabric>) -> Self {
        Self {
            address,
            codecs,
            fabric,
        }
    }
}

impl EvalSite for ProductFabricSite {
    fn site_kind(&self) -> &'static str {
        "expression-tree-product"
    }

    fn address(&self) -> &ServerAddress {
        &self.address
    }

    fn codecs(&self) -> &[Symbol] {
        &self.codecs
    }

    fn answer(&self, cx: &mut Cx, frame: ServerFrame) -> Result<ServerFrame> {
        if frame.kind != FrameKind::Request {
            return Err(Error::Eval(format!(
                "expression-tree product site cannot answer {}",
                frame.kind.as_symbol()
            )));
        }
        let correlation = frame.msg_id;
        let consistency = frame.envelope.consistency;
        let reply_codec = frame
            .envelope
            .reply_codec_hint
            .clone()
            .filter(|codec| self.codecs.contains(codec))
            .unwrap_or_else(|| frame.codec.clone());
        let request = eval_request_from_frame(cx, &frame)?;
        let reply = self.fabric.realize(cx, request)?;
        let mut reply = server_frame_from_reply(cx, &reply_codec, reply, consistency)?;
        reply.correlate = correlation;
        Ok(reply)
    }

    fn as_eval_fabric(&self) -> Option<&dyn EvalFabric> {
        Some(self.fabric.as_ref())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn create_session(fabric: &dyn EvalFabric, cx: &mut Cx, storage: &str) -> Result<Symbol> {
    let reply = fabric.realize(
        cx,
        request(Expr::Map(vec![
            (
                Expr::Symbol(Symbol::new("op")),
                Expr::Symbol(Symbol::qualified("expr-tree-server", "create")),
            ),
            (
                Expr::Symbol(Symbol::new("storage")),
                Expr::String(storage.to_owned()),
            ),
        ])),
    )?;
    match reply.value.object().as_expr(cx)? {
        Expr::Symbol(resource) => Ok(resource),
        error => Err(remote_error("create session", &error)),
    }
}

fn close_session(fabric: &dyn EvalFabric, cx: &mut Cx, resource: &Symbol) -> Result<()> {
    let reply = fabric.realize(
        cx,
        request(Expr::Map(vec![
            (
                Expr::Symbol(Symbol::new("op")),
                Expr::Symbol(Symbol::qualified("expr-tree-server", "close")),
            ),
            (
                Expr::Symbol(Symbol::new("resource")),
                Expr::Symbol(resource.clone()),
            ),
        ])),
    )?;
    match reply.value.object().as_expr(cx)? {
        Expr::Bool(true) => Ok(()),
        error => Err(remote_error("close session", &error)),
    }
}

fn request(expr: Expr) -> EvalRequest {
    EvalRequest {
        expr,
        result_shape: None,
        required_capabilities: Vec::new(),
        deadline: None,
        consistency: Consistency::LocalFirst,
        mode: EvalMode::Eval,
        answer_limit: None,
        stream_buffer: None,
        stream: false,
        trace: false,
    }
}

fn remote_error(action: &str, expr: &Expr) -> Error {
    Error::HostError(format!("expression-tree {action} failed: {expr:?}"))
}

fn bridge_thread(address: &ServerAddress) -> u64 {
    match address {
        ServerAddress::InProcess { thread } => *thread,
        _ => unreachable!("the product bridge is always in-process"),
    }
}
