/// A codec policy inherited through the finite namespace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodecPolicy {
    codec: String,
}

impl CodecPolicy {
    /// Create a named codec policy.
    pub fn new(codec: impl Into<String>) -> Self {
        Self {
            codec: codec.into(),
        }
    }

    /// The selected codec name.
    pub fn codec(&self) -> &str {
        &self.codec
    }
}

/// Patch applied at one namespace node.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PolicyPatch {
    codec: Option<Option<CodecPolicy>>,
}

impl PolicyPatch {
    /// No local policy changes.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Set or replace the inherited codec policy at this node.
    pub fn set_codec(codec: CodecPolicy) -> Self {
        Self {
            codec: Some(Some(codec)),
        }
    }

    /// Clear an inherited codec policy for this node and descendants.
    pub fn clear_codec() -> Self {
        Self { codec: Some(None) }
    }

    pub(crate) fn apply_to(&self, effective: &mut EffectivePolicy) {
        if let Some(codec) = &self.codec {
            effective.codec = codec.clone();
        }
    }
}

/// Fully resolved policy at a namespace node.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EffectivePolicy {
    codec: Option<CodecPolicy>,
}

impl EffectivePolicy {
    /// Create an empty effective policy.
    pub fn empty() -> Self {
        Self::default()
    }

    /// The active codec policy, when any.
    pub fn codec(&self) -> Option<&CodecPolicy> {
        self.codec.as_ref()
    }
}
