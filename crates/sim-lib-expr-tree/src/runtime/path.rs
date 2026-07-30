use sim_table_core::{TablePath, TablePathRef};

use crate::runtime_support::debug_error;

pub(super) fn resolve_path(text: &str, base: &TablePath) -> std::result::Result<TablePath, String> {
    TablePathRef::parse(text)
        .and_then(|reference| reference.resolve(base))
        .map_err(debug_error)
}

pub(super) fn child_path(parent: &TablePath, name: &str) -> std::result::Result<TablePath, String> {
    let mut path = parent.clone();
    path.push(name).map_err(debug_error)?;
    Ok(path)
}

pub(super) fn split_path(path: &TablePath) -> std::result::Result<(TablePath, String), String> {
    let Some((name, parent)) = path.segments().split_last() else {
        return Err("root has no parent or child name".to_owned());
    };
    Ok((
        TablePath::from_segments(parent).map_err(debug_error)?,
        name.clone(),
    ))
}

pub(super) fn path_within(parent: &TablePath, candidate: &TablePath) -> bool {
    parent.segments().len() <= candidate.segments().len()
        && parent
            .segments()
            .iter()
            .zip(candidate.segments())
            .all(|(left, right)| left == right)
}
