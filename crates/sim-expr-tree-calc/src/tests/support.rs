use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use sim_incremental_core::ObservationKind;
use sim_kernel::{
    Args, Callable, Cx, DefaultFactory, Dir, EagerPolicy, Error, Expr, Object, ObjectCompat,
    StrictNames, Symbol, Table, Value,
};
use sim_table_core::TablePath;

use crate::{CalcQuery, EXPR_TREE_REF, ExprTreeCalc, ExprTreeRefPolicy, calc::CalcState};

#[derive(Clone, Default)]
pub(super) struct TestRuntime {
    pub(super) source: Arc<Mutex<String>>,
    calls: Arc<Mutex<BTreeMap<String, usize>>>,
    pub(super) fail: Arc<AtomicBool>,
    pub(super) fail_attempts: Arc<AtomicUsize>,
}

impl TestRuntime {
    fn context(&self) -> Cx {
        let mut cx = strict_context();
        bind_callable(
            &mut cx,
            "probe",
            ProbeCallable {
                source: Arc::clone(&self.source),
                calls: Arc::clone(&self.calls),
            },
        );
        bind_callable(
            &mut cx,
            "concat",
            ConcatCallable {
                calls: Arc::clone(&self.calls),
            },
        );
        bind_callable(&mut cx, "lambda", LambdaCallable);
        bind_callable(&mut cx, "return-lambda", ReturnLambdaCallable);
        bind_callable(&mut cx, "return-dir", ReturnDirCallable);
        bind_callable(&mut cx, "return-opaque", ReturnOpaqueCallable);
        bind_callable(
            &mut cx,
            "fallible",
            FallibleCallable {
                fail: Arc::clone(&self.fail),
                attempts: Arc::clone(&self.fail_attempts),
            },
        );
        cx
    }

    pub(super) fn count(&self, key: &str) -> usize {
        self.calls
            .lock()
            .expect("call counter poisoned")
            .get(key)
            .copied()
            .unwrap_or(0)
    }
}

pub(super) fn runtime_calc(runtime: TestRuntime) -> ExprTreeCalc {
    ExprTreeCalc::with_context_factory(move || runtime.context())
}

pub(super) fn install_diamond(calc: &mut ExprTreeCalc) {
    calc.set_cell(path("/a"), probe_expr());
    calc.set_cell(
        path("/b"),
        call(
            "concat",
            vec![
                explicit_ref("/a"),
                Expr::String("-b".to_owned()),
                Expr::String("b".to_owned()),
            ],
        ),
    );
    calc.set_cell(
        path("/c"),
        call(
            "concat",
            vec![
                explicit_ref("/a"),
                Expr::String("-c".to_owned()),
                Expr::String("c".to_owned()),
            ],
        ),
    );
    calc.set_cell(
        path("/d"),
        call(
            "concat",
            vec![
                explicit_ref("/b"),
                explicit_ref("/c"),
                Expr::String("d".to_owned()),
            ],
        ),
    );
}

pub(super) fn probe_expr() -> Expr {
    call("probe", vec![])
}

pub(super) fn explicit_ref(reference: &str) -> Expr {
    Expr::Call {
        operator: Box::new(Expr::Symbol(Symbol::new(EXPR_TREE_REF))),
        args: vec![Expr::String(reference.to_owned())],
    }
}

pub(super) fn call(name: &str, args: Vec<Expr>) -> Expr {
    Expr::Call {
        operator: Box::new(Expr::Symbol(Symbol::new(name))),
        args,
    }
}

pub(super) fn path(input: &str) -> TablePath {
    TablePath::parse_absolute(input).unwrap()
}

pub(super) fn dependencies(
    calc: &mut ExprTreeCalc,
    key: &str,
) -> Vec<(CalcQuery, ObservationKind)> {
    calc.cell_dependencies(&path(key)).unwrap()
}

pub(super) fn strict_context() -> Cx {
    Cx::new(
        Arc::new(ExprTreeRefPolicy::new(StrictNames(EagerPolicy))),
        Arc::new(DefaultFactory),
    )
}

pub(super) fn lock_probe_context(state: Arc<RwLock<CalcState>>) -> Cx {
    let mut cx = strict_context();
    bind_callable(&mut cx, "lock-probe", LockProbeCallable { state });
    cx
}

pub(super) fn value_expr(value: Value) -> Expr {
    value
        .object()
        .as_expr(&mut strict_context())
        .expect("test value must expose an expression")
}

fn bind_callable<T>(cx: &mut Cx, name: &str, callable: T)
where
    T: Callable + ObjectCompat + 'static,
{
    let value = cx.factory().opaque(Arc::new(callable)).unwrap();
    cx.env_mut().define(Symbol::new(name), value);
}

fn string_arg(cx: &mut Cx, value: &Value) -> sim_kernel::Result<String> {
    match value.object().as_expr(cx)? {
        Expr::String(value) => Ok(value),
        other => Err(Error::Eval(format!(
            "expected string argument, got {other:?}"
        ))),
    }
}

struct ProbeCallable {
    source: Arc<Mutex<String>>,
    calls: Arc<Mutex<BTreeMap<String, usize>>>,
}

impl Callable for ProbeCallable {
    fn call(&self, cx: &mut Cx, _args: Args) -> sim_kernel::Result<Value> {
        *self
            .calls
            .lock()
            .unwrap()
            .entry("probe".to_owned())
            .or_default() += 1;
        cx.factory().string(self.source.lock().unwrap().clone())
    }
}

impl_test_callable!(ProbeCallable, "#<probe>");

struct ConcatCallable {
    calls: Arc<Mutex<BTreeMap<String, usize>>>,
}

