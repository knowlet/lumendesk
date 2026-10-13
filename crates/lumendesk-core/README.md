# LumenDesk core

Independent display-free contracts. This crate does not simulate or claim OS
window support. Desktop adapters drive the lifecycle and transport the bridge.

## Typed bridge

```rust
use lumendesk_core::{Authorization, Bridge, BridgeConfig, CallRequest,
    MethodSchema, Schema, WindowId};

let bridge = Bridge::new(BridgeConfig::production("lumendesk://app")?)?;
bridge.register::<String, String, _>(
    MethodSchema::new("greet", Schema::String, Schema::String),
    |context, name| {
        context.check_cancelled()?;
        Ok(format!("Hello, {name}!"))
    },
)?;
let window = WindowId(1);
let session = bridge.create_session(window, "lumendesk://app/index.html")?;
let auth = Authorization {
    window, session, page_url: "lumendesk://app/index.html".into(), top_level: true,
};
let response = bridge.dispatch(&auth, CallRequest {
    id: 1, method: "greet".into(), args: serde_json::json!("Orion"),
});
assert_eq!(response.result?, serde_json::json!("Hello, Orion!"));
# Ok::<(), lumendesk_core::CoreError>(())
```

The host supplies `Authorization` separately from untrusted request JSON. Never
trust a page's assertion of its origin, window, or top-level status. Tokens use
256 bits of OS randomness and must remain inside the trusted top-level runtime
closure. Exact parsed scheme, host, and port must match configured origins;
production never implicitly trusts development servers. Windows custom-protocol
adapters may explicitly configure `http://lumendesk.app` instead.

Call `navigate(window)` before every navigation/reload, even same-origin
navigation. Establish a fresh session only for trusted pages. Call
`close_window(window)` on destruction. Window IDs cannot be reused after close.
Before delivering a response, the UI thread checks that its destination still
owns the original session. `authorize()` is available for this check.

Request IDs are JavaScript-safe nonnegative integers, strictly increasing within
one session. The host must admit requests in ID order; concurrent execution does
not relax that rule. `cancel()` records bounded tombstones for requests waiting
in a host queue and cancels already running requests. Long handlers poll their
cancellation token; cancellation cannot undo side effects or kill arbitrary
Rust code. A cancelled call never returns a successful result. Handlers run
without the bridge mutex and panics become `BackendPanic` with unwind builds.

`dispatch_json` checks raw payload size before parsing. `dispatch` accepts a
preparsed request and enforces the serialized request size too. Default limits
are 1 MiB per request/result and 64 pending calls/cancellation tombstones per
session. Backend worker queues need their own bound.

## Streams

`CallContext::stream<T>(capacity)` creates a bounded ordered stream tied to the
call. It permits at most 16 streams per call. A full queue returns `QueueFull`;
the producer deliberately retries after the consumer drains. Bounds count items,
so transport adapters must also limit serialized item size. Completion has a
separate slot and follows all queued values. The host must drain streams before
delivering call completion. Returning from the handler seals unfinished streams
and retained contexts/producers cannot append after the call ends. A receiver
that leaves early cancels the call. Navigation/closure cancels buffered streams,
including streams whose handler already returned.

`bounded_stream` is also usable independently. An unfinished standalone producer
reports an explicit error on drop. Dropping a fully drained receiver does not
cancel a successfully completed operation.

## Events and lifecycle

`EventBus` is owned mutably by the host event loop. Register schemas, open each
window, then register subscriptions. Events emitted before `mark_ready` queue in
a per-window FIFO. Listener matching occurs at delivery: unsubscribing before
readiness prevents that queued delivery. Once subscriptions are consumed before
invocation. A full destination rejects a whole broadcast before any delivery.
Navigation removes old listeners/queued events; close removes the window.
Callbacks run in registration order and should remain short. Callback panics do
not starve other recipients and are reported after delivery as `BackendPanic`.

`lifecycle::Lifecycle` records `Created → Starting → Ready → Closing → Closed`.
Late ready callbacks run immediately. All close-request listeners are consulted;
any cancellation preserves the open state. `destroy` bypasses cancellation.
This state machine is a contract for an OS adapter, not an event loop itself.

## Assets and schemas

`assets::AssetStore` serves owned in-memory bytes with explicit MIME types. It
repeatedly decodes paths with a finite limit, rejects traversal/backslashes/NUL
and malformed escapes, and only falls back to the entry page for extensionless
missing paths. Missing `.js`, `.css`, images, and dotfiles remain errors.

`SchemaRegistry::typescript()` deterministically emits typed method/event
interfaces. It does not generate or silently install a transport implementation.
Schemas are explicit and checked on arguments and results in addition to serde.
`Integer` is limited to ±(2^53−1); encode larger Rust integers as strings.
`Number` has JavaScript number semantics. Nullable values and optional object
properties are distinct; an optional field does not automatically accept null.
Use `Field::optional(Schema::nullable(...))` when both absence and null are valid.
Object schemas reject unknown fields unless `additional` is explicitly true.

## Checks

From the workspace root:

```sh
cargo test -p lumendesk-core
cargo clippy -p lumendesk-core --all-targets -- -D warnings
sh crates/lumendesk-core/tests/typescript/check.sh
```

The last command requires TypeScript (`tsc`) on PATH, or an executable path in
`TSC`. It runs a strict positive fixture with compiler-checked expected errors,
then verifies a separate invalid fixture fails compilation. The Rust integration
test verifies the checked-in client matches fresh deterministic schema output.
