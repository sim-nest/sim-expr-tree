use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use sim_kernel::{Expr, Symbol};
use sim_lib_stream_core::{
    BufferPolicy, PushResult, StreamDirection, StreamItem, StreamMedia, StreamMetadata,
    StreamPacket, StreamValue, stream_cancel_bang, stream_next_bang,
};

/// A bounded standard stream endpoint carrying expression-tree progress and
/// change packets.
#[derive(Clone)]
pub struct CalcWatch {
    stream: Arc<StreamValue>,
    overflow_evidence: Arc<AtomicU64>,
}

impl CalcWatch {
    pub(super) fn new(id: u64, policy: BufferPolicy) -> Self {
        let metadata = StreamMetadata::new(
            Symbol::qualified("expr-tree/watch", id.to_string()),
            StreamMedia::Data,
            StreamDirection::Source,
            Symbol::qualified("clock", "control"),
            policy,
        );
        Self {
            stream: Arc::new(StreamValue::push(metadata)),
            overflow_evidence: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Returns the existing standard stream value.
    #[must_use]
    pub fn stream(&self) -> &Arc<StreamValue> {
        &self.stream
    }

    /// Consumes the next packet through the ordinary stream operation.
    pub fn next(&self) -> sim_kernel::Result<Option<StreamItem>> {
        stream_next_bang(&self.stream)
    }

    /// Cancels the endpoint through the ordinary stream operation.
    pub fn cancel(&self) -> sim_kernel::Result<()> {
        stream_cancel_bang(&self.stream)
    }

    /// Returns explicit lifetime overflow evidence for this endpoint.
    #[must_use]
    pub fn overflow_evidence(&self) -> u64 {
        self.overflow_evidence.load(Ordering::Acquire)
    }

    pub(super) fn emit(&self, kind: &'static str, fields: Vec<(Expr, Expr)>) {
        let overflow_before = self.overflow_evidence();
        let mut payload = vec![
            (
                Expr::Symbol(Symbol::new("kind")),
                Expr::Symbol(Symbol::qualified("expr-tree", kind)),
            ),
            (
                Expr::Symbol(Symbol::new("prior-overflows")),
                Expr::String(overflow_before.to_string()),
            ),
        ];
        payload.extend(fields);
        let item = StreamItem::new(StreamPacket::data(
            Symbol::qualified("expr-tree", kind),
            Expr::Map(payload),
        ));
        let overflowed = matches!(
            self.stream.push_packet(item),
            Ok(PushResult::DroppedNewest(_))
                | Ok(PushResult::DroppedOldest(_))
                | Ok(PushResult::Rejected(_))
                | Ok(PushResult::Closed(_))
                | Err(_)
        );
        if overflowed {
            self.overflow_evidence.fetch_add(1, Ordering::AcqRel);
        }
    }
}
