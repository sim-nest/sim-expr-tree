use crate::{CellId, DirId, GeneratedNameKind, NamespaceName};

/// Typed failures for backend-neutral expression-tree namespace records.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NamespaceError {
    /// An id constructor received empty text.
    EmptyId {
        /// The id family being constructed.
        kind: &'static str,
    },
    /// A child name is not legal as a canonical Table path segment.
    IllegalName(String),
    /// A parent directory is unknown to this namespace.
    MissingDir(DirId),
    /// A cell is unknown to this namespace.
    MissingCell(CellId),
    /// A directory is unknown to this namespace.
    MissingDirRecord(DirId),
    /// A child name is already occupied under the parent directory.
    NameCollision {
        /// The parent directory containing the conflicting name.
        parent: DirId,
        /// The conflicting child name.
        name: NamespaceName,
    },
    /// A cell record cannot be created without a durable name reservation.
    MissingReservation {
        /// The parent directory for the requested create.
        parent: DirId,
        /// The child name that was not reserved.
        name: NamespaceName,
    },
    /// A recovered generated-name counter is behind durable namespace state.
    CounterCorruption {
        /// The parent directory whose counter is corrupt.
        parent: DirId,
        /// The generated-name family whose counter is corrupt.
        kind: GeneratedNameKind,
        /// The occupied candidate produced by the corrupt counter.
        candidate: NamespaceName,
    },
    /// Another writer lane is already active.
    WriterAlreadyActive,
    /// The caller supplied no valid writer lane.
    InvalidWriterLane,
    /// The root directory cannot be renamed or moved.
    RootDirCannotMove,
    /// A directory cannot be moved below itself.
    DirMoveCycle {
        /// The directory being moved.
        dir: DirId,
        /// The requested new parent.
        new_parent: DirId,
    },
}
