use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

// conformance: bootloader-owned expression-tree executable envelope

#[test]
fn product_binary_reports_standard_bootloader_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_sim-expr-tree"))
        .arg("--help")
        .output()
        .expect("run sim-expr-tree help");

    assert!(
        output.status.success(),
        "help failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage: sim"), "{stdout}");
    assert!(stdout.contains("--config-file"), "{stdout}");
}

#[test]
fn product_binary_boots_configured_backend_and_shuts_down() {
    let path = temp_config_path();
    fs::write(
        &path,
        "[lib/expr-tree-serve]\n\
         dry-run = true\n\
         storage = \"process-smoke-backend\"\n\
         browser-resource = \"process-smoke-tree\"\n\
         bridge-thread = 82001\n",
    )
    .expect("write product config");

    let output = Command::new(env!("CARGO_BIN_EXE_sim-expr-tree"))
        .arg("--config-file")
        .arg(&path)
        .output()
        .expect("run configured sim-expr-tree product");
    let _ = fs::remove_file(&path);

    assert!(
        output.status.success(),
        "configured boot failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("sim-web-shell: dry-run OK"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

fn temp_config_path() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "sim-expr-tree-product-{}-{nonce}.toml",
        std::process::id()
    ))
}
