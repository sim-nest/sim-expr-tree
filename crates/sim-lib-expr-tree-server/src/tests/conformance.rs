use std::sync::Arc;

use sim_kernel::{Expr, Symbol};
use sim_lib_intent::{Origin, intent};
use sim_lib_server::{EvalSite, ServerAddress, eval_reply_from_frame, server_frame_from_request};
use sim_value::build;

use crate::{
    ExpressionTreeServer, ExpressionTreeServerLib, expr_tree_server_site_symbol,
    install_expr_tree_server_lib,
};

use super::support::{
    error_code, full_cx, read_cx, realize_expr, request, runtime_call, snapshot_revision, target,
    web_commit,
};

#[test]
fn loadable_export_is_both_eval_site_and_eval_fabric() {
    let mut cx = full_cx();
    let manifest = sim_kernel::Lib::manifest(&ExpressionTreeServerLib);
    assert!(manifest.exports.iter().any(
        |export| matches!(export, sim_kernel::Export::Site { symbol, .. }
            if symbol == &expr_tree_server_site_symbol())
    ));
    install_expr_tree_server_lib(&mut cx).unwrap();
    let value = cx
        .registry()
        .site_by_symbol(&expr_tree_server_site_symbol())
        .unwrap();
    assert!(value.object().as_eval_fabric().is_some());
}

#[test]
fn two_clients_share_one_session_while_other_sessions_stay_isolated() {
    let server = ExpressionTreeServer::local();
    let mut creator = full_cx();
    let shared = server.create_session(&mut creator, "shared").unwrap();
    let isolated = server.create_session(&mut creator, "isolated").unwrap();

    let mut client_a = full_cx();
    let path = realize_expr(
        &server,
        &mut client_a,
        runtime_call(
            &shared,
            "new-cell",
            vec![
                Expr::String("/".to_owned()),
                Expr::String("answer".to_owned()),
                Expr::String("forty-two".to_owned()),
            ],
        ),
    );
    assert_eq!(path, Expr::String("/answer".to_owned()));

    let mut client_b = full_cx();
    assert_eq!(
        realize_expr(
            &server,
            &mut client_b,
            runtime_call(&shared, "ref", vec![Expr::String("/answer".to_owned())],),
        ),
        Expr::String("forty-two".to_owned())
    );
    let absent = realize_expr(
        &server,
        &mut client_b,
        runtime_call(&isolated, "ref", vec![Expr::String("/answer".to_owned())]),
    );
    assert_eq!(error_code(&absent).as_deref(), Some("operation-failed"));

    let mut reconnected = full_cx();
    assert_eq!(
        realize_expr(
            &server,
            &mut reconnected,
            runtime_call(&shared, "ref", vec![Expr::String("/answer".to_owned())],),
        ),
        Expr::String("forty-two".to_owned())
    );
}

#[test]
fn surface_intent_runs_through_existing_codec_and_optimistic_revision() {
    let server = ExpressionTreeServer::local();
    let mut cx = full_cx();
    let session = server.create_session(&mut cx, "surface").unwrap();
    let snapshot = server.snapshot(&session).unwrap();
    let revision = snapshot_revision(&snapshot);
    let create_dir = intent(
        "create",
        Origin::human(7),
        vec![
            ("class", build::sym("directory")),
            ("at", target(&session, revision, "/")),
            ("args", build::list(vec![build::text("work")])),
        ],
    );
    let updated = server
        .apply_intent(&mut cx, &session, revision, &create_dir)
        .unwrap();
    assert_eq!(snapshot_revision(&updated), revision + 1);

    let stale = server
        .apply_intent(&mut cx, &session, revision, &create_dir)
        .unwrap_err();
    assert_eq!(stale.code(), "stale-revision");
}

#[test]
fn caller_authority_is_diminished_and_errors_remain_structured() {
    let server = ExpressionTreeServer::local();
    let mut creator = full_cx();
    let session = server.create_session(&mut creator, "authority").unwrap();
    let expected = server.snapshot(&session).unwrap();
    let operation = runtime_call(
        &session,
        "new-dir",
        vec![
            Expr::String("/".to_owned()),
            Expr::String("denied".to_owned()),
        ],
    );
    let mut read_only = read_cx();
    let unchanged = expected.clone();
    let reply = web_commit(&server, &mut read_only, &session, operation, expected);
    assert_eq!(error_code(&reply).as_deref(), Some("authority-denied"));
    assert!(server.snapshot(&session).unwrap().canonical_eq(&unchanged));
}

#[test]
fn server_frames_preserve_correlation_and_use_existing_adapters() {
    let server = ExpressionTreeServer::new(
        ServerAddress::Local,
        vec![Symbol::qualified("codec", "lisp")],
        Arc::new(sim_lib_server::DeterministicWallClock::new(10, 1)),
        Default::default(),
    )
    .unwrap();
    let mut cx = full_cx();
    let mut frame = server_frame_from_request(
        &mut cx,
        &Symbol::qualified("codec", "lisp"),
        request(build::map(vec![
            (
                "op",
                Expr::Symbol(Symbol::qualified("expr-tree-server", "create")),
            ),
            ("storage", build::text("framed")),
        ])),
    )
    .unwrap();
    frame.msg_id = Some(91);
    let reply = server.answer(&mut cx, frame).unwrap();
    assert_eq!(reply.correlate, Some(91));
    let value = eval_reply_from_frame(&mut cx, &reply).unwrap().value;
    assert!(matches!(
        value.object().as_expr(&mut cx).unwrap(),
        Expr::Symbol(symbol)
            if symbol.namespace.as_deref() == Some("expr-tree/session")
    ));
    assert!(EvalSite::as_eval_fabric(&server).is_some());
}
