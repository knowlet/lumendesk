//! The host supplies authorization separately from request JSON. Sessions are not
//! a substitute for a trusted IPC transport: a frontend cannot attest its origin.
//! Request IDs increase strictly within each session, starting at any integer.
use crate::{
    CoreError, MethodSchema, Result, SchemaRegistry, StreamMethodSchema, StreamSink, WindowId,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TrustedOrigin {
    scheme: String,
    host: String,
    port: Option<u16>,
}

impl TrustedOrigin {
    pub fn parse(origin: &str) -> Result<Self> {
        let url = Url::parse(origin)
            .map_err(|_| CoreError::InvalidConfiguration("invalid origin URL".into()))?;
        if !matches!(url.path(), "" | "/") || url.query().is_some() || url.fragment().is_some() {
            return Err(CoreError::InvalidConfiguration(
                "trusted origins cannot contain paths, queries, or fragments".into(),
            ));
        }
        Self::from_url(&url).map_err(|_| {
            CoreError::InvalidConfiguration(
                "origin needs a host and cannot contain credentials".into(),
            )
        })
    }

    fn from_page(page: &str) -> Result<Self> {
        let url = Url::parse(page).map_err(|_| CoreError::Unauthorized)?;
        Self::from_url(&url)
    }
    fn from_url(url: &Url) -> Result<Self> {
        if !url.username().is_empty()
            || url.password().is_some()
            || url.cannot_be_a_base()
            || matches!(
                url.scheme(),
                "file" | "data" | "javascript" | "about" | "blob"
            )
        {
            return Err(CoreError::Unauthorized);
        }
        let host = url
            .host_str()
            .filter(|h| !h.is_empty())
            .ok_or(CoreError::Unauthorized)?;
        Ok(Self {
            scheme: url.scheme().into(),
            host: host.to_ascii_lowercase(),
            port: url.port_or_known_default(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct BridgeConfig {
    pub trusted_origins: Vec<TrustedOrigin>,
    pub max_payload_bytes: usize,
    pub max_pending_per_session: usize,
}

impl BridgeConfig {
    /// Production has no implicit development/localhost permissions.
    pub fn production(origin: &str) -> Result<Self> {
        Ok(Self {
            trusted_origins: vec![TrustedOrigin::parse(origin)?],
            max_payload_bytes: 1024 * 1024,
            max_pending_per_session: 64,
        })
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionToken(String);
impl std::fmt::Debug for SessionToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionToken(<redacted>)")
    }
}
impl SessionToken {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct Authorization {
    pub window: WindowId,
    pub session: SessionToken,
    pub page_url: String,
    pub top_level: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallRequest {
    pub id: u64,
    pub method: String,
    pub args: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CallResponse {
    pub id: u64,
    pub result: Result<Value>,
}

#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    own: Arc<AtomicBool>,
    parents: Vec<Arc<AtomicBool>>,
}
impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.own.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.own.load(Ordering::Acquire)
            || self
                .parents
                .iter()
                .any(|parent| parent.load(Ordering::Acquire))
    }
    fn child(&self) -> Self {
        let mut parents = self.parents.clone();
        parents.push(self.own.clone());
        Self {
            own: Arc::new(AtomicBool::new(false)),
            parents,
        }
    }
    pub fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(CoreError::Cancelled)
        } else {
            Ok(())
        }
    }
}

type StreamCompletion = Box<dyn FnOnce(Result<()>) + Send>;
#[derive(Default)]
struct StreamScope {
    completions: Vec<StreamCompletion>,
    finished: bool,
}

#[derive(Clone)]
pub struct CallContext {
    pub window: WindowId,
    pub request_id: u64,
    pub cancellation: CancellationToken,
    streams: Arc<Mutex<StreamScope>>,
}
impl CallContext {
    pub fn check_cancelled(&self) -> Result<()> {
        self.cancellation.check()
    }
    fn finish_streams(&self, result: Result<()>) {
        let completions = {
            let mut scope = self.streams.lock().unwrap();
            scope.finished = true;
            std::mem::take(&mut scope.completions)
        };
        for complete in completions {
            complete(result.clone());
        }
    }
    /// Open a bounded stream tied to this call. At most 16 streams may be opened.
    /// Returning from the handler closes every unfinished stream, preserving FIFO
    /// values before completion. The host drains streams before delivering the
    /// call response. A retained producer cannot emit after the call has ended.
    pub fn stream<T: Send + 'static>(
        &self,
        capacity: usize,
    ) -> Result<(crate::StreamSender<T>, crate::StreamReceiver<T>)> {
        self.check_cancelled()?;
        let mut streams = self.streams.lock().unwrap();
        if streams.finished {
            return Err(CoreError::StreamFinished);
        }
        if streams.completions.len() >= 16 {
            return Err(CoreError::QueueFull);
        }
        let (sender, receiver) = crate::bounded_stream(capacity, self.cancellation.clone())?;
        streams.completions.push(sender.completion_hook());
        Ok((sender, receiver))
    }
}

impl std::fmt::Debug for CallContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallContext")
            .field("window", &self.window)
            .field("request_id", &self.request_id)
            .field("cancellation", &self.cancellation)
            .finish_non_exhaustive()
    }
}

type Handler = Arc<dyn Fn(CallContext, Value) -> Result<Value> + Send + Sync>;
type StreamHandler =
    Arc<dyn Fn(CallContext, Value, crate::StreamSender<Value>) -> Result<Value> + Send + Sync>;
struct Session {
    token: SessionToken,
    origin: TrustedOrigin,
    last_request: Option<u64>,
    pending: BTreeMap<u64, CancellationToken>,
    precancelled: BTreeSet<u64>,
    cancellation: CancellationToken,
}
impl Session {
    fn cancel_all(&self) {
        self.cancellation.cancel();
        for token in self.pending.values() {
            token.cancel();
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.cancel_all();
    }
}
#[derive(Default)]
struct State {
    methods: BTreeMap<String, Handler>,
    stream_methods: BTreeMap<String, StreamHandler>,
    schemas: SchemaRegistry,
    windows: BTreeMap<WindowId, Option<Session>>,
    closed: BTreeSet<WindowId>,
}
struct Inner {
    config: BridgeConfig,
    state: Mutex<State>,
}

#[derive(Clone)]
pub struct Bridge(Arc<Inner>);

impl Bridge {
    pub fn new(config: BridgeConfig) -> Result<Self> {
        if config.trusted_origins.is_empty()
            || config.max_payload_bytes == 0
            || config.max_pending_per_session == 0
        {
            return Err(CoreError::InvalidConfiguration(
                "an origin and positive bridge limits are required".into(),
            ));
        }
        Ok(Self(Arc::new(Inner {
            config,
            state: Mutex::new(State::default()),
        })))
    }

    /// Explicit schemas are checked in addition to serde deserialization.
    /// Handlers run without the bridge lock and must poll cooperative cancellation
    /// during long work. The bridge suppresses a result returned after cancellation.
    pub fn register<A, R, F>(&self, schema: MethodSchema, handler: F) -> Result<()>
    where
        A: DeserializeOwned + 'static,
        R: Serialize + 'static,
        F: Fn(CallContext, A) -> Result<R> + Send + Sync + 'static,
    {
        let mut state = self.0.state.lock().unwrap();
        state.schemas.register_method(schema.clone())?;
        let input_schema = schema.arguments;
        let result_schema = schema.result;
        state.methods.insert(
            schema.name,
            Arc::new(move |context, args| {
                context.check_cancelled()?;
                input_schema
                    .validate(&args)
                    .map_err(CoreError::InvalidArguments)?;
                let typed = serde_json::from_value(args)
                    .map_err(|e| CoreError::InvalidArguments(e.to_string()))?;
                let output = handler(context.clone(), typed)?;
                context.check_cancelled()?;
                let result = serde_json::to_value(output)
                    .map_err(|e| CoreError::InvalidResult(e.to_string()))?;
                result_schema
                    .validate(&result)
                    .map_err(CoreError::InvalidResult)?;
                Ok(result)
            }),
        );
        Ok(())
    }

    /// Register a frontend-facing stream. Item serialization, item schema, item
    /// size, argument schema and terminal result schema are checked independently.
    /// Returning (or panicking) seals the producer and supplies its terminal result.
    pub fn register_stream<A, T, R, F>(&self, schema: StreamMethodSchema, handler: F) -> Result<()>
    where
        A: DeserializeOwned + 'static,
        T: Serialize + 'static,
        R: Serialize + 'static,
        F: Fn(CallContext, A, StreamSink<T>) -> Result<R> + Send + Sync + 'static,
    {
        let mut state = self.0.state.lock().unwrap();
        state.schemas.register_stream(schema.clone())?;
        let max_payload_bytes = self.0.config.max_payload_bytes;
        state.stream_methods.insert(
            schema.name,
            Arc::new(move |context, args, sender| {
                context.check_cancelled()?;
                schema
                    .arguments
                    .validate(&args)
                    .map_err(CoreError::InvalidArguments)?;
                let typed = serde_json::from_value(args)
                    .map_err(|error| CoreError::InvalidArguments(error.to_string()))?;
                let sink = StreamSink::new(sender, schema.item.clone(), max_payload_bytes);
                let output = handler(context.clone(), typed, sink)?;
                context.check_cancelled()?;
                let result = serde_json::to_value(output)
                    .map_err(|error| CoreError::InvalidResult(error.to_string()))?;
                schema
                    .result
                    .validate(&result)
                    .map_err(CoreError::InvalidResult)?;
                Ok(result)
            }),
        );
        Ok(())
    }

    /// Admit a stream in the same ordered request-ID namespace as ordinary calls.
    /// Run the returned job on a backend worker and retain its receiver on the
    /// trusted host. Admission does not execute user code or start an OS thread.
    /// The session budget includes streams that finished but have not been drained.
    pub fn begin_stream(
        &self,
        auth: &Authorization,
        request: CallRequest,
        capacity: usize,
    ) -> Result<(StreamCall, BridgeStreamReceiver)> {
        if capacity == 0 {
            return Err(CoreError::InvalidConfiguration(
                "stream capacity must be positive".into(),
            ));
        }
        self.validate_request(&request)?;
        let (handler, cancellation) = {
            let mut state = self.0.state.lock().unwrap();
            self.check_authorization(&mut state, auth)?;
            let handler = state
                .stream_methods
                .get(&request.method)
                .cloned()
                .ok_or_else(|| CoreError::UnknownMethod(request.method.clone()))?;
            let session = self.check_authorization(&mut state, auth)?;
            let cancellation = self.reserve_request(session, request.id)?;
            (handler, cancellation)
        };
        let context = CallContext {
            window: auth.window,
            request_id: request.id,
            cancellation,
            streams: Arc::new(Mutex::new(StreamScope::default())),
        };
        // Capacity is checked above; the newly created scope has no streams.
        // A concurrent navigation can still cancel between admission and creation.
        let (sender, receiver) = match context.stream(capacity) {
            Ok(pair) => pair,
            Err(error) => {
                self.release_request(auth, request.id);
                return Err(error);
            }
        };
        let terminal = Arc::new(Mutex::new(None));
        Ok((
            StreamCall {
                bridge: self.clone(),
                auth: auth.clone(),
                context,
                terminal: terminal.clone(),
                work: Some((handler, request.args, sender)),
            },
            BridgeStreamReceiver {
                bridge: self.clone(),
                auth: auth.clone(),
                request_id: request.id,
                receiver,
                terminal,
                released: AtomicBool::new(false),
            },
        ))
    }

    /// Raw-wire variant; enforce the byte limit before parsing untrusted JSON.
    pub fn begin_stream_json(
        &self,
        auth: &Authorization,
        bytes: &[u8],
        capacity: usize,
    ) -> Result<(StreamCall, BridgeStreamReceiver)> {
        if bytes.len() > self.0.config.max_payload_bytes {
            return Err(CoreError::PayloadTooLarge);
        }
        let request = serde_json::from_slice(bytes)
            .map_err(|error| CoreError::InvalidArguments(error.to_string()))?;
        self.begin_stream(auth, request, capacity)
    }

    pub fn schemas(&self) -> SchemaRegistry {
        self.0.state.lock().unwrap().schemas.clone()
    }

    /// Creates a host-bound session, invalidating any earlier page in this window.
    /// Tokens contain 256 bits of OS randomness. Keep them inside the trusted
    /// top-level runtime closure; never expose them to embedded page content.
    pub fn create_session(&self, window: WindowId, page_url: &str) -> Result<SessionToken> {
        let origin = TrustedOrigin::from_page(page_url)?;
        if !self.0.config.trusted_origins.contains(&origin) {
            return Err(CoreError::Unauthorized);
        }
        let mut state = self.0.state.lock().unwrap();
        if state.closed.contains(&window) {
            return Err(CoreError::ClosedWindow);
        }
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes)
            .map_err(|_| CoreError::Backend("OS randomness unavailable; session denied".into()))?;
        let token = SessionToken(bytes.iter().map(|byte| format!("{byte:02x}")).collect());
        state.windows.insert(
            window,
            Some(Session {
                token: token.clone(),
                origin,
                last_request: None,
                pending: BTreeMap::new(),
                precancelled: BTreeSet::new(),
                cancellation: CancellationToken::new(),
            }),
        );
        Ok(token)
    }

    /// Call before all navigation, including same-origin navigation and reload.
    /// The host must then explicitly create a new session for a trusted page.
    pub fn navigate(&self, window: WindowId) -> Result<()> {
        let mut state = self.0.state.lock().unwrap();
        let slot = state
            .windows
            .get_mut(&window)
            .ok_or(CoreError::ClosedWindow)?;
        if let Some(session) = slot.take() {
            session.cancel_all();
        }
        Ok(())
    }

    pub fn close_window(&self, window: WindowId) -> Result<()> {
        let mut state = self.0.state.lock().unwrap();
        let session = state
            .windows
            .remove(&window)
            .ok_or(CoreError::ClosedWindow)?;
        if let Some(session) = session {
            session.cancel_all();
        }
        state.closed.insert(window);
        Ok(())
    }

    fn check_authorization<'a>(
        &self,
        state: &'a mut State,
        auth: &Authorization,
    ) -> Result<&'a mut Session> {
        if !auth.top_level {
            return Err(CoreError::Unauthorized);
        }
        let origin = TrustedOrigin::from_page(&auth.page_url)?;
        if !self.0.config.trusted_origins.contains(&origin) {
            return Err(CoreError::Unauthorized);
        }
        let slot = state
            .windows
            .get_mut(&auth.window)
            .ok_or(CoreError::ClosedWindow)?;
        let session = slot.as_mut().ok_or(CoreError::StaleSession)?;
        if session.token != auth.session || session.origin != origin {
            return Err(CoreError::StaleSession);
        }
        Ok(session)
    }

    pub fn cancel(&self, auth: &Authorization, request_id: u64) -> Result<bool> {
        if request_id > 9_007_199_254_740_991 {
            return Err(CoreError::InvalidArguments(
                "request IDs must be JavaScript-safe integers".into(),
            ));
        }
        let mut state = self.0.state.lock().unwrap();
        let session = self.check_authorization(&mut state, auth)?;
        if let Some(token) = session.pending.get(&request_id) {
            token.cancel();
            return Ok(true);
        }
        if session.last_request.is_some_and(|last| request_id <= last) {
            return Ok(false);
        }
        if session.precancelled.contains(&request_id) {
            return Ok(true);
        }
        if session.precancelled.len() + session.pending.len()
            >= self.0.config.max_pending_per_session
        {
            return Err(CoreError::QueueFull);
        }
        session.precancelled.insert(request_id);
        Ok(true)
    }

    /// Recheck on the host UI thread before delivering a completed response.
    pub fn authorize(&self, auth: &Authorization) -> Result<()> {
        self.check_authorization(&mut self.0.state.lock().unwrap(), auth)
            .map(|_| ())
    }

    pub fn dispatch_json(&self, auth: &Authorization, bytes: &[u8]) -> Result<CallResponse> {
        if bytes.len() > self.0.config.max_payload_bytes {
            return Err(CoreError::PayloadTooLarge);
        }
        let request = serde_json::from_slice(bytes)
            .map_err(|e| CoreError::InvalidArguments(e.to_string()))?;
        Ok(self.dispatch(auth, request))
    }

    pub fn dispatch(&self, auth: &Authorization, request: CallRequest) -> CallResponse {
        let id = request.id;
        CallResponse {
            id,
            result: self.dispatch_inner(auth, request),
        }
    }

    fn validate_request(&self, request: &CallRequest) -> Result<()> {
        if request.id > 9_007_199_254_740_991 {
            return Err(CoreError::InvalidArguments(
                "request IDs must be JavaScript-safe integers".into(),
            ));
        }
        let bytes =
            serde_json::to_vec(&request).map_err(|e| CoreError::InvalidArguments(e.to_string()))?;
        if bytes.len() > self.0.config.max_payload_bytes {
            return Err(CoreError::PayloadTooLarge);
        }
        Ok(())
    }

    fn reserve_request(&self, session: &mut Session, id: u64) -> Result<CancellationToken> {
        if session.last_request.is_some_and(|last| id <= last) {
            return Err(CoreError::ReusedRequestId);
        }
        if session.precancelled.remove(&id) {
            session.last_request = Some(id);
            session.precancelled.retain(|other| *other > id);
            return Err(CoreError::Cancelled);
        }
        let future_tombstones = session
            .precancelled
            .iter()
            .filter(|other| **other > id)
            .count();
        if session.pending.len() + future_tombstones >= self.0.config.max_pending_per_session {
            return Err(CoreError::QueueFull);
        }
        session.last_request = Some(id);
        session.precancelled.retain(|other| *other > id);
        let cancellation = session.cancellation.child();
        session.pending.insert(id, cancellation.clone());
        Ok(cancellation)
    }

    fn release_request(&self, auth: &Authorization, request_id: u64) {
        let mut state = self.0.state.lock().unwrap();
        if let Some(Some(session)) = state.windows.get_mut(&auth.window)
            && session.token == auth.session
        {
            session.pending.remove(&request_id);
        }
    }

    fn dispatch_inner(&self, auth: &Authorization, request: CallRequest) -> Result<Value> {
        self.validate_request(&request)?;
        let (handler, cancellation) = {
            let mut state = self.0.state.lock().unwrap();
            self.check_authorization(&mut state, auth)?;
            let handler = state
                .methods
                .get(&request.method)
                .cloned()
                .ok_or_else(|| CoreError::UnknownMethod(request.method.clone()))?;
            let session = self.check_authorization(&mut state, auth)?;
            let cancellation = self.reserve_request(session, request.id)?;
            (handler, cancellation)
        };
        let streams = Arc::new(Mutex::new(StreamScope::default()));
        let context = CallContext {
            window: auth.window,
            request_id: request.id,
            cancellation: cancellation.clone(),
            streams: streams.clone(),
        };
        let mut result = catch_unwind(AssertUnwindSafe(|| handler(context, request.args)))
            .unwrap_or(Err(CoreError::BackendPanic));
        if let Ok(value) = &result {
            match serde_json::to_vec(value) {
                Ok(bytes) if bytes.len() > self.0.config.max_payload_bytes => {
                    result = Err(CoreError::PayloadTooLarge)
                }
                Err(error) => result = Err(CoreError::InvalidResult(error.to_string())),
                _ => (),
            }
        }
        {
            let mut state = self.0.state.lock().unwrap();
            if cancellation.is_cancelled() || self.check_authorization(&mut state, auth).is_err() {
                result = Err(CoreError::Cancelled);
            }
            if let Some(Some(session)) = state.windows.get_mut(&auth.window)
                && session.token == auth.session
            {
                session.pending.remove(&request.id);
            }
        }
        let completions = {
            let mut scope = streams.lock().unwrap();
            scope.finished = true;
            std::mem::take(&mut scope.completions)
        };
        for complete in completions {
            complete(result.as_ref().map(|_| ()).map_err(Clone::clone));
        }
        result
    }
}

