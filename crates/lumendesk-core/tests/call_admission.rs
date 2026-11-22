//! Public-API, display-free conformance for host-independent call scheduling.
use lumendesk_core::{
    Authorization, Bridge, BridgeConfig, CallJob, CallRequest, CoreError, MethodSchema, Schema,
    StreamItem, StreamMethodSchema, StreamResponseItem, WindowId,
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

const WAIT: Duration = Duration::from_secs(5);

fn session(bridge: &Bridge, window: u64) -> Authorization {
    Authorization {
        window: WindowId(window),
        session: bridge
            .create_session(WindowId(window), "lumendesk://app/index.html")
            .unwrap(),
        page_url: "lumendesk://app/index.html".into(),
        top_level: true,
    }
}

fn fixture(limit: usize) -> (Bridge, Authorization, Arc<AtomicUsize>) {
    let mut config = BridgeConfig::production("lumendesk://app").unwrap();
    config.max_pending_per_session = limit;
    config.max_payload_bytes = 256;
    let bridge = Bridge::new(config).unwrap();
    let executions = Arc::new(AtomicUsize::new(0));
    let count = executions.clone();
    bridge
        .register(
            MethodSchema::new("echo", Schema::String, Schema::String),
            move |_, value: String| {
                count.fetch_add(1, Ordering::SeqCst);
                Ok(value)
            },
        )
        .unwrap();
    let auth = session(&bridge, 1);
    (bridge, auth, executions)
}

fn request(id: u64, method: &str, args: Value) -> CallRequest {
    CallRequest {
        id,
        method: method.into(),
        args,
    }
}

fn echo(id: u64) -> CallRequest {
    request(id, "echo", json!(format!("呼叫 {id}")))
}

#[test]
fn synchronous_dispatch_still_requires_admission_order() {
    let (bridge, auth, count) = fixture(2);
    assert!(bridge.dispatch(&auth, echo(2)).result.is_ok());
    assert_eq!(
        bridge.dispatch(&auth, echo(1)).result,
        Err(CoreError::ReusedRequestId)
    );
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn admitted_jobs_execute_in_every_three_job_permutation() {
    fn assert_send<T: Send>() {}
    assert_send::<CallJob>();
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let (bridge, auth, count) = fixture(3);
        let mut jobs: Vec<_> = (1..=3)
            .map(|id| Some(bridge.begin_call(&auth, echo(id)).unwrap()))
            .collect();
        assert_eq!(count.load(Ordering::SeqCst), 0);
        for index in order {
            let job = jobs[index].take().unwrap();
            let id = index as u64 + 1;
            assert_eq!(job.request_id(), id);
            assert_eq!(job.authorization().session, auth.session);
            assert!(!format!("{job:?}").contains(auth.session.as_str()));
            let response = job.run();
            assert_eq!(response.id, id);
            assert_eq!(response.result, Ok(echo(id).args));
        }
        assert_eq!(count.load(Ordering::SeqCst), 3);
        assert!(
            bridge
                .begin_call(&auth, echo(4))
                .unwrap()
                .run()
                .result
                .is_ok()
        );
    }
}

#[test]
fn later_worker_completes_while_earlier_worker_is_blocked() {
    let (bridge, auth, _) = fixture(2);
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Mutex::new(release_rx);
    bridge
        .register(
            MethodSchema::new("block", Schema::Null, Schema::Null),
            move |_, _: ()| {
                started_tx.send(()).unwrap();
                release_rx.lock().unwrap().recv_timeout(WAIT).unwrap();
                Ok(())
            },
        )
        .unwrap();
    let first = bridge
        .begin_call(&auth, request(1, "block", Value::Null))
        .unwrap();
    let second = bridge.begin_call(&auth, echo(2)).unwrap();
    let worker = thread::spawn(move || first.run());
    started_rx.recv_timeout(WAIT).unwrap();
    assert_eq!(second.run().result, Ok(echo(2).args));
    release_tx.send(()).unwrap();
    assert_eq!(worker.join().unwrap().result, Ok(Value::Null));
}

#[test]
fn synchronous_and_admitted_calls_share_the_namespace() {
    let (bridge, auth, count) = fixture(2);
    let first = bridge.begin_call(&auth, echo(1)).unwrap();
    assert_eq!(bridge.dispatch(&auth, echo(2)).result, Ok(echo(2).args));
    assert_eq!(first.run().result, Ok(echo(1).args));
    assert_eq!(count.load(Ordering::SeqCst), 2);
    assert_eq!(
        bridge.begin_call(&auth, echo(2)).unwrap_err(),
        CoreError::ReusedRequestId
    );
}

#[test]
fn calls_and_streams_share_ids_and_pending_budget() {
    let (bridge, auth, _) = fixture(2);
    bridge
        .register_stream::<(), u32, (), _>(
            StreamMethodSchema::new("count", Schema::Null, Schema::Integer, Schema::Null),
            |_, _, sink| {
                sink.send(7)?;
                Ok(())
            },
        )
        .unwrap();
    let first = bridge.begin_call(&auth, echo(1)).unwrap();
    assert!(matches!(
        bridge.begin_stream(&auth, request(1, "count", Value::Null), 1),
        Err(CoreError::ReusedRequestId)
    ));
    let (producer, receiver) = bridge
        .begin_stream(&auth, request(2, "count", Value::Null), 1)
        .unwrap();
    assert_eq!(
        bridge.begin_call(&auth, echo(3)).unwrap_err(),
        CoreError::QueueFull
    );
    drop(first);
    let third = bridge.begin_call(&auth, echo(3)).unwrap();
    producer.run();
    assert_eq!(
        bridge.begin_call(&auth, echo(4)).unwrap_err(),
        CoreError::QueueFull
    );
    assert_eq!(third.run().result, Ok(echo(3).args));
    assert_eq!(
        receiver.try_next().unwrap(),
        Some(StreamResponseItem::Value {
            sequence: 0,
            value: json!(7)
        })
    );
    assert_eq!(
        receiver.try_next().unwrap(),
        Some(StreamResponseItem::Complete(Ok(Value::Null)))
    );
    assert!(
        bridge
            .begin_call(&auth, echo(4))
            .unwrap()
            .run()
            .result
            .is_ok()
    );
}

#[test]
fn dropping_unrun_jobs_releases_budget_without_reusing_ids() {
    let (bridge, auth, count) = fixture(1);
    for id in 1..=100 {
        let job = bridge.begin_call(&auth, echo(id)).unwrap();
        assert_eq!(
            bridge.begin_call(&auth, echo(id + 1)).unwrap_err(),
            CoreError::QueueFull
        );
        drop(job);
        assert_eq!(
            bridge.begin_call(&auth, echo(id)).unwrap_err(),
            CoreError::ReusedRequestId
        );
        assert_eq!(bridge.cancel(&auth, id), Ok(false));
    }
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert!(
        bridge
            .begin_call(&auth, echo(101))
            .unwrap()
            .run()
            .result
            .is_ok()
    );
}

#[test]
fn cancellation_before_run_suppresses_handler_and_releases_only_on_exit() {
    let (bridge, auth, count) = fixture(1);
    let job = bridge.begin_call(&auth, echo(1)).unwrap();
    assert_eq!(bridge.cancel(&auth, 1), Ok(true));
    assert_eq!(bridge.is_request_cancelled(&auth, 1), Ok(true));
    assert_eq!(
        bridge.begin_call(&auth, echo(2)).unwrap_err(),
        CoreError::QueueFull
    );
    assert_eq!(job.run().result, Err(CoreError::Cancelled));
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert!(
        bridge
            .begin_call(&auth, echo(2))
            .unwrap()
            .run()
            .result
            .is_ok()
    );
}

#[test]
fn precancellation_is_shared_with_admission_and_does_not_execute() {
    let (bridge, auth, count) = fixture(1);
    assert_eq!(bridge.cancel(&auth, 1), Ok(true));
    assert_eq!(
        bridge.begin_call(&auth, echo(1)).unwrap_err(),
        CoreError::Cancelled
    );
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert_eq!(
        bridge.begin_call(&auth, echo(1)).unwrap_err(),
        CoreError::ReusedRequestId
    );
    assert!(
        bridge
            .begin_call(&auth, echo(2))
            .unwrap()
            .run()
            .result
            .is_ok()
    );
}

#[test]
fn cancelled_running_handler_keeps_its_budget_until_it_returns() {
    let (bridge, auth, _) = fixture(1);
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Mutex::new(release_rx);
    bridge
        .register(
            MethodSchema::new("block", Schema::Null, Schema::Null),
            move |context, _: ()| {
                started_tx.send(context.cancellation.clone()).unwrap();
                release_rx.lock().unwrap().recv_timeout(WAIT).unwrap();
                // Deliberately does not cooperate; Core must suppress this success.
                Ok(())
            },
        )
        .unwrap();
    let job = bridge
        .begin_call(&auth, request(1, "block", Value::Null))
        .unwrap();
    let worker = thread::spawn(move || job.run());
    let token = started_rx.recv_timeout(WAIT).unwrap();
    bridge.cancel(&auth, 1).unwrap();
    assert!(token.is_cancelled());
    assert_eq!(
        bridge.begin_call(&auth, echo(2)).unwrap_err(),
        CoreError::QueueFull
    );
    release_tx.send(()).unwrap();
    assert_eq!(worker.join().unwrap().result, Err(CoreError::Cancelled));
    assert!(
        bridge
            .begin_call(&auth, echo(2))
            .unwrap()
            .run()
            .result
            .is_ok()
    );
}

#[test]
fn stale_job_cleanup_never_releases_replacement_sessions_reservation() {
    for navigate_first in [false, true] {
        for run_old in [false, true] {
            let (bridge, old_auth, count) = fixture(1);
            let old = bridge.begin_call(&old_auth, echo(1)).unwrap();
            if navigate_first {
                bridge.navigate(old_auth.window).unwrap();
            }
            let fresh_auth = session(&bridge, old_auth.window.0);
            let fresh = bridge.begin_call(&fresh_auth, echo(1)).unwrap();
            if run_old {
                assert_eq!(old.run().result, Err(CoreError::Cancelled));
            } else {
                drop(old);
            }
            assert_eq!(count.load(Ordering::SeqCst), 0);
            assert_eq!(
                bridge.begin_call(&fresh_auth, echo(2)).unwrap_err(),
                CoreError::QueueFull
            );
            assert_eq!(fresh.run().result, Ok(echo(1).args));
            assert!(
                bridge
                    .begin_call(&fresh_auth, echo(2))
                    .unwrap()
                    .run()
                    .result
                    .is_ok()
            );
            assert_eq!(count.load(Ordering::SeqCst), 2);
        }
    }
}

#[test]
fn window_close_cancels_queued_jobs_and_does_not_cross_windows() {
    let (bridge, first_auth, count) = fixture(1);
    let second_auth = session(&bridge, 2);
    let first = bridge.begin_call(&first_auth, echo(1)).unwrap();
    let second = bridge.begin_call(&second_auth, echo(1)).unwrap();
    bridge.close_window(first_auth.window).unwrap();
    assert_eq!(first.run().result, Err(CoreError::Cancelled));
    assert_eq!(second.run().result, Ok(echo(1).args));
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(
        bridge.begin_call(&first_auth, echo(2)).unwrap_err(),
        CoreError::ClosedWindow
    );
}

#[test]
fn rejected_authority_does_not_consume_id_or_capacity() {
    let (bridge, auth, count) = fixture(1);
    let other = session(&bridge, 2);
    let mut forged = auth.clone();
    forged.top_level = false;
    assert_eq!(
        bridge.begin_call(&forged, echo(1)).unwrap_err(),
        CoreError::Unauthorized
    );
    forged = auth.clone();
    forged.page_url = "lumendesk://app.evil/".into();
    assert_eq!(
        bridge.begin_call(&forged, echo(1)).unwrap_err(),
        CoreError::Unauthorized
    );
    forged = auth.clone();
    forged.session = other.session;
    assert_eq!(
        bridge.begin_call(&forged, echo(1)).unwrap_err(),
        CoreError::StaleSession
    );
    assert!(
        bridge
            .begin_call(&auth, echo(1))
            .unwrap()
            .run()
            .result
            .is_ok()
    );
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn wire_validation_is_bounded_and_admission_errors_do_not_reserve() {
    let (bridge, auth, count) = fixture(1);
    assert_eq!(
        bridge.begin_call_json(&auth, &[b'x'; 257]).unwrap_err(),
        CoreError::PayloadTooLarge
    );
    for wire in [
        b"not-json".as_slice(),
        br#"{"id":1,"method":"echo","args":"x","extra":true}"#.as_slice(),
        br#"{"id":1,"id":1,"method":"echo","args":"x"}"#.as_slice(),
    ] {
        assert!(matches!(
            bridge.begin_call_json(&auth, wire),
            Err(CoreError::InvalidArguments(_))
        ));
    }
    assert!(matches!(
        bridge.begin_call(&auth, echo(9_007_199_254_740_992)),
        Err(CoreError::InvalidArguments(_))
    ));
    assert!(matches!(
        bridge.begin_call(&auth, request(1, "missing", Value::Null)),
        Err(CoreError::UnknownMethod(_))
    ));
    assert_eq!(count.load(Ordering::SeqCst), 0);
    let bytes = serde_json::to_vec(&echo(1)).unwrap();
    assert_eq!(
        bridge.begin_call_json(&auth, &bytes).unwrap().run().result,
        Ok(echo(1).args)
    );
}

#[test]
fn schema_rejection_happens_without_user_code_and_releases_reservation() {
    let (bridge, auth, count) = fixture(1);
    let job = bridge
        .begin_call(&auth, request(1, "echo", json!(12)))
        .unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert!(matches!(
        job.run().result,
        Err(CoreError::InvalidArguments(_))
    ));
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert!(
        bridge
            .begin_call(&auth, echo(2))
            .unwrap()
            .run()
            .result
            .is_ok()
    );
}

#[test]
fn result_schema_and_success_error_payload_bounds_survive_scheduling() {
    let (bridge, auth, _) = fixture(1);
    bridge
        .register(
            MethodSchema::new("bad", Schema::Null, Schema::Integer),
            |_, _: ()| Ok("wrong type"),
        )
        .unwrap();
    bridge
        .register(
            MethodSchema::new("large", Schema::Null, Schema::String),
            |_, _: ()| Ok("x".repeat(300)),
        )
        .unwrap();
    bridge
        .register::<(), (), _>(
            MethodSchema::new("error", Schema::Null, Schema::Null),
            |_, _| Err(CoreError::Backend("界".repeat(100))),
        )
        .unwrap();
    assert!(matches!(
        bridge
            .begin_call(&auth, request(1, "bad", Value::Null))
            .unwrap()
            .run()
            .result,
        Err(CoreError::InvalidResult(_))
    ));
    for (id, method) in [(2, "large"), (3, "error")] {
        assert_eq!(
            bridge
                .begin_call(&auth, request(id, method, Value::Null))
                .unwrap()
                .run()
                .result,
            Err(CoreError::PayloadTooLarge)
        );
    }
    assert!(
        bridge
            .begin_call(&auth, echo(4))
            .unwrap()
            .run()
            .result
            .is_ok()
    );
}

#[test]
fn panic_finishes_call_scoped_streams_and_releases_capacity() {
    let (bridge, auth, _) = fixture(1);
    let (retained_tx, retained_rx) = mpsc::channel();
    bridge
        .register::<(), (), _>(
            MethodSchema::new("panic", Schema::Null, Schema::Null),
            move |context, _| {
                let (sender, receiver) = context.stream::<u8>(1)?;
                sender.send(7)?;
                retained_tx.send((sender, receiver, context)).unwrap();
                panic!("synthetic handler panic");
            },
        )
        .unwrap();
    let job = bridge
        .begin_call(&auth, request(1, "panic", Value::Null))
        .unwrap();
    assert_eq!(job.run().result, Err(CoreError::BackendPanic));
    let (sender, receiver, context) = retained_rx.recv_timeout(WAIT).unwrap();
    assert_eq!(sender.send(8), Err(CoreError::StreamFinished));
    assert!(matches!(
        context.stream::<u8>(1),
        Err(CoreError::StreamFinished)
    ));
    assert_eq!(
        receiver.try_next().unwrap(),
        Some(StreamItem::Value {
            sequence: 0,
            value: 7
        })
    );
    assert_eq!(
        receiver.try_next().unwrap(),
        Some(StreamItem::Complete(Err(CoreError::BackendPanic)))
    );
    assert!(
        bridge
            .begin_call(&auth, echo(2))
            .unwrap()
            .run()
            .result
            .is_ok()
    );
}

#[test]
fn successful_worker_result_still_requires_delivery_time_authorization() {
    let (bridge, auth, _) = fixture(1);
    let job = bridge.begin_call(&auth, echo(1)).unwrap();
    let destination = job.authorization().clone();
    assert_eq!(job.run().result, Ok(echo(1).args));
    bridge.navigate(auth.window).unwrap();
    assert_eq!(bridge.authorize(&destination), Err(CoreError::StaleSession));
}
