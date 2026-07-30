use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use sim_kernel::{
    CapabilityName, Consistency, Cx, EvalFabric, EvalMode, EvalRequest, Expr, Symbol,
};
use sim_lib_server::{ServerAddress, WallClock, WallTimestamp};
use sim_value::{access, build};

use crate::{ExpressionTreeServer, ExpressionTreeServerLimits, SessionId};

pub(crate) fn full_cx() -> Cx {
    runtime_cx(&[
        sim_lib_expr_tree::expr_tree_read_capability(),
        sim_lib_expr_tree::expr_tree_write_capability(),
        sim_lib_expr_tree::expr_tree_calculate_capability(),
        sim_lib_expr_tree::expr_tree_mount_capability(),
    ])
}

pub(crate) fn read_cx() -> Cx {
    runtime_cx(&[sim_lib_expr_tree::expr_tree_read_capability()])
}

fn runtime_cx(capabilities: &[CapabilityName]) -> Cx {
    let mut cx = sim_kernel::testing::eager_cx();
    let codec_id = cx.registry_mut().fresh_codec_id();
    cx.load_lib(&sim_codec_lisp::LispCodecLib::new(codec_id).unwrap())
        .unwrap();
    for capability in capabilities {
        cx.grant(capability.clone());
    }
    sim_lib_expr_tree::install_expr_tree_lib(&mut cx).unwrap();
    cx
}

pub(crate) fn request(expr: Expr) -> EvalRequest {
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

pub(crate) fn realize_expr(server: &ExpressionTreeServer, cx: &mut Cx, expr: Expr) -> Expr {
    server
        .realize(cx, request(expr))
        .unwrap()
        .value
        .object()
        .as_expr(cx)
        .unwrap()
}

pub(crate) fn runtime_call(session: &SessionId, name: &str, args: Vec<Expr>) -> Expr {
    let mut all = vec![Expr::Symbol(session.resource())];
    all.extend(args);
    Expr::Call {
        operator: Box::new(Expr::Symbol(Symbol::qualified("expr-tree", name))),
        args: all,
    }
}

pub(crate) fn web_commit(
    server: &ExpressionTreeServer,
    cx: &mut Cx,
    session: &SessionId,
    operation: Expr,
    expected: Expr,
) -> Expr {
    realize_expr(
        server,
        cx,
        build::map(vec![
            (
                "op",
                Expr::Symbol(Symbol::qualified("web-session", "commit")),
            ),
            ("resource", Expr::Symbol(session.resource())),
            ("operation", operation),
            ("expected-current", expected),
        ]),
    )
}

pub(crate) fn error_code(expr: &Expr) -> Option<String> {
    access::field_sym(expr, "error").map(|symbol| symbol.name.to_string())
}

pub(crate) fn snapshot_revision(expr: &Expr) -> u64 {
    match access::field(expr, "revision").unwrap() {
        Expr::Number(number) => number.canonical.parse().unwrap(),
        other => panic!("unexpected revision {other:?}"),
    }
}

pub(crate) fn target(session: &SessionId, revision: u64, path: &str) -> Expr {
    build::map(vec![
        ("tree", Expr::Symbol(session.resource())),
        ("revision", build::uint(revision)),
        ("path", build::text(path)),
    ])
}

pub(crate) fn server_with(
    limits: ExpressionTreeServerLimits,
    clock: Arc<dyn WallClock>,
) -> ExpressionTreeServer {
    ExpressionTreeServer::new(
        ServerAddress::Local,
        vec![Symbol::qualified("codec", "lisp")],
        clock,
        limits,
    )
    .unwrap()
}

pub(crate) struct ScriptClock {
    values: Mutex<VecDeque<u64>>,
}

impl ScriptClock {
    pub(crate) fn new(values: impl IntoIterator<Item = u64>) -> Self {
        Self {
            values: Mutex::new(values.into_iter().collect()),
        }
    }
}

impl WallClock for ScriptClock {
    fn now(&self) -> sim_kernel::Result<WallTimestamp> {
        self.values
            .lock()
            .unwrap()
            .pop_front()
            .map(WallTimestamp::from_unix_millis)
            .ok_or_else(|| sim_kernel::Error::HostError("clock exhausted".to_owned()))
    }
}
