use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use sim_kernel::Expr;

use super::*;
use crate::{BackendRefreshSample, MountRefreshSource};

struct TestRefreshSource {
    samples: Mutex<VecDeque<Result<BackendRefreshSample, String>>>,
    calls: AtomicUsize,
    watch: bool,
}

impl TestRefreshSource {
    fn new(samples: impl IntoIterator<Item = BackendRefreshSample>) -> Self {
        Self {
            samples: Mutex::new(samples.into_iter().map(Ok).collect()),
            calls: AtomicUsize::new(0),
            watch: false,
        }
    }

    fn watch_managed() -> Self {
        Self {
            samples: Mutex::new(VecDeque::new()),
            calls: AtomicUsize::new(0),
            watch: true,
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::Acquire)
    }
}

impl MountRefreshSource for TestRefreshSource {
    fn has_watch_contract(&self) -> bool {
        self.watch
    }

    fn sample(&self) -> Result<BackendRefreshSample, String> {
        self.calls.fetch_add(1, Ordering::AcqRel);
        self.samples
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Err("no test sample".to_owned()))
    }
}

fn external_sample(epoch: u64, listing: u64, stamp: u64) -> BackendRefreshSample {
    BackendRefreshSample::new(MountEpoch::new(epoch))
        .with_listing("/external", listing)
        .with_stamp("/external/value", stamp)
}

#[test]
fn refresh_is_explicit_and_invalidates_stale_external_observations() {
    let runtime = TestRuntime::default();
    *runtime.source.lock().unwrap() = "old".to_owned();
    let source = Arc::new(TestRefreshSource::new([
        external_sample(1, 1, 1),
        external_sample(1, 1, 2),
    ]));
    let mut calc = runtime_calc(runtime.clone());
    calc.mount(
        path("/external"),
        MountResource::Dir,
        BackendKind::Filesystem,
        MountEpoch::new(1),
    );
    calc.attach_refresh_source(&path("/external"), source.clone())
        .unwrap();
    calc.set_cell(path("/external/value"), probe_expr());
    calc.set_cell(path("/result"), explicit_ref("/external/value"));

    let initial = calc.refresh().unwrap();
    assert_eq!(initial.sampled_mounts, vec!["/external"]);
    assert!(initial.invalidated_observations > 0);
    assert_eq!(
        value_expr(calc.verify_cell(&path("/result")).unwrap()),
        Expr::String("old".to_owned())
    );
    assert_eq!(runtime.count("probe"), 1);

    *runtime.source.lock().unwrap() = "new".to_owned();
    assert_eq!(
        value_expr(calc.current_cell(&path("/result")).unwrap()),
        Expr::String("old".to_owned())
    );
    calc.current_cell(&path("/result"))
        .unwrap()
        .object()
        .display(&mut strict_context())
        .unwrap();
    assert_eq!(
        value_expr(calc.verify_cell(&path("/result")).unwrap()),
        Expr::String("old".to_owned()),
        "verification must not poll an external backend implicitly"
    );
    let _ = calc.explain(&path("/result"));
    assert_eq!(
        source.calls(),
        1,
        "ordinary reads and display/explanation must not sample backends"
    );

    let refreshed = calc.refresh().unwrap();
    assert_eq!(refreshed.changed_stamps, vec!["/external/value"]);
    assert!(refreshed.invalidated_observations >= 3);
    assert_eq!(
        value_expr(calc.verify_cell(&path("/result")).unwrap()),
        Expr::String("new".to_owned())
    );
    assert_eq!(runtime.count("probe"), 2);
}

#[test]
fn refresh_compares_epochs_listings_and_stamps_without_polling_watch_mounts() {
    let polled = Arc::new(TestRefreshSource::new([
        external_sample(2, 3, 4),
        external_sample(3, 5, 6),
    ]));
    let watched = Arc::new(TestRefreshSource::watch_managed());
    let mut calc = ExprTreeCalc::new();
    calc.mount(
        path("/external"),
        MountResource::Dir,
        BackendKind::Filesystem,
        MountEpoch::new(2),
    );
    calc.mount(
        path("/watched"),
        MountResource::Dir,
        BackendKind::Database,
        MountEpoch::new(9),
    );
    calc.attach_refresh_source(&path("/external"), polled.clone())
        .unwrap();
    calc.attach_refresh_source(&path("/watched"), watched.clone())
        .unwrap();

    let first = calc.refresh().unwrap();
    assert_eq!(first.sampled_mounts, vec!["/external"]);
    assert_eq!(first.watch_managed_mounts, vec!["/watched"]);
    assert_eq!(watched.calls(), 0);
    let second = calc.refresh().unwrap();
    assert_eq!(second.changed_epochs, vec!["/external"]);
    assert_eq!(second.changed_listings, vec!["/external"]);
    assert_eq!(second.changed_stamps, vec!["/external/value"]);
    assert_eq!(watched.calls(), 0);
}
