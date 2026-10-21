//! System-webview host with embedded assets and an origin-bound, session-scoped bridge.
use crate::core::assets::AssetStore;
use crate::core::{
    Authorization, Bridge, BridgeConfig, BridgeStreamReceiver, CallRequest, CallResponse,
    CoreError, Field, MethodSchema, Schema, SessionToken, TrustedOrigin, WindowId,
};
use crate::web_streams::{MAX_STREAMS, StreamBook, WorkBudget, WorkPermit};
use crate::{DesktopError, FormState, WindowOptions};
use serde::Deserialize;
use serde_json::Value;
use std::{
    borrow::Cow,
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc, Mutex,
        mpsc::{self},
    },
    time::{Duration, Instant},
};
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    platform::run_return::EventLoopExtRunReturn,
    window::WindowBuilder,
};
use wry::{WebView, WebViewBuilder};

pub const APP_ORIGIN: &str = if cfg!(target_os = "windows") {
    "http://lumendesk.app"
} else {
    "lumendesk://app"
};
const APP_URL: &str = "lumendesk://app/index.html";
const MAX_WIRE_BYTES: usize = 1024 * 1024;
const QUEUE_SIZE: usize = 32;
const CSP: &str = "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'none'; frame-src 'none'; child-src 'none'; worker-src 'none'; frame-ancestors 'none'; object-src 'none'; base-uri 'none'; form-action 'none'";

