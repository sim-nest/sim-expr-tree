use super::*;

mod graph;
mod state;

pub(super) use graph::restore_value;
use graph::{decode_node, decode_reverse, encode_node, encode_reverse};
use state::{
    decode_expr_map, decode_queue, decode_receipts, decode_refresh_samples, encode_expr_map,
    encode_queue, encode_receipts, encode_refresh_samples,
};

#[derive(Debug)]
pub(super) enum DecodeError {
    Incompatible,
    Corrupt(String),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Incompatible => f.write_str("incompatible derived graph schema"),
            Self::Corrupt(message) => f.write_str(message),
        }
    }
}

type DecodeResult<T> = Result<T, DecodeError>;

pub(super) fn encode_persisted(
    persisted: &PersistedCalc,
    cx: &mut Cx,
) -> Result<Expr, DerivedSnapshotError> {
    let graph = persisted
        .graph
        .nodes
        .iter()
        .map(|node| encode_node(node, cx))
        .collect::<Result<Vec<_>, _>>()?;
    let mut body = record(vec![
        ("schema", number(persisted.schema)),
        ("source-generation", number(persisted.source_generation)),
        ("control-generation", number(persisted.control_generation)),
        ("source-identity", number(persisted.source_identity)),
        ("control-identity", number(persisted.control_identity)),
        ("graph", Expr::Vector(graph)),
        ("reverse", encode_reverse(&persisted.reverse)),
        ("receipts", encode_receipts(&persisted.receipts)),
        ("queue", encode_queue(&persisted.queue)),
        ("next-request-id", number(persisted.next_request_id)),
        ("next-logical-tick", number(persisted.next_logical_tick)),
        ("next-volatile", number(persisted.next_volatile)),
        (
            "refresh-samples",
            encode_refresh_samples(&persisted.refresh_samples),
        ),
        ("last-good", encode_expr_map(&persisted.last_good)),
    ]);
    let checksum = super::identity::expr_identity(&body);
    let Expr::Map(fields) = &mut body else {
        unreachable!("record helper must return a map")
    };
    fields.push((Expr::Symbol(Symbol::new("checksum")), number(checksum)));
    Ok(body)
}

pub(super) fn decode_persisted(expr: &Expr, cx: &mut Cx) -> DecodeResult<PersistedCalc> {
    let fields = record_fields(expr)?;
    let schema = parse_u64(required(&fields, "schema")?)?;
    if schema != GRAPH_SCHEMA_VERSION {
        return Err(DecodeError::Incompatible);
    }
    let checksum = parse_u64(required(&fields, "checksum")?)?;
    let Expr::Map(entries) = expr else {
        return corrupt("expected persisted record");
    };
    let checksum_body = Expr::Map(
        entries
            .iter()
            .filter(|(key, _)| !matches!(key, Expr::Symbol(key) if key.to_string() == "checksum"))
            .cloned()
            .collect(),
    );
    if checksum != super::identity::expr_identity(&checksum_body) {
        return corrupt("derived snapshot checksum mismatch");
    }
    let graph = GraphSnapshot::new(
        vector(required(&fields, "graph")?)?
            .iter()
            .map(|node| decode_node(node, cx))
            .collect::<DecodeResult<Vec<_>>>()?,
    );
    if graph.nodes.len() > MAX_PERSISTED_GRAPH_NODES
        || graph
            .nodes
            .iter()
            .map(|node| node.dependencies.len())
            .sum::<usize>()
            > MAX_PERSISTED_GRAPH_EDGES
    {
        return corrupt("persisted graph exceeds hard snapshot bounds");
    }
    Ok(PersistedCalc {
        schema,
        source_generation: parse_u64(required(&fields, "source-generation")?)?,
        control_generation: parse_u64(required(&fields, "control-generation")?)?,
        source_identity: parse_u64(required(&fields, "source-identity")?)?,
        control_identity: parse_u64(required(&fields, "control-identity")?)?,
        graph,
        reverse: decode_reverse(required(&fields, "reverse")?)?,
        receipts: decode_receipts(required(&fields, "receipts")?)?,
        queue: decode_queue(required(&fields, "queue")?)?,
        next_request_id: parse_u64(required(&fields, "next-request-id")?)?,
        next_logical_tick: parse_u64(required(&fields, "next-logical-tick")?)?,
        next_volatile: parse_u64(required(&fields, "next-volatile")?)?,
        refresh_samples: decode_refresh_samples(required(&fields, "refresh-samples")?)?,
        last_good: decode_expr_map(required(&fields, "last-good")?)?,
    })
}

fn record(entries: Vec<(&str, Expr)>) -> Expr {
    Expr::Map(
        entries
            .into_iter()
            .map(|(key, value)| (Expr::Symbol(Symbol::new(key)), value))
            .collect(),
    )
}

fn record_fields(expr: &Expr) -> DecodeResult<BTreeMap<String, &Expr>> {
    let Expr::Map(entries) = expr else {
        return corrupt("expected persisted record");
    };
    let mut fields = BTreeMap::new();
    for (key, value) in entries {
        let Expr::Symbol(key) = key else {
            return corrupt("persisted record key is not a symbol");
        };
        if fields.insert(key.to_string(), value).is_some() {
            return corrupt("duplicate persisted record field");
        }
    }
    Ok(fields)
}

fn required<'a>(fields: &'a BTreeMap<String, &'a Expr>, key: &str) -> DecodeResult<&'a Expr> {
    fields
        .get(key)
        .copied()
        .ok_or_else(|| DecodeError::Corrupt(format!("missing persisted field {key:?}")))
}

fn vector(expr: &Expr) -> DecodeResult<&[Expr]> {
    match expr {
        Expr::Vector(items) => Ok(items),
        _ => corrupt("expected persisted vector"),
    }
}

fn text(value: impl Into<String>) -> Expr {
    Expr::String(value.into())
}

fn parse_text(expr: &Expr) -> DecodeResult<&str> {
    match expr {
        Expr::String(value) => Ok(value),
        _ => corrupt("expected persisted text"),
    }
}

fn number(value: u64) -> Expr {
    Expr::String(value.to_string())
}

fn parse_u64(expr: &Expr) -> DecodeResult<u64> {
    parse_text(expr)?
        .parse()
        .map_err(|_| DecodeError::Corrupt("invalid persisted integer".to_owned()))
}

fn parse_usize(expr: &Expr) -> DecodeResult<usize> {
    parse_u64(expr)?
        .try_into()
        .map_err(|_| DecodeError::Corrupt("persisted integer exceeds usize".to_owned()))
}

fn optional_number(value: Option<u64>) -> Expr {
    value.map_or(Expr::Nil, number)
}

fn parse_optional_u64(expr: &Expr) -> DecodeResult<Option<u64>> {
    match expr {
        Expr::Nil => Ok(None),
        _ => parse_u64(expr).map(Some),
    }
}

fn parse_bool(expr: &Expr) -> DecodeResult<bool> {
    match expr {
        Expr::Bool(value) => Ok(*value),
        _ => corrupt("expected persisted boolean"),
    }
}

fn corrupt<T>(message: impl Into<String>) -> DecodeResult<T> {
    Err(DecodeError::Corrupt(message.into()))
}
