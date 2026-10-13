# Pull-driven frontend streams

Streaming methods are registered separately from one-shot calls. `StreamMethodSchema`
describes arguments, item type and terminal-result type. The Rust handler receives
a `StreamSink<T>`; `send` fails with QueueFull, while `send_wait` waits for bounded
capacity and observes cooperative cancellation. No entire stream is collected.

The host admits ordinary calls and streaming starts through one ordered worker
per window, keeping the shared request-ID namespace ordered. Streaming producers
run separately so waiting for a slow consumer never blocks ordinary calls.

JavaScript usage:

```js
const abort = new AbortController();
const progress = window.lumen.stream('progress', {count: 36}, abort.signal);
try {
  for await (const step of progress) {
    output.textContent = step.label;
  }
  console.log('Completed steps:', await progress.result);
} catch (error) {
  console.log('Stopped:', error.message);
}
// To stop early: abort.abort(), or break from the for-await loop.
```

Generated `Client.stream` types distinguish item and final-result types. Streaming
methods cannot be invoked through `call`, or vice versa. The iterator also accepts
an optional `{signal, timeoutMs}` runtime options object. The generated typed API
exposes the standard AbortSignal form.

## Flow control and lifetime

- Creating an iterator registers work but does not grant an item credit.
- Each `next()` grants exactly one credit. Concurrent reads on one iterator fail
  explicitly rather than creating an unbounded pending-read queue.
- One host value settles one read. The next read after the final item receives
  completion, and `.result` resolves with the typed final result.
- Backend errors terminate after previously queued items. Cancellation discards
  buffered work and rejects pending reads/result.
- `return`, `throw`, AbortSignal, pagehide, navigation and window close cancel.
- Frontend limit: 16 live streams and 32 combined active streams/calls.
- Host limit: 64 streams total, 16/window, 16 windows, 128 normal calls, and bounded
  incoming/worker queues. A cancelled backend that ignores cancellation keeps its
  thread permit; cancellation never permits unlimited replacement workers.
- Default frontend idle/read timeout is 30 seconds. The host ceiling is 60 seconds,
  with an explicit terminal error even when a longer frontend timeout was requested.
- Awaiting `.result` without consuming items does not drain them. It backpressures
  the producer and can time out. Drain first, then await the terminal result.

## Example and verification

```sh
cargo run -p lumendesk --example web_stream --features webview
LUMENDESK_PRINT_TYPES=1 cargo run -p lumendesk --example web_stream --features webview
cargo test -p lumendesk-core --test frontend_streams
cargo test -p lumendesk --all-features web_streams
node --test tests/runtime.test.mjs
```

The shared `crates/lumendesk/tests/stream-frames.json` is asserted by Rust host tests and
consumed verbatim by JavaScript tests. This catches mismatched transport shape.
Tests exercise a slow consumer, cancellation while full, terminal ordering,
malformed frames, registration errors, timeouts, session invalidation and caps.
These checks do not substitute for running a real OS webview; that remains blocked
by this environment's display-socket restriction.
