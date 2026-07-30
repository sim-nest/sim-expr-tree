use sim_expr_tree_calc::CalcTrigger;
use sim_kernel::Expr;

const MAX_SOURCE_PROJECTION_BYTES: usize = 65_536;

pub(crate) fn source_projection(source: &Expr) -> std::result::Result<String, String> {
    let projection = format!("{source:?}");
    if projection.len() > MAX_SOURCE_PROJECTION_BYTES {
        Err(format!(
            "source projection exceeds {MAX_SOURCE_PROJECTION_BYTES} bytes"
        ))
    } else {
        Ok(projection)
    }
}

pub(crate) fn trigger_name(trigger: CalcTrigger) -> &'static str {
    match trigger {
        CalcTrigger::Automatic => "automatic",
        CalcTrigger::OnDemand => "on-demand",
        CalcTrigger::Manual => "manual",
        CalcTrigger::Frozen => "frozen",
    }
}

pub(crate) fn debug_error(error: impl std::fmt::Debug) -> String {
    format!("{error:?}")
}
