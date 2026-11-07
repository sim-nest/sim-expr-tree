// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Thin simdoc launcher: local recipes, then the isolated encoder.
//! `crate-catalog` stays on the sim-tooling workspace manifest.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn run(args: Vec<String>) -> Result<(), String> {
    crate::recipes::run()?;
    run_repo_tool(args, "simdoc")
}

pub fn run_repo_tool(args: Vec<String>, command_name: &str) -> Result<(), String> {
    let program = args.first().map(String::as_str).unwrap_or("xtask");
    if args.get(1).map(String::as_str) != Some(command_name) {
        return Err(format!("usage: {program} {command_name} [--check]"));
    }

    let root = env::current_dir().map_err(|err| format!("current dir: {err}"))?;
    let simdoc = command_name == "simdoc";
    let manifest = if simdoc {
        locate_simdoc_manifest(&root)?
    } else {
        locate_sim_tooling_manifest(&root)?
    };
    let mut command = Command::new("cargo");
    if simdoc {
        command.args(["run", "--locked", "--offline", "--manifest-path"]);
    } else {
        command.args(["run", "--manifest-path"]);
    }
    command.arg(manifest);
    command.args(["--quiet", "--", command_name]);
    if simdoc {
        command.arg("--repo-root");
        command.arg(&root);
    } else if !has_repo_arg(&args) {
        command.arg("--repo");
        command.arg(&root);
    }
    for arg in args.iter().skip(2) {
        command.arg(arg);
    }

    let status = command
        .status()
        .map_err(|err| format!("run {command_name} tool: {err}"))?;
    if status.success() {
        Ok(())
    } else if simdoc {
        Err(format!(
            "isolated simdoc encoder failed with status {status}"
        ))
    } else {
        Err(format!(
            "shared {command_name} tool failed with status {status}"
        ))
    }
}

fn has_repo_arg(args: &[String]) -> bool {
    args.iter()
        .skip(2)
        .any(|arg| arg == "--repo" || arg.starts_with("--repo="))
}

/// Workspace manifest used by `crate-catalog`. The simdoc verb is not here.
fn locate_sim_tooling_manifest(repo_root: &Path) -> Result<PathBuf, String> {
    if let Ok(path) = env::var("SIMDOC_TOOLING_MANIFEST") {
        return Ok(PathBuf::from(path));
    }
    let sibling = repo_root
        .parent()
        .unwrap_or(repo_root)
        .join("sim-tooling")
        .join("Cargo.toml");
    if sibling.is_file() {
        return Ok(sibling);
    }
    Err("set SIMDOC_TOOLING_MANIFEST to the sim-tooling Cargo.toml".to_owned())
}

fn locate_simdoc_manifest(repo_root: &Path) -> Result<PathBuf, String> {
    if let Ok(path) = env::var("SIMDOC_TOOLING_MANIFEST") {
        // CI and release generation set this to the tooling workspace
        // manifest. That manifest refuses the simdoc verb.
        return isolated_manifest_from_hint(Path::new(&path));
    }
    let isolated = repo_root
        .parent()
        .unwrap_or(repo_root)
        .join("sim-tooling")
        .join("crates")
        .join("simdoc")
        .join("Cargo.toml");
    if isolated.is_file() {
        return Ok(isolated);
    }
    Err(
        "set SIMDOC_TOOLING_MANIFEST to the sim-tooling checkout or crates/simdoc/Cargo.toml"
            .to_owned(),
    )
}

fn isolated_manifest_from_hint(hint: &Path) -> Result<PathBuf, String> {
    let isolated_rel = Path::new("crates").join("simdoc").join("Cargo.toml");
    let candidates = [
        hint.to_path_buf(),
        hint.join("Cargo.toml"),
        hint.join(&isolated_rel),
        hint.parent().unwrap_or(hint).join(&isolated_rel),
    ];
    for candidate in candidates {
        if candidate.is_file() && candidate.ends_with(&isolated_rel) {
            return Ok(candidate);
        }
    }
    Err(format!(
        "SIMDOC_TOOLING_MANIFEST={} does not locate crates/simdoc/Cargo.toml",
        hint.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn isolated_rel() -> PathBuf {
        Path::new("crates").join("simdoc").join("Cargo.toml")
    }

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask lives in the repository root")
            .to_path_buf()
    }

    #[test]
    fn tooling_hints_resolve_to_the_isolated_encoder() {
        let repo = repo_root();
        let selected = locate_simdoc_manifest(&repo).expect("isolated encoder");
        assert!(selected.ends_with(isolated_rel()));
        assert!(selected.with_file_name("Cargo.lock").is_file());

        let tooling = selected
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .expect("crates/simdoc lives in the tooling checkout");
        assert_eq!(
            isolated_manifest_from_hint(&tooling.join("Cargo.toml")).expect("workspace hint"),
            selected
        );
        assert_eq!(
            isolated_manifest_from_hint(&selected).expect("isolated hint"),
            selected
        );
        assert_eq!(
            isolated_manifest_from_hint(tooling).expect("checkout hint"),
            selected
        );
    }

    #[test]
    fn missing_tooling_hint_is_an_error() {
        let missing = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("no-such-tooling")
            .join("Cargo.toml");
        let error = isolated_manifest_from_hint(&missing).expect_err("missing hint");
        assert!(error.contains("does not locate crates/simdoc/Cargo.toml"));
    }

    #[test]
    fn catalog_locator_keeps_the_workspace_manifest() {
        let selected = locate_sim_tooling_manifest(&repo_root()).expect("workspace manifest");
        if let Ok(hint) = env::var("SIMDOC_TOOLING_MANIFEST") {
            assert_eq!(selected, PathBuf::from(hint));
        } else {
            let workspace = Path::new("sim-tooling").join("Cargo.toml");
            assert!(selected.ends_with(&workspace));
            assert!(!selected.ends_with(isolated_rel()));
        }
    }
}