/// Transport-neutral stream frames. Completion is emitted exactly once and only
/// after every buffered item. Cancellation instead returns `Err(Cancelled)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StreamResponseItem {
    Value { sequence: u64, value: Value },
    Complete(Result<Value>),
}

type TerminalResult = Arc<Mutex<Option<Result<Value>>>>;

/// An admitted one-shot producer job. The host chooses a bounded worker pool;
/// this type never creates threads. Dropping an unrun job reports a terminal error.
pub struct StreamCall {
    bridge: Bridge,
    auth: Authorization,
    context: CallContext,
    terminal: TerminalResult,
    work: Option<(StreamHandler, Value, crate::StreamSender<Value>)>,
}

impl StreamCall {
    pub fn run(mut self) {
        let (handler, args, sender) = self.work.take().expect("stream job runs once");
        let result = catch_unwind(AssertUnwindSafe(|| {
            handler(self.context.clone(), args, sender)
        }))
        .unwrap_or(Err(CoreError::BackendPanic));
        self.finish(result);
    }
    pub fn request_id(&self) -> u64 {
        self.context.request_id
    }
    pub fn authorization(&self) -> &Authorization {
        &self.auth
    }

    fn finish(&self, mut result: Result<Value>) {
        if let Ok(value) = &result {
            match serde_json::to_vec(value) {
                Ok(bytes) if bytes.len() > self.bridge.0.config.max_payload_bytes => {
                    result = Err(CoreError::PayloadTooLarge);
                }
                Err(error) => result = Err(CoreError::InvalidResult(error.to_string())),
                _ => (),
            }
        }
        if self.context.cancellation.is_cancelled() || self.bridge.authorize(&self.auth).is_err() {
            result = Err(CoreError::Cancelled);
        }
        let completion = result.as_ref().map(|_| ()).map_err(Clone::clone);
        *self.terminal.lock().unwrap() = Some(result);
        self.context.finish_streams(completion);
    }
}

