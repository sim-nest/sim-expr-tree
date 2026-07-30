use std::{fs, path::PathBuf, sync::Arc};

use sim_codec_json::{JsonProjectionMode, project_expr_to_json};
use sim_kernel::{Cx, Expr, Symbol};
use sim_lib_intent::{Origin, intent};
use sim_lib_server::{DeterministicWallClock, ServerAddress, register_loopback_transport_endpoint};
use sim_lib_view::{LensRegistry, surface};
use sim_lib_view_expr_tree::{
    expression_tree_surface_codec_symbol, register_expression_tree_surface_codec,
};
use sim_lib_web_bridge::{DesktopHost, PhoneHost, RemoteTransport, SessionStatus, Transport};
use sim_value::build;
use sim_web_shell::{LiveSessionTable, LiveSessionTableConfig};

use crate::{ExpressionTreeServer, ExpressionTreeWebSurfaceFactory, SessionId};

use super::support::{full_cx, read_cx, realize_expr, runtime_call};

const ADDRESS_THREAD: u64 = 17_025;
const STORAGE: &str = "recipe-server-backed-web-session";
const FIXTURE_ROOT: &str = "../../recipes/03-server/web-session/fixtures";

fn registry() -> LensRegistry {
    let mut registry = LensRegistry::new();
    register_expression_tree_surface_codec(&mut registry);
    registry
}

fn connect(cx: &mut Cx, address: &ServerAddress) -> RemoteTransport {
    let mut transport = RemoteTransport::local_server_address(
        format!("in-process:{ADDRESS_THREAD}"),
        address.clone(),
    )
    .with_offered_codecs(vec![Symbol::qualified("codec", "lisp")]);
    transport.connect(cx).unwrap();
    transport
}

fn target(session: &SessionId, revision: u64, path: &str) -> Expr {
    build::map(vec![
        ("tree", Expr::Symbol(session.resource())),
        ("revision", build::uint(revision)),
        ("path", build::text(path)),
    ])
}

fn create_dir(session: &SessionId, revision: u64, path: &str, name: &str) -> Expr {
    intent(
        "create",
        Origin::human(revision),
        vec![
            ("class", build::sym("directory")),
            ("at", target(session, revision, path)),
            ("args", build::list(vec![build::text(name)])),
        ],
    )
}

fn create_cell(session: &SessionId, revision: u64, path: &str, name: &str, source: &str) -> Expr {
    intent(
        "create",
        Origin::human(revision),
        vec![
            ("class", build::sym("cell")),
            ("at", target(session, revision, path)),
            (
                "args",
                build::list(vec![build::text(name), build::text(source)]),
            ),
        ],
    )
}

fn disclosure(session: &SessionId, revision: u64, path: &str, open: bool) -> Expr {
    intent(
        "tree-disclosure",
        Origin::human(revision),
        vec![
            ("target", target(session, revision, path)),
            ("open", Expr::Bool(open)),
        ],
    )
}

fn edit_source(session: &SessionId, revision: u64, path: &str, source: &str) -> Expr {
    intent(
        "edit-field",
        Origin::human(revision),
        vec![
            ("target", target(session, revision, path)),
            ("path", build::list(vec![build::text("source")])),
            ("value", build::text(source)),
        ],
    )
}

fn set_codec(session: &SessionId, revision: u64, path: &str, codec: &str) -> Expr {
    intent(
        "set-param",
        Origin::human(revision),
        vec![
            ("target", target(session, revision, path)),
            ("param", build::sym("codec-policy")),
            ("value", build::text(codec)),
        ],
    )
}

fn invoke(session: &SessionId, revision: u64, path: &str, operation: &str) -> Expr {
    intent(
        "invoke",
        Origin::human(revision),
        vec![
            ("target", target(session, revision, path)),
            ("op", build::sym(operation)),
            ("args", build::list(Vec::new())),
        ],
    )
}

