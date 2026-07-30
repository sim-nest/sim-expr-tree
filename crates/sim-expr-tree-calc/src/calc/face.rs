use sim_codec::{
    DecodePosition, DecodedForm, Input, Output, decode_default_with_codec_and_limits,
    encode_with_codec,
};
use sim_expr_tree_core::FaceBudget;
use sim_kernel::{
    CapabilitySet, Cx, EncodeOptions, EncodePosition, Expr, ReadPolicy, Symbol, TrustLevel,
    read_eval_capability,
};
use sim_table_core::TablePath;

use super::*;

mod budget;
mod model;

use budget::{bounded_value_expr, inspect_expr};
pub use model::{
    EncodedFace, FaceContent, FaceDimension, FaceIssue, FaceMetadata, FacePosition,
    SourceEditOutcome,
};

const MAX_METADATA_MESSAGE_BYTES: usize = 1_024;

impl ExprTreeCalc {
    /// Decodes edited source through the inherited installed codec under
    /// explicit position, limits, caller trust, and diminished cell authority.
    ///
    /// A rejected edit leaves the prior source unchanged.
    pub fn edit_cell_source(
        &mut self,
        path: TablePath,
        input: Input,
        caller_policy: ReadPolicy,
    ) -> SourceEditOutcome {
        let policy = self.effective_codec_policy(&path);
        let position = policy.source_position();
        let Some(codec_name) = policy.source_codec().map(str::to_owned) else {
            return SourceEditOutcome {
                metadata: metadata(
                    None,
                    position.into(),
                    FaceIssue::Unsupported {
                        reason: "no source codec selected".to_owned(),
                    },
                ),
            };
        };

        if position == DecodePosition::Eval && caller_policy.trust == TrustLevel::Untrusted {
            return SourceEditOutcome {
                metadata: metadata(
                    Some(codec_name),
                    position.into(),
                    FaceIssue::CodecFailure {
                        message: "eval-position source requires a trusted caller".to_owned(),
                    },
                ),
            };
        }

        let authority = self.effective_authority(&path);
        let read_policy = diminished_read_policy(caller_policy, authority.capabilities(), position);
        let codec = codec_symbol(&codec_name);
        let limits = policy.decode_limits();
        let mut cx = (self.context_factory)();
        let decoded = cx.with_capabilities(authority.capabilities().clone(), |cx| {
            decode_default_with_codec_and_limits(cx, &codec, input, read_policy, position, limits)
        });
        match decoded {
            Ok(decoded) => {
                let source = match decoded {
                    DecodedForm::Datum(datum) => Expr::from(datum),
                    DecodedForm::Term(term) => Expr::from(term),
                };
                self.set_cell(path, source);
                SourceEditOutcome {
                    metadata: metadata(Some(codec_name), position.into(), FaceIssue::Complete),
                }
            }
            Err(error) => SourceEditOutcome {
                metadata: metadata(
                    Some(codec_name),
                    position.into(),
                    FaceIssue::CodecFailure {
                        message: bounded_message(error.to_string()),
                    },
                ),
            },
        }
    }

    /// Encodes the authored source expression under its inherited codec,
    /// position, and source-only face budget.
    #[must_use]
    pub fn source_face(&self, path: &TablePath) -> EncodedFace {
        let policy = self.effective_codec_policy(path);
        let position = encode_position_for_decode(policy.source_position());
        let Some(codec_name) = policy.source_codec().map(str::to_owned) else {
            return unsupported_face(None, position, "no source codec selected");
        };
        let source = self
            .state
            .read()
            .expect("calc state poisoned")
            .cells
            .get(&path_key(path))
            .cloned();
        let Some(source) = source else {
            return unsupported_face(Some(codec_name), position, "cell has no authored source");
        };
        let mut cx = (self.context_factory)();
        encode_expr_face(
            &mut cx,
            codec_name,
            position,
            &source,
            policy.source_budget(),
        )
    }

    /// Encodes the current arbitrary result under its inherited codec,
    /// position, and result-only face budget.
    ///
    /// The ordinary [`Value`] remains in the calculator regardless of whether
    /// it has a safe bounded presentation projection.
    #[must_use]
    pub fn result_face(&self, path: &TablePath) -> EncodedFace {
        let policy = self.effective_codec_policy(path);
        let position = policy.result_position();
        let Some(codec_name) = policy.result_codec().map(str::to_owned) else {
            return unsupported_face(None, position, "no result codec selected");
        };
        let value = match self.current_cell(path) {
            Ok(value) => value,
            Err(error) => {
                return unsupported_face(
                    Some(codec_name),
                    position,
                    &format!("no current result: {}", bounded_message(error.to_string())),
                );
            }
        };
        let budget = policy.result_budget();
        let mut cx = (self.context_factory)();
        let expr = match bounded_value_expr(&mut cx, &value, budget) {
            Ok(expr) => expr,
            Err(issue) => {
                return EncodedFace {
                    content: None,
                    metadata: metadata(Some(codec_name), position.into(), issue),
                };
            }
        };
        encode_preflighted_expr_face(&mut cx, codec_name, position, &expr, budget)
    }
}

