use lumendesk_core::{
    Authorization, Bridge, BridgeConfig, BridgeStreamReceiver, CallRequest, CoreError,
    MethodSchema, Result, Schema, SchemaRegistry, StreamCall, StreamMethodSchema,
    StreamResponseItem, StreamSink, WindowId,
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

fn configured(limit: usize, bytes: usize) -> (Bridge, Authorization) {
    let mut config = BridgeConfig::production("lumendesk://app").unwrap();
    config.max_pending_per_session = limit;
    config.max_payload_bytes = bytes;
    let bridge = Bridge::new(config).unwrap();
    let auth = Authorization {
        window: WindowId(1),
        session: bridge
            .create_session(WindowId(1), "lumendesk://app/")
            .unwrap(),
        page_url: "lumendesk://app/".into(),
        top_level: true,
    };
    (bridge, auth)
}
fn setup() -> (Bridge, Authorization) {
    configured(64, 1024)
}
fn request(id: u64, method: &str, args: Value) -> CallRequest {
    CallRequest {
        id,
        method: method.into(),
        args,
    }
}
fn schema(name: &str) -> StreamMethodSchema {
    StreamMethodSchema::new(name, Schema::Integer, Schema::Integer, Schema::String)
}
fn register_count(bridge: &Bridge) {
    bridge
        .register_stream(schema("count"), |_, count: u32, tx: StreamSink<u32>| {
            for number in 0..count {
                tx.send_wait(number)?;
            }
            Ok("done")
        })
        .unwrap();
}
fn next(receiver: &BridgeStreamReceiver) -> Result<StreamResponseItem> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(item) = receiver.try_next()? {
            return Ok(item);
        }
        assert!(Instant::now() < deadline, "stream failed to make progress");
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn jobs_and_receivers_are_send_and_keep_original_authority() {
    fn assert_send<T: Send>() {}
    assert_send::<StreamCall>();
    assert_send::<BridgeStreamReceiver>();
    let (bridge, auth) = setup();
    register_count(&bridge);
    let (job, receiver) = bridge
        .begin_stream(&auth, request(5, "count", json!(0)), 1)
        .unwrap();
    assert_eq!(job.request_id(), 5);
    assert_eq!(receiver.request_id(), 5);
    assert_eq!(job.authorization().session, auth.session);
    assert_eq!(receiver.authorization().window, auth.window);
    job.run();
    assert_eq!(
        next(&receiver),
        Ok(StreamResponseItem::Complete(Ok(json!("done"))))
    );
    assert!(receiver.is_complete());
    assert_eq!(receiver.try_next(), Ok(None));
}

#[test]
fn slow_pull_consumer_observes_bounded_ordered_values_then_result() {
    let (bridge, auth) = setup();
    register_count(&bridge);
    let (job, receiver) = bridge
        .begin_stream(&auth, request(1, "count", json!(256)), 2)
        .unwrap();
    let worker = thread::spawn(move || job.run());
    for expected in 0..256 {
        assert!(receiver.queued_len() <= 2);
        assert_eq!(
            next(&receiver),
            Ok(StreamResponseItem::Value {
                sequence: expected,
                value: json!(expected),
            })
        );
        if expected % 32 == 0 {
            thread::sleep(Duration::from_millis(2));
        }
    }
    assert_eq!(
        next(&receiver),
        Ok(StreamResponseItem::Complete(Ok(json!("done"))))
    );
    worker.join().unwrap();
    assert_eq!(receiver.try_next(), Ok(None));
}

#[test]
fn nonblocking_backpressure_rejection_does_not_consume_sequence() {
    let (bridge, auth) = setup();
    let (drained_tx, drained_rx) = mpsc::channel();
    let drained_rx = Mutex::new(drained_rx);
    let (full_tx, full_rx) = mpsc::channel();
    bridge
        .register_stream(schema("count"), move |_, _: u32, tx: StreamSink<u32>| {
            assert_eq!(tx.send(10), Ok(0));
            assert_eq!(tx.send(20), Err(CoreError::QueueFull));
            full_tx.send(()).unwrap();
            drained_rx.lock().unwrap().recv().unwrap();
            assert_eq!(tx.send(30), Ok(1));
            Ok("done")
        })
        .unwrap();
    let (job, receiver) = bridge
        .begin_stream(&auth, request(1, "count", json!(1)), 1)
        .unwrap();
    let worker = thread::spawn(move || job.run());
    full_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(receiver.queued_len(), 1);
    assert_eq!(
        next(&receiver),
        Ok(StreamResponseItem::Value {
            sequence: 0,
            value: json!(10)
        })
    );
    drained_tx.send(()).unwrap();
    assert_eq!(
        next(&receiver),
        Ok(StreamResponseItem::Value {
            sequence: 1,
            value: json!(30)
        })
    );
    assert_eq!(
        next(&receiver),
        Ok(StreamResponseItem::Complete(Ok(json!("done"))))
    );
    worker.join().unwrap();
}

#[test]
fn blocked_producer_wakes_when_consumer_drops() {
    let (bridge, auth) = setup();
    let (started_tx, started_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    bridge
        .register_stream(schema("count"), move |ctx, _: u32, tx: StreamSink<u32>| {
            tx.send(1)?;
            started_tx.send(ctx.cancellation.clone()).unwrap();
            let outcome = tx.send_wait(2);
            done_tx.send(outcome.clone()).unwrap();
            outcome?;
            Ok("done")
        })
        .unwrap();
    let (job, receiver) = bridge
        .begin_stream(&auth, request(1, "count", json!(2)), 1)
        .unwrap();
    let worker = thread::spawn(move || job.run());
    let token = started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    drop(receiver);
    assert!(token.is_cancelled());
    assert_eq!(
        done_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        Err(CoreError::Cancelled)
    );
    worker.join().unwrap();
}

#[test]
fn navigation_interrupts_blocked_producer_and_discards_buffered_values() {
    let (bridge, auth) = setup();
    let (started_tx, started_rx) = mpsc::channel();
    bridge
        .register_stream(schema("count"), move |_, _: u32, tx: StreamSink<u32>| {
            tx.send(1)?;
            started_tx.send(()).unwrap();
            tx.send_wait(2)?;
            Ok("done")
        })
        .unwrap();
    let (job, receiver) = bridge
        .begin_stream(&auth, request(1, "count", json!(2)), 1)
        .unwrap();
    let worker = thread::spawn(move || job.run());
    started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    bridge.navigate(auth.window).unwrap();
    assert_eq!(receiver.try_next(), Err(CoreError::Cancelled));
    assert_eq!(receiver.queued_len(), 0);
    worker.join().unwrap();
    assert!(matches!(
        bridge.begin_stream(&auth, request(2, "count", json!(1)), 1),
        Err(CoreError::StaleSession)
    ));
}

#[test]
fn terminal_failure_and_panic_follow_accepted_values() {
    for panic in [false, true] {
        let (bridge, auth) = setup();
        bridge
            .register_stream(
                schema("fail"),
                move |_, _: u32, tx: StreamSink<u32>| -> Result<String> {
                    tx.send(7)?;
                    if panic {
                        panic!("contained stream panic");
                    }
                    Err(CoreError::Backend("expected failure".into()))
                },
            )
            .unwrap();
        let (job, receiver) = bridge
            .begin_stream(&auth, request(1, "fail", json!(0)), 1)
            .unwrap();
        job.run();
        assert_eq!(
            next(&receiver),
            Ok(StreamResponseItem::Value {
                sequence: 0,
                value: json!(7)
            })
        );
        let error = if panic {
            CoreError::BackendPanic
        } else {
            CoreError::Backend("expected failure".into())
        };
        assert_eq!(
            next(&receiver),
            Ok(StreamResponseItem::Complete(Err(error)))
        );
        assert_eq!(receiver.try_next(), Ok(None));
    }
}

#[test]
fn retained_producer_and_context_are_sealed_on_handler_return() {
    let (bridge, auth) = setup();
    let (tx, rx) = mpsc::channel();
    bridge
        .register_stream(
            schema("count"),
            move |ctx, _: u32, sender: StreamSink<u32>| {
                sender.send(1)?;
                tx.send((ctx, sender)).unwrap();
                Ok("done")
            },
        )
        .unwrap();
    let (job, receiver) = bridge
        .begin_stream(&auth, request(1, "count", json!(1)), 1)
        .unwrap();
    job.run();
    let (context, sender) = rx.recv().unwrap();
    assert_eq!(sender.send(2), Err(CoreError::StreamFinished));
    assert_eq!(sender.send_wait(2), Err(CoreError::StreamFinished));
    assert!(matches!(
        context.stream::<u32>(1),
        Err(CoreError::StreamFinished)
    ));
    next(&receiver).unwrap();
    next(&receiver).unwrap();
    drop(receiver);
    assert!(!context.cancellation.is_cancelled());
}

#[test]
fn finished_but_undrained_streams_keep_pending_budget_and_remain_cancellable() {
    let (bridge, auth) = configured(1, 1024);
    register_count(&bridge);
    let (job, receiver) = bridge
        .begin_stream(&auth, request(1, "count", json!(1)), 1)
        .unwrap();
    job.run();
    assert!(matches!(
        bridge.begin_stream(&auth, request(2, "count", json!(0)), 1),
        Err(CoreError::QueueFull)
    ));
    assert_eq!(bridge.cancel(&auth, 1), Ok(true));
    assert_eq!(receiver.try_next(), Err(CoreError::Cancelled));
    let (job, receiver) = bridge
        .begin_stream(&auth, request(2, "count", json!(0)), 1)
        .unwrap();
    job.run();
    next(&receiver).unwrap();
    assert_eq!(bridge.cancel(&auth, 2), Ok(false));
}

#[test]
fn completion_drain_releases_budget_and_navigation_cancels_finished_buffer() {
    let (bridge, auth) = configured(1, 1024);
    register_count(&bridge);
    let (job, receiver) = bridge
        .begin_stream(&auth, request(1, "count", json!(0)), 1)
        .unwrap();
    job.run();
    next(&receiver).unwrap();
    let (job, buffered) = bridge
        .begin_stream(&auth, request(2, "count", json!(1)), 1)
        .unwrap();
    job.run();
    bridge.navigate(auth.window).unwrap();
    assert_eq!(buffered.try_next(), Err(CoreError::Cancelled));
}

#[test]
fn dropped_unrun_job_reports_failure_and_consumer_dropout_prevents_handler() {
    let (bridge, auth) = setup();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    bridge
        .register_stream(schema("count"), move |_, _: u32, _: StreamSink<u32>| {
            observed.fetch_add(1, Ordering::SeqCst);
            Ok("done")
        })
        .unwrap();
    let (job, receiver) = bridge
        .begin_stream(&auth, request(1, "count", json!(0)), 1)
        .unwrap();
    drop(job);
    assert!(matches!(
        next(&receiver),
        Ok(StreamResponseItem::Complete(Err(CoreError::Backend(_))))
    ));
    let (job, receiver) = bridge
        .begin_stream(&auth, request(2, "count", json!(0)), 1)
        .unwrap();
    drop(receiver);
    job.run();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn normal_calls_and_streams_share_replay_namespace_and_precancellation() {
    let (bridge, auth) = setup();
    register_count(&bridge);
    bridge
        .register(
            MethodSchema::new("call", Schema::Null, Schema::Null),
            |_, _: ()| Ok(()),
        )
        .unwrap();
    assert!(
        bridge
            .dispatch(&auth, request(1, "call", Value::Null))
            .result
            .is_ok()
    );
    assert!(matches!(
        bridge.begin_stream(&auth, request(1, "count", json!(0)), 1),
        Err(CoreError::ReusedRequestId)
    ));
    let (job, receiver) = bridge
        .begin_stream(&auth, request(2, "count", json!(0)), 1)
        .unwrap();
    assert_eq!(
        bridge
            .dispatch(&auth, request(2, "call", Value::Null))
            .result,
        Err(CoreError::ReusedRequestId)
    );
    job.run();
    next(&receiver).unwrap();
    assert_eq!(bridge.cancel(&auth, 3), Ok(true));
    assert!(matches!(
        bridge.begin_stream(&auth, request(3, "count", json!(0)), 1),
        Err(CoreError::Cancelled)
    ));
    assert!(matches!(
        bridge.begin_stream(&auth, request(3, "count", json!(0)), 1),
        Err(CoreError::ReusedRequestId)
    ));
}

#[test]
fn invalid_authority_and_wrong_method_kind_never_run_producers() {
    let (bridge, auth) = setup();
    register_count(&bridge);
    bridge
        .register(
            MethodSchema::new("call", Schema::Null, Schema::Null),
            |_, _: ()| Ok(()),
        )
        .unwrap();
    for method in ["unknown", "call"] {
        assert!(matches!(
            bridge.begin_stream(&auth, request(1, method, Value::Null), 1),
            Err(CoreError::UnknownMethod(_))
        ));
    }
    assert!(matches!(
        bridge.dispatch(&auth, request(1, "count", json!(0))).result,
        Err(CoreError::UnknownMethod(_))
    ));
    for url in ["https://app/", "lumendesk://app.evil/"] {
        let mut invalid = auth.clone();
        invalid.page_url = url.into();
        assert!(matches!(
            bridge.begin_stream(&invalid, request(1, "count", json!(0)), 1),
            Err(CoreError::Unauthorized)
        ));
    }
    let mut frame = auth.clone();
    frame.top_level = false;
    assert!(matches!(
        bridge.begin_stream(&frame, request(1, "count", json!(0)), 1),
        Err(CoreError::Unauthorized)
    ));
    let session2 = bridge
        .create_session(WindowId(2), "lumendesk://app/")
        .unwrap();
    let mut foreign = auth.clone();
    foreign.session = session2;
    assert!(matches!(
        bridge.begin_stream(&foreign, request(1, "count", json!(0)), 1),
        Err(CoreError::StaleSession)
    ));
    bridge.close_window(auth.window).unwrap();
    assert!(matches!(
        bridge.begin_stream(&auth, request(1, "count", json!(0)), 1),
        Err(CoreError::ClosedWindow)
    ));
}

#[test]
fn schema_validation_covers_arguments_items_and_terminal_result() {
    let (bridge, auth) = setup();
    register_count(&bridge);
    let (job, receiver) = bridge
        .begin_stream(&auth, request(1, "count", json!("wrong")), 1)
        .unwrap();
    job.run();
    assert!(matches!(
        next(&receiver),
        Ok(StreamResponseItem::Complete(Err(
            CoreError::InvalidArguments(_)
        )))
    ));
    bridge
        .register_stream(schema("bad-item"), |_, _: u32, tx: StreamSink<&str>| {
            tx.send("wrong")?;
            Ok("done")
        })
        .unwrap();
    let (job, receiver) = bridge
        .begin_stream(&auth, request(2, "bad-item", json!(0)), 1)
        .unwrap();
    job.run();
    assert!(matches!(
        next(&receiver),
        Ok(StreamResponseItem::Complete(Err(CoreError::InvalidResult(
            _
        ))))
    ));
    bridge
        .register_stream(schema("bad-result"), |_, _: u32, tx: StreamSink<u32>| {
            tx.send(1)?;
            Ok(false)
        })
        .unwrap();
    let (job, receiver) = bridge
        .begin_stream(&auth, request(3, "bad-result", json!(0)), 1)
        .unwrap();
    job.run();
    next(&receiver).unwrap();
    assert!(matches!(
        next(&receiver),
        Ok(StreamResponseItem::Complete(Err(CoreError::InvalidResult(
            _
        ))))
    ));
}

#[test]
fn request_item_and_result_size_limits_are_independent() {
    let (bridge, auth) = configured(64, 128);
    bridge
        .register_stream(
            StreamMethodSchema::new("large-item", Schema::Null, Schema::String, Schema::Null),
            |_, _: (), tx: StreamSink<String>| {
                tx.send("x".repeat(129))?;
                Ok(())
            },
        )
        .unwrap();
    bridge
        .register_stream(
            StreamMethodSchema::new("large-result", Schema::Null, Schema::Null, Schema::String),
            |_, _: (), _: StreamSink<()>| Ok("x".repeat(129)),
        )
        .unwrap();
    assert!(matches!(
        bridge.begin_stream_json(&auth, &[b'x'; 129], 1),
        Err(CoreError::PayloadTooLarge)
    ));
    assert!(matches!(
        bridge.begin_stream_json(
            &auth,
            br#"{"id":1,"method":"large-item","args":null,"extra":1}"#,
            1
        ),
        Err(CoreError::InvalidArguments(_))
    ));
    assert!(matches!(
        bridge.begin_stream(&auth, request(1, "large-item", json!("x".repeat(129))), 1),
        Err(CoreError::PayloadTooLarge)
    ));
    assert!(matches!(
        bridge.begin_stream(
            &auth,
            request(9_007_199_254_740_992, "large-item", Value::Null),
            1
        ),
        Err(CoreError::InvalidArguments(_))
    ));
    for (id, method) in [(1, "large-item"), (2, "large-result")] {
        let (job, receiver) = bridge
            .begin_stream(&auth, request(id, method, Value::Null), 1)
            .unwrap();
        job.run();
        assert_eq!(
            next(&receiver),
            Ok(StreamResponseItem::Complete(Err(
                CoreError::PayloadTooLarge
            )))
        );
    }
}

#[test]
fn registration_collisions_are_atomic_in_both_directions() {
    let (bridge, _) = setup();
    register_count(&bridge);
    assert!(matches!(
        bridge.register(
            MethodSchema::new("count", Schema::Null, Schema::Null),
            |_, _: ()| Ok(())
        ),
        Err(CoreError::DuplicateRegistration(_))
    ));
    bridge
        .register(
            MethodSchema::new("call", Schema::Null, Schema::Null),
            |_, _: ()| Ok(()),
        )
        .unwrap();
    assert!(matches!(
        bridge.register_stream(schema("call"), |_, _: u32, _: StreamSink<u32>| Ok("done")),
        Err(CoreError::DuplicateRegistration(_))
    ));
    assert_eq!(bridge.schemas().streams().len(), 1);
    assert_eq!(bridge.schemas().methods().len(), 1);
}

#[test]
fn legacy_schemas_parse_and_stream_generation_is_deterministic() {
    let old: SchemaRegistry = serde_json::from_str(r#"{"methods":{},"events":{}}"#).unwrap();
    assert!(old.streams().is_empty());
    let mut a = old.clone();
    let mut b = old;
    for name in ["z", "a\"b"] {
        a.register_stream(schema(name)).unwrap();
    }
    for name in ["a\"b", "z"] {
        b.register_stream(schema(name)).unwrap();
    }
    assert_eq!(a.typescript(), b.typescript());
    assert!(
        a.typescript()
            .contains("\"a\\\"b\": { args: number; item: number; result: string }")
    );
    assert!(a.typescript().contains("readonly result: Promise<R>"));
    let restored: SchemaRegistry =
        serde_json::from_value(serde_json::to_value(&a).unwrap()).unwrap();
    assert_eq!(a.typescript(), restored.typescript());
}

#[test]
fn zero_capacity_does_not_consume_id_and_empty_stream_completes() {
    let (bridge, auth) = setup();
    register_count(&bridge);
    assert!(matches!(
        bridge.begin_stream(&auth, request(1, "count", json!(0)), 0),
        Err(CoreError::InvalidConfiguration(_))
    ));
    let bytes = serde_json::to_vec(&request(1, "count", json!(0))).unwrap();
    let (job, receiver) = bridge.begin_stream_json(&auth, &bytes, 1).unwrap();
    job.run();
    assert_eq!(
        next(&receiver),
        Ok(StreamResponseItem::Complete(Ok(json!("done"))))
    );
}

#[test]
fn closing_a_window_cancels_its_stream_without_touching_another() {
    let (bridge, auth) = setup();
    register_count(&bridge);
    let other = Authorization {
        window: WindowId(2),
        session: bridge
            .create_session(WindowId(2), "lumendesk://app/")
            .unwrap(),
        page_url: "lumendesk://app/".into(),
        top_level: true,
    };
    let (job1, receiver1) = bridge
        .begin_stream(&auth, request(1, "count", json!(1)), 1)
        .unwrap();
    let (job2, receiver2) = bridge
        .begin_stream(&other, request(1, "count", json!(1)), 1)
        .unwrap();
    job1.run();
    job2.run();
    bridge.close_window(auth.window).unwrap();
    assert_eq!(receiver1.try_next(), Err(CoreError::Cancelled));
    assert!(matches!(
        next(&receiver2),
        Ok(StreamResponseItem::Value { .. })
    ));
    assert_eq!(
        next(&receiver2),
        Ok(StreamResponseItem::Complete(Ok(json!("done"))))
    );
}

#[test]
fn dropping_an_old_receiver_cannot_release_a_new_session_request() {
    let (bridge, mut auth) = configured(1, 1024);
    register_count(&bridge);
    let (old_job, old_receiver) = bridge
        .begin_stream(&auth, request(1, "count", json!(0)), 1)
        .unwrap();
    auth.session = bridge
        .create_session(auth.window, "lumendesk://app/")
        .unwrap();
    let (new_job, new_receiver) = bridge
        .begin_stream(&auth, request(1, "count", json!(0)), 1)
        .unwrap();
    drop(old_receiver);
    old_job.run();
    assert!(matches!(
        bridge.begin_stream(&auth, request(2, "count", json!(0)), 1),
        Err(CoreError::QueueFull)
    ));
    new_job.run();
    assert_eq!(
        next(&new_receiver),
        Ok(StreamResponseItem::Complete(Ok(json!("done"))))
    );
}
