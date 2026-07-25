//! Package metadata floor checks for public crates in this workspace.

use std::fs;
use std::path::{Path, PathBuf};

pub fn run() -> Result<(), String> {
    let root = std::env::current_dir().map_err(|err| format!("current dir: {err}"))?;
    let mut manifests = Vec::new();
    collect_manifests(&root.join("crates"), &mut manifests)?;
    manifests.sort();

    let mut missing = Vec::new();
    for manifest in &manifests {
        let package = manifest
            .parent()
            .ok_or_else(|| format!("manifest has no parent: {}", manifest.display()))?;
        for file in ["README.md", "BROCHURE.md"] {
            if !package.join(file).is_file() {
                missing.push(format!("{}/{}", relative_path(&root, package), file));
            }
        }
        let text = fs::read_to_string(manifest)
            .map_err(|err| format!("read {}: {err}", manifest.display()))?;
        for key in ["description", "license.workspace", "repository.workspace"] {
            if !text.contains(key) {
                missing.push(format!("{} missing {key}", relative_path(&root, manifest)));
            }
        }
    }

    if !missing.is_empty() {
        for item in &missing {
            eprintln!("error: {item}");
        }
        return Err(format!(
            "check-package-floors: {} missing metadata item(s)",
            missing.len()
        ));
    }

    println!(
        "check-package-floors: OK ({} package manifest(s))",
        manifests.len()
    );
    Ok(())
}

fn collect_manifests(dir: &Path, manifests: &mut Vec<PathBuf>) -> Result<(), String> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).map_err(|err| format!("read {}: {err}", dir.display()))? {
        let entry = entry.map_err(|err| format!("read {} entry: {err}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            let manifest = path.join("Cargo.toml");
            if manifest.is_file() {
                manifests.push(manifest);
            }
        }
    }
    Ok(())
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}
