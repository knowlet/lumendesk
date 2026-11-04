//! Bounded real-engine acceptance fixture; not a production application API.
use lumendesk::{
    WindowOptions,
    core::{Bridge, BridgeConfig, CoreError, MethodSchema, Schema, StreamMethodSchema},
    webview::{APP_ORIGIN, WebApplication, demo_assets},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const CASES: &[&str] = &[
    "repeated-ipc",
    "event-payload",
    "schema-rejection",
    "stream-order",
    "backpressure",
    "cancel-restart",
    "frame-csp",
    "external-navigation",
    "navigation-cleanup",
];
#[derive(Default)]
struct State {
    passed: BTreeSet<String>,
    failures: Vec<String>,
    produced: BTreeMap<String, u32>,
    active: BTreeSet<String>,
    stopped: BTreeSet<String>,
    phase_request: Option<u64>,
    navigated: bool,
    ready: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StreamArgs {
    name: String,
    count: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Report {
    case: String,
    ok: bool,
    detail: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Echo {
    text: String,
    sequence: u32,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("LUMENDESK_SMOKE_MS").is_some() {
        return Err("acceptance must close through the window manager, not a smoke timer".into());
    }
    let state = Arc::new(Mutex::new(State::default()));
    let bridge = Bridge::new(BridgeConfig::production(APP_ORIGIN)?)?;
    bridge.register::<Echo, Echo, _>(
        MethodSchema::new(
            "echo",
            Schema::object([
                (
                    "text".into(),
                    lumendesk::core::Field::required(Schema::String),
                ),
                (
                    "sequence".into(),
                    lumendesk::core::Field::required(Schema::Integer),
                ),
            ]),
            Schema::Json,
        ),
        |_, value| Ok(value),
    )?;
    let stats = state.clone();
    bridge.register::<Value, Value, _>(
        MethodSchema::new("stats", Schema::Null, Schema::Json),
        move |_, _| {
            let state = stats.lock().unwrap();
            Ok(json!({"produced":state.produced,"active":state.active,"stopped":state.stopped}))
        },
    )?;
    let reports = state.clone();
    bridge.register::<Report, bool, _>(
        MethodSchema::new("report", Schema::Json, Schema::Boolean),
        move |_, report| {
            let mut state = reports.lock().unwrap();
            if !CASES.contains(&report.case.as_str()) || !report.ok {
                state
                    .failures
                    .push(format!("{}: {}", report.case, report.detail));
                eprintln!("LUMENDESK_WEB_FAILURE {}", report.detail);
            } else {
                state.passed.insert(report.case.clone());
            }
            println!("LUMENDESK_WEB_CASE {} {}", report.case, report.ok);
            Ok(report.ok)
        },
    )?;
    let phase = state.clone();
    bridge.register::<String, bool, _>(
        MethodSchema::new("phase", Schema::String, Schema::Boolean),
        move |ctx, value| {
            let mut state = phase.lock().unwrap();
            match value.as_str() {
                "before" => {
                    state.phase_request = Some(ctx.request_id);
                    Ok(true)
                }
                "after" => {
                    state.navigated = state.phase_request.is_some_and(|id| ctx.request_id < id);
                    Ok(state.navigated)
                }
                _ => Err(CoreError::InvalidArguments("unknown phase".into())),
            }
        },
    )?;
    let stream_state = state.clone();
    bridge.register_stream::<StreamArgs, u32, u32, _>(
        StreamMethodSchema::new("numbers", Schema::Json, Schema::Integer, Schema::Integer),
        move |ctx, args, sink| {
            if args.count > 10000
                || !["ordered", "slow", "restart", "navigate", "close"]
                    .contains(&args.name.as_str())
            {
                return Err(CoreError::InvalidArguments("invalid test stream".into()));
            }
            {
                let mut state = stream_state.lock().unwrap();
                if !state.active.insert(args.name.clone()) {
                    return Err(CoreError::InvalidArguments("duplicate test stream".into()));
                }
                state.produced.insert(args.name.clone(), 0);
            }
            let result = (|| {
                for index in 0..args.count {
                    ctx.check_cancelled()?;
                    sink.send_wait(index)?;
                    stream_state
                        .lock()
                        .unwrap()
                        .produced
                        .insert(args.name.clone(), index + 1);
                }
                Ok(args.count)
            })();
            let mut state = stream_state.lock().unwrap();
            state.active.remove(&args.name);
            state.stopped.insert(args.name);
            result
        },
    )?;
    let readiness = state.clone();
    bridge.register::<Value, bool, _>(
        MethodSchema::new("ready", Schema::Null, Schema::Boolean),
        move |_, _| {
            let mut state = readiness.lock().unwrap();
            if state.passed.len() != CASES.len()
                || !state.failures.is_empty()
                || !state.navigated
                || !state.active.contains("close")
            {
                return Err(CoreError::Backend("incomplete acceptance evidence".into()));
            }
            state.ready = true;
            println!("LUMENDESK_WEB_READY_TO_CLOSE");
            std::io::stdout()
                .flush()
                .map_err(|error| CoreError::Backend(error.to_string()))?;
            Ok(true)
        },
    )?;
    let mut app = WebApplication::new(bridge.clone());
    app.register_event("probe", Schema::String)?;
    let events = app.events();
    bridge.register::<String, bool, _>(
        MethodSchema::new("emit-probe", Schema::String, Schema::Boolean),
        move |ctx, value| {
            let events = events.clone();
            std::thread::spawn(move || {
                // Deliver after the RPC returns, while no stream ticker is active.
                std::thread::sleep(Duration::from_millis(100));
                if let Err(error) = events.emit(Some(ctx.window), "probe", value) {
                    eprintln!("acceptance event delivery failed: {error}");
                }
            });
            Ok(true)
        },
    )?;
    app.add_window(
        WindowOptions::new("LumenDesk Web Acceptance"),
        demo_assets(include_str!("web_acceptance.html"))?,
    )?;
    app.run()?;
    // Closing the last window must cancel its actual producer, not merely hide UI.
    let deadline = Instant::now() + Duration::from_secs(3);
    while !state.lock().unwrap().active.is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let state = state.lock().unwrap();
    if !state.ready
        || !state.failures.is_empty()
        || !state.active.is_empty()
        || !state.stopped.contains("close")
    {
        return Err(format!(
            "incomplete acceptance: ready={}, failures={:?}, active={:?}",
            state.ready, state.failures, state.active
        )
        .into());
    }
    println!("LUMENDESK_WEB_ACCEPTANCE_PASS");
    Ok(())
}