fn submit(
    host: &mut DesktopHost<RemoteTransport>,
    cx: &mut Cx,
    registry: &LensRegistry,
    pane: &Symbol,
    submitted: Expr,
) -> Expr {
    let updates = host.submit(cx, registry, pane, submitted).unwrap();
    assert_eq!(
        updates.len(),
        1,
        "one authoritative change updates one pane"
    );
    updates[0].scene.clone()
}

fn scene_json(scene: &Expr) -> serde_json::Value {
    let mut value = project_expr_to_json(scene, JsonProjectionMode::UntaggedInterop);
    canonicalize_session_ids(&mut value);
    value
}

fn canonicalize_session_ids(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(text) => {
            if text.starts_with("expr-tree/session/") {
                *text = "expr-tree/session/fixture".to_owned();
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                canonicalize_session_ids(item);
            }
        }
        serde_json::Value::Object(entries) => {
            for value in entries.values_mut() {
                canonicalize_session_ids(value);
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(FIXTURE_ROOT)
        .join(name)
}

fn check_fixture(name: &str, profile: &str, title: &str, scene: &Expr) {
    let value = serde_json::json!({
        "schema": "sim.browser-scene-fixture/v1",
        "profile": profile,
        "title": title,
        "scene": scene_json(scene),
    });
    let rendered = format!("{}\n", serde_json::to_string_pretty(&value).unwrap());
    let path = fixture_path(name);
    if std::env::var_os("SIM_UPDATE_EXPR_TREE_WEB_FIXTURES").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &rendered).unwrap();
        eprintln!("updated {}", path.display());
    } else {
        assert_eq!(
            fs::read_to_string(&path).unwrap_or_else(|error| {
                panic!(
                    "read {}: {error}; regenerate with SIM_UPDATE_EXPR_TREE_WEB_FIXTURES=1",
                    path.display()
                )
            }),
            rendered,
            "{} drifted; regenerate with SIM_UPDATE_EXPR_TREE_WEB_FIXTURES=1",
            path.display()
        );
    }
}

