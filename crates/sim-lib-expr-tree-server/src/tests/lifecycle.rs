use std::sync::Arc;

use sim_kernel::Expr;
use sim_lib_server::DeterministicWallClock;

use crate::{ExpressionTreeServerLimits, WatchId};

use super::support::{ScriptClock, error_code, full_cx, realize_expr, runtime_call, server_with};

#[test]
fn slow_watch_reports_backpressure_and_cancellation() {
    let limits = ExpressionTreeServerLimits {
        watch_capacity: 2,
        ..Default::default()
    };
    let server = server_with(limits, Arc::new(DeterministicWallClock::new(1_000, 1)));
    let mut cx = full_cx();
    let session = server.create_session(&mut cx, "watch").unwrap();
    let watch = server.subscribe(&session).unwrap();
    for name in ["a", "b", "c"] {
        realize_expr(
            &server,
            &mut cx,
            runtime_call(
                &session,
                "new-dir",
                vec![Expr::String("/".to_owned()), Expr::String(name.to_owned())],
            ),
        );
    }
    let batch = server.poll_watch(&session, &watch, 8).unwrap();
    assert_eq!(batch.events.len(), 2);
    assert_eq!(batch.dropped, 1);
    assert!(batch.events[0].logical_tick < batch.events[1].logical_tick);

    server.cancel_watch(&session, &watch).unwrap();
    let cancelled = server.poll_watch(&session, &watch, 8).unwrap();
    assert!(cancelled.cancelled);
    assert!(cancelled.events.is_empty());
    assert!(
        server
            .cancel_watch(&session, &WatchId("not-this-watch".to_owned()))
            .is_err()
    );
}

#[test]
fn wall_observations_may_move_backward_without_affecting_logical_order() {
    let server = server_with(
        Default::default(),
        Arc::new(ScriptClock::new([1_000, 900, 800, 700, 600, 500])),
    );
    let mut cx = full_cx();
    let session = server.create_session(&mut cx, "clock").unwrap();
    let watch = server.subscribe(&session).unwrap();
    for name in ["first", "second"] {
        realize_expr(
            &server,
            &mut cx,
            runtime_call(
                &session,
                "new-dir",
                vec![Expr::String("/".to_owned()), Expr::String(name.to_owned())],
            ),
        );
    }
    let events = server.poll_watch(&session, &watch, 8).unwrap().events;
    assert_eq!(events.len(), 2);
    assert!(events[0].logical_tick < events[1].logical_tick);
    assert!(events[0].wall_ms > events[1].wall_ms);
}

#[test]
fn idle_expiry_cancel_and_restart_fail_closed() {
    let limits = ExpressionTreeServerLimits {
        max_idle_ticks: 2,
        ..Default::default()
    };
    let server = server_with(limits, Arc::new(DeterministicWallClock::new(1, 1)));
    let mut cx = full_cx();
    let expired = server.create_session(&mut cx, "idle").unwrap();
    assert_eq!(server.maintenance_tick(3).unwrap(), 1);
    assert_eq!(
        server.snapshot(&expired).unwrap_err().code(),
        "unknown-session"
    );

    let cancelled = server.create_session(&mut cx, "cancelled").unwrap();
    assert!(server.close_session(&cancelled).unwrap());
    assert!(!server.close_session(&cancelled).unwrap());

    let live = server.create_session(&mut cx, "restart").unwrap();
    let restarted = server_with(limits, Arc::new(DeterministicWallClock::new(100, 1)));
    let old = realize_expr(
        &restarted,
        &mut cx,
        runtime_call(&live, "list", vec![Expr::String("/".to_owned())]),
    );
    assert_eq!(error_code(&old).as_deref(), Some("unknown-session"));
}
