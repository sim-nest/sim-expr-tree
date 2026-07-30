//! Checked recipe metadata and Lisp-source gate.

use std::fs;
use std::path::{Path, PathBuf};

pub fn run() -> Result<(), String> {
    let root = std::env::current_dir().map_err(|err| format!("current dir: {err}"))?;
    let mut checked = 0usize;
    let mut runnable = 0usize;
    for recipe_path in recipe_files(&root)? {
        let recipe_dir = recipe_path
            .parent()
            .ok_or_else(|| format!("recipe path has no parent: {}", recipe_path.display()))?;
        let metadata = fs::read_to_string(&recipe_path)
            .map_err(|err| format!("read {}: {err}", recipe_path.display()))?;
        let setup_name =
            quoted_value(&metadata, "setup").unwrap_or_else(|| "setup.siml".to_owned());
        let expected_name =
            quoted_value(&metadata, "expected").unwrap_or_else(|| "expected.txt".to_owned());
        let purpose_name =
            quoted_value(&metadata, "purpose").unwrap_or_else(|| "purpose.md".to_owned());
        let setup = required_text(recipe_dir, &setup_name)?;
        let expected = required_text(recipe_dir, &expected_name)?;
        required_text(recipe_dir, &purpose_name)?;
        validate_balanced_lisp(&setup)
            .map_err(|error| format!("{}: {error}", recipe_dir.display()))?;

        if quoted_value(&metadata, "package").as_deref() == Some("sim-lib-expr-tree") {
            validate_runtime_recipe(&root, recipe_dir, &metadata, &setup, &expected)?;
            runnable += 1;
        }
        checked += 1;
    }
    println!(
        "check-recipes: checked {checked} recipe(s), including {runnable} runnable expression-tree Lisp recipe(s)"
    );
    Ok(())
}

fn validate_runtime_recipe(
    root: &Path,
    recipe_dir: &Path,
    metadata: &str,
    setup: &str,
    expected: &str,
) -> Result<(), String> {
    for key in ["id", "title", "codec", "harness", "package", "test"] {
        if quoted_value(metadata, key).is_none() {
            return Err(format!("{} missing quoted {key}", recipe_dir.display()));
        }
    }
    if quoted_value(metadata, "codec").as_deref() != Some("lisp") {
        return Err(format!(
            "{} must use codec = \"lisp\"",
            recipe_dir.display()
        ));
    }
    if quoted_value(metadata, "harness").as_deref() != Some("cargo-test") {
        return Err(format!(
            "{} must use the cargo-test harness",
            recipe_dir.display()
        ));
    }
    if setup.trim() == expected.trim() {
        return Err(format!(
            "{} expected evidence must not duplicate Lisp source",
            recipe_dir.display()
        ));
    }
    if !setup.contains("(expr-tree/open ") {
        return Err(format!(
            "{} does not open the loadable expression-tree library",
            recipe_dir.display()
        ));
    }
    let test = quoted_value(metadata, "test").expect("validated test");
    let test_name = test.rsplit("::").next().expect("nonempty test name");
    let tests_source = required_text(&root.join("crates/sim-lib-expr-tree/src"), "tests.rs")?;
    if !tests_source.contains(&format!("fn {test_name}(")) {
        return Err(format!(
            "{} names missing cargo test {test}",
            recipe_dir.display()
        ));
    }
    Ok(())
}

fn required_text(dir: &Path, name: &str) -> Result<String, String> {
    let path = dir.join(name);
    let text =
        fs::read_to_string(&path).map_err(|err| format!("read {}: {err}", path.display()))?;
    if text.trim().is_empty() {
        Err(format!("{} must not be empty", path.display()))
    } else {
        Ok(text)
    }
}

fn quoted_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let line = line.trim();
        let value = line
            .strip_prefix(key)?
            .trim_start()
            .strip_prefix('=')?
            .trim();
        value
            .strip_prefix('"')?
            .strip_suffix('"')
            .map(str::to_owned)
    })
}

fn validate_balanced_lisp(source: &str) -> Result<(), String> {
    let mut stack = Vec::new();
    let mut string = false;
    let mut escape = false;
    for (index, character) in source.char_indices() {
        if string {
            if escape {
                escape = false;
            } else if character == '\\' {
                escape = true;
            } else if character == '"' {
                string = false;
            }
            continue;
        }
        match character {
            '"' => string = true,
            '(' | '[' | '{' => stack.push((character, index)),
            ')' | ']' | '}' => {
                let Some((open, _)) = stack.pop() else {
                    return Err(format!("unmatched {character} at byte {index}"));
                };
                if !matches!((open, character), ('(', ')') | ('[', ']') | ('{', '}')) {
                    return Err(format!("mismatched {open} and {character} at byte {index}"));
                }
            }
            _ => {}
        }
    }
    if string {
        return Err("unterminated string literal".to_owned());
    }
    if let Some((open, index)) = stack.pop() {
        return Err(format!("unclosed {open} from byte {index}"));
    }
    Ok(())
}

fn recipe_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    collect(&root.join("crates"), &mut files)?;
    files.sort();
    Ok(files)
}

fn collect(path: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(path).map_err(|err| format!("read {}: {err}", path.display()))? {
        let entry = entry.map_err(|err| format!("read {} entry: {err}", path.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect(&path, files)?;
        } else if path.file_name().and_then(|name| name.to_str()) == Some("recipe.toml") {
            files.push(path);
        }
    }
    Ok(())
}
