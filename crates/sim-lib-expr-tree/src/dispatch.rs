use std::sync::Arc;

use sim_expr_tree_calc::{
    CalcExplanation, CalcOutcome, CalcReason, CalcRequestMode, CalcStatus, CalcTrigger,
    DirectedCalcReport,
};
use sim_kernel::{Cx, Error, Result, Symbol, Value};
use sim_table_core::{TablePath, TablePathRef};

use crate::{
    handle::TreeRuntime,
    operation::OperationKind,
    parse::{
        backend_arg, calc_policy_arg, codec_policy_arg, mount_epoch_arg, mount_resource_arg,
        optional_name_arg, request_id_arg, source_arg, string_arg, tree_arg,
    },
    runtime::TreeState,
};

const MAX_ERROR_CHARS: usize = 512;
const MAX_REASON_CHARS: usize = 256;
const MAX_REASONS: usize = 16;

pub(crate) fn dispatch(
    kind: OperationKind,
    runtime: &TreeRuntime,
    cx: &mut Cx,
    args: Vec<Value>,
) -> Result<Value> {
    let result = match kind {
        OperationKind::Open => open(runtime, cx, &args),
        OperationKind::NewCell => new_cell(cx, &args),
        OperationKind::NewDir => new_dir(cx, &args),
        OperationKind::Mount => mount(cx, &args),
        OperationKind::Unmount => unmount(cx, &args),
        OperationKind::Move => move_entry(cx, &args),
        OperationKind::Rename => rename_entry(cx, &args),
        OperationKind::Delete => delete(cx, &args),
        OperationKind::SetExpr => set_expr(cx, &args),
        OperationKind::SetCalcPolicy => set_calc_policy(cx, &args),
        OperationKind::SetCodecPolicy => set_codec_policy(cx, &args),
        OperationKind::Ref => reference(cx, &args),
        OperationKind::List => list(cx, &args),
        OperationKind::Calculate => calculate(cx, &args, CalcRequestMode::Verify),
        OperationKind::Recalculate => calculate(cx, &args, CalcRequestMode::ForceRoots),
        OperationKind::RecalculateRecursive => {
            calculate(cx, &args, CalcRequestMode::ForceRecursive)
        }
        OperationKind::Cancel => cancel(cx, &args),
        OperationKind::Refresh => refresh(cx, &args),
        OperationKind::Status => status(cx, &args),
        OperationKind::Explain => explain(cx, &args),
        OperationKind::Watch => watch(cx, &args),
    };
    result.map_err(|error| bounded_error(operation_name(kind), error))
}

pub(crate) fn bounded_error(operation: &str, detail: impl std::fmt::Display) -> Error {
    let detail = bound_text(&detail.to_string(), MAX_ERROR_CHARS);
    Error::Eval(format!("expr-tree/{operation}: {detail}"))
}

fn open(runtime: &TreeRuntime, cx: &mut Cx, args: &[Value]) -> std::result::Result<Value, String> {
    let storage = string_arg(cx, &args[0], "storage name")?;
    let handle = runtime.open(&storage)?;
    cx.factory()
        .opaque(Arc::new(handle))
        .map_err(|error| error.to_string())
}

fn new_cell(cx: &mut Cx, args: &[Value]) -> std::result::Result<Value, String> {
    let tree = tree_arg(&args[0])?;
    let parent = string_arg(cx, &args[1], "parent path")?;
    let name = optional_name_arg(cx, &args[2])?;
    let source = source_arg(cx, &args[3])?;
    let path = with_state(&tree, |state| {
        state.new_cell(&parent, name.as_deref(), source)
    })?;
    string_value(cx, path)
}

fn new_dir(cx: &mut Cx, args: &[Value]) -> std::result::Result<Value, String> {
    let tree = tree_arg(&args[0])?;
    let parent = string_arg(cx, &args[1], "parent path")?;
    let name = optional_name_arg(cx, &args[2])?;
    let path = with_state(&tree, |state| state.new_dir(&parent, name.as_deref()))?;
    string_value(cx, path)
}

fn mount(cx: &mut Cx, args: &[Value]) -> std::result::Result<Value, String> {
    let tree = tree_arg(&args[0])?;
    let path = string_arg(cx, &args[1], "mount path")?;
    let backend = backend_arg(cx, &args[2])?;
    let resource = mount_resource_arg(cx, &args[3])?;
    let epoch = mount_epoch_arg(cx, &args[4])?;
    let path = with_state(&tree, |state| state.mount(&path, backend, resource, epoch))?;
    string_value(cx, path)
}

