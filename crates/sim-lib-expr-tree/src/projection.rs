use std::sync::Arc;

use sim_kernel::{Cx, Factory, Ref, Result, Symbol, Value, card::Card};

use crate::{
    operation::{OperationSpec, operation_specs},
    shape::{argument_shape, result_shape},
};

/// Projects one browseable Card for every stable expression-tree operation.
pub fn operation_cards(cx: &mut Cx) -> Result<Vec<Value>> {
    let contracts = operation_specs()
        .into_iter()
        .map(|spec| {
            (
                spec,
                argument_shape(spec.name, spec.min_args, spec.max_args, spec.args_detail),
                result_shape(spec.name, spec.result_detail),
            )
        })
        .collect::<Vec<_>>();
    cards_for_contracts(cx.factory(), &contracts)
}

pub(crate) fn cards_for_contracts(
    factory: &dyn Factory,
    contracts: &[(OperationSpec, Value, Value)],
) -> Result<Vec<Value>> {
    contracts
        .iter()
        .map(|(spec, args_shape, result_shape)| {
            operation_card(factory, *spec, args_shape.clone(), result_shape.clone())
        })
        .collect()
}

fn operation_card(
    factory: &dyn Factory,
    spec: OperationSpec,
    args_shape: Value,
    result_shape: Value,
) -> Result<Value> {
    let subject = Ref::Symbol(spec.symbol());
    let entries = vec![
        (field("subject"), factory.symbol(spec.symbol())?),
        (
            field("kind"),
            factory.symbol(Symbol::qualified("expr-tree", "operation"))?,
        ),
        (
            field("help"),
            factory.string(format!(
                "expr-tree/{}: {}; result: {}",
                spec.name, spec.args_detail, spec.result_detail
            ))?,
        ),
        (field("args"), args_shape),
        (field("result"), result_shape),
        (field("tests"), factory.list(Vec::new())?),
        (
            field("ops"),
            factory.list(vec![factory.symbol(spec.symbol())?])?,
        ),
        (
            field("requires"),
            factory.list(vec![factory.symbol(Symbol::qualified(
                "capability",
                spec.capability.name().as_str(),
            ))?])?,
        ),
        (field("see-also"), factory.list(Vec::new())?),
        (field("shape-known"), factory.bool(true)?),
    ];
    factory.opaque(Arc::new(Card::new(subject, entries)))
}

fn field(name: &str) -> Symbol {
    Symbol::new(name.to_owned())
}
