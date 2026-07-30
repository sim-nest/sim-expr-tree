//! Bounded Scene projection for expression-tree snapshots.

use sim_kernel::{Error, Expr, Result, Symbol};
use sim_lib_scene::node;
use sim_lib_view::SurfaceCaps;
use sim_value::{access, build};

use crate::budget::{BudgetExhausted, RenderBudget, RenderBudgetState};

const MAX_BADGES: usize = 16;
const MAX_ROOTS: usize = 256;

pub(crate) fn encode(snapshot: &Expr, caps: &SurfaceCaps) -> Result<Expr> {
    let context = Context::read(snapshot, caps)?;
    let nodes = required_list(snapshot, "nodes")?;
    let mut global = RenderBudgetState::new(context.budget);
    let mut rendered = Vec::new();
    for value in nodes.iter().take(MAX_ROOTS) {
        match render_node(value, &context, &mut global, 1) {
            Ok(scene) => rendered.push(scene),
            Err(exhausted) => {
                rendered.push(continuation(
                    "More root nodes",
                    exhausted,
                    None,
                    None,
                    &context,
                ));
                break;
            }
        }
    }
    if nodes.len() > MAX_ROOTS {
        rendered.push(continuation(
            "More root nodes",
            BudgetExhausted::Nodes { limit: MAX_ROOTS },
            None,
            None,
            &context,
        ));
    }
    let scene = node(
        "box",
        vec![
            ("role", build::sym("expression-tree")),
            ("aria-label", build::text("Expression tree outline")),
            ("children", build::list(rendered)),
            (
                "budget",
                build::map(vec![
                    ("nodes", build::uint(context.budget.nodes as u64)),
                    ("depth", build::uint(context.budget.depth as u64)),
                    (
                        "encoded-bytes",
                        build::uint(context.budget.encoded_bytes as u64),
                    ),
                    ("face-bytes", build::uint(context.budget.face_bytes as u64)),
                ]),
            ),
        ],
    );
    sim_lib_scene::validate_scene(&scene)
        .map_err(|error| Error::HostError(format!("invalid expression-tree Scene: {error}")))?;
    Ok(scene)
}

struct Context<'a> {
    tree: &'a Expr,
    revision: u64,
    layout: Layout,
    budget: RenderBudget,
}

#[derive(Clone, Copy)]
enum Layout {
    Columns,
    Stacked,
}

impl<'a> Context<'a> {
    fn read(snapshot: &'a Expr, caps: &SurfaceCaps) -> Result<Self> {
        let tag = required_symbol(snapshot, "type")?;
        if tag.namespace.as_deref() != Some("expr-tree-view")
            || tag.name.as_ref() != crate::model::SNAPSHOT_TYPE
        {
            return Err(invalid("value is not an expression-tree snapshot"));
        }
        let density = caps
            .display_density()
            .map(|symbol| symbol.name.to_string())
            .unwrap_or_else(|| "regular".to_owned());
        let (layout, budget) = match density.as_str() {
            "glance" => (Layout::Stacked, RenderBudget::new(24, 8, 16 * 1024, 1024)),
            "compact" => (Layout::Stacked, RenderBudget::new(128, 16, 64 * 1024, 4096)),
            "dense" => (Layout::Columns, RenderBudget::interactive()),
            _ => (
                Layout::Columns,
                RenderBudget::new(320, 24, 128 * 1024, 8192),
            ),
        };
        Ok(Self {
            tree: required(snapshot, "tree")?,
            revision: required_u64(snapshot, "revision")?,
            layout,
            budget,
        })
    }

    fn target(&self, path: &str, continuation: Option<&str>, request_id: Option<u64>) -> Expr {
        let mut fields = vec![
            ("tree", self.tree.clone()),
            ("revision", build::uint(self.revision)),
            ("path", build::text(path)),
        ];
        if let Some(token) = continuation {
            fields.push(("continuation", build::text(token)));
        }
        if let Some(request_id) = request_id {
            fields.push(("request-id", build::text(request_id.to_string())));
        }
        build::map(fields)
    }
}

