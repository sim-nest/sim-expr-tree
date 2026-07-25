/// Monotonic namespace revision paired with the logical tick that produced it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RevisionTick {
    revision: u64,
    logical_tick: u64,
}

impl RevisionTick {
    /// Create an explicit revision/tick pair.
    pub fn new(revision: u64, logical_tick: u64) -> Self {
        Self {
            revision,
            logical_tick,
        }
    }

    /// The durable revision number.
    pub fn revision(self) -> u64 {
        self.revision
    }

    /// The serialized writer tick.
    pub fn logical_tick(self) -> u64 {
        self.logical_tick
    }

    pub(crate) fn next_after(self) -> Self {
        Self {
            revision: self.revision + 1,
            logical_tick: self.logical_tick + 1,
        }
    }
}

/// Optional wall-clock observation in Unix milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WallTimeMs(u64);

impl WallTimeMs {
    /// Record a Unix-millisecond observation.
    pub fn new(unix_millis: u64) -> Self {
        Self(unix_millis)
    }

    /// Return the observed Unix milliseconds.
    pub fn unix_millis(self) -> u64 {
        self.0
    }
}

/// Stamp attached to durable namespace records.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Stamp {
    revision_tick: RevisionTick,
    wall_time_ms: Option<WallTimeMs>,
}

impl Stamp {
    /// Create a stamp from logical and optional wall-clock observations.
    pub fn new(revision_tick: RevisionTick, wall_time_ms: Option<WallTimeMs>) -> Self {
        Self {
            revision_tick,
            wall_time_ms,
        }
    }

    /// The revision/tick pair.
    pub fn revision_tick(self) -> RevisionTick {
        self.revision_tick
    }

    /// The optional wall-clock observation.
    pub fn wall_time_ms(self) -> Option<WallTimeMs> {
        self.wall_time_ms
    }
}
