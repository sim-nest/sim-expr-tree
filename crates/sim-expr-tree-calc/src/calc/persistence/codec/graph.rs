use super::*;

pub(super) fn encode_node(
    node: &SnapshotNode<CalcQuery, MemoValue>,
    cx: &mut Cx,
) -> Result<Expr, DerivedSnapshotError> {
    Ok(record(vec![
        ("key", encode_query(&node.key)),
        ("revision", number(node.revision.get())),
        ("dirty", Expr::Bool(node.dirty)),
        ("value", encode_optional_memo(node.value.as_ref(), cx)?),
        (
            "fingerprint",
            optional_number(node.fingerprint.map(ValueFingerprint::get)),
        ),
        (
            "observations",
            Expr::Vector(node.dependencies.iter().map(encode_observation).collect()),
        ),
    ]))
}

pub(super) fn decode_node(
    expr: &Expr,
    cx: &mut Cx,
) -> DecodeResult<SnapshotNode<CalcQuery, MemoValue>> {
    let fields = record_fields(expr)?;
    let (value, reusable) = decode_optional_memo(required(&fields, "value")?, cx)?;
    Ok(SnapshotNode {
        key: decode_query(required(&fields, "key")?)?,
        revision: Revision::new(parse_u64(required(&fields, "revision")?)?),
        dirty: parse_bool(required(&fields, "dirty")?)? || !reusable,
        value,
        fingerprint: parse_optional_u64(required(&fields, "fingerprint")?)?
            .map(ValueFingerprint::new),
        dependencies: vector(required(&fields, "observations")?)?
            .iter()
            .map(decode_observation)
            .collect::<DecodeResult<Vec<_>>>()?,
    })
}

fn encode_optional_memo(
    memo: Option<&MemoValue>,
    cx: &mut Cx,
) -> Result<Expr, DerivedSnapshotError> {
    let Some(memo) = memo else {
        return Ok(Expr::Nil);
    };
    match &memo.outcome {
        MemoOutcome::Value(value) => Ok(record(vec![
            ("kind", text("value")),
            ("expr", value.object().as_expr(cx).map_err(backend_error)?),
            ("volatile", Expr::Bool(memo.is_volatile())),
        ])),
        MemoOutcome::Failure(failure) => Ok(record(vec![
            ("kind", text("failure")),
            ("failure", encode_failure(failure)),
        ])),
    }
}

fn decode_optional_memo(expr: &Expr, cx: &mut Cx) -> DecodeResult<(Option<MemoValue>, bool)> {
    if matches!(expr, Expr::Nil) {
        return Ok((None, true));
    }
    let fields = record_fields(expr)?;
    match parse_text(required(&fields, "kind")?)? {
        "value" => {
            let projected = required(&fields, "expr")?.clone();
            let (value, structurally_reusable) = restore_value(cx, projected.clone())
                .map_err(|error| DecodeError::Corrupt(error.to_string()))?;
            let volatile = parse_bool(required(&fields, "volatile")?)?;
            let memo = if volatile {
                MemoValue::volatile(value, 0)
            } else {
                MemoValue::canonical(value, projected.canonical_key())
            };
            Ok((Some(memo), structurally_reusable && !volatile))
        }
        "failure" => Ok((
            Some(MemoValue::failure(decode_failure(required(
                &fields, "failure",
            )?)?)),
            true,
        )),
        other => corrupt(format!("unknown memo kind {other:?}")),
    }
}

pub(in crate::calc::persistence) fn restore_value(
    cx: &mut Cx,
    expr: Expr,
) -> sim_kernel::Result<(Value, bool)> {
    match expr {
        Expr::Nil => cx.factory().nil().map(|value| (value, true)),
        Expr::Bool(raw) => cx.factory().bool(raw).map(|value| (value, true)),
        Expr::Number(raw) => cx
            .factory()
            .number_literal(raw.domain, raw.canonical)
            .map(|value| (value, true)),
        Expr::Symbol(raw) => cx.factory().symbol(raw).map(|value| (value, true)),
        Expr::String(raw) => cx.factory().string(raw).map(|value| (value, true)),
        Expr::Bytes(raw) => cx.factory().bytes(raw).map(|value| (value, true)),
        Expr::List(items) => {
            let mut reusable = true;
            let values = items
                .into_iter()
                .map(|item| {
                    let (value, item_reusable) = restore_value(cx, item)?;
                    reusable &= item_reusable;
                    Ok(value)
                })
                .collect::<sim_kernel::Result<Vec<_>>>()?;
            cx.factory().list(values).map(|value| (value, reusable))
        }
        Expr::Map(entries) => {
            let mut reusable = true;
            let values = entries
                .into_iter()
                .map(|(key, value)| {
                    let Expr::Symbol(key) = key else {
                        return Err(sim_kernel::Error::Eval(
                            "persisted value map key is not a symbol".to_owned(),
                        ));
                    };
                    let (value, item_reusable) = restore_value(cx, value)?;
                    reusable &= item_reusable;
                    Ok((key, value))
                })
                .collect::<sim_kernel::Result<Vec<_>>>()?;
            cx.factory().table(values).map(|value| (value, reusable))
        }
        other => cx.factory().expr(other).map(|value| (value, false)),
    }
}

