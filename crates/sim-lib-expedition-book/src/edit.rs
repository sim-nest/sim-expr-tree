//! Optimistic semantic editing.

use crate::model::{Branch, EvidenceRef, ExpeditionBookSnapshot, Objection};
use std::collections::{BTreeMap, BTreeSet};

/// Visible edit refusal or conflict.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EditError {
    /// Caller edited an obsolete snapshot.
    StaleRevision {
        /// Expected caller revision.
        expected: u64,
        /// Current book revision.
        actual: u64,
    },
    /// Requested branch is absent.
    MissingBranch(String),
    /// A sealed branch refuses mutation.
    SealedBranch(String),
    /// Stable identifier is already occupied.
    DuplicateId(String),
    /// Choice does not name a current branch.
    InvalidChoice(String),
    /// Retention would remove the root or chosen branch.
    InvalidRetention(String),
}

/// Revisioned editable expedition book.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpeditionBook {
    /// Stable book id.
    pub id: String,
    /// Current optimistic revision.
    pub revision: u64,
    /// Branches keyed by stable id.
    pub branches: BTreeMap<String, Branch>,
    /// Chosen branch, if a choice has been made.
    pub choice: Option<String>,
}

impl ExpeditionBook {
    /// Creates a book with one root branch.
    pub fn new(id: impl Into<String>, thesis: impl Into<String>) -> Self {
        let root = Branch {
            id: "root".into(),
            parent: None,
            thesis: thesis.into(),
            evidence: vec![],
            objections: BTreeMap::new(),
            next_play: None,
            sealed: false,
        };
        Self {
            id: id.into(),
            revision: 0,
            branches: BTreeMap::from([(root.id.clone(), root)]),
            choice: None,
        }
    }

    fn editable(&self, expected: u64, branch: &str) -> Result<(), EditError> {
        if expected != self.revision {
            return Err(EditError::StaleRevision {
                expected,
                actual: self.revision,
            });
        }
        let branch = self
            .branches
            .get(branch)
            .ok_or_else(|| EditError::MissingBranch(branch.into()))?;
        if branch.sealed {
            return Err(EditError::SealedBranch(branch.id.clone()));
        }
        Ok(())
    }

    fn changed(&mut self) {
        self.revision += 1;
    }

    /// Forks a branch while preserving exact reference identities.
    pub fn branch(
        &mut self,
        expected: u64,
        parent: &str,
        id: impl Into<String>,
        thesis: impl Into<String>,
    ) -> Result<u64, EditError> {
        self.editable(expected, parent)?;
        let id = id.into();
        if self.branches.contains_key(&id) {
            return Err(EditError::DuplicateId(id));
        }
        let evidence = self.branches[parent].evidence.clone();
        self.branches.insert(
            id.clone(),
            Branch {
                id,
                parent: Some(parent.into()),
                thesis: thesis.into(),
                evidence,
                objections: BTreeMap::new(),
                next_play: None,
                sealed: false,
            },
        );
        self.changed();
        Ok(self.revision)
    }

    /// Returns two branches for explicit semantic comparison.
    pub fn compare(&self, left: &str, right: &str) -> Result<(&Branch, &Branch), EditError> {
        Ok((
            self.branches
                .get(left)
                .ok_or_else(|| EditError::MissingBranch(left.into()))?,
            self.branches
                .get(right)
                .ok_or_else(|| EditError::MissingBranch(right.into()))?,
        ))
    }

    /// Attaches immutable references, without copying their content.
    pub fn reference(
        &mut self,
        expected: u64,
        branch: &str,
        refs: impl IntoIterator<Item = EvidenceRef>,
    ) -> Result<u64, EditError> {
        self.editable(expected, branch)?;
        let row = self.branches.get_mut(branch).expect("checked");
        row.evidence.extend(refs);
        row.evidence.sort();
        row.evidence.dedup();
        self.changed();
        Ok(self.revision)
    }

    /// Adds a visible objection.
    pub fn object(
        &mut self,
        expected: u64,
        branch: &str,
        objection: Objection,
    ) -> Result<u64, EditError> {
        self.editable(expected, branch)?;
        if self.branches[branch].objections.contains_key(&objection.id) {
            return Err(EditError::DuplicateId(objection.id));
        }
        self.branches
            .get_mut(branch)
            .expect("checked")
            .objections
            .insert(objection.id.clone(), objection);
        self.changed();
        Ok(self.revision)
    }

    /// Chooses one live branch.
    pub fn choose(&mut self, expected: u64, branch: &str) -> Result<u64, EditError> {
        if expected != self.revision {
            return Err(EditError::StaleRevision {
                expected,
                actual: self.revision,
            });
        }
        if !self.branches.contains_key(branch) {
            return Err(EditError::InvalidChoice(branch.into()));
        }
        self.choice = Some(branch.into());
        self.changed();
        Ok(self.revision)
    }

    /// Records the next play.
    pub fn set_next_play(
        &mut self,
        expected: u64,
        branch: &str,
        play: impl Into<String>,
    ) -> Result<u64, EditError> {
        self.editable(expected, branch)?;
        self.branches.get_mut(branch).expect("checked").next_play = Some(play.into());
        self.changed();
        Ok(self.revision)
    }

    /// Seals a branch against edits.
    pub fn seal(&mut self, expected: u64, branch: &str) -> Result<u64, EditError> {
        self.editable(expected, branch)?;
        self.branches.get_mut(branch).expect("checked").sealed = true;
        self.changed();
        Ok(self.revision)
    }

    /// Explicitly reopens a sealed branch.
    pub fn reopen(&mut self, expected: u64, branch: &str) -> Result<u64, EditError> {
        if expected != self.revision {
            return Err(EditError::StaleRevision {
                expected,
                actual: self.revision,
            });
        }
        let row = self
            .branches
            .get_mut(branch)
            .ok_or_else(|| EditError::MissingBranch(branch.into()))?;
        row.sealed = false;
        self.changed();
        Ok(self.revision)
    }

    /// Retains an explicit branch subset.
    pub fn retain(&mut self, expected: u64, ids: &BTreeSet<String>) -> Result<u64, EditError> {
        if expected != self.revision {
            return Err(EditError::StaleRevision {
                expected,
                actual: self.revision,
            });
        }
        if !ids.contains("root") || self.choice.as_ref().is_some_and(|id| !ids.contains(id)) {
            return Err(EditError::InvalidRetention(
                "root and chosen branch must be retained".into(),
            ));
        }
        self.branches.retain(|id, _| ids.contains(id));
        self.changed();
        Ok(self.revision)
    }

    /// Creates the versioned Citizen snapshot used for export.
    pub fn snapshot(&self) -> ExpeditionBookSnapshot {
        ExpeditionBookSnapshot {
            book_id: self.id.clone(),
            revision: self.revision,
            payload: crate::codec::encode(self),
        }
    }
}
