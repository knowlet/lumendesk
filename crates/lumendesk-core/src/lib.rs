//! Independent, display-free desktop contracts. This crate never opens a window.
//!
//! Bridge authorization metadata must come from the host adapter, never from
//! untrusted JSON. Navigation must invalidate a session before committing a page.
#![doc = include_str!("../README.md")]
pub mod assets;
pub mod bridge;
pub mod events;
pub mod lifecycle;
pub mod schema;
pub mod stream;

pub use bridge::{
    Authorization, Bridge, BridgeConfig, CallContext, CallRequest, CallResponse, CancellationToken,
    SessionToken, TrustedOrigin,
};
pub use events::{Event, EventBus, ListenerId};
pub use schema::{Field, MethodSchema, Schema, SchemaRegistry};
use serde::{Deserialize, Serialize};
pub use stream::{StreamItem, StreamReceiver, StreamSender, bounded_stream};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WindowId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "code", content = "message", rename_all = "snake_case")]
pub enum CoreError {
    #[error("invalid configuration: {0}")]
    InvalidConfiguration(String),
    #[error("unauthorized page")]
    Unauthorized,
    #[error("stale or missing page session")]
    StaleSession,
    #[error("window is closed or unknown")]
    ClosedWindow,
    #[error("unknown method: {0}")]
    UnknownMethod(String),
    #[error("duplicate registration: {0}")]
    DuplicateRegistration(String),
    #[error("request ID must increase within a session")]
    ReusedRequestId,
    #[error("payload exceeds the configured limit")]
    PayloadTooLarge,
    #[error("invalid arguments: {0}")]
    InvalidArguments(String),
    #[error("invalid result: {0}")]
    InvalidResult(String),
    #[error("operation was cancelled")]
    Cancelled,
    #[error("bounded queue is full")]
    QueueFull,
    #[error("stream has completed")]
    StreamFinished,
    #[error("backend failed: {0}")]
    Backend(String),
    #[error("backend panicked")]
    BackendPanic,
}

pub type Result<T> = std::result::Result<T, CoreError>;
