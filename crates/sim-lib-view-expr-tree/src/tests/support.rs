use sim_kernel::Expr;
use sim_value::{access, build};

use crate::{
    ExpressionTreeSnapshot, FaceSnapshot, Freshness, NodeDetail, NodeSnapshot, ReceiptSummary,
    TimestampSummary,
};

pub(super) const TREE_REVISION: u64 = 17;

pub(super) fn detail(freshness: Freshness) -> NodeDetail {
    NodeDetail {
        source: FaceSnapshot::text("(+ 20 22)", "codec/lisp"),
        result: FaceSnapshot::text("42", "codec/lisp"),
        freshness,
        source_revision: 17,
        result_revision: Some(16),
        timestamps: TimestampSummary {
            source_changed_ms: Some(1_700_000_000_100),
            result_checked_ms: Some(1_700_000_000_200),
        },
        policy_badges: vec!["Automatic".to_owned(), "codec/lisp".to_owned()],
        receipt: Some(ReceiptSummary {
            request_id: 9,
            outcome: "succeeded".to_owned(),
            dependencies: 3,
            omitted_dependencies: 2,
            started_tick: 40,
            finished_tick: 44,
        }),
    }
}

pub(super) fn expanded_cell(path: &str, name: &str, freshness: Freshness) -> NodeSnapshot {
    NodeSnapshot::expanded_cell(path, name, TREE_REVISION, detail(freshness))
}

pub(super) fn snapshot(nodes: Vec<NodeSnapshot>) -> Expr {
    ExpressionTreeSnapshot::new(
        Expr::String("tree:workbook".to_owned()),
        TREE_REVISION,
        nodes,
    )
    .to_expr()
}

pub(super) fn collect_kind<'a>(expr: &'a Expr, kind: &str, found: &mut Vec<&'a Expr>) {
    match expr {
        Expr::Map(entries) => {
            if access::field_sym(expr, "kind").is_some_and(|symbol| {
                symbol.namespace.as_deref() == Some("scene") && symbol.name.as_ref() == kind
            }) {
                found.push(expr);
            }
            for (key, value) in entries {
                collect_kind(key, kind, found);
                collect_kind(value, kind, found);
            }
        }
        Expr::List(items) | Expr::Vector(items) | Expr::Set(items) | Expr::Block(items) => {
            for item in items {
                collect_kind(item, kind, found);
            }
        }
        Expr::Call { operator, args } => {
            collect_kind(operator, kind, found);
            for arg in args {
                collect_kind(arg, kind, found);
            }
        }
        Expr::Infix { left, right, .. } => {
            collect_kind(left, kind, found);
            collect_kind(right, kind, found);
        }
        Expr::Prefix { arg, .. } | Expr::Postfix { arg, .. } => collect_kind(arg, kind, found),
        Expr::Quote { expr, .. } | Expr::Extension { payload: expr, .. } => {
            collect_kind(expr, kind, found);
        }
        Expr::Annotated {
            expr, annotations, ..
        } => {
            collect_kind(expr, kind, found);
            for (_, value) in annotations {
                collect_kind(value, kind, found);
            }
        }
        Expr::Nil
        | Expr::Bool(_)
        | Expr::Number(_)
        | Expr::Symbol(_)
        | Expr::Local(_)
        | Expr::String(_)
        | Expr::Bytes(_) => {}
    }
}

pub(super) fn symbol_field(expr: &Expr, name: &str) -> Option<String> {
    access::field_sym(expr, name).map(|symbol| symbol.name.to_string())
}

pub(super) fn target(path: &str, revision: u64) -> Expr {
    build::map(vec![
        ("tree", build::text("tree:workbook")),
        ("revision", build::uint(revision)),
        ("path", build::text(path)),
    ])
}

pub(super) fn target_with_request(path: &str, revision: u64, request_id: u64) -> Expr {
    build::map(vec![
        ("tree", build::text("tree:workbook")),
        ("revision", build::uint(revision)),
        ("path", build::text(path)),
        ("request-id", build::uint(request_id)),
    ])
}