fn render_node(
    value: &Expr,
    context: &Context<'_>,
    global: &mut RenderBudgetState,
    depth: usize,
) -> core::result::Result<Expr, BudgetExhausted> {
    let path = field_str(value, "path").map_err(|_| face_error(&context.budget))?;
    let name = field_str(value, "name").map_err(|_| face_error(&context.budget))?;
    let kind = field_name(value, "node-type").map_err(|_| face_error(&context.budget))?;
    let open = access::field_bool(value, "open").unwrap_or(false);
    let estimated = name.len().saturating_add(path.len()).saturating_add(192);
    global.admit(depth, Some(name), estimated)?;
    let target = context.target(path, None, None);
    let label = format!(
        "{name} — {}",
        if kind == "directory" {
            "directory"
        } else {
            "cell"
        }
    );
    let mut nodes = Vec::new();
    if open {
        match kind.as_str() {
            "directory" => {
                render_directory_body(value, context, global, depth + 1, path, &mut nodes)?
            }
            "cell" => render_cell_body(value, context, global, depth + 1, path, name, &mut nodes)?,
            _ => nodes.push(text("Unsupported expression-tree node type")),
        }
    }
    Ok(node(
        "tree",
        vec![
            ("label", build::text(label)),
            ("open", Expr::Bool(open)),
            ("aria-expanded", Expr::Bool(open)),
            (
                "aria-label",
                build::text(format!(
                    "{name}, {kind}, {}",
                    if open { "expanded" } else { "collapsed" }
                )),
            ),
            ("disclosure-target", target),
            ("nodes", build::list(nodes)),
        ],
    ))
}

fn render_directory_body(
    value: &Expr,
    context: &Context<'_>,
    global: &mut RenderBudgetState,
    depth: usize,
    path: &str,
    output: &mut Vec<Expr>,
) -> core::result::Result<(), BudgetExhausted> {
    // This read is intentionally below the `open` branch in `render_node`.
    // Collapsed directories never inspect their body or descendants.
    let Some(body) = access::field(value, "body") else {
        return Ok(());
    };
    if matches!(body, Expr::Nil) {
        output.push(text("Loading children…"));
        return Ok(());
    }
    let state = field_name(body, "page-state").unwrap_or_else(|_| "invalid".to_owned());
    let children = required_list(body, "nodes").map_err(|_| face_error(&context.budget))?;
    let mut subtree = RenderBudgetState::new(context.budget.subtree());
    for child in children {
        let child_face = access::field_str(child, "name").unwrap_or("child");
        let estimate = child_face.len().saturating_add(192);
        if let Err(exhausted) = subtree.admit(depth, Some(child_face), estimate) {
            output.push(continuation(
                "More descendants",
                exhausted,
                None,
                Some(path),
                context,
            ));
            return Ok(());
        }
        match render_node(child, context, global, depth) {
            Ok(rendered) => output.push(rendered),
            Err(exhausted) => {
                output.push(continuation(
                    "More descendants",
                    exhausted,
                    None,
                    Some(path),
                    context,
                ));
                return Ok(());
            }
        }
    }
    if state == "truncated" {
        let token = field_str(body, "continuation").map_err(|_| face_error(&context.budget))?;
        let remaining = access::field(body, "remaining").and_then(as_u64);
        output.push(continuation(
            "More descendants require explicit continuation",
            BudgetExhausted::Nodes {
                limit: children.len(),
            },
            Some(token),
            Some(path),
            context,
        ));
        if let Some(remaining) = remaining {
            output.push(text(format!("{remaining} descendants remain")));
        }
    } else if state != "complete" {
        output.push(text("Invalid child page state"));
    }
    Ok(())
}

fn render_cell_body(
    value: &Expr,
    context: &Context<'_>,
    global: &mut RenderBudgetState,
    depth: usize,
    path: &str,
    name: &str,
    output: &mut Vec<Expr>,
) -> core::result::Result<(), BudgetExhausted> {
    // Like directory children, faces are inspected only after explicit
    // disclosure. A collapsed cell therefore fetches and renders no faces.
    let body = required(value, "body").map_err(|_| face_error(&context.budget))?;
    let freshness = field_name(body, "freshness").unwrap_or_else(|_| "unknown".to_owned());
    global.admit(depth, Some(&freshness), 128)?;
    let source_revision = required_u64(body, "source-revision").unwrap_or(0);
    let result_revision = access::field(body, "result-revision").and_then(as_u64);
    let request_id = access::field(body, "receipt")
        .filter(|receipt| !matches!(receipt, Expr::Nil))
        .and_then(|receipt| access::field(receipt, "request-id"))
        .and_then(as_u64);
    let target = context.target(path, None, request_id);
    output.push(node(
        "badge-cluster",
        vec![(
            "badges",
            build::list(status_badges(body, &freshness, context)),
        )],
    ));
    let source = face_node(
        "source",
        required(body, "source").map_err(|_| face_error(&context.budget))?,
        &target,
        name,
        false,
        context,
    );
    let result = face_node(
        "result",
        required(body, "result").map_err(|_| face_error(&context.budget))?,
        &target,
        name,
        true,
        context,
    );
    output.push(node(
        "stack",
        vec![
            (
                "dir",
                build::sym(match context.layout {
                    Layout::Columns => "row",
                    Layout::Stacked => "column",
                }),
            ),
            (
                "aria-label",
                build::text(format!("{name} source and result")),
            ),
            ("children", build::list(vec![source, result])),
        ],
    ));
    output.push(text(format!(
        "source r{source_revision} · result {}",
        result_revision
            .map(|revision| format!("r{revision}"))
            .unwrap_or_else(|| "not committed".to_owned())
    )));
    output.push(timestamp_node(body));
    output.push(receipt_node(body));
    output.push(actions(&target, request_id.is_some()));
    Ok(())
}