#[test]
fn recipe_server_backed_web_session_runs_desktop_phone_and_failure_paths() {
    let address = ServerAddress::InProcess {
        thread: ADDRESS_THREAD,
    };
    let server = Arc::new(
        ExpressionTreeServer::new(
            address.clone(),
            vec![Symbol::qualified("codec", "lisp")],
            Arc::new(DeterministicWallClock::new(1_700_000_000_000, 10)),
            Default::default(),
        )
        .unwrap(),
    );
    let _endpoint = register_loopback_transport_endpoint(address.clone(), server.clone()).unwrap();

    let mut creator = full_cx();
    let session = server.create_session(&mut creator, STORAGE).unwrap();
    let isolated = server
        .create_session(&mut creator, "recipe-server-backed-isolated")
        .unwrap();
    let watch = server.subscribe(&session).unwrap();
    let registry = registry();
    let codec = expression_tree_surface_codec_symbol();

    let mut desktop_cx = full_cx();
    let desktop_transport = connect(&mut desktop_cx, &address);
    let mut desktop = DesktopHost::with_surface_codec(desktop_transport, codec.clone());
    let pane = Symbol::new("desktop:main");
    let initial = desktop
        .open_pane(&mut desktop_cx, &registry, pane.clone(), session.resource())
        .unwrap();
    sim_lib_scene::validate_scene(&initial).unwrap();
    assert_eq!(desktop.surface_codec(), &codec);

    let mut revision = server.revision(&session).unwrap();
    submit(
        &mut desktop,
        &mut desktop_cx,
        &registry,
        &pane,
        create_dir(&session, revision, "/", "model"),
    );
    revision = server.revision(&session).unwrap();
    submit(
        &mut desktop,
        &mut desktop_cx,
        &registry,
        &pane,
        disclosure(&session, revision, "/model", true),
    );
    revision = server.revision(&session).unwrap();
    submit(
        &mut desktop,
        &mut desktop_cx,
        &registry,
        &pane,
        create_cell(&session, revision, "/model", "answer", "1"),
    );
    revision = server.revision(&session).unwrap();
    submit(
        &mut desktop,
        &mut desktop_cx,
        &registry,
        &pane,
        set_codec(&session, revision, "/model/answer", "codec/lisp"),
    );
    revision = server.revision(&session).unwrap();
    submit(
        &mut desktop,
        &mut desktop_cx,
        &registry,
        &pane,
        disclosure(&session, revision, "/model/answer", true),
    );
    revision = server.revision(&session).unwrap();
    let automatic = submit(
        &mut desktop,
        &mut desktop_cx,
        &registry,
        &pane,
        edit_source(&session, revision, "/model/answer", "41"),
    );
    let automatic_json = scene_json(&automatic).to_string();
    assert!(
        automatic_json.contains("fresh") && automatic_json.contains("receipt #"),
        "automatic progress and its committed receipt are observable in the next Scene"
    );

    revision = server.revision(&session).unwrap();
    let _calculated = submit(
        &mut desktop,
        &mut desktop_cx,
        &registry,
        &pane,
        invoke(&session, revision, "/model/answer", "calculate"),
    );
    revision = server.revision(&session).unwrap();
    let explain_updates = desktop
        .submit(
            &mut desktop_cx,
            &registry,
            &pane,
            invoke(&session, revision, "/model/answer", "explain"),
        )
        .unwrap();
    assert!(
        explain_updates.is_empty(),
        "read-only explanation crosses the server without inventing a change"
    );
    assert_eq!(server.revision(&session).unwrap(), revision);

    let collapsed = submit(
        &mut desktop,
        &mut desktop_cx,
        &registry,
        &pane,
        disclosure(&session, revision, "/model", false),
    );
    let collapsed_json = scene_json(&collapsed).to_string();
    assert!(
        !collapsed_json.contains("41") && !collapsed_json.contains("receipt #"),
        "collapsed directories fetch and render neither descendant faces nor receipts"
    );
    revision = server.revision(&session).unwrap();
    submit(
        &mut desktop,
        &mut desktop_cx,
        &registry,
        &pane,
        disclosure(&session, revision, "/model", true),
    );
    revision = server.revision(&session).unwrap();
    let desktop_scene = submit(
        &mut desktop,
        &mut desktop_cx,
        &registry,
        &pane,
        disclosure(&session, revision, "/model/answer", true),
    );

    let watched = server.poll_watch(&session, &watch, 32).unwrap();
    assert!(
        watched.events.iter().any(|event| event.kind == "set-expr")
            && watched.events.iter().any(|event| event.kind == "calculate"),
        "server progress is observable through the bounded authoritative watch"
    );

    let mut phone_cx = full_cx();
    let phone_transport = connect(&mut phone_cx, &address);
    let mut phone = PhoneHost::with_surface_codec(phone_transport, codec.clone());
    let phone_scene = phone
        .open(&mut phone_cx, &registry, session.resource())
        .unwrap();
    sim_lib_scene::validate_scene(&phone_scene).unwrap();
    assert_eq!(phone.surface_codec(), &codec);
    assert_ne!(
        scene_json(&desktop_scene),
        scene_json(&phone_scene),
        "desktop and phone are distinct projections of one authoritative snapshot"
    );

    let stale_revision = server.revision(&session).unwrap();
    let updated = submit(
        &mut desktop,
        &mut desktop_cx,
        &registry,
        &pane,
        edit_source(&session, stale_revision, "/model/answer", "42"),
    );
    let stale = phone
        .submit(
            &mut phone_cx,
            &registry,
            edit_source(&session, stale_revision, "/model/answer", "stale"),
        )
        .unwrap_err();
    assert!(
        stale.to_string().contains("stale-revision"),
        "stale browser commits fail visibly: {stale}"
    );

    phone.transport_mut().disconnect();
    phone.transport_mut().begin_reconnect();
    phone.transport_mut().connect(&mut phone_cx).unwrap();
    assert_eq!(phone.transport_mut().status(), SessionStatus::Connected);
    let reconnected = phone
        .open(&mut phone_cx, &registry, session.resource())
        .unwrap();
    let reconnected_json = scene_json(&reconnected).to_string();
    assert!(
        reconnected_json.contains("42"),
        "reconnect refreshes the authoritative value: {reconnected_json}"
    );

    let web_factory = ExpressionTreeWebSurfaceFactory::new(
        format!("in-process:{ADDRESS_THREAD}"),
        address.clone(),
        "demo",
        session.resource(),
        || Ok(full_cx()),
    )
    .with_surface_caps(surface::preset("phone").unwrap());
    let mut browser_sessions = LiveSessionTable::with_config(
        Box::new(web_factory),
        LiveSessionTableConfig {
            capacity: 4,
            idle_ttl: std::time::Duration::from_secs(60),
        },
    );
    let (left_browser, left_scene) = browser_sessions.open(None, "demo", "main").unwrap();
    let (right_browser, right_scene) = browser_sessions.open(None, "demo", "main").unwrap();
    assert_ne!(
        left_browser, right_browser,
        "each browser receives an opaque isolated surface session"
    );
    assert_eq!(
        scene_json(&left_scene),
        scene_json(&right_scene),
        "isolated browser surfaces project the same authoritative tree"
    );
    assert!(
        browser_sessions
            .open(Some(&left_browser), "another-tree", "main")
            .unwrap_err()
            .contains("outside this expression-tree surface"),
        "a browser alias cannot select a different authoritative resource"
    );
    let explain_revision = server.revision(&session).unwrap();
    assert!(
        browser_sessions
            .submit(
                &right_browser,
                "main",
                &invoke(&session, explain_revision, "/model/answer", "explain"),
            )
            .unwrap()
            .is_empty(),
        "the injected product surface submits through the generic web-session table"
    );

    let mut read_only = read_cx();
    let viewer_transport = connect(&mut read_only, &address);
    let mut viewer = PhoneHost::with_surface_codec(viewer_transport, codec);
    viewer
        .open(&mut read_only, &registry, session.resource())
        .unwrap();
    viewer.transport_mut().disconnect();
    viewer.transport_mut().begin_reconnect();
    viewer.transport_mut().connect(&mut read_only).unwrap();
    viewer
        .open(&mut read_only, &registry, session.resource())
        .unwrap();
    let denied_revision = server.revision(&session).unwrap();
    let denied = viewer
        .submit(
            &mut read_only,
            &registry,
            edit_source(&session, denied_revision, "/model/answer", "forbidden"),
        )
        .unwrap_err();
    assert!(
        denied.to_string().contains("authority-denied"),
        "reconnect must not widen a read-only browser authority: {denied}"
    );

    let isolated_scene = {
        let mut isolated_cx = full_cx();
        let isolated_transport = connect(&mut isolated_cx, &address);
        let mut isolated_phone = PhoneHost::with_surface_codec(
            isolated_transport,
            expression_tree_surface_codec_symbol(),
        );
        isolated_phone
            .open(&mut isolated_cx, &registry, isolated.resource())
            .unwrap()
    };
    assert!(
        !scene_json(&isolated_scene).to_string().contains("answer"),
        "an isolated browser resource exposes no other session's tree"
    );

    check_fixture(
        "desktop.json",
        "desktop",
        "Expression tree - desktop",
        &updated,
    );
    check_fixture(
        "phone.json",
        "phone",
        "Expression tree - phone",
        &reconnected,
    );

    let value = realize_expr(
        &server,
        &mut desktop_cx,
        runtime_call(
            &session,
            "ref",
            vec![Expr::String("/model/answer".to_owned())],
        ),
    );
    assert_eq!(value, Expr::String("42".to_owned()));
}
