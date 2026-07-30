//! Standard Intent decoding and existing expression-tree operation compilation.

use sim_kernel::{Error, Expr, Result, Symbol};
use sim_lib_view::Operation;
use sim_value::{access, build};

const COMMAND_TYPE: &str = "command";
const DISCLOSURE_KIND: &str = "tree-disclosure";

pub(crate) fn decode(snapshot: &Expr, intent: &Expr) -> Result<Expr> {
    let context = SnapshotContext::read(snapshot)?;
    let kind =
        access::field_sym(intent, "kind").ok_or_else(|| invalid("Intent is missing a kind"))?;
    if kind.namespace.as_deref() != Some("intent") {
        return Err(invalid("Intent kind must use the intent namespace"));
    }
    if kind.name.as_ref() == DISCLOSURE_KIND {
        validate_disclosure(intent)?;
    } else {
        sim_lib_intent::validate_intent(intent)
            .map_err(|error| invalid(format!("invalid Intent: {error}")))?;
    }
    let command = match kind.name.as_ref() {
        DISCLOSURE_KIND => disclosure(&context, intent)?,
        "tap" => tap(&context, intent)?,
        "edit-field" => edit(&context, intent)?,
        "create" => create(&context, intent)?,
        "move" => move_node(&context, intent)?,
        "delete" => delete(&context, intent)?,
        "invoke" => invoke(&context, intent)?,
        "set-param" => policy(&context, intent)?,
        "cancel" => cancel(&context, intent)?,
        other => {
            return Err(invalid(format!(
                "unsupported expression-tree Intent {other}"
            )));
        }
    };
    Ok(command)
}

pub(crate) fn commit(command: &Expr) -> Result<Operation> {
    require_command(command)?;
    let action = required_symbol(command, "action")?;
    let tree = required(command, "tree")?.clone();
    let path = access::field_str(command, "path").map(str::to_owned);
    let args = list_field(command, "args")?;
    let (form, capability) = match action.name.as_ref() {
        "disclose" | "continue" | "open-policy" => (
            local_form(action.clone(), command),
            sim_lib_expr_tree::expr_tree_read_capability(),
        ),
        "set-expr" => (
            runtime_form("set-expr", tree, path_arg(path)?, one_arg(args)?),
            sim_lib_expr_tree::expr_tree_write_capability(),
        ),
        "new-cell" => (
            runtime_form_many("new-cell", tree, path_arg(path)?, args),
            sim_lib_expr_tree::expr_tree_write_capability(),
        ),
        "new-dir" => (
            runtime_form_many("new-dir", tree, path_arg(path)?, args),
            sim_lib_expr_tree::expr_tree_write_capability(),
        ),
        "move" => (
            runtime_form("move", tree, path_arg(path)?, one_arg(args)?),
            sim_lib_expr_tree::expr_tree_write_capability(),
        ),
        "delete" => (
            runtime_form_one("delete", tree, path_arg(path)?),
            sim_lib_expr_tree::expr_tree_write_capability(),
        ),
        "set-calc-policy" | "set-codec-policy" => (
            runtime_form(action.name.as_ref(), tree, path_arg(path)?, one_arg(args)?),
            sim_lib_expr_tree::expr_tree_write_capability(),
        ),
        "calculate" | "recalculate" | "recalculate-recursive" => (
            runtime_form_one(action.name.as_ref(), tree, path_arg(path)?),
            sim_lib_expr_tree::expr_tree_calculate_capability(),
        ),
        "cancel" => (
            runtime_form_one("cancel", tree, one_arg(args)?),
            sim_lib_expr_tree::expr_tree_calculate_capability(),
        ),
        "explain" => (
            runtime_form_one("explain", tree, path_arg(path)?),
            sim_lib_expr_tree::expr_tree_read_capability(),
        ),
        other => return Err(invalid(format!("unknown proposed action {other}"))),
    };
    Ok(Operation::new(form).requiring(capability))
}

