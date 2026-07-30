use std::sync::Arc;

use sim_kernel::{
    AbiVersion, Args, Callable, CapabilityName, ClassRef, Cx, Dependency, Export, Lib, LibManifest,
    LibTarget, Linker, LoadCx, Object, ObjectCompat, RawArgs, Result, ShapeRef, Symbol, Value,
    Version,
};

use crate::{
    capability::{
        expr_tree_calculate_capability, expr_tree_mount_capability, expr_tree_read_capability,
        expr_tree_write_capability,
    },
    citizen::{
        durable_policy_class_symbol, durable_source_class_symbol, expr_tree_citizen_registry,
    },
    dispatch::dispatch,
    handle::TreeRuntime,
    projection::cards_for_contracts,
    shape::{
        argument_shape, operation_args_shape_symbol, operation_result_shape_symbol, result_shape,
    },
    source::RawSource,
};

/// Stable manifest id for the loadable expression-tree library.
pub fn expr_tree_lib_symbol() -> Symbol {
    Symbol::qualified("lib", "expr-tree")
}

/// Value export containing one Card projection per operation.
pub fn expr_tree_operation_cards_symbol() -> Symbol {
    Symbol::qualified("expr-tree", "operation-cards")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OperationKind {
    Open,
    NewCell,
    NewDir,
    Mount,
    Unmount,
    Move,
    Rename,
    Delete,
    SetExpr,
    SetCalcPolicy,
    SetCodecPolicy,
    Ref,
    List,
    Calculate,
    Recalculate,
    RecalculateRecursive,
    Cancel,
    Refresh,
    Status,
    Explain,
    Watch,
}

#[derive(Clone, Copy)]
pub(crate) enum CapabilityKind {
    Read,
    Write,
    Calculate,
    Mount,
}

impl CapabilityKind {
    pub(crate) fn name(self) -> CapabilityName {
        match self {
            Self::Read => expr_tree_read_capability(),
            Self::Write => expr_tree_write_capability(),
            Self::Calculate => expr_tree_calculate_capability(),
            Self::Mount => expr_tree_mount_capability(),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct OperationSpec {
    pub(crate) kind: OperationKind,
    pub(crate) name: &'static str,
    pub(crate) min_args: usize,
    pub(crate) max_args: usize,
    pub(crate) capability: CapabilityKind,
    pub(crate) args_detail: &'static str,
    pub(crate) result_detail: &'static str,
}

impl OperationSpec {
    pub(crate) fn symbol(self) -> Symbol {
        Symbol::qualified("expr-tree", self.name)
    }
}

/// Host-registered expression-tree runtime library.
pub struct ExprTreeLib;

impl Lib for ExprTreeLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: expr_tree_lib_symbol(),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: vec![Dependency {
                id: Symbol::qualified("codec", "lisp"),
                minimum_version: None,
            }],
            capabilities: Vec::new(),
            exports: expr_tree_exports(),
        }
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        expr_tree_citizen_registry()?.install_all(linker)?;
        let runtime = Arc::new(TreeRuntime::new());
        let mut contracts = Vec::new();
        for spec in operation_specs() {
            let args_shape =
                argument_shape(spec.name, spec.min_args, spec.max_args, spec.args_detail);
            let result_shape = result_shape(spec.name, spec.result_detail);
            linker.shape_value(operation_args_shape_symbol(spec.name), args_shape.clone())?;
            linker.shape_value(
                operation_result_shape_symbol(spec.name),
                result_shape.clone(),
            )?;
            linker.function_value(
                spec.symbol(),
                cx.factory().opaque(Arc::new(OperationFunction {
                    spec,
                    runtime: Arc::clone(&runtime),
                    args_shape: args_shape.clone(),
                    result_shape: result_shape.clone(),
                }))?,
            )?;
            contracts.push((spec, args_shape, result_shape));
        }
        let cards = cards_for_contracts(cx.factory(), &contracts)?;
        linker.value(
            expr_tree_operation_cards_symbol(),
            cx.factory().list(cards)?,
        )?;
        Ok(())
    }
}

/// Installs [`ExprTreeLib`] exactly once.
pub fn install_expr_tree_lib(cx: &mut Cx) -> Result<()> {
    if cx.registry().lib(&expr_tree_lib_symbol()).is_none() {
        cx.load_lib(&ExprTreeLib)?;
    }
    Ok(())
}

/// Returns every stable operation symbol in product-contract order.
pub fn expr_tree_operation_symbols() -> Vec<Symbol> {
    operation_specs()
        .into_iter()
        .map(OperationSpec::symbol)
        .collect()
}

/// Returns the manifest exports for classes, operations, Shapes, and Cards.
pub fn expr_tree_exports() -> Vec<Export> {
    let mut exports = vec![
        Export::Class {
            symbol: durable_source_class_symbol(),
            class_id: None,
        },
        Export::Class {
            symbol: durable_policy_class_symbol(),
            class_id: None,
        },
        Export::Value {
            symbol: expr_tree_operation_cards_symbol(),
        },
    ];
    for spec in operation_specs() {
        exports.push(Export::Function {
            symbol: spec.symbol(),
            function_id: None,
        });
        exports.push(Export::Shape {
            symbol: operation_args_shape_symbol(spec.name),
            shape_id: None,
        });
        exports.push(Export::Shape {
            symbol: operation_result_shape_symbol(spec.name),
            shape_id: None,
        });
    }
    exports
}

struct OperationFunction {
    spec: OperationSpec,
    runtime: Arc<TreeRuntime>,
    args_shape: Value,
    result_shape: Value,
}

impl Object for OperationFunction {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<function {}>", self.spec.symbol()))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for OperationFunction {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Function"))
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for OperationFunction {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        self.invoke(cx, args.into_vec())
    }

    fn call_exprs(&self, cx: &mut Cx, args: RawArgs) -> Result<Value> {
        let expressions = args.into_exprs();
        if matches!(
            self.spec.kind,
            OperationKind::NewCell | OperationKind::SetExpr
        ) {
            let source_index = expressions.len().saturating_sub(1);
            let mut values = Vec::with_capacity(expressions.len());
            for (index, expression) in expressions.into_iter().enumerate() {
                if index == source_index {
                    values.push(cx.factory().opaque(Arc::new(RawSource(expression)))?);
                } else {
                    values.push(cx.eval_expr(expression)?);
                }
            }
            self.invoke(cx, values)
        } else {
            let values = expressions
                .into_iter()
                .map(|expression| cx.eval_expr(expression))
                .collect::<Result<Vec<_>>>()?;
            self.invoke(cx, values)
        }
    }

    fn browse_args_shape(&self, _cx: &mut Cx) -> Result<Option<ShapeRef>> {
        Ok(Some(self.args_shape.clone()))
    }

    fn browse_result_shape(&self, _cx: &mut Cx) -> Result<Option<ShapeRef>> {
        Ok(Some(self.result_shape.clone()))
    }
}

