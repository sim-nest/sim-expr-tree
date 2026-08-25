//! Disposable read projection.

use crate::edit::ExpeditionBook;

/// A bounded, reproducible derived face.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BookProjection {
    /// Source revision.
    pub revision: u64,
    /// Human-readable rows.
    pub rows: Vec<String>,
    /// Whether branches were omitted.
    pub truncated: bool,
}

/// Projects at most `limit` branches without changing book semantics.
pub fn project(book: &ExpeditionBook, limit: usize) -> BookProjection {
    let rows = book
        .branches
        .values()
        .take(limit)
        .map(|b| {
            format!(
                "{} | {} | objections={} | next={}",
                b.id,
                b.thesis,
                b.objections.len(),
                b.next_play.as_deref().unwrap_or("-")
            )
        })
        .collect::<Vec<_>>();
    BookProjection {
        revision: book.revision,
        truncated: rows.len() < book.branches.len(),
        rows,
    }
}