fn encode_observation(observation: &Observation<CalcQuery>) -> Expr {
    record(vec![
        ("key", encode_query(observation.key())),
        ("kind", text(observation_kind_name(observation.kind()))),
        ("revision", number(observation.revision().get())),
        (
            "fingerprint",
            optional_number(observation.fingerprint().map(ValueFingerprint::get)),
        ),
    ])
}

fn decode_observation(expr: &Expr) -> DecodeResult<Observation<CalcQuery>> {
    let fields = record_fields(expr)?;
    Ok(Observation::new(
        decode_query(required(&fields, "key")?)?,
        decode_observation_kind(parse_text(required(&fields, "kind")?)?)?,
        Revision::new(parse_u64(required(&fields, "revision")?)?),
        parse_optional_u64(required(&fields, "fingerprint")?)?.map(ValueFingerprint::new),
    ))
}

pub(super) fn observation_kind_name(kind: &ObservationKind) -> &'static str {
    match kind {
        ObservationKind::Read => "read",
        ObservationKind::Missing => "missing",
        ObservationKind::Listing => "listing",
        ObservationKind::Policy => "policy",
        ObservationKind::Epoch => "epoch",
        ObservationKind::Custom("cell-source") => "custom:cell-source",
        ObservationKind::Custom("lexical-binding") => "custom:lexical-binding",
        ObservationKind::Custom("codec-registry") => "custom:codec-registry",
        ObservationKind::Custom("force-epoch") => "custom:force-epoch",
        ObservationKind::Custom("lookup-step") => "custom:lookup-step",
        ObservationKind::Custom(_) => "custom:unsupported",
    }
}

pub(super) fn decode_observation_kind(name: &str) -> DecodeResult<ObservationKind> {
    Ok(match name {
        "read" => ObservationKind::Read,
        "missing" => ObservationKind::Missing,
        "listing" => ObservationKind::Listing,
        "policy" => ObservationKind::Policy,
        "epoch" => ObservationKind::Epoch,
        "custom:cell-source" => ObservationKind::Custom("cell-source"),
        "custom:lexical-binding" => ObservationKind::Custom("lexical-binding"),
        "custom:codec-registry" => ObservationKind::Custom("codec-registry"),
        "custom:force-epoch" => ObservationKind::Custom("force-epoch"),
        "custom:lookup-step" => ObservationKind::Custom("lookup-step"),
        other => return corrupt(format!("unknown observation kind {other:?}")),
    })
}

pub(super) fn encode_query(query: &CalcQuery) -> Expr {
    let (kind, argument) = match query {
        CalcQuery::Cell(value) => ("cell", Some(value.as_str())),
        CalcQuery::NameSlot(value) => ("name-slot", Some(value.as_str())),
        CalcQuery::LookupStep(value) => ("lookup-step", Some(value.as_str())),
        CalcQuery::Listing(value) => ("listing", Some(value.as_str())),
        CalcQuery::MountEpoch(value) => ("mount-epoch", Some(value.as_str())),
        CalcQuery::EffectivePolicy(value) => ("effective-policy", Some(value.as_str())),
        CalcQuery::AuthorityPolicy(value) => ("authority-policy", Some(value.as_str())),
        CalcQuery::CodecRegistry => ("codec-registry", None),
        CalcQuery::AuthorityCeiling => ("authority-ceiling", None),
        CalcQuery::ForceEpoch(value) => ("force-epoch", Some(value.as_str())),
    };
    record(vec![
        ("kind", text(kind)),
        (
            "argument",
            argument.map_or(Expr::Nil, |value| Expr::String(value.to_owned())),
        ),
    ])
}