fn encode_expr_face(
    cx: &mut Cx,
    codec_name: String,
    position: EncodePosition,
    expr: &Expr,
    budget: FaceBudget,
) -> EncodedFace {
    if let Err(issue) = inspect_expr(expr, budget) {
        return EncodedFace {
            content: None,
            metadata: metadata(Some(codec_name), position.into(), issue),
        };
    }
    encode_preflighted_expr_face(cx, codec_name, position, expr, budget)
}

fn encode_preflighted_expr_face(
    cx: &mut Cx,
    codec_name: String,
    position: EncodePosition,
    expr: &Expr,
    budget: FaceBudget,
) -> EncodedFace {
    let codec = codec_symbol(&codec_name);
    let options = EncodeOptions {
        position,
        ..EncodeOptions::default()
    };
    match encode_with_codec(cx, &codec, expr, options) {
        Ok(output) => {
            let output_bytes = output_len(&output);
            if output_bytes > budget.max_bytes() {
                return EncodedFace {
                    content: None,
                    metadata: metadata(
                        Some(codec_name),
                        position.into(),
                        FaceIssue::Truncated {
                            dimension: FaceDimension::Bytes,
                            limit: budget.max_bytes(),
                            observed: output_bytes,
                        },
                    ),
                };
            }
            EncodedFace {
                content: Some(match output {
                    Output::Text(text) => FaceContent::Text(text),
                    Output::Bytes(bytes) => FaceContent::Bytes(bytes),
                }),
                metadata: metadata(Some(codec_name), position.into(), FaceIssue::Complete),
            }
        }
        Err(error) => EncodedFace {
            content: None,
            metadata: metadata(
                Some(codec_name),
                position.into(),
                FaceIssue::CodecFailure {
                    message: bounded_message(error.to_string()),
                },
            ),
        },
    }
}

fn unsupported_face(codec: Option<String>, position: EncodePosition, reason: &str) -> EncodedFace {
    EncodedFace {
        content: None,
        metadata: metadata(
            codec,
            position.into(),
            FaceIssue::Unsupported {
                reason: bounded_message(reason.to_owned()),
            },
        ),
    }
}

fn output_len(output: &Output) -> usize {
    match output {
        Output::Text(text) => text.len(),
        Output::Bytes(bytes) => bytes.len(),
    }
}

fn encode_position_for_decode(position: DecodePosition) -> EncodePosition {
    match position {
        DecodePosition::Eval => EncodePosition::Eval,
        DecodePosition::Quote => EncodePosition::Quote,
        DecodePosition::Data => EncodePosition::Data,
        DecodePosition::Pattern => EncodePosition::Pattern,
    }
}

fn diminished_read_policy(
    caller: ReadPolicy,
    authority: &CapabilitySet,
    position: DecodePosition,
) -> ReadPolicy {
    let mut capabilities = caller.capabilities.intersect(authority);
    if position != DecodePosition::Eval {
        let read_eval = read_eval_capability();
        capabilities = capabilities
            .iter()
            .filter(|capability| *capability != &read_eval)
            .cloned()
            .fold(CapabilitySet::new(), CapabilitySet::grant);
    }
    ReadPolicy {
        trust: caller.trust,
        capabilities,
    }
}

fn codec_symbol(name: &str) -> Symbol {
    name.split_once('/')
        .or_else(|| name.split_once(':'))
        .map_or_else(
            || Symbol::new(name.to_owned()),
            |(namespace, local)| Symbol::qualified(namespace.to_owned(), local.to_owned()),
        )
}

fn metadata(codec: Option<String>, position: FacePosition, issue: FaceIssue) -> FaceMetadata {
    FaceMetadata {
        codec,
        position,
        issue,
    }
}

fn bounded_message(message: String) -> String {
    if message.len() <= MAX_METADATA_MESSAGE_BYTES {
        return message;
    }
    let mut end = MAX_METADATA_MESSAGE_BYTES;
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...[metadata truncated]", &message[..end])
}
