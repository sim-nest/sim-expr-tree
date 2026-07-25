use sim_kernel::{
    Cx, Demand, Error as KernelError, EvalPolicy, Expr, Phase, PreparedArgs, Symbol, Value,
    object::RawArgs,
};

pub const EXPR_TREE_REF: &str = "expr-tree/ref";

pub struct ExprTreeRefPolicy<P> {
    inner: P,
}

impl<P> ExprTreeRefPolicy<P> {
    pub fn new(inner: P) -> Self {
        Self { inner }
    }

    pub fn inner(&self) -> &P {
        &self.inner
    }
}

impl<P: EvalPolicy> EvalPolicy for ExprTreeRefPolicy<P> {
    fn name(&self) -> &'static str {
        "expr-tree-ref"
    }

    fn allow_macro_expansion(&self, phase: Phase) -> bool {
        self.inner.allow_macro_expansion(phase)
    }

    fn prepare_call_args(
        &self,
        cx: &mut Cx,
        raw: RawArgs,
        demands: &[Demand],
    ) -> sim_kernel::Result<PreparedArgs> {
        self.inner.prepare_call_args(cx, raw, demands)
    }

    fn force(&self, cx: &mut Cx, value: Value, demand: Demand) -> sim_kernel::Result<Value> {
        self.inner.force(cx, value, demand)
    }

    fn eval_expr(&self, cx: &mut Cx, expr: Expr) -> sim_kernel::Result<Value> {
        self.inner.eval_expr(cx, expr)
    }

    fn resolve_unbound_symbol(&self, cx: &mut Cx, symbol: Symbol) -> sim_kernel::Result<Value> {
        cx.factory().string(format!("expr-tree/ref:{symbol}"))
    }

    fn resolve_unbound_call(
        &self,
        cx: &mut Cx,
        operator: Symbol,
        args: Vec<Expr>,
    ) -> sim_kernel::Result<Value> {
        if operator.to_string() == EXPR_TREE_REF {
            let Some(reference) = args.first() else {
                return Err(KernelError::Eval(
                    "expr-tree/ref requires a reference".to_owned(),
                ));
            };
            return cx
                .factory()
                .string(format!("expr-tree/ref:{}", expr_text(reference)));
        }
        self.inner.resolve_unbound_call(cx, operator, args)
    }
}

fn expr_text(expr: &Expr) -> String {
    match expr {
        Expr::String(value) => value.clone(),
        Expr::Symbol(symbol) => symbol.to_string(),
        other => format!("{other:?}"),
    }
}