impl Callable for ConcatCallable {
    fn call(&self, cx: &mut Cx, args: Args) -> sim_kernel::Result<Value> {
        let values = args.values();
        let Some((label, parts)) = values.split_last() else {
            return Err(Error::Eval("concat requires a counter label".to_owned()));
        };
        let label = string_arg(cx, label)?;
        *self
            .calls
            .lock()
            .unwrap()
            .entry(format!("concat-{label}"))
            .or_default() += 1;
        let mut out = String::new();
        for part in parts {
            out.push_str(&string_arg(cx, part)?);
        }
        cx.factory().string(out)
    }
}

impl_test_callable!(ConcatCallable, "#<concat>");

struct LambdaCallable;

impl Callable for LambdaCallable {
    fn call(&self, _cx: &mut Cx, args: Args) -> sim_kernel::Result<Value> {
        args.values()
            .first()
            .cloned()
            .ok_or_else(|| Error::Eval("lambda requires one argument".to_owned()))
    }
}

impl_test_callable!(LambdaCallable, "#<lambda>");

macro_rules! return_callable {
    ($type:ident, $value:expr, $display:expr) => {
        struct $type;

        impl Callable for $type {
            fn call(&self, cx: &mut Cx, _args: Args) -> sim_kernel::Result<Value> {
                cx.factory().opaque(Arc::new($value))
            }
        }

        impl_test_callable!($type, $display);
    };
}

return_callable!(ReturnLambdaCallable, LambdaCallable, "#<return-lambda>");
return_callable!(ReturnDirCallable, EmptyDir, "#<return-dir>");
return_callable!(ReturnOpaqueCallable, OpaqueMarker, "#<return-opaque>");

struct FallibleCallable {
    fail: Arc<AtomicBool>,
    attempts: Arc<AtomicUsize>,
}

impl Callable for FallibleCallable {
    fn call(&self, cx: &mut Cx, _args: Args) -> sim_kernel::Result<Value> {
        self.attempts.fetch_add(1, Ordering::AcqRel);
        if self.fail.load(Ordering::Acquire) {
            Err(Error::Eval("requested failure".to_owned()))
        } else {
            cx.factory().string("good".to_owned())
        }
    }
}

impl_test_callable!(FallibleCallable, "#<fallible>");

struct LockProbeCallable {
    state: Arc<RwLock<CalcState>>,
}

impl Callable for LockProbeCallable {
    fn call(&self, cx: &mut Cx, _args: Args) -> sim_kernel::Result<Value> {
        let guard = self
            .state
            .try_write()
            .expect("calculator state lock or mutable borrow spanned SIM evaluation");
        drop(guard);
        cx.factory().string("unlocked".to_owned())
    }
}

impl_test_callable!(LockProbeCallable, "#<lock-probe>");

pub(super) struct OpaqueMarker;

impl Object for OpaqueMarker {
    fn display(&self, _cx: &mut Cx) -> sim_kernel::Result<String> {
        Ok("#<opaque-marker>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for OpaqueMarker {}

struct EmptyDir;

impl Object for EmptyDir {
    fn display(&self, _cx: &mut Cx) -> sim_kernel::Result<String> {
        Ok("#<empty-dir>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for EmptyDir {
    fn as_table_impl(&self) -> Option<&dyn Table> {
        Some(self)
    }

    fn as_dir(&self) -> Option<&dyn Dir> {
        Some(self)
    }
}

impl Table for EmptyDir {
    fn backend_symbol(&self) -> Symbol {
        Symbol::new("test/empty-dir")
    }

    fn get(&self, cx: &mut Cx, _key: Symbol) -> sim_kernel::Result<Value> {
        cx.factory().nil()
    }

    fn set(&self, _cx: &mut Cx, _key: Symbol, _value: Value) -> sim_kernel::Result<()> {
        Err(Error::Eval("empty test dir is immutable".to_owned()))
    }

    fn has(&self, _cx: &mut Cx, _key: Symbol) -> sim_kernel::Result<bool> {
        Ok(false)
    }

    fn del(&self, cx: &mut Cx, _key: Symbol) -> sim_kernel::Result<Value> {
        cx.factory().nil()
    }

    fn keys(&self, _cx: &mut Cx) -> sim_kernel::Result<Vec<Symbol>> {
        Ok(Vec::new())
    }

    fn entries(&self, _cx: &mut Cx) -> sim_kernel::Result<Vec<(Symbol, Value)>> {
        Ok(Vec::new())
    }

    fn len(&self, _cx: &mut Cx) -> sim_kernel::Result<usize> {
        Ok(0)
    }

    fn clear(&self, _cx: &mut Cx) -> sim_kernel::Result<()> {
        Ok(())
    }
}

impl Dir for EmptyDir {
    fn mkdir(&self, _cx: &mut Cx, _name: Symbol) -> sim_kernel::Result<Value> {
        Err(Error::Eval("empty test dir is immutable".to_owned()))
    }

    fn opendir(&self, _cx: &mut Cx, _name: Symbol) -> sim_kernel::Result<Option<Value>> {
        Ok(None)
    }

    fn rmdir(&self, _cx: &mut Cx, _name: Symbol) -> sim_kernel::Result<Value> {
        Err(Error::Eval("empty test dir is immutable".to_owned()))
    }

    fn is_dir(&self, _cx: &mut Cx, _name: Symbol) -> sim_kernel::Result<bool> {
        Ok(false)
    }
}

macro_rules! impl_test_callable {
    ($ty:ty, $display:expr) => {
        impl Object for $ty {
            fn display(&self, _cx: &mut Cx) -> sim_kernel::Result<String> {
                Ok($display.to_owned())
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }

        impl ObjectCompat for $ty {
            fn as_callable(&self) -> Option<&dyn Callable> {
                Some(self)
            }
        }
    };
}

use impl_test_callable;
