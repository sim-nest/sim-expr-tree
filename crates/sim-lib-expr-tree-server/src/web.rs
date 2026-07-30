//! Product composition for the generic SIM browser shell.

use std::sync::Arc;

use sim_kernel::{Cx, Error, Expr, Result, Symbol};
use sim_lib_server::ServerAddress;
use sim_lib_view::{LensRegistry, SurfaceCaps, surface};
use sim_lib_view_expr_tree::{
    expression_tree_surface_codec_symbol, register_expression_tree_surface_codec,
};
use sim_lib_web_bridge::{RemoteTransport, SceneUpdate, Session};
use sim_web_shell::{LiveSurface, LiveSurfaceFactory};

type ContextFactory = dyn Fn() -> Result<Cx> + Send + Sync;

/// One browser-owned expression-tree surface over a real server transport.
///
/// The surface composes the product codec with the generic session bus. It owns
/// no HTTP routes, JavaScript, Scene interpretation, or tree semantics.
pub struct ExpressionTreeWebSurface {
    session: Session<RemoteTransport>,
    registry: LensRegistry,
    cx: Cx,
    caps: SurfaceCaps,
    browser_resource: String,
    authoritative_resource: Symbol,
}

impl ExpressionTreeWebSurface {
    fn connect(
        endpoint: String,
        address: ServerAddress,
        offered_codecs: Vec<Symbol>,
        caps: SurfaceCaps,
        browser_resource: String,
        authoritative_resource: Symbol,
        mut cx: Cx,
    ) -> Result<Self> {
        let mut transport = RemoteTransport::local_server_address(endpoint, address)
            .with_offered_codecs(offered_codecs);
        transport.connect(&mut cx)?;
        let mut registry = LensRegistry::new();
        register_expression_tree_surface_codec(&mut registry);
        Ok(Self {
            session: Session::new(transport),
            registry,
            cx,
            caps,
            browser_resource,
            authoritative_resource,
        })
    }

    fn checked_resource(&self, requested: &str) -> Result<Symbol> {
        if requested == self.browser_resource {
            Ok(self.authoritative_resource.clone())
        } else {
            Err(Error::HostError(format!(
                "browser resource {requested:?} is outside this expression-tree surface"
            )))
        }
    }
}

impl LiveSurface for ExpressionTreeWebSurface {
    fn open(&mut self, resource: &str, pane: &str) -> Result<Expr> {
        let resource = self.checked_resource(resource)?;
        self.session.open_codec(
            &mut self.cx,
            &self.registry,
            Symbol::new(pane),
            resource,
            expression_tree_surface_codec_symbol(),
            self.caps.clone(),
        )
    }

    fn submit(&mut self, pane: &str, intent: &Expr) -> Result<Vec<SceneUpdate>> {
        self.session.submit_intent_at_rendered_revision(
            &mut self.cx,
            &self.registry,
            &Symbol::new(pane),
            intent,
        )?;
        self.session.pump(&mut self.cx, &self.registry)
    }
}

/// Builds isolated browser surfaces for one authoritative expression-tree
/// resource.
///
/// Each call to [`LiveSurfaceFactory::create`] obtains a fresh caller context
/// from `context_factory`, negotiates a fresh [`RemoteTransport`], and owns a
/// separate reversible session. The opaque browser alias is mapped to exactly
/// one server resource, so callers cannot select another tree by editing a URL.
pub struct ExpressionTreeWebSurfaceFactory {
    endpoint: String,
    address: ServerAddress,
    offered_codecs: Vec<Symbol>,
    caps: SurfaceCaps,
    browser_resource: String,
    authoritative_resource: Symbol,
    context_factory: Arc<ContextFactory>,
}

impl ExpressionTreeWebSurfaceFactory {
    /// Creates a factory using the `webui` surface capability preset.
    pub fn new(
        endpoint: impl Into<String>,
        address: ServerAddress,
        browser_resource: impl Into<String>,
        authoritative_resource: Symbol,
        context_factory: impl Fn() -> Result<Cx> + Send + Sync + 'static,
    ) -> Self {
        Self {
            endpoint: endpoint.into(),
            address,
            offered_codecs: vec![Symbol::qualified("codec", "lisp")],
            caps: surface::preset("webui").expect("webui is a known surface preset"),
            browser_resource: browser_resource.into(),
            authoritative_resource,
            context_factory: Arc::new(context_factory),
        }
    }

    /// Selects the server frame codecs offered by every new remote transport.
    pub fn with_offered_codecs(mut self, offered_codecs: Vec<Symbol>) -> Self {
        self.offered_codecs = offered_codecs;
        self
    }

    /// Selects open surface capabilities for every new browser projection.
    pub fn with_surface_caps(mut self, caps: SurfaceCaps) -> Self {
        self.caps = caps;
        self
    }
}

impl LiveSurfaceFactory for ExpressionTreeWebSurfaceFactory {
    fn create(&self) -> Result<Box<dyn LiveSurface>> {
        let cx = (self.context_factory)()?;
        ExpressionTreeWebSurface::connect(
            self.endpoint.clone(),
            self.address.clone(),
            self.offered_codecs.clone(),
            self.caps.clone(),
            self.browser_resource.clone(),
            self.authoritative_resource.clone(),
            cx,
        )
        .map(|surface| Box::new(surface) as Box<dyn LiveSurface>)
    }
}