fn status_badges(body: &Expr, freshness: &str, context: &Context<'_>) -> Vec<Expr> {
    let status = match freshness {
        "fresh" => "ok",
        "failed" | "blocked" => "error",
        "maybe-stale" | "pending" => "warning",
        _ => "info",
    };
    let mut badges = vec![node(
        "badge",
        vec![
            ("status", build::sym(status)),
            ("label", build::text(freshness_label(freshness))),
            (
                "aria-label",
                build::text(format!("Calculation status: {freshness}")),
            ),
        ],
    )];
    if let Some(Expr::List(policies)) = access::field(body, "policy-badges") {
        for policy in policies.iter().take(MAX_BADGES) {
            let label = match policy {
                Expr::String(text) if text.len() <= context.budget.face_bytes => text.as_str(),
                _ => "policy unavailable",
            };
            badges.push(node(
                "badge",
                vec![
                    ("status", build::sym("policy")),
                    ("label", build::text(label)),
                    ("aria-label", build::text(format!("Policy: {label}"))),
                ],
            ));
        }
    }
    badges
}

fn face_node(
    role: &str,
    face: &Expr,
    target: &Expr,
    name: &str,
    readonly: bool,
    context: &Context<'_>,
) -> Expr {
    let state = field_name(face, "state").unwrap_or_else(|_| "invalid".to_owned());
    let codec = access::field_str(face, "codec");
    let content = access::field(face, "content");
    let visible = match (state.as_str(), content) {
        ("complete", Some(Expr::String(text))) if text.len() <= context.budget.face_bytes => {
            text.clone()
        }
        ("complete", Some(Expr::String(text))) => {
            format!(
                "truncated: face exceeds {} bytes ({})",
                context.budget.face_bytes,
                text.len()
            )
        }
        ("complete", Some(Expr::Bytes(bytes))) => format!("binary face ({} bytes)", bytes.len()),
        ("truncated", _) => format!(
            "truncated: {} limit {}, observed {}",
            field_name(face, "dimension").unwrap_or_else(|_| "unknown".to_owned()),
            access::field(face, "limit").and_then(as_u64).unwrap_or(0),
            access::field(face, "observed")
                .and_then(as_u64)
                .unwrap_or(0)
        ),
        ("unsupported", _) => format!(
            "unsupported: {}",
            access::field_str(face, "reason").unwrap_or("no safe projection")
        ),
        ("codec-failure", _) => format!(
            "codec failure: {}",
            access::field_str(face, "message").unwrap_or("unknown failure")
        ),
        _ => "face unavailable".to_owned(),
    };
    let child = if readonly || state != "complete" {
        node(
            "text",
            vec![
                ("text", build::text(visible)),
                ("aria-label", build::text(format!("{role} face for {name}"))),
            ],
        )
    } else {
        let mut fields = vec![
            ("name", build::text(role)),
            ("value", build::text(visible)),
            ("path", build::list(vec![build::text("source")])),
            ("target", target.clone()),
            ("readonly", Expr::Bool(false)),
            ("aria-label", build::text(format!("Edit source for {name}"))),
        ];
        if let Some(codec) = codec {
            fields.push(("value-codec", build::text(codec)));
        }
        node("field", fields)
    };
    node(
        "box",
        vec![
            ("role", build::sym(role)),
            ("aria-label", build::text(format!("{role} face for {name}"))),
            ("children", build::list(vec![child])),
        ],
    )
}

fn timestamp_node(body: &Expr) -> Expr {
    let source = access::field(body, "source-changed-ms").and_then(as_u64);
    let result = access::field(body, "result-checked-ms").and_then(as_u64);
    text(format!(
        "source changed {} · result checked {}",
        time_label(source),
        time_label(result)
    ))
}

fn receipt_node(body: &Expr) -> Expr {
    let Some(receipt) = access::field(body, "receipt").filter(|value| !matches!(value, Expr::Nil))
    else {
        return text("receipt: none");
    };
    let dependencies = access::field(receipt, "dependencies")
        .and_then(as_u64)
        .unwrap_or(0);
    let omitted = access::field(receipt, "omitted-dependencies")
        .and_then(as_u64)
        .unwrap_or(0);
    text(format!(
        "receipt #{}: {}, {} dependencies (+{} omitted), ticks {}–{}",
        access::field(receipt, "request-id")
            .and_then(as_u64)
            .unwrap_or(0),
        access::field_sym(receipt, "outcome")
            .map(|symbol| symbol.name.to_string())
            .unwrap_or_else(|| "unknown".to_owned()),
        dependencies,
        omitted,
        access::field(receipt, "started-tick")
            .and_then(as_u64)
            .unwrap_or(0),
        access::field(receipt, "finished-tick")
            .and_then(as_u64)
            .unwrap_or(0),
    ))
}

