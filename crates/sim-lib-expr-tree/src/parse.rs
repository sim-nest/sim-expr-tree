use sim_expr_tree_calc::{CalcPolicyPatch, CalcTrigger, CodecPolicyPatch};
use sim_expr_tree_core::{BackendKind, MountEpoch, MountResource};
use sim_kernel::{Cx, Expr, Value};

use crate::{DurablePolicyRecord, TreeHandle};

pub(crate) fn tree_arg(value: &Value) -> std::result::Result<TreeHandle, String> {
    value
        .object()
        .downcast_ref::<TreeHandle>()
        .cloned()
        .ok_or_else(|| "expected opaque expression-tree handle".to_owned())
}

pub(crate) fn string_arg(
    cx: &mut Cx,
    value: &Value,
    field: &'static str,
) -> std::result::Result<String, String> {
    match value
        .object()
        .as_expr(cx)
        .map_err(|error| error.to_string())?
    {
        Expr::String(text) => Ok(text),
        Expr::Symbol(symbol) => Ok(symbol.to_string()),
        other => Err(format!("{field} must be string or symbol, found {other:?}")),
    }
}

pub(crate) fn optional_name_arg(
    cx: &mut Cx,
    value: &Value,
) -> std::result::Result<Option<String>, String> {
    match value
        .object()
        .as_expr(cx)
        .map_err(|error| error.to_string())?
    {
        Expr::Nil => Ok(None),
        Expr::String(text) => Ok(Some(text)),
        Expr::Symbol(symbol) => Ok(Some(symbol.to_string())),
        other => Err(format!(
            "optional name must be nil, string, or symbol, found {other:?}"
        )),
    }
}

pub(crate) fn source_arg(cx: &mut Cx, value: &Value) -> std::result::Result<Expr, String> {
    value
        .object()
        .as_expr(cx)
        .map_err(|error| error.to_string())
}

pub(crate) fn request_id_arg(cx: &mut Cx, value: &Value) -> std::result::Result<u64, String> {
    let text = string_arg(cx, value, "request id")?;
    text.parse::<u64>()
        .map_err(|_| "request id must be unsigned decimal text".to_owned())
}

pub(crate) fn mount_epoch_arg(
    cx: &mut Cx,
    value: &Value,
) -> std::result::Result<MountEpoch, String> {
    let text = string_arg(cx, value, "mount epoch")?;
    let epoch = text
        .parse::<u64>()
        .map_err(|_| "mount epoch must be unsigned decimal text".to_owned())?;
    Ok(MountEpoch::new(epoch))
}

pub(crate) fn backend_arg(cx: &mut Cx, value: &Value) -> std::result::Result<BackendKind, String> {
    match string_arg(cx, value, "backend")?.as_str() {
        "memory" => Ok(BackendKind::Memory),
        "filesystem" => Ok(BackendKind::Filesystem),
        "database" => Ok(BackendKind::Database),
        "read-only" => Ok(BackendKind::ReadOnly),
        "mounted-namespace" => Ok(BackendKind::MountedNamespace),
        other => Err(format!("unsupported expression-tree backend {other}")),
    }
}

pub(crate) fn mount_resource_arg(
    cx: &mut Cx,
    value: &Value,
) -> std::result::Result<MountResource, String> {
    match string_arg(cx, value, "mount resource")?.as_str() {
        "table" => Ok(MountResource::Table),
        "dir" => Ok(MountResource::Dir),
        other => Err(format!(
            "mount resource must be table or dir, found {other}"
        )),
    }
}