struct SnapshotContext<'a> {
    tree: &'a Expr,
    revision: u64,
}

impl<'a> SnapshotContext<'a> {
    fn read(snapshot: &'a Expr) -> Result<Self> {
        let tag = required_symbol(snapshot, "type")?;
        if tag.namespace.as_deref() != Some("expr-tree-view")
            || tag.name.as_ref() != crate::model::SNAPSHOT_TYPE
        {
            return Err(invalid("value is not an expression-tree snapshot"));
        }
        Ok(Self {
            tree: required(snapshot, "tree")?,
            revision: required_u64(snapshot, "revision")?,
        })
    }

    fn target(&self, value: &Expr) -> Result<Target> {
        let tree = required(value, "tree")?;
        if tree != self.tree {
            return Err(invalid("Intent target names a different expression tree"));
        }
        let revision = required_u64(value, "revision")?;
        if revision != self.revision {
            return Err(invalid(format!(
                "stale expression-tree revision {revision}; current revision is {}",
                self.revision
            )));
        }
        Ok(Target {
            path: access::field_str(value, "path")
                .ok_or_else(|| invalid("Intent target is missing path"))?
                .to_owned(),
            continuation: access::field_str(value, "continuation").map(str::to_owned),
            request_id: access::field(value, "request-id")
                .map(request_id_expr)
                .transpose()?,
        })
    }
}

struct Target {
    path: String,
    continuation: Option<String>,
    request_id: Option<Expr>,
}

fn disclosure(context: &SnapshotContext<'_>, intent: &Expr) -> Result<Expr> {
    let target = context.target(required(intent, "target")?)?;
    let open = access::field_bool(intent, "open")
        .ok_or_else(|| invalid("tree disclosure is missing boolean open"))?;
    command(
        context,
        "disclose",
        Some(target.path),
        vec![Expr::Bool(open)],
        target.continuation.as_deref(),
    )
}

fn tap(context: &SnapshotContext<'_>, intent: &Expr) -> Result<Expr> {
    let target = context.target(required(intent, "target")?)?;
    let control = required_name(intent, "control")?;
    match control.as_str() {
        "continue" => {
            let token = target
                .continuation
                .as_deref()
                .ok_or_else(|| invalid("continuation action has no continuation token"))?;
            command(
                context,
                "continue",
                Some(target.path),
                Vec::new(),
                Some(token),
            )
        }
        "calculate" | "recalculate" | "recalculate-recursive" | "explain" | "policy" => command(
            context,
            if control == "policy" {
                "open-policy"
            } else {
                &control
            },
            Some(target.path),
            Vec::new(),
            None,
        ),
        "cancel" => command(
            context,
            "cancel",
            Some(target.path),
            vec![
                target
                    .request_id
                    .ok_or_else(|| invalid("cancel action has no active request id"))?,
            ],
            None,
        ),
        other => Err(invalid(format!(
            "unsupported expression-tree control {other}"
        ))),
    }
}

fn edit(context: &SnapshotContext<'_>, intent: &Expr) -> Result<Expr> {
    let target = context.target(required(intent, "target")?)?;
    let segments = list_field(intent, "path")?;
    if segments.as_slice() != [Expr::String("source".to_owned())] {
        return Err(invalid("expression-tree edit path must be [\"source\"]"));
    }
    command(
        context,
        "set-expr",
        Some(target.path),
        vec![required(intent, "value")?.clone()],
        None,
    )
}

fn create(context: &SnapshotContext<'_>, intent: &Expr) -> Result<Expr> {
    let target = context.target(required(intent, "at")?)?;
    let class = required_name(intent, "class")?;
    let (action, expected_args) = match class.as_str() {
        "cell" | "expr-tree-cell" => ("new-cell", 2),
        "directory" | "dir" | "expr-tree-directory" => ("new-dir", 1),
        _ => {
            return Err(invalid(format!(
                "unsupported expression-tree class {class}"
            )));
        }
    };
    let args = list_field(intent, "args")?;
    if args.len() != expected_args {
        return Err(invalid(format!(
            "{class} creation requires {expected_args} argument(s)"
        )));
    }
    command(context, action, Some(target.path), args, None)
}

