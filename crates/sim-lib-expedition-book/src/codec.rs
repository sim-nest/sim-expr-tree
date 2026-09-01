//! Canonical versioned book codec.

use crate::{
    edit::ExpeditionBook,
    model::{Branch, EvidenceRef, Objection},
};
use std::collections::BTreeMap;

/// Codec rejection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CodecError {
    /// Unknown schema version.
    UnsupportedVersion(String),
    /// Malformed canonical payload.
    Malformed(String),
}

fn field(value: &str) -> String {
    format!("{}:{}", value.len(), value)
}
fn take(input: &mut &str) -> Result<String, CodecError> {
    let colon = input
        .find(':')
        .ok_or_else(|| CodecError::Malformed("missing field length".into()))?;
    let len: usize = input[..colon]
        .parse()
        .map_err(|_| CodecError::Malformed("invalid field length".into()))?;
    let rest = &input[colon + 1..];
    if rest.len() < len || !rest.is_char_boundary(len) {
        return Err(CodecError::Malformed("truncated field".into()));
    }
    let value = rest[..len].to_owned();
    *input = &rest[len..];
    Ok(value)
}

/// Encodes the current canonical `expedition-book/v1` format.
pub fn encode(book: &ExpeditionBook) -> String {
    let mut out = "expedition-book/v1".to_owned();
    for value in [
        &book.id,
        &book.revision.to_string(),
        book.choice.as_deref().unwrap_or(""),
    ] {
        out.push_str(&field(value));
    }
    out.push_str(&field(&book.branches.len().to_string()));
    for branch in book.branches.values() {
        out.push_str(&field(&branch.id));
        out.push_str(&field(branch.parent.as_deref().unwrap_or("")));
        out.push_str(&field(&branch.thesis));
        out.push_str(&field(branch.next_play.as_deref().unwrap_or("")));
        out.push_str(&field(if branch.sealed { "1" } else { "0" }));
        out.push_str(&field(&branch.evidence.len().to_string()));
        for r in &branch.evidence {
            out.push_str(&field(&r.kind));
            out.push_str(&field(&r.content_key));
        }
        out.push_str(&field(&branch.objections.len().to_string()));
        for objection in branch.objections.values() {
            out.push_str(&field(&objection.id));
            out.push_str(&field(&objection.statement));
            out.push_str(&field(&objection.evidence.len().to_string()));
            for r in &objection.evidence {
                out.push_str(&field(&r.kind));
                out.push_str(&field(&r.content_key));
            }
        }
    }
    out
}

/// Decodes v1. The explicit migration policy rejects pre-version payloads;
/// future versions require a named migration rather than best-effort reading.
pub fn decode(encoded: &str) -> Result<ExpeditionBook, CodecError> {
    const HEADER: &str = "expedition-book/v1";
    if !encoded.starts_with(HEADER) {
        return Err(CodecError::UnsupportedVersion(
            encoded
                .split_once(|c: char| c.is_ascii_digit())
                .map_or("unversioned", |(_, v)| v)
                .chars()
                .take(8)
                .collect(),
        ));
    }
    let mut input = &encoded[HEADER.len()..];
    let id = take(&mut input)?;
    let revision = take(&mut input)?
        .parse()
        .map_err(|_| CodecError::Malformed("revision".into()))?;
    let choice = match take(&mut input)?.as_str() {
        "" => None,
        s => Some(s.into()),
    };
    let count: usize = take(&mut input)?
        .parse()
        .map_err(|_| CodecError::Malformed("branch count".into()))?;
    let mut branches = BTreeMap::new();
    for _ in 0..count {
        let branch_id = take(&mut input)?;
        let parent = match take(&mut input)?.as_str() {
            "" => None,
            s => Some(s.into()),
        };
        let thesis = take(&mut input)?;
        let next_play = match take(&mut input)?.as_str() {
            "" => None,
            s => Some(s.into()),
        };
        let sealed = take(&mut input)? == "1";
        let refs: usize = take(&mut input)?
            .parse()
            .map_err(|_| CodecError::Malformed("reference count".into()))?;
        let mut evidence = Vec::new();
        for _ in 0..refs {
            evidence.push(EvidenceRef {
                kind: take(&mut input)?,
                content_key: take(&mut input)?,
            });
        }
        let objections_count: usize = take(&mut input)?
            .parse()
            .map_err(|_| CodecError::Malformed("objection count".into()))?;
        let mut objections = BTreeMap::new();
        for _ in 0..objections_count {
            let id = take(&mut input)?;
            let statement = take(&mut input)?;
            let n: usize = take(&mut input)?
                .parse()
                .map_err(|_| CodecError::Malformed("objection reference count".into()))?;
            let mut refs = Vec::new();
            for _ in 0..n {
                refs.push(EvidenceRef {
                    kind: take(&mut input)?,
                    content_key: take(&mut input)?,
                });
            }
            objections.insert(
                id.clone(),
                Objection {
                    id,
                    statement,
                    evidence: refs,
                },
            );
        }
        branches.insert(
            branch_id.clone(),
            Branch {
                id: branch_id,
                parent,
                thesis,
                evidence,
                objections,
                next_play,
                sealed,
            },
        );
    }
    if !input.is_empty() {
        return Err(CodecError::Malformed("trailing bytes".into()));
    }
    Ok(ExpeditionBook {
        id,
        revision,
        branches,
        choice,
    })
}
