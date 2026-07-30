//! Internal total and per-subtree rendering budgets.

#[derive(Clone, Copy)]
pub(crate) struct RenderBudget {
    pub(crate) nodes: usize,
    pub(crate) depth: usize,
    pub(crate) encoded_bytes: usize,
    pub(crate) face_bytes: usize,
}

impl RenderBudget {
    pub(crate) const fn new(
        nodes: usize,
        depth: usize,
        encoded_bytes: usize,
        face_bytes: usize,
    ) -> Self {
        Self {
            nodes,
            depth,
            encoded_bytes,
            face_bytes,
        }
    }

    pub(crate) const fn interactive() -> Self {
        Self::new(512, 32, 256 * 1024, 8 * 1024)
    }

    pub(crate) const fn subtree(self) -> Self {
        Self::new(
            min(self.nodes, 96),
            min(self.depth, 12),
            min(self.encoded_bytes, 64 * 1024),
            self.face_bytes,
        )
    }

    pub(crate) const fn malformed(self) -> BudgetExhausted {
        BudgetExhausted::EncodedBytes {
            limit: self.encoded_bytes,
        }
    }
}

const fn min(left: usize, right: usize) -> usize {
    if left < right { left } else { right }
}

pub(crate) struct RenderBudgetState {
    budget: RenderBudget,
    nodes_used: usize,
    bytes_used: usize,
}

impl RenderBudgetState {
    pub(crate) const fn new(budget: RenderBudget) -> Self {
        Self {
            budget,
            nodes_used: 0,
            bytes_used: 0,
        }
    }

    pub(crate) fn admit(
        &mut self,
        depth: usize,
        face: Option<&str>,
        encoded_bytes: usize,
    ) -> core::result::Result<(), BudgetExhausted> {
        if self.nodes_used >= self.budget.nodes {
            return Err(BudgetExhausted::Nodes {
                limit: self.budget.nodes,
            });
        }
        if depth > self.budget.depth {
            return Err(BudgetExhausted::Depth {
                limit: self.budget.depth,
            });
        }
        if face.is_some_and(|value| value.len() > self.budget.face_bytes) {
            return Err(BudgetExhausted::FaceBytes {
                limit: self.budget.face_bytes,
            });
        }
        if self.bytes_used.saturating_add(encoded_bytes) > self.budget.encoded_bytes {
            return Err(BudgetExhausted::EncodedBytes {
                limit: self.budget.encoded_bytes,
            });
        }
        self.nodes_used += 1;
        self.bytes_used = self.bytes_used.saturating_add(encoded_bytes);
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub(crate) enum BudgetExhausted {
    Nodes { limit: usize },
    Depth { limit: usize },
    EncodedBytes { limit: usize },
    FaceBytes { limit: usize },
}

impl BudgetExhausted {
    pub(crate) const fn reason(self) -> &'static str {
        match self {
            Self::Nodes { .. } => "nodes",
            Self::Depth { .. } => "depth",
            Self::EncodedBytes { .. } => "encoded-bytes",
            Self::FaceBytes { .. } => "face-bytes",
        }
    }

    pub(crate) const fn limit(self) -> usize {
        match self {
            Self::Nodes { limit }
            | Self::Depth { limit }
            | Self::EncodedBytes { limit }
            | Self::FaceBytes { limit } => limit,
        }
    }
}