fn unmount(cx: &mut Cx, args: &[Value]) -> std::result::Result<Value, String> {
    let tree = tree_arg(&args[0])?;
    let path = string_arg(cx, &args[1], "mount path")?;
    let removed = with_state(&tree, |state| state.unmount(&path))?;
    bool_value(cx, removed)
}

fn move_entry(cx: &mut Cx, args: &[Value]) -> std::result::Result<Value, String> {
    let tree = tree_arg(&args[0])?;
    let from = string_arg(cx, &args[1], "source path")?;
    let to = string_arg(cx, &args[2], "target path")?;
    let path = with_state(&tree, |state| state.move_entry(&from, &to))?;
    string_value(cx, path)
}

fn rename_entry(cx: &mut Cx, args: &[Value]) -> std::result::Result<Value, String> {
    let tree = tree_arg(&args[0])?;
    let path = string_arg(cx, &args[1], "path")?;
    let name = string_arg(cx, &args[2], "new name")?;
    let path = with_state(&tree, |state| state.rename_entry(&path, &name))?;
    string_value(cx, path)
}

fn delete(cx: &mut Cx, args: &[Value]) -> std::result::Result<Value, String> {
    let tree = tree_arg(&args[0])?;
    let path = string_arg(cx, &args[1], "path")?;
    let deleted = with_state(&tree, |state| state.delete(&path))?;
    bool_value(cx, deleted)
}

fn set_expr(cx: &mut Cx, args: &[Value]) -> std::result::Result<Value, String> {
    let tree = tree_arg(&args[0])?;
    let path = string_arg(cx, &args[1], "cell path")?;
    let source = source_arg(cx, &args[2])?;
    let path = with_state(&tree, |state| state.set_expr(&path, source))?;
    string_value(cx, path)
}

fn set_calc_policy(cx: &mut Cx, args: &[Value]) -> std::result::Result<Value, String> {
    let tree = tree_arg(&args[0])?;
    let path = string_arg(cx, &args[1], "policy path")?;
    let patch = calc_policy_arg(cx, &args[2])?;
    let record = with_state(&tree, |state| state.set_calc_policy(&path, patch))?;
    cx.factory()
        .opaque(Arc::new(record))
        .map_err(|error| error.to_string())
}

fn set_codec_policy(cx: &mut Cx, args: &[Value]) -> std::result::Result<Value, String> {
    let tree = tree_arg(&args[0])?;
    let path = string_arg(cx, &args[1], "policy path")?;
    let patch = codec_policy_arg(cx, &args[2])?;
    let record = with_state(&tree, |state| state.set_codec_policy(&path, patch))?;
    cx.factory()
        .opaque(Arc::new(record))
        .map_err(|error| error.to_string())
}

fn reference(cx: &mut Cx, args: &[Value]) -> std::result::Result<Value, String> {
    let tree = tree_arg(&args[0])?;
    let reference = string_arg(cx, &args[1], "path reference")?;
    let base = if args.len() == 3 {
        string_arg(cx, &args[2], "base directory")?
    } else {
        "/".to_owned()
    };
    let path = resolve_reference(&reference, &base)?;
    with_state(&tree, |state| {
        state.current_value(&path.to_absolute_reference())
    })
}

fn list(cx: &mut Cx, args: &[Value]) -> std::result::Result<Value, String> {
    let tree = tree_arg(&args[0])?;
    let path = string_arg(cx, &args[1], "directory path")?;
    let rows = with_state(&tree, |state| state.list(&path))?;
    let values = rows
        .into_iter()
        .map(|(path, kind)| {
            cx.factory().table(vec![
                (field("path"), cx.factory().string(path)?),
                (
                    field("kind"),
                    cx.factory().symbol(Symbol::qualified("expr-tree", kind))?,
                ),
            ])
        })
        .collect::<Result<Vec<_>>>()
        .map_err(|error| error.to_string())?;
    cx.factory().list(values).map_err(|error| error.to_string())
}

fn calculate(
    cx: &mut Cx,
    args: &[Value],
    mode: CalcRequestMode,
) -> std::result::Result<Value, String> {
    let tree = tree_arg(&args[0])?;
    let path = string_arg(cx, &args[1], "cell path")?;
    let report = with_state(&tree, |state| state.calculate(&path, mode))?;
    report_value(report)
}