fn move_node(context: &SnapshotContext<'_>, intent: &Expr) -> Result<Expr> {
    let source = context.target(required(intent, "node")?)?;
    let destination = context.target(required(intent, "at")?)?;
    command(
        context,
        "move",
        Some(source.path),
        vec![Expr::String(destination.path)],
        None,
    )
}

fn delete(context: &SnapshotContext<'_>, intent: &Expr) -> Result<Expr> {
    let targets = list_field(intent, "targets")?;
    if targets.len() != 1 {
        return Err(invalid(
            "delete must name exactly one expression-tree target",
        ));
    }
    let target = context.target(&targets[0])?;
    command(context, "delete", Some(target.path), Vec::new(), None)
}

fn invoke(context: &SnapshotContext<'_>, intent: &Expr) -> Result<Expr> {
    let target = context.target(required(intent, "target")?)?;
    let op = required_name(intent, "op")?;
    match op.as_str() {
        "calculate" | "recalculate" | "recalculate-recursive" | "explain" => {
            if !list_field(intent, "args")?.is_empty() {
                return Err(invalid(format!("{op} takes no surface arguments")));
            }
            command(context, &op, Some(target.path), Vec::new(), None)
        }
        "cancel" => command(
            context,
            "cancel",
            Some(target.path),
            list_field(intent, "args")?,
            None,
        ),
        other => Err(invalid(format!(
            "unsupported expression-tree invoke {other}"
        ))),
    }
}

fn policy(context: &SnapshotContext<'_>, intent: &Expr) -> Result<Expr> {
    let target = context.target(required(intent, "target")?)?;
    let param = required_name(intent, "param")?;
    let action = match param.as_str() {
        "calculation-policy" | "calc-policy" => "set-calc-policy",
        "codec-policy" => "set-codec-policy",
        _ => {
            return Err(invalid(format!(
                "unsupported expression-tree policy {param}"
            )));
        }
    };
    command(
        context,
        action,
        Some(target.path),
        vec![required(intent, "value")?.clone()],
        None,
    )
}

fn cancel(context: &SnapshotContext<'_>, intent: &Expr) -> Result<Expr> {
    let target = context.target(required(intent, "target")?)?;
    let request = access::field(intent, "request-id")
        .map(request_id_expr)
        .transpose()?
        .or(target.request_id)
        .ok_or_else(|| invalid("cancel Intent is missing request-id"))?;
    command(context, "cancel", Some(target.path), vec![request], None)
}

fn command(
    context: &SnapshotContext<'_>,
    action: &str,
    path: Option<String>,
    args: Vec<Expr>,
    continuation: Option<&str>,
) -> Result<Expr> {
    let mut fields = vec![
        (
            "type",
            Expr::Symbol(Symbol::qualified("expr-tree-view", COMMAND_TYPE)),
        ),
        ("action", build::sym(action)),
        ("tree", context.tree.clone()),
        ("revision", build::uint(context.revision)),
        ("args", build::list(args)),
    ];
    if let Some(path) = path {
        fields.push(("path", build::text(path)));
    }
    if let Some(token) = continuation {
        if token.is_empty() || token.len() > 512 {
            return Err(invalid("continuation token must be 1..=512 bytes"));
        }
        fields.push(("continuation", build::text(token)));
    }
    Ok(build::map(fields))
}

