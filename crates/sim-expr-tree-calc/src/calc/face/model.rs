use sim_codec::DecodePosition;
use sim_kernel::EncodePosition;

/// Position recorded beside a source edit or encoded face.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FacePosition {
    /// Evaluated expression position.
    Eval,
    /// Quoted expression position.
    Quote,
    /// Inert data position.
    Data,
    /// Pattern position.
    Pattern,
}

impl From<DecodePosition> for FacePosition {
    fn from(value: DecodePosition) -> Self {
        match value {
            DecodePosition::Eval => Self::Eval,
            DecodePosition::Quote => Self::Quote,
            DecodePosition::Data => Self::Data,
            DecodePosition::Pattern => Self::Pattern,
        }
    }
}

impl From<EncodePosition> for FacePosition {
    fn from(value: EncodePosition) -> Self {
        match value {
            EncodePosition::Eval => Self::Eval,
            EncodePosition::Quote => Self::Quote,
            EncodePosition::Data => Self::Data,
            EncodePosition::Pattern => Self::Pattern,
        }
    }
}

/// Resource dimension that stopped a bounded face operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FaceDimension {
    /// Aggregate scalar payload or encoded output bytes.
    Bytes,
    /// Expression nesting depth.
    Depth,
    /// Expression nodes or runtime collection items.
    Items,
}

/// Explicit outcome metadata for source edits and encoded faces.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FaceIssue {
    /// The operation completed within every bound.
    Complete,
    /// The input or output exceeded one explicit budget.
    Truncated {
        /// Budget dimension reached.
        dimension: FaceDimension,
        /// Configured maximum.
        limit: usize,
        /// First observed value beyond the maximum, or the final encoded size.
        observed: usize,
    },
    /// The selected value or policy has no safe presentation path.
    Unsupported {
        /// Stable bounded explanation.
        reason: String,
    },
    /// Codec lookup, decoding, or encoding failed closed.
    CodecFailure {
        /// Stable bounded codec diagnostic.
        message: String,
    },
}

/// Metadata carried for every edit and face, including failures.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FaceMetadata {
    pub(super) codec: Option<String>,
    pub(super) position: FacePosition,
    pub(super) issue: FaceIssue,
}

impl FaceMetadata {
    /// Selected codec name, when one was configured.
    #[must_use]
    pub fn codec(&self) -> Option<&str> {
        self.codec.as_deref()
    }

    /// Explicit decode or encode position.
    #[must_use]
    pub const fn position(&self) -> FacePosition {
        self.position
    }

    /// Completion, truncation, unsupported-value, or codec-failure metadata.
    #[must_use]
    pub const fn issue(&self) -> &FaceIssue {
        &self.issue
    }

    /// Whether the operation completed successfully.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        matches!(self.issue, FaceIssue::Complete)
    }
}

/// Encoded face payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FaceContent {
    /// Text codec output.
    Text(String),
    /// Binary codec output.
    Bytes(Vec<u8>),
}

/// A bounded presentation face plus explicit outcome metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncodedFace {
    pub(super) content: Option<FaceContent>,
    pub(super) metadata: FaceMetadata,
}

impl EncodedFace {
    /// Encoded payload, absent for every non-complete outcome.
    #[must_use]
    pub const fn content(&self) -> Option<&FaceContent> {
        self.content.as_ref()
    }

    /// Explicit outcome metadata.
    #[must_use]
    pub const fn metadata(&self) -> &FaceMetadata {
        &self.metadata
    }
}

/// Result of applying edited source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceEditOutcome {
    pub(super) metadata: FaceMetadata,
}

impl SourceEditOutcome {
    /// Whether the decoded source replaced the cell.
    #[must_use]
    pub const fn applied(&self) -> bool {
        self.metadata.is_complete()
    }

    /// Explicit decode outcome metadata.
    #[must_use]
    pub const fn metadata(&self) -> &FaceMetadata {
        &self.metadata
    }
}