#[derive(Clone, Debug)]
struct HostEvent {
    name: String,
    payload: Value,
}
#[derive(Default)]
struct EventState {
    queues: BTreeMap<WindowId, VecDeque<HostEvent>>,
    schemas: BTreeMap<String, Schema>,
}
/// Schema-checked, bounded Rust-to-web delivery. Broadcast admission is atomic.
#[derive(Clone)]
pub struct EventSender {
    state: Arc<Mutex<EventState>>,
}
impl EventSender {
    pub fn emit(
        &self,
        target: Option<WindowId>,
        name: impl Into<String>,
        payload: impl serde::Serialize,
    ) -> Result<(), CoreError> {
        let name = name.into();
        let payload =
            serde_json::to_value(payload).map_err(|e| CoreError::InvalidResult(e.to_string()))?;
        if serde_json::to_vec(&payload)
            .map_err(|e| CoreError::InvalidResult(e.to_string()))?
            .len()
            > MAX_WIRE_BYTES
        {
            return Err(CoreError::PayloadTooLarge);
        }
        let mut state = self.state.lock().unwrap();
        state
            .schemas
            .get(&name)
            .ok_or_else(|| CoreError::InvalidConfiguration(format!("unregistered event: {name}")))?
            .validate(&payload)
            .map_err(CoreError::InvalidArguments)?;
        let targets = if let Some(id) = target {
            if !state.queues.contains_key(&id) {
                return Err(CoreError::ClosedWindow);
            }
            vec![id]
        } else {
            state.queues.keys().copied().collect()
        };
        if targets.iter().any(|id| state.queues[id].len() >= 64) {
            return Err(CoreError::QueueFull);
        }
        for id in targets {
            state.queues.get_mut(&id).unwrap().push_back(HostEvent {
                name: name.clone(),
                payload: payload.clone(),
            });
        }
        Ok(())
    }
}
struct WindowDefinition {
    id: WindowId,
    options: WindowOptions,
    assets: AssetStore,
}
/// Multiple system-webview windows share a bridge but have isolated page sessions.
pub struct WebApplication {
    bridge: Bridge,
    windows: Vec<WindowDefinition>,
    sender: EventSender,
    close_handler: Option<Box<dyn FnMut(WindowId) -> bool>>,
}
impl WebApplication {
    pub fn new(bridge: Bridge) -> Self {
        Self {
            bridge,
            windows: vec![],
            close_handler: None,
            sender: EventSender {
                state: Arc::new(Mutex::new(EventState::default())),
            },
        }
    }
    pub fn add_window(
        &mut self,
        options: WindowOptions,
        assets: AssetStore,
    ) -> Result<WindowId, DesktopError> {
        options.validate()?;
        if self.windows.len() >= 16 {
            return Err(DesktopError::Backend(
                "at most 16 web windows are supported".into(),
            ));
        }
        let id = WindowId(self.windows.len() as u64 + 1);
        self.sender
            .state
            .lock()
            .unwrap()
            .queues
            .insert(id, VecDeque::new());
        self.windows.push(WindowDefinition {
            id,
            options,
            assets,
        });
        Ok(id)
    }
    pub fn register_event(
        &mut self,
        name: impl Into<String>,
        schema: Schema,
    ) -> Result<(), CoreError> {
        let name = name.into();
        if name.is_empty() || name.len() > 128 {
            return Err(CoreError::InvalidConfiguration(
                "event name must be 1–128 bytes".into(),
            ));
        }
        let mut state = self.sender.state.lock().unwrap();
        if state.schemas.contains_key(&name) {
            return Err(CoreError::DuplicateRegistration(name));
        }
        state.schemas.insert(name, schema);
        Ok(())
    }
    /// Return false from the callback to cancel a close request.
    pub fn on_close(&mut self, callback: impl FnMut(WindowId) -> bool + 'static) {
        self.close_handler = Some(Box::new(callback));
    }
    pub fn events(&self) -> EventSender {
        self.sender.clone()
    }
    pub fn run(self) -> Result<(), DesktopError> {
        run_application(self)
    }
}
enum UiMessage {
    Started(WindowId),
    Ready(WindowId, String),
    Response(WindowId, SessionToken, CallResponse),
    StreamStarted(Authorization, u64, Result<BridgeStreamReceiver, CoreError>),
}
struct OpenWindow {
    window: tao::window::Window,
    view: WebView,
    session: Option<SessionToken>,
    close_attempts: usize,
    commands: mpsc::Receiver<(Authorization, Command)>,
    jobs: mpsc::SyncSender<Job>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireMessage {
    session: SessionToken,
    request: Option<CallRequest>,
    cancel: Option<u64>,
    stream: Option<CallRequest>,
    pull: Option<u64>,
}
enum Command {
    Call(CallRequest),
    Stream(CallRequest),
    Pull(u64),
    Cancel(u64),
}
enum Job {
    Call(Authorization, CallRequest, WorkPermit),
    Stream(Authorization, CallRequest, WorkPermit),
}
impl WireMessage {
    fn command(self) -> Option<(SessionToken, Command)> {
        let command = match (self.request, self.stream, self.pull, self.cancel) {
            (Some(request), None, None, None) => Command::Call(request),
            (None, Some(request), None, None) => Command::Stream(request),
            (None, None, Some(id), None) => Command::Pull(id),
            (None, None, None, Some(id)) => Command::Cancel(id),
            _ => return None,
        };
        Some((self.session, command))
    }
}
fn backend(error: impl std::fmt::Display) -> DesktopError {
    DesktopError::Backend(error.to_string())
}

fn run_application(mut app: WebApplication) -> Result<(), DesktopError> {
    if app.windows.is_empty() {
        return Err(DesktopError::Backend(
            "at least one web window is required".into(),
        ));
    }
    let mut event_loop = EventLoopBuilder::<UiMessage>::with_user_event().build();
    let mut open = BTreeMap::new();
    let call_budget = WorkBudget::default();
    let stream_budget = WorkBudget::default();
    let mut streams = StreamBook::default();
    let expected = TrustedOrigin::parse(APP_ORIGIN).map_err(backend)?;
    for definition in app.windows {
        let id = definition.id;
        let window = WindowBuilder::new()
            .with_title(&definition.options.title)
            .with_inner_size(LogicalSize::new(
                definition.options.width,
                definition.options.height,
            ))
            .with_min_inner_size(LogicalSize::new(160.0, 120.0))
            .with_resizable(definition.options.resizable)
            .with_decorations(definition.options.decorated)
            .build(&event_loop)
            .map_err(backend)?;
        let assets = definition.assets;
        let navigation_bridge = app.bridge.clone();
        let navigation_proxy = event_loop.create_proxy();
        let trusted = expected.clone();
        let (jobs_tx, jobs_rx) = mpsc::sync_channel::<Job>(QUEUE_SIZE);
        let (commands_tx, commands_rx) = mpsc::sync_channel(64);
        let worker_bridge = app.bridge.clone();
        let worker_proxy = event_loop.create_proxy();
        std::thread::spawn(move || {
            while let Ok(job) = jobs_rx.recv() {
                match job {
                    Job::Call(auth, request, _permit) => {
                        let response = worker_bridge.dispatch(&auth, request);
                        let _ = worker_proxy.send_event(UiMessage::Response(
                            id,
                            auth.session,
                            response,
                        ));
                    }
                    Job::Stream(auth, request, permit) => {
                        let request_id = request.id;
                        match worker_bridge.begin_stream(&auth, request, 16) {
                            Ok((call, receiver)) => {
                                let _ = worker_proxy.send_event(UiMessage::StreamStarted(
                                    auth,
                                    request_id,
                                    Ok(receiver),
                                ));
                                std::thread::spawn(move || {
                                    let _permit = permit;
                                    call.run();
                                });
                            }
                            Err(error) => {
                                let _ = worker_proxy.send_event(UiMessage::StreamStarted(
                                    auth,
                                    request_id,
                                    Err(error),
                                ));
                            }
                        }
                    }
                }
            }
        });
        let ipc_bridge = app.bridge.clone();
        let page_proxy = event_loop.create_proxy();
        let builder = WebViewBuilder::new()
            .with_url(APP_URL)
            .with_devtools(false)
            .with_incognito(true)
            .with_custom_protocol("lumendesk".into(), move |_, request| {
                let response = wry::http::Response::builder()
                    .header("Content-Security-Policy", CSP)
                    .header("X-Content-Type-Options", "nosniff")
                    .header("Cache-Control", "no-store");
                if request.method() != wry::http::Method::GET {
                    return response
                        .status(405)
                        .body(Cow::Borrowed(&b"Method not allowed"[..]))
                        .unwrap();
                }
                match assets.resolve(request.uri().path()) {
                    Ok(asset) => response
                        .header("Content-Type", asset.asset.mime())
                        .body(Cow::Owned(asset.asset.bytes().to_vec()))
                        .unwrap(),
                    Err(_) => response
                        .status(404)
                        .header("Content-Type", "text/plain")
                        .body(Cow::Borrowed(&b"Asset not found"[..]))
                        .unwrap(),
                }
            })
            .with_navigation_handler(move |url| {
                let allowed =
                    TrustedOrigin::parse(&origin_only(&url)).is_ok_and(|origin| origin == trusted);
                if allowed {
                    let _ = navigation_bridge.navigate(id);
                    let _ = navigation_proxy.send_event(UiMessage::Started(id));
                }
                allowed
            })
            .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
            .with_permission_handler(|_| wry::PermissionResponse::Deny)
            .with_download_started_handler(|_, _| false)
            .with_on_page_load_handler(move |event, url| {
                let message = match event {
                    wry::PageLoadEvent::Started => UiMessage::Started(id),
                    wry::PageLoadEvent::Finished => UiMessage::Ready(id, url),
                };
                let _ = page_proxy.send_event(message);
            })
            .with_ipc_handler(move |request| {
                if request.body().len() > MAX_WIRE_BYTES {
                    return;
                }
                let Ok(message) = serde_json::from_str::<WireMessage>(request.body()) else {
                    return;
                };
                let Some((session, command)) = message.command() else {
                    return;
                };
                // Only a private top-level session is authorized; IPC URL alone cannot
                // attest iframe identity on Linux. CSP denies all embedded frames.
                let auth = Authorization {
                    window: id,
                    session,
                    page_url: request.uri().to_string(),
                    top_level: true,
                };
                if ipc_bridge.authorize(&auth).is_err() {
                    return;
                }
                if let Command::Cancel(request_id) = &command {
                    // Cancellation must not wait behind a saturated command queue.
                    // The UI book also cleans up below or on its next cancellation poll.
                    let _ = ipc_bridge.cancel(&auth, *request_id);
                }
                if commands_tx.try_send((auth, command)).is_err() {
                    // The trusted runtime has a smaller pending limit. Raw IPC flooding
                    // is dropped instead of allocating unbounded event-loop messages.
                    eprintln!("lumendesk: incoming IPC queue full; request will time out");
                }
            });
        #[cfg(target_os = "linux")]
        let view = {
            use tao::platform::unix::WindowExtUnix;
            use wry::WebViewBuilderExtUnix;
            builder.build_gtk(window.gtk_window()).map_err(backend)?
        };
        #[cfg(not(target_os = "linux"))]
        let view = builder.build(&window).map_err(backend)?;
        open.insert(
            id,
            OpenWindow {
                window,
                view,
                session: None,
                close_attempts: 0,
                commands: commands_rx,
                jobs: jobs_tx,
            },
        );
    }
    let started = Instant::now();
    let smoke = std::env::var("LUMENDESK_SMOKE_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_millis);
    let cancel_first = std::env::var_os("LUMENDESK_CANCEL_FIRST_CLOSE").is_some();
    event_loop.run_return(move |event, _, flow| {
        *flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(30));
        match event {
            Event::WindowEvent { window_id, event: WindowEvent::CloseRequested, .. } => {
                let id = open.iter().find(|(_, window)| window.window.id() == window_id).map(|(id, _)| *id);
                if let Some(id) = id {
                    let window = open.get_mut(&id).unwrap();
                    window.close_attempts += 1;
                    let cancelled = (cancel_first && window.close_attempts == 1)
                        || app.close_handler.as_mut().is_some_and(|callback| !callback(id));
                    if cancelled {
                        eprintln!("lumendesk: close request cancelled");
                    } else {
                        let _ = app.bridge.close_window(id);
                        app.sender.state.lock().unwrap().queues.remove(&id);
                        streams.clear(id);
                        open.remove(&id);
                    }
                }
            }
            Event::UserEvent(UiMessage::Started(id)) => {
                let _ = app.bridge.navigate(id);
                streams.clear(id);
                if let Some(window) = open.get_mut(&id) {
                    window.session = None;
                }
                if let Some(queue) = app.sender.state.lock().unwrap().queues.get_mut(&id) {
                    queue.clear();
                }
            }
            Event::UserEvent(UiMessage::Ready(id, url)) => {
                if let Some(window) = open.get_mut(&id) {
                    match app.bridge.create_session(id, &url) {
                        Ok(token) => {
                            let runtime = serde_json::json!({"platform":std::env::consts::OS,"windowId":id.0});
                            let script = include_str!("runtime.js")
                                .replace("__SESSION__", &serde_json::to_string(&token).unwrap())
                                .replace("__RUNTIME__", &runtime.to_string());
                            if window.view.evaluate_script(&script).is_ok() {
                                window.session = Some(token);
                            }
                        }
                        Err(error) => eprintln!("lumendesk: page session denied: {error}"),
                    }
                }
            }
            Event::UserEvent(UiMessage::StreamStarted(auth,id,result)) => { streams.attach(&auth,id,result); }
            Event::UserEvent(UiMessage::Response(id, token, response)) => {
                if let Some(window) = open.get(&id)
                    && window.session.as_ref() == Some(&token)
                {
                    let script = response_script(&token, &response);
                    let _ = window.view.evaluate_script(&script);
                }
            }
            Event::MainEventsCleared => {
                route_commands(&mut open,&app.bridge,&mut streams,&call_budget,&stream_budget);
                for (id,token,frame) in streams.poll(&app.bridge,Instant::now()) {
                    if let Some(window)=open.get(&id) && window.session.as_ref()==Some(&token) {send_stream_frame(&window.view,&token,&frame);}
                }
                for (id, window) in &open {
                    if let Some(token) = &window.session {
                        let events = app.sender.state.lock().unwrap().queues.get_mut(id)
                            .map(std::mem::take).unwrap_or_default();
                        for event in events {
                            deliver_event(&window.view, token, &event);
                        }
                    }
                }
                if smoke.is_some_and(|duration| started.elapsed() >= duration) {
                    for id in open.keys() {
                        let _ = app.bridge.close_window(*id);
                        streams.clear(*id);
                    }
                    open.clear();
                    app.sender.state.lock().unwrap().queues.clear();
                    eprintln!("lumendesk: web smoke complete");
                }
            }
            _ => {}
        }
        if open.is_empty() {
            *flow = ControlFlow::Exit;
        }
    });
    Ok(())
}
// Bind at JavaScript execution time as well as host enqueue time. Engine script
// evaluation is asynchronous; request IDs restart in a replacement page.
fn json_script_argument(value: &impl serde::Serialize) -> String {
    // JSON object keys such as __proto__ are data. A JavaScript object literal
    // interprets that key as a prototype setter, so parse an escaped JSON string.
    let json = serde_json::to_string(value).unwrap();
    format!("JSON.parse({})", serde_json::to_string(&json).unwrap())
}
fn response_script(token: &SessionToken, response: &CallResponse) -> String {
    format!(
        "window.__lumenResponse && window.__lumenResponse({},{});",
        serde_json::to_string(token).unwrap(),
        json_script_argument(response)
    )
}
fn stream_script(token: &SessionToken, frame: &Value) -> String {
    format!(
        "window.__lumenStream && window.__lumenStream({},{});",
        serde_json::to_string(token).unwrap(),
        json_script_argument(frame)
    )
}
fn send_stream_frame(view: &WebView, token: &SessionToken, frame: &Value) {
    let _ = view.evaluate_script(&stream_script(token, frame));
}
fn stream_error(view: &WebView, token: &SessionToken, id: u64, error: CoreError) {
    send_stream_frame(
        view,
        token,
        &serde_json::json!({"id":id,"done":true,"result":Result::<Value,CoreError>::Err(error)}),
    );
}
fn route_commands(
    open: &mut BTreeMap<WindowId, OpenWindow>,
    bridge: &Bridge,
    streams: &mut StreamBook,
    call_budget: &WorkBudget,
    stream_budget: &WorkBudget,
) {
    for window in open.values_mut() {
        // Limit work per event-loop pass, even when a trusted page misbehaves.
        for _ in 0..64 {
            let Ok((auth, command)) = window.commands.try_recv() else {
                break;
            };
            if window.session.as_ref() != Some(&auth.session) || bridge.authorize(&auth).is_err() {
                continue;
            }
            match command {
                Command::Call(request) => {
                    let id = request.id;
                    let token = auth.session.clone();
                    let result = call_budget.acquire(128).and_then(|permit| {
                        window
                            .jobs
                            .try_send(Job::Call(auth, request, permit))
                            .map_err(|_| CoreError::QueueFull)
                    });
                    if let Err(error) = result {
                        let response = CallResponse {
                            id,
                            result: Err(error),
                        };
                        let _ = window
                            .view
                            .evaluate_script(&response_script(&token, &response));
                    }
                }
                Command::Stream(request) => {
                    let id = request.id;
                    match streams.reserve(auth.clone(), id) {
                        Err(error) => stream_error(&window.view, &auth.session, id, error),
                        Ok(()) => {
                            let result = stream_budget.acquire(MAX_STREAMS).and_then(|permit| {
                                window
                                    .jobs
                                    .try_send(Job::Stream(auth.clone(), request, permit))
                                    .map_err(|_| CoreError::QueueFull)
                            });
                            if let Err(error) = result {
                                streams.remove(auth.window, id);
                                stream_error(&window.view, &auth.session, id, error);
                            }
                        }
                    }
                }
                Command::Pull(id) => {
                    if let Err(error) = streams.credit(&auth, id) {
                        let _ = bridge.cancel(&auth, id);
                        streams.remove(auth.window, id);
                        stream_error(&window.view, &auth.session, id, error);
                    }
                }
                Command::Cancel(id) => {
                    let _ = bridge.cancel(&auth, id);
                    streams.remove(auth.window, id);
                }
            }
        }
    }
}

fn origin_only(url: &str) -> String {
    // Parse via the same URL implementation as the core; do not use prefix trust.
    url::Url::parse(url)
        .ok()
        .filter(|url| url.username().is_empty() && url.password().is_none())
        .map(|url| {
            let port = url.port().map(|p| format!(":{p}")).unwrap_or_default();
            format!(
                "{}://{}{}",
                url.scheme(),
                url.host_str().unwrap_or_default(),
                port
            )
        })
        .unwrap_or_default()
}
fn event_script(token: &SessionToken, event: &HostEvent) -> String {
    format!(
        "window.__lumenEvent && window.__lumenEvent({},{},{});",
        serde_json::to_string(token).unwrap(),
        serde_json::to_string(&event.name).unwrap(),
        json_script_argument(&event.payload)
    )
}
fn deliver_event(view: &WebView, token: &SessionToken, event: &HostEvent) {
    let _ = view.evaluate_script(&event_script(token, event));
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GreetingArgs {
    name: String,
    enthusiastic: bool,
}
pub fn demo_assets(html: &'static str) -> Result<AssetStore, DesktopError> {
    let mut assets = AssetStore::new("index.html").map_err(backend)?;
    for (path, body, mime) in [
        ("index.html", html, "text/html; charset=utf-8"),
        (
            "style.css",
            include_str!("../../../assets/style.css"),
            "text/css",
        ),
        (
            "app.js",
            include_str!("../../../assets/app.js"),
            "text/javascript",
        ),
        (
            "spark.svg",
            include_str!("../../../assets/spark.svg"),
            "image/svg+xml",
        ),
    ] {
        assets
            .insert(path, body.as_bytes().to_vec(), mime)
            .map_err(backend)?;
    }
    Ok(assets)
}
/// Launch an embedded page with the example `greet` service. Custom apps should
/// construct `WebApplication` with their own bridge and assets instead.
pub fn run_html(options: WindowOptions, html: &'static str) -> Result<(), DesktopError> {
    let bridge =
        Bridge::new(BridgeConfig::production(APP_ORIGIN).map_err(backend)?).map_err(backend)?;
    let mut app = WebApplication::new(bridge.clone());
    app.add_window(options, demo_assets(html)?)?;
    let events = app.events();
    let state = Arc::new(Mutex::new(FormState::default()));
    let args = Schema::object([
        ("name".into(), Field::required(Schema::String)),
        ("enthusiastic".into(), Field::required(Schema::Boolean)),
    ]);
    let result = Schema::object([
        ("name".into(), Field::required(Schema::String)),
        ("enthusiastic".into(), Field::required(Schema::Boolean)),
        ("greeting".into(), Field::required(Schema::String)),
        ("submissions".into(), Field::required(Schema::Integer)),
    ]);
    app.register_event("greeting", result.clone())
        .map_err(backend)?;
    bridge
        .register::<GreetingArgs, FormState, _>(
            MethodSchema::new("greet", args, result),
            move |context, args| {
                context.check_cancelled()?;
                if args.name.trim().is_empty() || args.name.len() > 1024 {
                    return Err(CoreError::InvalidArguments(
                        "name must contain 1–1024 bytes".into(),
                    ));
                }
                let mut state = state.lock().unwrap();
                state.name = args.name;
                state.enthusiastic = args.enthusiastic;
                state.submit();
                events.emit(None, "greeting", &*state)?;
                Ok(state.clone())
            },
        )
        .map_err(backend)?;
    app.run()
}
pub fn run_demo(options: WindowOptions) -> Result<(), DesktopError> {
    run_html(options, include_str!("../../../assets/index.html"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn delivery_scripts_bind_the_execution_to_the_original_page_session() {
        let fixture: Value =
            serde_json::from_str(include_str!("../tests/session-delivery.json")).unwrap();
        let token: SessionToken = serde_json::from_value(fixture["session"].clone()).unwrap();
        let payload = fixture["payload"].clone();
        let scripts = [
            response_script(
                &token,
                &CallResponse {
                    id: 1,
                    result: Ok(payload.clone()),
                },
            ),
            stream_script(&token, &serde_json::json!({"id":2,"item":payload})),
            event_script(
                &token,
                &HostEvent {
                    name: "changed".into(),
                    payload,
                },
            ),
        ];
        assert_eq!(serde_json::to_value(scripts).unwrap(), fixture["scripts"]);
    }

    fn app() -> WebApplication {
        let bridge = Bridge::new(BridgeConfig::production(APP_ORIGIN).unwrap()).unwrap();
        let mut app = WebApplication::new(bridge);
        app.register_event("changed", Schema::Integer).unwrap();
        app.add_window(
            WindowOptions::new("one"),
            demo_assets("<p>hello</p>").unwrap(),
        )
        .unwrap();
        app.add_window(
            WindowOptions::new("two"),
            demo_assets("<p>hello</p>").unwrap(),
        )
        .unwrap();
        app
    }
    #[test]
    fn events_are_bounded_typed_and_targeted() {
        let app = app();
        let events = app.events();
        assert!(events.emit(Some(WindowId(99)), "changed", 1).is_err());
        assert!(events.emit(None, "changed", "not an integer").is_err());
        assert!(events.emit(None, "unknown", 1).is_err());
        events.emit(Some(WindowId(1)), "changed", 9).unwrap();
        let state = events.state.lock().unwrap();
        assert_eq!(state.queues[&WindowId(1)].len(), 1);
        assert!(state.queues[&WindowId(2)].is_empty());
    }
    #[test]
    fn broadcast_overflow_never_partially_delivers() {
        let app = app();
        let events = app.events();
        for i in 0..64 {
            events.emit(Some(WindowId(2)), "changed", i).unwrap();
        }
        assert_eq!(events.emit(None, "changed", 65), Err(CoreError::QueueFull));
        assert!(events.state.lock().unwrap().queues[&WindowId(1)].is_empty());
    }
    #[test]
    fn bundled_assets_embed_all_resources_and_reject_traversal() {
        let assets = demo_assets(include_str!("../../../assets/index.html")).unwrap();
        for path in ["/index.html", "/style.css", "/app.js", "/spark.svg"] {
            assert!(!assets.resolve(path).unwrap().asset.bytes().is_empty());
        }
        assert!(assets.resolve("/people/orion").unwrap().is_fallback);
        for path in [
            "/missing.png",
            "/../secret",
            "/%2e%2e/secret",
            "/%252e%252e/secret",
        ] {
            assert!(assets.resolve(path).is_err());
        }
    }
    #[test]
    fn exact_origin_matching_is_not_prefix_matching() {
        for directive in [
            "frame-src 'none'",
            "child-src 'none'",
            "worker-src 'none'",
            "frame-ancestors 'none'",
            "connect-src 'none'",
        ] {
            assert!(CSP.contains(directive));
        }
        let trusted = TrustedOrigin::parse(APP_ORIGIN).unwrap();
        assert_eq!(
            TrustedOrigin::parse(&origin_only(&format!("{APP_ORIGIN}/index.html"))).unwrap(),
            trusted
        );
        for url in [
            "https://evil.example/",
            "lumendesk://app.evil/",
            "http://lumendesk.app.evil/",
            "https://lumendesk.app/",
        ] {
            assert!(TrustedOrigin::parse(&origin_only(url)).map_or(true, |o| o != trusted));
        }
    }
}
