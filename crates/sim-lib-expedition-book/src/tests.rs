use crate::*;
use std::collections::BTreeSet;

struct Resolver(BTreeSet<EvidenceRef>);
impl EvidenceResolver for Resolver {
    fn exists(&self, reference: &EvidenceRef) -> bool {
        self.0.contains(reference)
    }
}

fn reference(kind: &str, key: &str) -> EvidenceRef {
    EvidenceRef::new(kind, key).unwrap()
}

#[test]
fn specimen_covers_success_failure_refusal_objection_choice_and_next_play() {
    let run = reference("run", "sha256:run");
    let claim = reference("claim", "sha256:claim");
    let mut book = ExpeditionBook::new("north", "Find the durable route");
    book.reference(0, "root", [run.clone()]).unwrap();
    book.branch(1, "root", "ridge", "Take the ridge").unwrap();
    book.branch(2, "root", "valley", "Take the valley").unwrap();
    book.object(
        3,
        "ridge",
        Objection {
            id: "weather".into(),
            statement: "Forecast refuses ridge travel".into(),
            evidence: vec![claim.clone()],
        },
    )
    .unwrap();
    book.set_next_play(4, "valley", "Inspect the ford").unwrap();
    book.choose(5, "valley").unwrap();
    assert_eq!(
        book.choose(5, "ridge"),
        Err(EditError::StaleRevision {
            expected: 5,
            actual: 6
        })
    );
    book.seal(6, "ridge").unwrap();
    assert_eq!(
        book.reference(7, "ridge", [claim.clone()]),
        Err(EditError::SealedBranch("ridge".into()))
    );
    assert_eq!(
        book.compare("missing", "valley"),
        Err(EditError::MissingBranch("missing".into()))
    );
    assert_eq!(
        book.branches["valley"].next_play.as_deref(),
        Some("Inspect the ford")
    );
}

#[test]
fn deleting_projections_and_reopening_two_branches_reproduces_exact_identities() {
    let run = reference("run", "sha256:run-7");
    let artifact = reference("artifact", "sha256:artifact-9");
    let mut book = ExpeditionBook::new("expedition-7", "Choose carefully");
    book.reference(0, "root", [run.clone(), artifact.clone()])
        .unwrap();
    book.branch(1, "root", "a", "A").unwrap();
    book.branch(2, "root", "b", "B").unwrap();
    book.object(
        3,
        "a",
        Objection {
            id: "o1".into(),
            statement: "counterexample".into(),
            evidence: vec![artifact.clone()],
        },
    )
    .unwrap();
    book.choose(4, "b").unwrap();
    book.seal(5, "a").unwrap();
    book.seal(6, "b").unwrap();
    book.reopen(7, "a").unwrap();
    book.reopen(8, "b").unwrap();
    let original = book.clone();
    let mut archive = Archive::default();
    archive.export("decision", &book, format!("{:?}", project(&book, 20)));
    assert!(archive.delete_projection("decision"));
    assert!(!archive.has_projection("decision"));
    let all = Resolver(BTreeSet::from([run.clone(), artifact.clone()]));
    assert_eq!(
        archive.replay("decision", &all).unwrap(),
        ReplayOutcome::Reproduced(original.clone())
    );
    let missing = archive
        .replay("decision", &Resolver(BTreeSet::from([run])))
        .unwrap();
    assert_eq!(
        missing,
        ReplayOutcome::MissingEvidence {
            book: original,
            missing: vec![artifact]
        }
    );
}

#[test]
fn codec_is_canonical_and_first_version_policy_is_explicit() {
    let book = ExpeditionBook::new("utf8", "rätt väg");
    let encoded = encode(&book);
    assert_eq!(decode(&encoded).unwrap(), book);
    assert!(matches!(
        decode("legacy payload"),
        Err(CodecError::UnsupportedVersion(_))
    ));
    assert!(matches!(
        decode("expedition-book/v2"),
        Err(CodecError::UnsupportedVersion(_))
    ));
}

#[test]
fn citizen_shape_and_lisp_inventory_are_stable() {
    assert_eq!(
        expedition_book_class_symbol().to_string(),
        "expedition-book/Snapshot"
    );
    let registry = expedition_book_citizen_registry().unwrap();
    registry
        .ensure_contains_symbols(&["expedition-book/Snapshot"])
        .unwrap();
    assert_eq!(lisp_operation_symbols().len(), 10);
}

#[test]
fn selective_retention_preserves_root_and_choice() {
    let mut book = ExpeditionBook::new("retain", "root");
    book.branch(0, "root", "a", "A").unwrap();
    book.branch(1, "root", "b", "B").unwrap();
    book.choose(2, "a").unwrap();
    assert!(matches!(
        book.retain(3, &BTreeSet::from(["root".into()])),
        Err(EditError::InvalidRetention(_))
    ));
    book.retain(3, &BTreeSet::from(["root".into(), "a".into()]))
        .unwrap();
    assert!(!book.branches.contains_key("b"));
}
// conformance: expedition-book tests prove revision, sealing, replay, and evidence semantics.