fn cancel(cx: &mut Cx, args: &[Value]) -> std::result::Result<Value, String> {
    let tree = tree_arg(&args[0])?;
    let request_id = request_id_arg(cx, &args[1])?;
    let cancelled = with_state(&tree, |state| Ok(state.cancel(request_id)))?;
    bool_value(cx, cancelled)
}

fn refresh(cx: &mut Cx, args: &[Value]) -> std::result::Result<Value, String> {
    let tree = tree_arg(&args[0])?;
    let report = with_state(&tree, TreeState::refresh)?;
    (|| -> Result<Value> {
        cx.factory().table(vec![
            (
                field("sampled-mounts"),
                kernel_string_list(cx, report.sampled_mounts)?,
            ),
            (
                field("watch-managed-mounts"),
                kernel_string_list(cx, report.watch_managed_mounts)?,
            ),
            (
                field("changed-epochs"),
                kernel_string_list(cx, report.changed_epochs)?,
            ),
            (
                field("invalidated-observations"),
                cx.factory()
                    .string(report.invalidated_observations.to_string())?,
            ),
        ])
    })()
    .map_err(|error| error.to_string())
}

fn status(cx: &mut Cx, args: &[Value]) -> std::result::Result<Value, String> {
    let tree = tree_arg(&args[0])?;
    let path = string_arg(cx, &args[1], "cell path")?;
    let (status, request_id) = with_state(&tree, |state| {
        Ok((state.status(&path)?, state.pending_request_id(&path)?))
    })?;
    (|| -> Result<Value> {
        cx.factory().table(vec![
            (field("status"), cx.factory().symbol(status_symbol(status))?),
            (
                field("pending-request-id"),
                match request_id {
                    Some(id) => cx.factory().string(id.get().to_string())?,
                    None => cx.factory().nil()?,
                },
            ),
        ])
    })()
    .map_err(|error| error.to_string())
}

fn explain(cx: &mut Cx, args: &[Value]) -> std::result::Result<Value, String> {
    let tree = tree_arg(&args[0])?;
    let path = string_arg(cx, &args[1], "cell path")?;
    let (explanation, source, request_id) = with_state(&tree, |state| {
        Ok((
            state.explanation(&path)?,
            state.source_record(&path)?,
            state.pending_request_id(&path)?,
        ))
    })?;
    explanation_value(cx, explanation, source, request_id)
}

fn watch(cx: &mut Cx, args: &[Value]) -> std::result::Result<Value, String> {
    let tree = tree_arg(&args[0])?;
    let stream = with_state(&tree, |state| Ok(state.watch()))?;
    cx.factory()
        .opaque(stream)
        .map_err(|error| error.to_string())
}

fn report_value(report: DirectedCalcReport) -> std::result::Result<Value, String> {
    let Some(cell) = report.cells.into_iter().last() else {
        return Err(format!(
            "calculation request {} returned no root outcome",
            report.request_id.get()
        ));
    };
    cell.result.map_err(|error| error.to_string())
}

fn explanation_value(
    cx: &mut Cx,
    explanation: CalcExplanation,
    source: crate::DurableSourceRecord,
    pending_request: Option<sim_expr_tree_calc::RequestId>,
) -> std::result::Result<Value, String> {
    (|| -> Result<Value> {
        let reasons = explanation
            .reasons
            .into_iter()
            .take(MAX_REASONS)
            .map(|reason| cx.factory().string(bound_text(&reason, MAX_REASON_CHARS)))
            .collect::<Result<Vec<_>>>()?;
        let receipt = match explanation.receipt {
            Some(receipt) => cx.factory().table(vec![
                (
                    field("request-id"),
                    cx.factory().string(receipt.request_id.get().to_string())?,
                ),
                (
                    field("source-revision"),
                    cx.factory().string(receipt.source_revision.to_string())?,
                ),
                (
                    field("dependencies"),
                    cx.factory()
                        .string(receipt.dependencies.len().to_string())?,
                ),
                (
                    field("omitted-dependencies"),
                    cx.factory()
                        .string(receipt.omitted_dependencies.to_string())?,
                ),
                (
                    field("outcome"),
                    cx.factory().string(bound_text(
                        &outcome_text(&receipt.outcome),
                        MAX_REASON_CHARS,
                    ))?,
                ),
                (
                    field("reason"),
                    cx.factory().symbol(reason_symbol(receipt.reason))?,
                ),
                (
                    field("trigger"),
                    cx.factory().symbol(trigger_symbol(receipt.trigger))?,
                ),
            ])?,
            None => cx.factory().nil()?,
        };
        cx.factory().table(vec![
            (field("cell"), cx.factory().string(explanation.cell)?),
            (
                field("status"),
                cx.factory().symbol(status_symbol(explanation.status))?,
            ),
            (
                field("source-revision"),
                cx.factory()
                    .string(explanation.source_revision.to_string())?,
            ),
            (
                field("policy-digest"),
                cx.factory()
                    .string(explanation.policy_digest.get().to_string())?,
            ),
            (
                field("authority-digest"),
                cx.factory()
                    .string(explanation.authority_digest.get().to_string())?,
            ),
            (field("reasons"), cx.factory().list(reasons)?),
            (field("receipt"), receipt),
            (
                field("pending-request-id"),
                match pending_request {
                    Some(id) => cx.factory().string(id.get().to_string())?,
                    None => cx.factory().nil()?,
                },
            ),
            (
                field("source-record"),
                cx.factory().opaque(Arc::new(source))?,
            ),
        ])
    })()
    .map_err(|error| error.to_string())
}

