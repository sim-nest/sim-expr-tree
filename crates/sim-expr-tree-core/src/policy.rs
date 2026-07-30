use sim_codec::{DecodeLimits, DecodePosition};
use sim_kernel::EncodePosition;

/// Absolute ceiling for bytes admitted to one encoded source or result face.
pub const HARD_MAX_FACE_BYTES: usize = 8 * 1024 * 1024;
/// Absolute ceiling for expression nesting inspected for one face.
pub const HARD_MAX_FACE_DEPTH: usize = 512;
/// Absolute ceiling for expression nodes or runtime collection items inspected
/// for one face.
pub const HARD_MAX_FACE_ITEMS: usize = 200_000;

/// Independent resource ceilings for one encoded source or result face.
///
/// Construction clamps every requested field to a non-configurable ceiling so
/// persisted policy can never turn a display operation into an unbounded walk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaceBudget {
    max_bytes: usize,
    max_depth: usize,
    max_items: usize,
}

impl FaceBudget {
    /// Creates a hard-clamped face budget.
    #[must_use]
    pub const fn new(max_bytes: usize, max_depth: usize, max_items: usize) -> Self {
        Self {
            max_bytes: if max_bytes > HARD_MAX_FACE_BYTES {
                HARD_MAX_FACE_BYTES
            } else {
                max_bytes
            },
            max_depth: if max_depth > HARD_MAX_FACE_DEPTH {
                HARD_MAX_FACE_DEPTH
            } else {
                max_depth
            },
            max_items: if max_items > HARD_MAX_FACE_ITEMS {
                HARD_MAX_FACE_ITEMS
            } else {
                max_items
            },
        }
    }

    /// Maximum encoded bytes and aggregate scalar payload bytes.
    #[must_use]
    pub const fn max_bytes(self) -> usize {
        self.max_bytes
    }

    /// Maximum expression nesting depth.
    #[must_use]
    pub const fn max_depth(self) -> usize {
        self.max_depth
    }

    /// Maximum expression nodes or runtime collection items.
    #[must_use]
    pub const fn max_items(self) -> usize {
        self.max_items
    }
}

impl Default for FaceBudget {
    fn default() -> Self {
        Self::new(256 * 1024, 128, 16_384)
    }
}

/// Patch applied at the tree, directory, or cell policy level.
///
/// Codec selectors are stable exported names, not process-local `CodecId`
/// values. The nested option permits a descendant to explicitly clear an
/// inherited source or result codec.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodecPolicyPatch {
    /// Inherit, clear, or replace the source codec name.
    pub source_codec: Option<Option<String>>,
    /// Replace the source decode/encode position.
    pub source_position: Option<DecodePosition>,
    /// Replace the bounded source decode limits.
    pub decode_limits: Option<DecodeLimits>,
    /// Replace the source-face resource budget.
    pub source_budget: Option<FaceBudget>,
    /// Inherit, clear, or replace the result codec name.
    pub result_codec: Option<Option<String>>,
    /// Replace the result encode position.
    pub result_position: Option<EncodePosition>,
    /// Replace the result-face resource budget.
    pub result_budget: Option<FaceBudget>,
}

impl CodecPolicyPatch {
    /// No local policy changes.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Selects the same codec for source and result faces.
    #[must_use]
    pub fn set_codec(codec: impl Into<String>) -> Self {
        let codec = codec.into();
        Self {
            source_codec: Some(Some(codec.clone())),
            result_codec: Some(Some(codec)),
            ..Self::default()
        }
    }

    /// Clears inherited source and result codecs.
    #[must_use]
    pub fn clear_codec() -> Self {
        Self {
            source_codec: Some(None),
            result_codec: Some(None),
            ..Self::default()
        }
    }

    /// Selects a source codec without changing any other field.
    #[must_use]
    pub fn source_codec(codec: impl Into<String>) -> Self {
        Self {
            source_codec: Some(Some(codec.into())),
            ..Self::default()
        }
    }

