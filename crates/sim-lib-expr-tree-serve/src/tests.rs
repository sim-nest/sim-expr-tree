use std::{
    fs,
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

// conformance: default expression-tree product composition and boot behavior

use sim_config::{ConfigDir, ConfigLayer, ConfigSource};
use sim_kernel::{CapabilityName, Expr, Lib, Symbol};
use sim_lib_expr_tree_server::{
    ExpressionTreeServer, ExpressionTreeServerLib, SessionId, expr_tree_server_site_symbol,
};
use sim_run_core::{CliCommand, RuntimeConfigState, parse_args};
use sim_web_shell::LiveSurfaceFactory;

use super::{
    ExprTreeServeLib, ExpressionTreeRecipe, ExpressionTreeServeConfig, ServerPlacement,
    component_identities, expr_tree_boot_args, expr_tree_bootloader, expr_tree_entrypoint_symbol,
    serve_config_symbol,
};

fn runtime_cx(capabilities: &[CapabilityName]) -> sim_kernel::Cx {
    let mut cx = sim_kernel::testing::eager_cx();
    let codec_id = cx.registry_mut().fresh_codec_id();
    cx.load_lib(&sim_codec_lisp::LispCodecLib::new(codec_id).unwrap())
        .unwrap();
    for capability in capabilities {
        cx.grant(capability.clone());
    }
    cx
}

fn product_cx() -> sim_kernel::Cx {
    runtime_cx(&[
        sim_lib_expr_tree::expr_tree_read_capability(),
        sim_lib_expr_tree::expr_tree_write_capability(),
        sim_lib_expr_tree::expr_tree_calculate_capability(),
    ])
}

fn config_state(entries: Vec<(&str, Expr)>) -> RuntimeConfigState {
    let dir = ConfigDir::one(
        serve_config_symbol(),
        Expr::Map(
            entries
                .into_iter()
                .map(|(key, value)| (Expr::String(key.to_owned()), value))
                .collect(),
        ),
    )
    .unwrap();
    let mut state = RuntimeConfigState::default();
    state.push_layer(ConfigLayer::new(
        ConfigSource::SingleFile {
            path: PathBuf::from("expr-tree-test.toml"),
        },
        dir,
    ));
    state
}

#[test]
fn manifest_and_recipe_name_the_existing_components() {
    let manifest = Lib::manifest(&ExprTreeServeLib::default());

    assert_eq!(manifest.id, serve_config_symbol());
    assert_eq!(manifest.exports[0].symbol(), &expr_tree_entrypoint_symbol());
    assert_eq!(
        component_identities(),
        [
            "sim-lib-expr-tree",
            "sim-lib-view-expr-tree",
            "sim-lib-expr-tree-server",
            "sim-web-shell",
        ]
    );
}

#[test]
fn runtime_config_selects_external_fabric_and_backend_state() {
    let state = config_state(vec![
        ("placement", Expr::String("external".to_owned())),
        (
            "server-site",
            Expr::String("site/external-expression-tree".to_owned()),
        ),
        ("storage", Expr::String("configured-backend".to_owned())),
        (
            "browser-resource",
            Expr::String("configured-tree".to_owned()),
        ),
        ("web-addr", Expr::String("127.0.0.1:9876".to_owned())),
        ("atelier-root", Expr::String(".sim/product".to_owned())),
        ("dry-run", Expr::Bool(true)),
    ]);

    let config = ExpressionTreeServeConfig::from_runtime_config(&state).unwrap();

    assert_eq!(
        config.placement,
        ServerPlacement::External {
            site: Symbol::new("site/external-expression-tree")
        }
    );
    assert_eq!(config.storage, "configured-backend");
    assert_eq!(config.browser_resource, "configured-tree");
    assert_eq!(config.web_addr, "127.0.0.1:9876");
    assert_eq!(config.atelier_root, PathBuf::from(".sim/product"));
    assert!(config.dry_run);
}

#[test]
fn recipe_fails_closed_without_expression_tree_authority() {
    let mut cx = runtime_cx(&[]);
    let config = ExpressionTreeServeConfig {
        bridge_thread: Some(81_001),
        dry_run: true,
        ..ExpressionTreeServeConfig::default()
    };

    let error = ExpressionTreeRecipe::new(config)
        .start(&mut cx)
        .err()
        .expect("missing read authority must reject session creation");

    assert!(error.to_string().contains("authority-denied"), "{error}");
    assert!(error.to_string().contains("expr-tree.read"), "{error}");
}

#[test]
fn in_process_recipe_opens_configured_storage_and_injects_the_web_surface() {
    let mut cx = product_cx();
    let config = ExpressionTreeServeConfig {
        storage: "configured-in-process-backend".to_owned(),
        browser_resource: "configured-browser-tree".to_owned(),
        bridge_thread: Some(81_002),
        dry_run: true,
        ..ExpressionTreeServeConfig::default()
    };
    let mut product = ExpressionTreeRecipe::new(config.clone())
        .start(&mut cx)
        .unwrap();

    assert!(product.owns_server());
    assert_eq!(product.config().storage, "configured-in-process-backend");
    let mut surface = product.surface_factory().create().unwrap();
    let scene = surface
        .open(&config.browser_resource, "main")
        .expect("generic web surface opens the configured tree");
    assert!(matches!(scene, Expr::Map(_)));

    product.shutdown(&mut cx).unwrap();
    product.shutdown(&mut cx).unwrap();
    assert!(product.is_shutdown());
}

#[test]
fn external_eval_fabric_smoke_and_graceful_shutdown() {
    let mut cx = product_cx();
    sim_lib_expr_tree::install_expr_tree_lib(&mut cx).unwrap();
    let external = Arc::new(ExpressionTreeServer::local());
    cx.load_lib(&ExpressionTreeServerLib::new(external.clone()))
        .unwrap();
    let config = ExpressionTreeServeConfig {
        placement: ServerPlacement::External {
            site: expr_tree_server_site_symbol(),
        },
        storage: "configured-external-backend".to_owned(),
        bridge_thread: Some(81_003),
        dry_run: true,
        ..ExpressionTreeServeConfig::default()
    };
    let mut product = ExpressionTreeRecipe::new(config).start(&mut cx).unwrap();
    let session = SessionId::from_resource(product.resource()).unwrap();

    assert!(!product.owns_server());
    assert!(external.snapshot(&session).is_ok());
    let mut surface = product.surface_factory().create().unwrap();
    assert!(surface.open("tree", "main").is_ok());

    product.shutdown(&mut cx).unwrap();
    assert!(product.is_shutdown());
    assert!(external.snapshot(&session).is_err());
}

#[test]
fn bootloader_uses_standard_config_flags_and_dispatches_the_product_verb() {
    let path = temp_config_path("boot");
    fs::write(
        &path,
        "[lib/expr-tree-serve]\ndry-run = true\nstorage = \"boot-configured\"\nbridge-thread = 81004\n",
    )
    .unwrap();
    let args = expr_tree_boot_args([
        "sim-expr-tree".into(),
        "--config-file".into(),
        path.as_os_str().to_owned(),
    ]);

    let code = expr_tree_bootloader().run(args).unwrap();

    let _ = fs::remove_file(&path);
    assert_eq!(code, 0);
}

#[test]
fn appended_product_verb_does_not_shadow_standard_help() {
    let command = parse_args(expr_tree_boot_args(["sim-expr-tree", "--help"])).unwrap();

    assert_eq!(command, CliCommand::Help);
}

fn temp_config_path(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "sim-expr-tree-serve-{label}-{}-{nonce}.toml",
        std::process::id()
    ))
}