fn resolve_reference(reference: &str, base: &str) -> std::result::Result<TablePath, String> {
    let base = TablePath::parse_absolute(base).map_err(|error| format!("{error:?}"))?;
    TablePathRef::parse(reference)
        .and_then(|reference| reference.resolve(&base))
        .map_err(|error| format!("{error:?}"))
}

fn with_state<T>(
    tree: &crate::TreeHandle,
    action: impl FnOnce(&mut TreeState) -> std::result::Result<T, String>,
) -> std::result::Result<T, String> {
    let mut state = tree
        .state
        .lock()
        .map_err(|_| "expression-tree state poisoned".to_owned())?;
    action(&mut state)
}

fn string_value(cx: &mut Cx, text: String) -> std::result::Result<Value, String> {
    cx.factory().string(text).map_err(|error| error.to_string())
}

fn bool_value(cx: &mut Cx, value: bool) -> std::result::Result<Value, String> {
    cx.factory().bool(value).map_err(|error| error.to_string())
}

fn kernel_string_list(cx: &Cx, values: Vec<String>) -> Result<Value> {
    let values = values
        .into_iter()
        .map(|value| cx.factory().string(value))
        .collect::<Result<Vec<_>>>()?;
    cx.factory().list(values)
}

fn status_symbol(status: CalcStatus) -> Symbol {
    Symbol::qualified(
        "expr-tree/status",
        match status {
            CalcStatus::NeverCalculated => "never-calculated",
            CalcStatus::Fresh => "fresh",
            CalcStatus::MaybeStale => "maybe-stale",
            CalcStatus::Pending => "pending",
            CalcStatus::Failed => "failed",
            CalcStatus::Frozen => "frozen",
            CalcStatus::Blocked => "blocked",
        },
    )
}

fn reason_symbol(reason: CalcReason) -> Symbol {
    Symbol::qualified(
        "expr-tree/reason",
        match reason {
            CalcReason::DirectedVerify => "directed-verify",
            CalcReason::DirectedForceRoots => "directed-force-roots",
            CalcReason::DirectedForceRecursive => "directed-force-recursive",
            CalcReason::AutomaticMutation => "automatic-mutation",
            CalcReason::Continuation => "continuation",
        },
    )
}

fn trigger_symbol(trigger: CalcTrigger) -> Symbol {
    Symbol::qualified(
        "expr-tree/trigger",
        match trigger {
            CalcTrigger::Automatic => "automatic",
            CalcTrigger::OnDemand => "on-demand",
            CalcTrigger::Manual => "manual",
            CalcTrigger::Frozen => "frozen",
        },
    )
}

fn outcome_text(outcome: &CalcOutcome) -> String {
    match outcome {
        CalcOutcome::Succeeded => "succeeded".to_owned(),
        CalcOutcome::Failed { message } => format!("failed: {message}"),
        CalcOutcome::Blocked { message } => format!("blocked: {message}"),
        CalcOutcome::Cancelled => "cancelled".to_owned(),
        CalcOutcome::BudgetExhausted { message, .. } => {
            format!("budget-exhausted: {message}")
        }
    }
}

fn operation_name(kind: OperationKind) -> &'static str {
    crate::operation::operation_specs()
        .into_iter()
        .find(|spec| spec.kind == kind)
        .map(|spec| spec.name)
        .unwrap_or("operation")
}

fn field(name: &str) -> Symbol {
    Symbol::new(name.to_owned())
}

fn bound_text(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let bounded = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{bounded}...")
    } else {
        bounded
    }
}
