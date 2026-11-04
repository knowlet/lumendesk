//! Ordered admission is separate from host-owned worker scheduling.
use super::{
    Authorization, Bridge, CallContext, CallRequest, CallResponse, Handler, StreamScope,
};
use crate::{CoreError, Result};
use serde_json::Value;
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Mutex},
};

impl Bridge {
    /// Admit an ordinary call before scheduling its handler on a bounded worker.
    ///
    /// Call this in authenticated transport order. Admission reserves the request
    /// ID and session budget without executing user code or creating a thread.
    /// Admitted jobs may then execute in any order. Method argument/result schemas
    /// are checked when the job runs, just as for `begin_stream`.
    ///
    /// Calls and streams share one ID namespace and one pending budget. Dropping
    /// an unrun job releases its reservation but does not make its ID reusable.
    /// Keep the original authorization and recheck it immediately before delivery.
    pub fn begin_call(&self, auth: &Authorization, request: CallRequest) -> Result<CallJob> {
        self.begin_call_inner(auth, request)
            .map_err(|error| self.bound_error(error))
    }

    /// Enforce the raw-wire byte limit before deserializing untrusted call JSON.
    pub fn begin_call_json(&self, auth: &Authorization, bytes: &[u8]) -> Result<CallJob> {
        if bytes.len() > self.0.config.max_payload_bytes {
            return Err(CoreError::PayloadTooLarge);
        }
        let request = serde_json::from_slice(bytes)
            .map_err(|error| self.bound_error(CoreError::InvalidArguments(error.to_string())))?;
        self.begin_call(auth, request)
    }

    pub(super) fn begin_call_inner(
        &self,
        auth: &Authorization,
        request: CallRequest,
    ) -> Result<CallJob> {
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
        Ok(CallJob {
            bridge: self.clone(),
            auth: auth.clone(),
            context: CallContext {
                window: auth.window,
                request_id: request.id,
                cancellation,
                streams: Arc::new(Mutex::new(StreamScope::default())),
            },
            work: Some((handler, request.args)),
            finished: false,
        })
    }
}

/// An admitted, owned, one-shot ordinary call. No executor or thread is created.
///
/// Run on a host-owned bounded worker. Cancellation is cooperative and does not
/// release a running call's reservation until the handler exits. Dropping an
/// unrun job cancels it and releases its reservation without executing user code
/// or sending a response. Hosts rejecting a job must handle that response themselves.
/// Host-wide worker limits must also span window and session replacement.
#[must_use = "an admitted call must be run or deliberately dropped to cancel it"]
pub struct CallJob {
    bridge: Bridge,
    auth: Authorization,
    context: CallContext,
    work: Option<(Handler, Value)>,
    finished: bool,
}

impl std::fmt::Debug for CallJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallJob")
            .field("window", &self.auth.window)
            .field("request_id", &self.context.request_id)
            .finish_non_exhaustive()
    }
}

impl CallJob {
    pub fn request_id(&self) -> u64 {
        self.context.request_id
    }

    pub fn authorization(&self) -> &Authorization {
        &self.auth
    }

    /// Execute once. Results returned after cancellation/navigation are suppressed.
    /// Check the original authorization again on the host before UI delivery.
    pub fn run(mut self) -> CallResponse {
        let (handler, args) = self.work.take().expect("call job runs once");
        let result = catch_unwind(AssertUnwindSafe(|| handler(self.context.clone(), args)))
            .unwrap_or(Err(CoreError::BackendPanic));
        let mut result = self.bridge.bound_result(result);
        {
            let mut state = self.bridge.0.state.lock().unwrap();
            if self.context.cancellation.is_cancelled()
                || self.bridge.check_authorization(&mut state, &self.auth).is_err()
            {
                result = Err(CoreError::Cancelled);
            }
            if let Some(Some(session)) = state.windows.get_mut(&self.auth.window)
                && session.token == self.auth.session
            {
                session.pending.remove(&self.context.request_id);
            }
        }
        let result = result.map_err(|error| self.bridge.bound_error(error));
        self.context
            .finish_streams(result.as_ref().map(|_| ()).map_err(Clone::clone));
        self.finished = true;
        CallResponse {
            id: self.context.request_id,
            result,
        }
    }
}

impl Drop for CallJob {
    fn drop(&mut self) {
        if !self.finished {
            self.context.cancellation.cancel();
            self.context.finish_streams(Err(CoreError::Cancelled));
            // Session identity is checked inside release_request: an old job can
            // never release a new page's reservation for the same numeric ID.
            self.bridge
                .release_request(&self.auth, self.context.request_id);
        }
    }
}