fn actions(target: &Expr, cancellable: bool) -> Expr {
    let mut children = vec![
        button("Calculate", "calculate", target),
        button("Recalculate", "recalculate", target),
        button("Recalculate recursively", "recalculate-recursive", target),
        button("Policy", "policy", target),
        button("Explain", "explain", target),
    ];
    if cancellable {
        children.push(button("Cancel", "cancel", target));
    }
    node(
        "stack",
        vec![
            ("dir", build::sym("row")),
            ("aria-label", build::text("Expression-tree actions")),
            ("children", build::list(children)),
        ],
    )
}

fn continuation(
    label: &str,
    exhausted: BudgetExhausted,
    token: Option<&str>,
    path: Option<&str>,
    context: &Context<'_>,
) -> Expr {
    let status = node(
        "box",
        vec![
            ("role", build::sym("continuation")),
            ("label", build::text(label)),
            ("truncated", Expr::Bool(true)),
            ("reason", build::sym(exhausted.reason())),
            ("limit", build::text(exhausted.limit().to_string())),
            ("aria-label", build::text(label)),
            ("children", build::list(vec![text(label)])),
        ],
    );
    let Some(token) = token else {
        return status;
    };
    node(
        "stack",
        vec![
            ("dir", build::sym("column")),
            (
                "children",
                build::list(vec![
                    status,
                    button(
                        "Load more",
                        "continue",
                        &context.target(path.unwrap_or("/"), Some(token), None),
                    ),
                ]),
            ),
        ],
    )
}

fn button(label: &str, control: &str, target: &Expr) -> Expr {
    node(
        "button",
        vec![
            ("label", build::text(label)),
            ("control", build::text(control)),
            ("target", target.clone()),
            ("aria-label", build::text(label)),
        ],
    )
}

fn text(content: impl Into<String>) -> Expr {
    node("text", vec![("text", build::text(content.into()))])
}

fn freshness_label(token: &str) -> &'static str {
    match token {
        "never-calculated" => "Never calculated",
        "fresh" => "Fresh",
        "maybe-stale" => "Maybe stale",
        "pending" => "Pending",
        "failed" => "Failed",
        "frozen" => "Frozen",
        "blocked" => "Blocked",
        _ => "Unknown",
    }
}

fn time_label(value: Option<u64>) -> String {
    value
        .map(|millis| format!("{millis} ms"))
        .unwrap_or_else(|| "not observed".to_owned())
}

fn face_error(budget: &RenderBudget) -> BudgetExhausted {
    budget.malformed()
}

fn required<'a>(map: &'a Expr, name: &str) -> Result<&'a Expr> {
    access::field(map, name).ok_or_else(|| invalid(format!("missing field {name}")))
}

fn required_list<'a>(map: &'a Expr, name: &str) -> Result<&'a [Expr]> {
    match required(map, name)? {
        Expr::List(items) | Expr::Vector(items) => Ok(items),
        _ => Err(invalid(format!("field {name} must be a list"))),
    }
}

fn required_symbol(map: &Expr, name: &str) -> Result<Symbol> {
    match required(map, name)? {
        Expr::Symbol(symbol) => Ok(symbol.clone()),
        _ => Err(invalid(format!("field {name} must be a symbol"))),
    }
}

fn required_u64(map: &Expr, name: &str) -> Result<u64> {
    required(map, name)
        .and_then(|value| as_u64(value).ok_or_else(|| invalid(format!("{name} must be u64"))))
}

fn as_u64(value: &Expr) -> Option<u64> {
    match value {
        Expr::Number(number) => number.canonical.parse().ok(),
        _ => None,
    }
}

fn field_str<'a>(map: &'a Expr, name: &str) -> Result<&'a str> {
    access::field_str(map, name).ok_or_else(|| invalid(format!("{name} must be text")))
}

fn field_name(map: &Expr, name: &str) -> Result<String> {
    match required(map, name)? {
        Expr::Symbol(symbol) => Ok(symbol.name.to_string()),
        Expr::String(text) => Ok(text.clone()),
        _ => Err(invalid(format!("{name} must be a name"))),
    }
}

fn invalid(message: impl Into<String>) -> Error {
    Error::HostError(message.into())
}