pub(crate) fn calc_policy_arg(
    cx: &mut Cx,
    value: &Value,
) -> std::result::Result<CalcPolicyPatch, String> {
    if let Some(record) = value.object().downcast_ref::<DurablePolicyRecord>() {
        return Ok(CalcPolicyPatch {
            trigger: Some(parse_trigger(&record.calc_trigger)?),
            ..CalcPolicyPatch::default()
        });
    }
    let expression = value
        .object()
        .as_expr(cx)
        .map_err(|error| error.to_string())?;
    match expression {
        Expr::String(trigger) => Ok(CalcPolicyPatch {
            trigger: Some(parse_trigger(&trigger)?),
            ..CalcPolicyPatch::default()
        }),
        Expr::Symbol(trigger) => Ok(CalcPolicyPatch {
            trigger: Some(parse_trigger(&trigger.to_string())?),
            ..CalcPolicyPatch::default()
        }),
        Expr::Map(entries) => {
            let trigger = map_text(&entries, "trigger")
                .map(parse_trigger)
                .transpose()?;
            let priority = map_text(&entries, "priority")
                .map(|value| {
                    value
                        .parse::<i16>()
                        .map_err(|_| "priority must be signed 16-bit decimal text".to_owned())
                })
                .transpose()?;
            let debounce_ms = map_text(&entries, "debounce-ms")
                .map(|value| {
                    value
                        .parse::<u32>()
                        .map_err(|_| "debounce-ms must be unsigned decimal text".to_owned())
                })
                .transpose()?;
            Ok(CalcPolicyPatch {
                trigger,
                priority,
                debounce_ms,
                ..CalcPolicyPatch::default()
            })
        }
        other => Err(format!(
            "calculation policy must be trigger text, map, or PolicyRecord, found {other:?}"
        )),
    }
}

pub(crate) fn codec_policy_arg(
    cx: &mut Cx,
    value: &Value,
) -> std::result::Result<CodecPolicyPatch, String> {
    if let Some(record) = value.object().downcast_ref::<DurablePolicyRecord>() {
        return Ok(CodecPolicyPatch {
            source_codec: Some(record.source_codec.clone()),
            result_codec: Some(record.result_codec.clone()),
            ..CodecPolicyPatch::default()
        });
    }
    let expression = value
        .object()
        .as_expr(cx)
        .map_err(|error| error.to_string())?;
    match expression {
        Expr::String(codec) => Ok(CodecPolicyPatch::set_codec(codec)),
        Expr::Symbol(codec) => Ok(CodecPolicyPatch::set_codec(codec.to_string())),
        Expr::Map(entries) => {
            let both = map_text(&entries, "codec").map(str::to_owned);
            let source = map_text(&entries, "source-codec")
                .map(parse_optional_codec)
                .transpose()?;
            let result = map_text(&entries, "result-codec")
                .map(parse_optional_codec)
                .transpose()?;
            Ok(CodecPolicyPatch {
                source_codec: source.or_else(|| both.clone().map(Some)),
                result_codec: result.or_else(|| both.map(Some)),
                ..CodecPolicyPatch::default()
            })
        }
        other => Err(format!(
            "codec policy must be codec text, map, or PolicyRecord, found {other:?}"
        )),
    }
}

fn parse_trigger(value: &str) -> std::result::Result<CalcTrigger, String> {
    match value {
        "automatic" => Ok(CalcTrigger::Automatic),
        "on-demand" => Ok(CalcTrigger::OnDemand),
        "manual" => Ok(CalcTrigger::Manual),
        "frozen" => Ok(CalcTrigger::Frozen),
        other => Err(format!("unknown calculation trigger {other}")),
    }
}

fn parse_optional_codec(value: &str) -> std::result::Result<Option<String>, String> {
    if value == "none" {
        Ok(None)
    } else if value.is_empty() {
        Err("codec name must not be empty".to_owned())
    } else {
        Ok(Some(value.to_owned()))
    }
}

fn map_text<'a>(entries: &'a [(Expr, Expr)], key: &str) -> Option<&'a str> {
    entries.iter().find_map(|(candidate, value)| {
        let matches = matches!(candidate, Expr::Symbol(symbol) if symbol.to_string() == key)
            || matches!(candidate, Expr::String(text) if text == key);
        if !matches {
            return None;
        }
        match value {
            Expr::String(text) => Some(text.as_str()),
            Expr::Symbol(symbol) => Some(symbol.name.as_ref()),
            _ => None,
        }
    })
}