impl Drop for StreamCall {
    fn drop(&mut self) {
        if self.work.is_some() {
            self.finish(Err(CoreError::Backend(
                "stream worker dropped before execution".into(),
            )));
        }
    }
}

/// Host-side receiver retaining the original window, session and request authority.
/// Only poll when the frontend has demand; do not eagerly drain into another queue.
/// Recheck `Bridge::authorize` immediately before delivering each frame to a page.
pub struct BridgeStreamReceiver {
    bridge: Bridge,
    auth: Authorization,
    request_id: u64,
    receiver: crate::StreamReceiver<Value>,
    terminal: TerminalResult,
    released: AtomicBool,
}

impl BridgeStreamReceiver {
    pub fn request_id(&self) -> u64 {
        self.request_id
    }
    pub fn authorization(&self) -> &Authorization {
        &self.auth
    }
    pub fn queued_len(&self) -> usize {
        self.receiver.queued_len()
    }
    pub fn is_complete(&self) -> bool {
        self.released.load(Ordering::Acquire)
    }
    pub fn cancel(&self) {
        self.receiver.cancel();
        self.release();
    }
    fn release(&self) {
        if !self.released.swap(true, Ordering::AcqRel) {
            self.bridge.release_request(&self.auth, self.request_id);
        }
    }
    pub fn try_next(&self) -> Result<Option<StreamResponseItem>> {
        match self.receiver.try_next() {
            Ok(Some(crate::StreamItem::Value { sequence, value })) => {
                Ok(Some(StreamResponseItem::Value { sequence, value }))
            }
            Ok(Some(crate::StreamItem::Complete(completion))) => {
                let result = self.terminal.lock().unwrap().take().unwrap_or_else(|| {
                    Err(completion.err().unwrap_or_else(|| {
                        CoreError::Backend("stream completed without a terminal result".into())
                    }))
                });
                self.release();
                Ok(Some(StreamResponseItem::Complete(result)))
            }
            Err(error) => {
                self.release();
                Err(error)
            }
            Ok(None) => Ok(None),
        }
    }
}

