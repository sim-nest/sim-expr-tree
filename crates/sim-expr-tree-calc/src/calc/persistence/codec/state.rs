use super::graph::{decode_observation_kind, decode_query, encode_query, observation_kind_name};
use super::*;

pub(super) fn encode_receipts(receipts: &BTreeMap<String, CalcReceipt>) -> Expr {
    Expr::Vector(receipts.values().map(encode_receipt).collect())
}

pub(super) fn decode_receipts(expr: &Expr) -> DecodeResult<BTreeMap<String, CalcReceipt>> {
    let mut receipts = BTreeMap::new();
    for row in vector(expr)? {
        let receipt = decode_receipt(row)?;
        if receipts.insert(receipt.cell.clone(), receipt).is_some() {
            return corrupt("duplicate persisted receipt");
        }
    }
    Ok(receipts)
}

fn encode_receipt(receipt: &CalcReceipt) -> Expr {
    record(vec![
        ("request-id", number(receipt.request_id.get())),
        ("cell", text(&receipt.cell)),
        ("source-revision", number(receipt.source_revision)),
        ("policy-digest", number(receipt.policy_digest.get())),
        ("authority-digest", number(receipt.authority_digest.get())),
        (
            "dependencies",
            Expr::Vector(
                receipt
                    .dependencies
                    .iter()
                    .map(|stamp| {
                        record(vec![
                            ("query", encode_query(&stamp.query)),
                            ("kind", text(observation_kind_name(&stamp.kind))),
                            ("revision", number(stamp.revision)),
                            ("fingerprint", optional_number(stamp.fingerprint)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "omitted-dependencies",
            number(receipt.omitted_dependencies as u64),
        ),
        ("dependency-digest", number(receipt.dependency_digest)),
        (
            "effects",
            Expr::Vector(
                receipt
                    .effects
                    .iter()
                    .map(|effect| {
                        record(vec![
                            ("kind", text(&effect.kind)),
                            ("aborted", Expr::Bool(effect.aborted)),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("omitted-effects", number(receipt.omitted_effects as u64)),
        ("started-tick", number(receipt.started_tick)),
        ("finished-tick", number(receipt.finished_tick)),
        ("wall-started-ms", optional_number(receipt.wall_started_ms)),
        (
            "wall-finished-ms",
            optional_number(receipt.wall_finished_ms),
        ),
        ("outcome", encode_outcome(&receipt.outcome)),
        (
            "result-fingerprint",
            optional_number(receipt.result_fingerprint),
        ),
        ("reason", text(reason_name(receipt.reason))),
        ("trigger", text(trigger_name(receipt.trigger))),
    ])
}

fn decode_receipt(expr: &Expr) -> DecodeResult<CalcReceipt> {
    let fields = record_fields(expr)?;
    Ok(CalcReceipt {
        request_id: RequestId::new(parse_u64(required(&fields, "request-id")?)?),
        cell: parse_text(required(&fields, "cell")?)?.to_owned(),
        source_revision: parse_u64(required(&fields, "source-revision")?)?,
        policy_digest: PolicyDigest::from_persisted(parse_u64(required(
            &fields,
            "policy-digest",
        )?)?),
        authority_digest: AuthorityDigest::from_persisted(parse_u64(required(
            &fields,
            "authority-digest",
        )?)?),
        dependencies: vector(required(&fields, "dependencies")?)?
            .iter()
            .map(|row| {
                let row_fields = record_fields(row)?;
                Ok(DependencyStamp {
                    query: decode_query(required(&row_fields, "query")?)?,
                    kind: decode_observation_kind(parse_text(required(&row_fields, "kind")?)?)?,
                    revision: parse_u64(required(&row_fields, "revision")?)?,
                    fingerprint: parse_optional_u64(required(&row_fields, "fingerprint")?)?,
                })
            })
            .collect::<DecodeResult<Vec<_>>>()?,
        omitted_dependencies: parse_usize(required(&fields, "omitted-dependencies")?)?,
        dependency_digest: parse_u64(required(&fields, "dependency-digest")?)?,
        effects: vector(required(&fields, "effects")?)?
            .iter()
            .map(|row| {
                let row_fields = record_fields(row)?;
                Ok(EffectStamp {
                    kind: parse_text(required(&row_fields, "kind")?)?.to_owned(),
                    aborted: parse_bool(required(&row_fields, "aborted")?)?,
                })
            })
            .collect::<DecodeResult<Vec<_>>>()?,
        omitted_effects: parse_usize(required(&fields, "omitted-effects")?)?,
        started_tick: parse_u64(required(&fields, "started-tick")?)?,
        finished_tick: parse_u64(required(&fields, "finished-tick")?)?,
        wall_started_ms: parse_optional_u64(required(&fields, "wall-started-ms")?)?,
        wall_finished_ms: parse_optional_u64(required(&fields, "wall-finished-ms")?)?,
        outcome: decode_outcome(required(&fields, "outcome")?)?,
        result_fingerprint: parse_optional_u64(required(&fields, "result-fingerprint")?)?,
        reason: decode_reason(parse_text(required(&fields, "reason")?)?)?,
        trigger: decode_trigger(parse_text(required(&fields, "trigger")?)?)?,
    })
}

fn encode_outcome(outcome: &CalcOutcome) -> Expr {
    match outcome {
        CalcOutcome::Succeeded => record(vec![("kind", text("succeeded"))]),
        CalcOutcome::Failed { message } => {
            record(vec![("kind", text("failed")), ("message", text(message))])
        }
        CalcOutcome::Blocked { message } => {
            record(vec![("kind", text("blocked")), ("message", text(message))])
        }
        CalcOutcome::Cancelled => record(vec![("kind", text("cancelled"))]),
        CalcOutcome::BudgetExhausted {
            message,
            continuation,
        } => record(vec![
            ("kind", text("budget-exhausted")),
            ("message", text(message)),
            ("continuation", optional_number(*continuation)),
        ]),
    }
}

fn decode_outcome(expr: &Expr) -> DecodeResult<CalcOutcome> {
    let fields = record_fields(expr)?;
    Ok(match parse_text(required(&fields, "kind")?)? {
        "succeeded" => CalcOutcome::Succeeded,
        "failed" => CalcOutcome::Failed {
            message: parse_text(required(&fields, "message")?)?.to_owned(),
        },
        "blocked" => CalcOutcome::Blocked {
            message: parse_text(required(&fields, "message")?)?.to_owned(),
        },
        "cancelled" => CalcOutcome::Cancelled,
        "budget-exhausted" => CalcOutcome::BudgetExhausted {
            message: parse_text(required(&fields, "message")?)?.to_owned(),
            continuation: parse_optional_u64(required(&fields, "continuation")?)?,
        },
        other => return corrupt(format!("unknown calculation outcome {other:?}")),
    })
}

fn reason_name(reason: CalcReason) -> &'static str {
    match reason {
        CalcReason::DirectedVerify => "directed-verify",
        CalcReason::DirectedForceRoots => "directed-force-roots",
        CalcReason::DirectedForceRecursive => "directed-force-recursive",
        CalcReason::AutomaticMutation => "automatic-mutation",
        CalcReason::Continuation => "continuation",
    }
}

fn decode_reason(name: &str) -> DecodeResult<CalcReason> {
    Ok(match name {
        "directed-verify" => CalcReason::DirectedVerify,
        "directed-force-roots" => CalcReason::DirectedForceRoots,
        "directed-force-recursive" => CalcReason::DirectedForceRecursive,
        "automatic-mutation" => CalcReason::AutomaticMutation,
        "continuation" => CalcReason::Continuation,
        other => return corrupt(format!("unknown calculation reason {other:?}")),
    })
}

fn trigger_name(trigger: CalcTrigger) -> &'static str {
    match trigger {
        CalcTrigger::Automatic => "automatic",
        CalcTrigger::OnDemand => "on-demand",
        CalcTrigger::Manual => "manual",
        CalcTrigger::Frozen => "frozen",
    }
}

fn decode_trigger(name: &str) -> DecodeResult<CalcTrigger> {
    Ok(match name {
        "automatic" => CalcTrigger::Automatic,
        "on-demand" => CalcTrigger::OnDemand,
        "manual" => CalcTrigger::Manual,
        "frozen" => CalcTrigger::Frozen,
        other => return corrupt(format!("unknown calculation trigger {other:?}")),
    })
}

pub(super) fn encode_queue(queue: &AutomaticQueueSnapshot) -> Expr {
    record(vec![
        ("generation", number(queue.generation)),
        ("next-sequence", number(queue.next_sequence)),
        (
            "entries",
            Expr::Vector(
                queue
                    .entries
                    .iter()
                    .map(|entry| {
                        record(vec![
                            ("request-id", number(entry.request_id.get())),
                            ("cell", text(&entry.cell)),
                            ("ready-at-ms", number(entry.ready_at_ms)),
                            ("priority", text(entry.priority.to_string())),
                            ("sequence", number(entry.sequence)),
                            ("bypasses", number(u64::from(entry.bypasses))),
                            (
                                "incremental-continuation",
                                optional_number(
                                    entry.incremental_continuation.map(ContinuationToken::get),
                                ),
                            ),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}

pub(super) fn decode_queue(expr: &Expr) -> DecodeResult<AutomaticQueueSnapshot> {
    let fields = record_fields(expr)?;
    Ok(AutomaticQueueSnapshot {
        generation: parse_u64(required(&fields, "generation")?)?,
        next_sequence: parse_u64(required(&fields, "next-sequence")?)?,
        entries: vector(required(&fields, "entries")?)?
            .iter()
            .map(|row| {
                let row_fields = record_fields(row)?;
                Ok(QueuedCalculation {
                    request_id: RequestId::new(parse_u64(required(&row_fields, "request-id")?)?),
                    cell: parse_text(required(&row_fields, "cell")?)?.to_owned(),
                    ready_at_ms: parse_u64(required(&row_fields, "ready-at-ms")?)?,
                    priority: parse_text(required(&row_fields, "priority")?)?
                        .parse()
                        .map_err(|_| DecodeError::Corrupt("invalid queue priority".to_owned()))?,
                    sequence: parse_u64(required(&row_fields, "sequence")?)?,
                    bypasses: parse_u64(required(&row_fields, "bypasses")?)?
                        .try_into()
                        .map_err(|_| {
                            DecodeError::Corrupt("invalid queue bypass count".to_owned())
                        })?,
                    incremental_continuation: parse_optional_u64(required(
                        &row_fields,
                        "incremental-continuation",
                    )?)?
                    .map(ContinuationToken::new),
                })
            })
            .collect::<DecodeResult<Vec<_>>>()?,
    })
}

pub(super) fn encode_refresh_samples(samples: &BTreeMap<String, BackendRefreshSample>) -> Expr {
    Expr::Vector(
        samples
            .iter()
            .map(|(mount, sample)| {
                record(vec![
                    ("mount", text(mount)),
                    ("epoch", number(sample.epoch.value())),
                    ("listings", encode_u64_map(&sample.listings)),
                    ("stamps", encode_u64_map(&sample.stamps)),
                ])
            })
            .collect(),
    )
}

pub(super) fn decode_refresh_samples(
    expr: &Expr,
) -> DecodeResult<BTreeMap<String, BackendRefreshSample>> {
    let mut samples = BTreeMap::new();
    for row in vector(expr)? {
        let fields = record_fields(row)?;
        let mount = parse_text(required(&fields, "mount")?)?.to_owned();
        let sample = BackendRefreshSample {
            epoch: MountEpoch::new(parse_u64(required(&fields, "epoch")?)?),
            listings: decode_u64_map(required(&fields, "listings")?)?,
            stamps: decode_u64_map(required(&fields, "stamps")?)?,
        };
        if samples.insert(mount, sample).is_some() {
            return corrupt("duplicate refresh sample mount");
        }
    }
    Ok(samples)
}

fn encode_u64_map(values: &BTreeMap<String, u64>) -> Expr {
    Expr::Vector(
        values
            .iter()
            .map(|(key, value)| record(vec![("key", text(key)), ("value", number(*value))]))
            .collect(),
    )
}

fn decode_u64_map(expr: &Expr) -> DecodeResult<BTreeMap<String, u64>> {
    let mut values = BTreeMap::new();
    for row in vector(expr)? {
        let fields = record_fields(row)?;
        let key = parse_text(required(&fields, "key")?)?.to_owned();
        let value = parse_u64(required(&fields, "value")?)?;
        if values.insert(key, value).is_some() {
            return corrupt("duplicate persisted map key");
        }
    }
    Ok(values)
}

pub(super) fn encode_expr_map(values: &BTreeMap<String, Expr>) -> Expr {
    Expr::Vector(
        values
            .iter()
            .map(|(key, value)| record(vec![("key", text(key)), ("value", value.clone())]))
            .collect(),
    )
}

pub(super) fn decode_expr_map(expr: &Expr) -> DecodeResult<BTreeMap<String, Expr>> {
    let mut values = BTreeMap::new();
    for row in vector(expr)? {
        let fields = record_fields(row)?;
        let key = parse_text(required(&fields, "key")?)?.to_owned();
        let value = required(&fields, "value")?.clone();
        if values.insert(key, value).is_some() {
            return corrupt("duplicate persisted expression key");
        }
    }
    Ok(values)
}