impl OperationFunction {
    fn invoke(&self, cx: &mut Cx, values: Vec<Value>) -> Result<Value> {
        if !(self.spec.min_args..=self.spec.max_args).contains(&values.len()) {
            return Err(crate::dispatch::bounded_error(
                self.spec.name,
                format!(
                    "expected {}..={} arguments, found {}",
                    self.spec.min_args,
                    self.spec.max_args,
                    values.len()
                ),
            ));
        }
        cx.require(&self.spec.capability.name())?;
        dispatch(self.spec.kind, &self.runtime, cx, values)
    }
}

pub(crate) fn operation_specs() -> Vec<OperationSpec> {
    use CapabilityKind::{Calculate as C, Mount as M, Read as R, Write as W};
    use OperationKind::*;

    vec![
        spec(
            Open,
            "open",
            1,
            1,
            R,
            "storage name",
            "opaque live tree handle",
        ),
        spec(
            NewCell,
            "new-cell",
            4,
            4,
            W,
            "tree, parent path, optional name, raw source Expr",
            "canonical cell path",
        ),
        spec(
            NewDir,
            "new-dir",
            3,
            3,
            W,
            "tree, parent path, optional name",
            "canonical directory path",
        ),
        spec(
            Mount,
            "mount",
            5,
            5,
            M,
            "tree, path, backend, table-or-dir, epoch",
            "canonical mount path",
        ),
        spec(
            Unmount,
            "unmount",
            2,
            2,
            M,
            "tree and mount path",
            "true after removal",
        ),
        spec(
            Move,
            "move",
            3,
            3,
            W,
            "tree, source path, target path",
            "canonical target path",
        ),
        spec(
            Rename,
            "rename",
            3,
            3,
            W,
            "tree, path, new segment",
            "canonical target path",
        ),
        spec(
            Delete,
            "delete",
            2,
            2,
            W,
            "tree and empty-dir-or-cell path",
            "true after removal",
        ),
        spec(
            SetExpr,
            "set-expr",
            3,
            3,
            W,
            "tree, cell path, raw source Expr",
            "canonical cell path",
        ),
        spec(
            SetCalcPolicy,
            "set-calc-policy",
            3,
            3,
            W,
            "tree, owner path, policy record or map",
            "durable policy Citizen",
        ),
        spec(
            SetCodecPolicy,
            "set-codec-policy",
            3,
            3,
            W,
            "tree, owner path, policy record or map",
            "durable policy Citizen",
        ),
        spec(
            Ref,
            "ref",
            2,
            3,
            R,
            "tree, path reference, optional base directory",
            "ordinary current cell Value",
        ),
        spec(
            List,
            "list",
            2,
            2,
            R,
            "tree and directory path",
            "bounded entry-card list",
        ),
        spec(
            Calculate,
            "calculate",
            2,
            2,
            C,
            "tree and cell path",
            "verified ordinary Value",
        ),
        spec(
            Recalculate,
            "recalculate",
            2,
            2,
            C,
            "tree and cell path",
            "root-forced ordinary Value",
        ),
        spec(
            RecalculateRecursive,
            "recalculate-recursive",
            2,
            2,
            C,
            "tree and cell path",
            "recursively forced ordinary Value",
        ),
        spec(
            Cancel,
            "cancel",
            2,
            2,
            C,
            "tree and request-id text",
            "whether queued work was cancelled",
        ),
        spec(
            Refresh,
            "refresh",
            1,
            1,
            C,
            "tree",
            "bounded refresh evidence table",
        ),
        spec(
            Status,
            "status",
            2,
            2,
            R,
            "tree and cell path",
            "non-evaluating status symbol",
        ),
        spec(
            Explain,
            "explain",
            2,
            2,
            R,
            "tree and cell path",
            "bounded receipt and durable-record Card",
        ),
        spec(
            Watch,
            "watch",
            1,
            1,
            R,
            "tree",
            "standard bounded Stream value",
        ),
    ]
}

fn spec(
    kind: OperationKind,
    name: &'static str,
    min_args: usize,
    max_args: usize,
    capability: CapabilityKind,
    args_detail: &'static str,
    result_detail: &'static str,
) -> OperationSpec {
    OperationSpec {
        kind,
        name,
        min_args,
        max_args,
        capability,
        args_detail,
        result_detail,
    }
}