    /// Selects a result codec without changing any other field.
    #[must_use]
    pub fn result_codec(codec: impl Into<String>) -> Self {
        Self {
            result_codec: Some(Some(codec.into())),
            ..Self::default()
        }
    }

    pub(crate) fn apply_to(&self, effective: &mut EffectiveCodecPolicy) {
        if let Some(codec) = &self.source_codec {
            effective.source_codec = codec.clone();
        }
        if let Some(position) = self.source_position {
            effective.source_position = position;
        }
        if let Some(limits) = self.decode_limits {
            effective.decode_limits = bound_decode_limits(limits);
        }
        if let Some(budget) = self.source_budget {
            effective.source_budget =
                FaceBudget::new(budget.max_bytes(), budget.max_depth(), budget.max_items());
        }
        if let Some(codec) = &self.result_codec {
            effective.result_codec = codec.clone();
        }
        if let Some(position) = self.result_position {
            effective.result_position = position;
        }
        if let Some(budget) = self.result_budget {
            effective.result_budget =
                FaceBudget::new(budget.max_bytes(), budget.max_depth(), budget.max_items());
        }
    }
}

/// Fully resolved codec and face policy at one namespace node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectiveCodecPolicy {
    source_codec: Option<String>,
    source_position: DecodePosition,
    decode_limits: DecodeLimits,
    source_budget: FaceBudget,
    result_codec: Option<String>,
    result_position: EncodePosition,
    result_budget: FaceBudget,
}

impl EffectiveCodecPolicy {
    /// Creates the bounded default policy with no selected codecs.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Resolves patches in tree-to-leaf order, replacing only fields each
    /// patch explicitly carries.
    #[must_use]
    pub fn derive(patches: impl IntoIterator<Item = CodecPolicyPatch>) -> Self {
        let mut effective = Self::default();
        for patch in patches {
            patch.apply_to(&mut effective);
        }
        effective
    }

    /// Active source codec name, when selected.
    #[must_use]
    pub fn source_codec(&self) -> Option<&str> {
        self.source_codec.as_deref()
    }

    /// Source decode position and matching source-face encode position.
    #[must_use]
    pub const fn source_position(&self) -> DecodePosition {
        self.source_position
    }

    /// Bounded limits used for edited source.
    #[must_use]
    pub const fn decode_limits(&self) -> DecodeLimits {
        self.decode_limits
    }

    /// Independent source-face budget.
    #[must_use]
    pub const fn source_budget(&self) -> FaceBudget {
        self.source_budget
    }

    /// Active result codec name, when selected.
    #[must_use]
    pub fn result_codec(&self) -> Option<&str> {
        self.result_codec.as_deref()
    }

    /// Result encode position.
    #[must_use]
    pub const fn result_position(&self) -> EncodePosition {
        self.result_position
    }

    /// Independent result-face budget.
    #[must_use]
    pub const fn result_budget(&self) -> FaceBudget {
        self.result_budget
    }
}

impl Default for EffectiveCodecPolicy {
    fn default() -> Self {
        Self {
            source_codec: None,
            source_position: DecodePosition::Data,
            decode_limits: DecodeLimits::default(),
            source_budget: FaceBudget::default(),
            result_codec: None,
            result_position: EncodePosition::Data,
            result_budget: FaceBudget::default(),
        }
    }
}

fn bound_decode_limits(requested: DecodeLimits) -> DecodeLimits {
    let hard = DecodeLimits::default();
    DecodeLimits {
        max_input_bytes: requested.max_input_bytes.min(hard.max_input_bytes),
        max_tokens: requested.max_tokens.min(hard.max_tokens),
        max_expr_nodes: requested.max_expr_nodes.min(hard.max_expr_nodes),
        max_depth: requested.max_depth.min(hard.max_depth),
        max_string_bytes: requested.max_string_bytes.min(hard.max_string_bytes),
        max_blob_bytes: requested.max_blob_bytes.min(hard.max_blob_bytes),
        max_collection_len: requested.max_collection_len.min(hard.max_collection_len),
        max_trivia_items: requested.max_trivia_items.min(hard.max_trivia_items),
    }
}