fn local_form(action: Symbol, command: &Expr) -> Expr {
    build::map(vec![
        (
            "op",
            Expr::Symbol(Symbol::qualified("expr-tree-view", action.name.as_ref())),
        ),
        (
            "tree",
            access::field(command, "tree").cloned().unwrap_or(Expr::Nil),
        ),
        (
            "path",
            access::field(command, "path").cloned().unwrap_or(Expr::Nil),
        ),
        (
            "revision",
            access::field(command, "revision")
                .cloned()
                .unwrap_or(Expr::Nil),
        ),
        (
            "args",
            access::field(command, "args").cloned().unwrap_or(Expr::Nil),
        ),
        (
            "continuation",
            access::field(command, "continuation")
                .cloned()
                .unwrap_or(Expr::Nil),
        ),
    ])
}

fn runtime_form_one(name: &str, tree: Expr, arg: Expr) -> Expr {
    Expr::Call {
        operator: Box::new(Expr::Symbol(Symbol::qualified("expr-tree", name))),
        args: vec![tree, arg],
    }
}

fn runtime_form(name: &str, tree: Expr, path: Expr, value: Expr) -> Expr {
    Expr::Call {
        operator: Box::new(Expr::Symbol(Symbol::qualified("expr-tree", name))),
        args: vec![tree, path, value],
    }
}

fn runtime_form_many(name: &str, tree: Expr, path: Expr, mut args: Vec<Expr>) -> Expr {
    let mut all = vec![tree, path];
    all.append(&mut args);
    Expr::Call {
        operator: Box::new(Expr::Symbol(Symbol::qualified("expr-tree", name))),
        args: all,
    }
}

fn path_arg(path: Option<String>) -> Result<Expr> {
    path.map(Expr::String)
        .ok_or_else(|| invalid("proposed command is missing path"))
}

fn one_arg(mut args: Vec<Expr>) -> Result<Expr> {
    if args.len() != 1 {
        return Err(invalid("proposed command must carry exactly one argument"));
    }
    Ok(args.remove(0))
}

fn require_command(command: &Expr) -> Result<()> {
    let tag = required_symbol(command, "type")?;
    if tag.namespace.as_deref() == Some("expr-tree-view") && tag.name.as_ref() == COMMAND_TYPE {
        Ok(())
    } else {
        Err(invalid("draft proposal is not an expression-tree command"))
    }
}

fn validate_disclosure(intent: &Expr) -> Result<()> {
    if sim_lib_intent::origin(intent).is_none() {
        return Err(invalid("tree disclosure is missing a valid origin"));
    }
    required(intent, "target")?;
    access::field_bool(intent, "open")
        .ok_or_else(|| invalid("tree disclosure is missing boolean open"))?;
    Ok(())
}

fn required<'a>(map: &'a Expr, name: &str) -> Result<&'a Expr> {
    access::field(map, name).ok_or_else(|| invalid(format!("missing field {name}")))
}

fn required_symbol(map: &Expr, name: &str) -> Result<Symbol> {
    match required(map, name)? {
        Expr::Symbol(symbol) => Ok(symbol.clone()),
        _ => Err(invalid(format!("field {name} must be a symbol"))),
    }
}

fn required_name(map: &Expr, name: &str) -> Result<String> {
    match required(map, name)? {
        Expr::Symbol(symbol) => Ok(symbol.name.to_string()),
        Expr::String(text) => Ok(text.clone()),
        _ => Err(invalid(format!("field {name} must be a name"))),
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

fn request_id_expr(value: &Expr) -> Result<Expr> {
    let request_id = match value {
        Expr::String(text) => text
            .parse::<u64>()
            .map_err(|_| invalid("request-id text must be unsigned decimal"))?,
        value => as_u64(value).ok_or_else(|| invalid("request-id must be unsigned decimal"))?,
    };
    Ok(Expr::String(request_id.to_string()))
}

fn list_field(map: &Expr, name: &str) -> Result<Vec<Expr>> {
    match required(map, name)? {
        Expr::List(items) | Expr::Vector(items) => Ok(items.clone()),
        _ => Err(invalid(format!("field {name} must be a list"))),
    }
}

fn invalid(message: impl Into<String>) -> Error {
    Error::HostError(message.into())
}