impl Drop for BridgeStreamReceiver {
    fn drop(&mut self) {
        if !self.receiver.is_complete() {
            self.receiver.cancel();
        }
        self.release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Schema;
    use serde_json::json;
    use std::sync::mpsc;

    fn bridge() -> Bridge {
        Bridge::new(BridgeConfig::production("lumendesk://app").unwrap()).unwrap()
    }
    fn auth(bridge: &Bridge, id: u64) -> Authorization {
        Authorization {
            window: WindowId(id),
            session: bridge
                .create_session(WindowId(id), "lumendesk://app/index.html")
                .unwrap(),
            page_url: "lumendesk://app/index.html".into(),
            top_level: true,
        }
    }
    fn request(id: u64, args: Value) -> CallRequest {
        CallRequest {
            id,
            method: "echo".into(),
            args,
        }
    }
    fn echo(bridge: &Bridge) {
        bridge
            .register(
                MethodSchema::new("echo", Schema::String, Schema::String),
                |_, value: String| Ok(value),
            )
            .unwrap();
    }

    #[test]
    fn typed_unicode_roundtrip_and_failure_contracts() {
        let bridge = bridge();
        echo(&bridge);
        let auth = auth(&bridge, 1);
        assert_eq!(
            bridge
                .dispatch(&auth, request(1, json!("您好 🌟")))
                .result
                .unwrap(),
            json!("您好 🌟")
        );
        assert!(matches!(
            bridge.dispatch(&auth, request(2, json!(12))).result,
            Err(CoreError::InvalidArguments(_))
        ));
        assert!(matches!(
            bridge
                .dispatch(
                    &auth,
                    CallRequest {
                        id: 3,
                        method: "missing".into(),
                        args: Value::Null
                    }
                )
                .result,
            Err(CoreError::UnknownMethod(_))
        ));
        assert_eq!(
            bridge.dispatch(&auth, request(1, json!("again"))).result,
            Err(CoreError::ReusedRequestId)
        );
        assert!(serde_json::to_value(&auth.session).unwrap().is_string());
    }

    #[test]
    fn rejects_external_lookalike_frames_and_credentials() {
        let bridge = bridge();
        echo(&bridge);
        let mut auth = auth(&bridge, 1);
        for url in [
            "https://app/",
            "lumendesk://app.evil/",
            "lumendesk://evil@app/",
            "lumendesk://app:123/",
            "file:///app",
            "data:text/plain,hi",
        ] {
            auth.page_url = url.into();
            assert_eq!(
                bridge.dispatch(&auth, request(1, json!("x"))).result,
                Err(CoreError::Unauthorized),
                "{url}"
            );
        }
        auth.page_url = "lumendesk://app/".into();
        auth.top_level = false;
        assert_eq!(
            bridge.dispatch(&auth, request(1, json!("x"))).result,
            Err(CoreError::Unauthorized)
        );
    }

    #[test]
    fn normalizes_standard_default_ports_but_never_trusts_localhost() {
        assert_eq!(
            TrustedOrigin::parse("https://EXAMPLE.com:443/").unwrap(),
            TrustedOrigin::parse("https://example.com").unwrap()
        );
        assert!(TrustedOrigin::parse("https://example.com/path").is_err());
        assert!(
            bridge()
                .create_session(WindowId(1), "http://localhost:5173/")
                .is_err()
        );
        let windows =
            Bridge::new(BridgeConfig::production("http://lumendesk.app").unwrap()).unwrap();
        assert!(
            windows
                .create_session(WindowId(1), "http://lumendesk.app/page")
                .is_ok()
        );
    }

    #[test]
    fn navigation_invalidates_authority_and_cancels_pending_work() {
        let bridge = bridge();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        bridge
            .register(
                MethodSchema::new("echo", Schema::Null, Schema::Null),
                move |ctx, _: ()| {
                    started_tx.send(ctx.cancellation.clone()).unwrap();
                    release_rx.lock().unwrap().recv().unwrap();
                    Ok(())
                },
            )
            .unwrap();
        let auth = auth(&bridge, 1);
        let worker_bridge = bridge.clone();
        let worker_auth = auth.clone();
        let worker = std::thread::spawn(move || {
            worker_bridge.dispatch(&worker_auth, request(1, Value::Null))
        });
        let cancellation = started_rx.recv().unwrap();
        bridge.navigate(auth.window).unwrap();
        assert!(cancellation.is_cancelled());
        release_tx.send(()).unwrap();
        assert_eq!(worker.join().unwrap().result, Err(CoreError::Cancelled));
        assert_eq!(
            bridge.dispatch(&auth, request(2, Value::Null)).result,
            Err(CoreError::StaleSession)
        );
        let fresh = bridge
            .create_session(auth.window, "lumendesk://app/")
            .unwrap();
        assert_ne!(fresh, auth.session);
    }

    #[test]
    fn sessions_do_not_cross_windows_or_bridges() {
        let bridge = bridge();
        echo(&bridge);
        let a = auth(&bridge, 1);
        let b = auth(&bridge, 2);
        let mut forged = b.clone();
        forged.session = a.session.clone();
        assert_eq!(
            bridge.dispatch(&forged, request(1, json!("x"))).result,
            Err(CoreError::StaleSession)
        );
        bridge.close_window(a.window).unwrap();
        assert_eq!(
            bridge.dispatch(&a, request(1, json!("x"))).result,
            Err(CoreError::ClosedWindow)
        );
        assert!(
            bridge
                .dispatch(&b, request(1, json!("still live")))
                .result
                .is_ok()
        );
        assert_eq!(
            bridge.create_session(a.window, "lumendesk://app/"),
            Err(CoreError::ClosedWindow)
        );
    }

    #[test]
    fn cancelling_before_dispatch_prevents_execution_and_has_a_bound() {
        let mut config = BridgeConfig::production("lumendesk://app").unwrap();
        config.max_pending_per_session = 2;
        let bridge = Bridge::new(config).unwrap();
        let executed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let calls = executed.clone();
        bridge
            .register(
                MethodSchema::new("echo", Schema::Null, Schema::Null),
                move |_, _: ()| {
                    calls.fetch_add(1, Ordering::Relaxed);
                    Ok(())
                },
            )
            .unwrap();
        let auth = auth(&bridge, 1);
        assert_eq!(bridge.cancel(&auth, 1), Ok(true));
        assert_eq!(bridge.cancel(&auth, 1), Ok(true));
        assert_eq!(bridge.cancel(&auth, 2), Ok(true));
        assert_eq!(bridge.cancel(&auth, 3), Err(CoreError::QueueFull));
        assert_eq!(
            bridge.dispatch(&auth, request(1, Value::Null)).result,
            Err(CoreError::Cancelled)
        );
        assert_eq!(
            bridge.dispatch(&auth, request(2, Value::Null)).result,
            Err(CoreError::Cancelled)
        );
        assert_eq!(executed.load(Ordering::Relaxed), 0);
        assert!(
            bridge
                .dispatch(&auth, request(3, Value::Null))
                .result
                .is_ok()
        );
        assert_eq!(executed.load(Ordering::Relaxed), 1);
        assert_eq!(bridge.cancel(&auth, 3), Ok(false));
        assert_eq!(
            bridge.dispatch(&auth, request(2, Value::Null)).result,
            Err(CoreError::ReusedRequestId)
        );
    }

    #[test]
    fn call_completion_closes_retained_streams_and_contexts() {
        let bridge = bridge();
        let auth = auth(&bridge, 1);
        let (transport, receiver) = mpsc::channel();
        bridge
            .register(
                MethodSchema::new("echo", Schema::Null, Schema::Null),
                move |ctx, _: ()| {
                    let (tx, rx) = ctx.stream::<u32>(2)?;
                    tx.send(1)?;
                    tx.send(2)?;
                    transport.send((tx, rx, ctx)).unwrap();
                    Ok(())
                },
            )
            .unwrap();
        assert!(
            bridge
                .dispatch(&auth, request(1, Value::Null))
                .result
                .is_ok()
        );
        let (tx, rx, ctx) = receiver.recv().unwrap();
        assert_eq!(tx.send(3), Err(CoreError::StreamFinished));
        assert!(matches!(
            ctx.stream::<u8>(1),
            Err(CoreError::StreamFinished)
        ));
        assert!(matches!(
            rx.try_next().unwrap(),
            Some(crate::StreamItem::Value {
                sequence: 0,
                value: 1
            })
        ));
        assert!(matches!(
            rx.try_next().unwrap(),
            Some(crate::StreamItem::Value {
                sequence: 1,
                value: 2
            })
        ));
        assert_eq!(
            rx.try_next().unwrap(),
            Some(crate::StreamItem::Complete(Ok(())))
        );
        drop(rx);
        assert!(!ctx.cancellation.is_cancelled());
    }

    #[test]
    fn leaving_a_call_stream_early_cancels_the_call() {
        let bridge = bridge();
        let auth = auth(&bridge, 1);
        bridge
            .register(
                MethodSchema::new("echo", Schema::Null, Schema::Null),
                |ctx, _: ()| {
                    let (tx, rx) = ctx.stream::<u8>(1)?;
                    drop(rx);
                    assert_eq!(tx.send(1), Err(CoreError::Cancelled));
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(
            bridge.dispatch(&auth, request(1, Value::Null)).result,
            Err(CoreError::Cancelled)
        );
    }

    #[test]
    fn navigation_cancels_buffered_streams_after_the_handler_finished() {
        let bridge = bridge();
        let auth = auth(&bridge, 1);
        let (transport, receiver) = mpsc::channel();
        bridge
            .register(
                MethodSchema::new("echo", Schema::Null, Schema::Null),
                move |ctx, _: ()| {
                    let (tx, rx) = ctx.stream::<u8>(1)?;
                    tx.send(1)?;
                    transport.send(rx).unwrap();
                    Ok(())
                },
            )
            .unwrap();
        assert!(
            bridge
                .dispatch(&auth, request(1, Value::Null))
                .result
                .is_ok()
        );
        bridge.navigate(auth.window).unwrap();
        assert_eq!(
            receiver.recv().unwrap().try_next(),
            Err(CoreError::Cancelled)
        );
    }

    #[test]
    fn backend_errors_and_result_schema_mismatch_are_explicit() {
        let bridge = bridge();
        let auth = auth(&bridge, 1);
        bridge
            .register(
                MethodSchema::new("echo", Schema::Null, Schema::Integer),
                |_, _: ()| Ok("invalid output"),
            )
            .unwrap();
        assert!(matches!(
            bridge.dispatch(&auth, request(1, Value::Null)).result,
            Err(CoreError::InvalidResult(_))
        ));
        bridge
            .register(
                MethodSchema::new("fail", Schema::Null, Schema::Null),
                |_, _: ()| -> Result<()> { Err(CoreError::Backend("service unavailable".into())) },
            )
            .unwrap();
        let response = bridge.dispatch(
            &auth,
            CallRequest {
                id: 2,
                method: "fail".into(),
                args: Value::Null,
            },
        );
        assert_eq!(
            response.result,
            Err(CoreError::Backend("service unavailable".into()))
        );
        assert!(matches!(
            bridge
                .dispatch(&auth, request(9_007_199_254_740_992, Value::Null))
                .result,
            Err(CoreError::InvalidArguments(_))
        ));
    }

    #[test]
    fn enforces_payload_result_limits_and_contains_panics() {
        let mut config = BridgeConfig::production("lumendesk://app").unwrap();
        config.max_payload_bytes = 128;
        let bridge = Bridge::new(config).unwrap();
        let auth = auth(&bridge, 1);
        echo(&bridge);
        assert_eq!(
            bridge
                .dispatch(&auth, request(1, json!("x".repeat(200))))
                .result,
            Err(CoreError::PayloadTooLarge)
        );
        assert_eq!(
            bridge.dispatch_json(&auth, &[b'x'; 129]),
            Err(CoreError::PayloadTooLarge)
        );
        bridge
            .register(
                MethodSchema::new("panic", Schema::Null, Schema::Null),
                |_, _: ()| -> Result<()> { panic!("contained") },
            )
            .unwrap();
        let response = bridge.dispatch(
            &auth,
            CallRequest {
                id: 2,
                method: "panic".into(),
                args: Value::Null,
            },
        );
        assert_eq!(response.result, Err(CoreError::BackendPanic));
        assert!(
            bridge
                .dispatch(&auth, request(3, json!("survived")))
                .result
                .is_ok()
        );
    }
}