pub(super) fn decode_query(expr: &Expr) -> DecodeResult<CalcQuery> {
    let fields = record_fields(expr)?;
    let kind = parse_text(required(&fields, "kind")?)?;
    let argument = match required(&fields, "argument")? {
        Expr::Nil => None,
        Expr::String(value) => Some(value.clone()),
        _ => return corrupt("query argument is not text or nil"),
    };
    let required_argument = || {
        argument
            .clone()
            .ok_or_else(|| DecodeError::Corrupt(format!("{kind} query has no argument")))
    };
    Ok(match kind {
        "cell" => CalcQuery::Cell(required_argument()?),
        "name-slot" => CalcQuery::NameSlot(required_argument()?),
        "lookup-step" => CalcQuery::LookupStep(required_argument()?),
        "listing" => CalcQuery::Listing(required_argument()?),
        "mount-epoch" => CalcQuery::MountEpoch(required_argument()?),
        "effective-policy" => CalcQuery::EffectivePolicy(required_argument()?),
        "authority-policy" => CalcQuery::AuthorityPolicy(required_argument()?),
        "codec-registry" if argument.is_none() => CalcQuery::CodecRegistry,
        "authority-ceiling" if argument.is_none() => CalcQuery::AuthorityCeiling,
        "force-epoch" => CalcQuery::ForceEpoch(required_argument()?),
        other => return corrupt(format!("unknown query kind {other:?}")),
    })
}

pub(super) fn encode_reverse(reverse: &BTreeMap<CalcQuery, BTreeSet<CalcQuery>>) -> Expr {
    Expr::Vector(
        reverse
            .iter()
            .map(|(key, dependents)| {
                record(vec![
                    ("key", encode_query(key)),
                    (
                        "dependents",
                        Expr::Vector(dependents.iter().map(encode_query).collect()),
                    ),
                ])
            })
            .collect(),
    )
}

pub(super) fn decode_reverse(
    expr: &Expr,
) -> DecodeResult<BTreeMap<CalcQuery, BTreeSet<CalcQuery>>> {
    let mut reverse = BTreeMap::new();
    for row in vector(expr)? {
        let fields = record_fields(row)?;
        let key = decode_query(required(&fields, "key")?)?;
        let dependents = vector(required(&fields, "dependents")?)?
            .iter()
            .map(decode_query)
            .collect::<DecodeResult<BTreeSet<_>>>()?;
        if reverse.insert(key.clone(), dependents).is_some() {
            return corrupt(format!("duplicate reverse-edge key {key:?}"));
        }
    }
    Ok(reverse)
}

fn encode_failure(failure: &CellFailure) -> Expr {
    match failure {
        CellFailure::Evaluation { message } => record(vec![
            ("kind", text("evaluation")),
            ("message", text(message)),
        ]),
        CellFailure::Cycle { path } => record(vec![
            ("kind", text("cycle")),
            (
                "path",
                Expr::Vector(path.iter().map(encode_query).collect()),
            ),
        ]),
        CellFailure::ExpressionDepth { limit } => record(vec![
            ("kind", text("expression-depth")),
            ("limit", number(*limit as u64)),
        ]),
        CellFailure::Blocked { path, reason } => record(vec![
            ("kind", text("blocked")),
            ("path", text(path)),
            ("reason", text(reason)),
        ]),
        CellFailure::RequiredCapability { path, capability } => record(vec![
            ("kind", text("required-capability")),
            ("path", text(path)),
            ("capability", text(capability.as_str())),
        ]),
    }
}

fn decode_failure(expr: &Expr) -> DecodeResult<CellFailure> {
    let fields = record_fields(expr)?;
    Ok(match parse_text(required(&fields, "kind")?)? {
        "evaluation" => CellFailure::Evaluation {
            message: parse_text(required(&fields, "message")?)?.to_owned(),
        },
        "cycle" => CellFailure::Cycle {
            path: vector(required(&fields, "path")?)?
                .iter()
                .map(decode_query)
                .collect::<DecodeResult<Vec<_>>>()?,
        },
        "expression-depth" => CellFailure::ExpressionDepth {
            limit: parse_usize(required(&fields, "limit")?)?,
        },
        "blocked" => CellFailure::Blocked {
            path: parse_text(required(&fields, "path")?)?.to_owned(),
            reason: parse_text(required(&fields, "reason")?)?.to_owned(),
        },
        "required-capability" => CellFailure::RequiredCapability {
            path: parse_text(required(&fields, "path")?)?.to_owned(),
            capability: sim_kernel::CapabilityName::new(parse_text(required(
                &fields,
                "capability",
            )?)?),
        },
        other => return corrupt(format!("unknown cell failure {other:?}")),
    })
}
