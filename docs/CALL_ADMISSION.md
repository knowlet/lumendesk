# Two-phase ordinary-call admission

This Core-only API is independent of native PR #3 and WebKit runtime PR #4.
See PR #5 for current-revision verification results; no new GUI/runtime or
performance acceptance is claimed by this document.

## Motivation

Frontend request IDs describe admission order, not worker completion order.
`Bridge::begin_stream` reserves a stream before the host schedules its producer.
Previously, ordinary `Bridge::dispatch` could only reserve its request when the
handler was executed. Passing ordinary requests straight to parallel workers
could therefore reject a valid earlier request as `ReusedRequestId` when a later
worker started first. Synchronous dispatch still requires ordered admission;
the new API supplies a separate scheduling boundary rather than relaxing replay
protection.

`Bridge::begin_call` / `Bridge::begin_call_json` return an owned, one-shot
`CallJob`. The trusted host admits jobs in transport order, then runs them on its
own bounded workers. No framework, executor, thread pool or UI renderer becomes
a dependency of `lumendesk-core`.

## Contract

- Admission checks authorization, request size/shape, method registration,
  monotonic request ID, pre-cancellation and the shared per-session budget.
  It does not execute application code. Method argument/result schemas continue
  to be checked on execution, as with the existing stream API.
- Calls and frontend streams share the same request-ID namespace and budget.
  Worker execution and completion may occur in a different order after admission.
- `CallJob::run(self)` executes at most once and returns the existing
  `CallResponse`. `dispatch` uses the same implementation synchronously.
- Dropping an unrun job cancels its token and releases its reservation without
  executing the handler. Its request ID remains consumed. There is no implicit
  response transport: a host rejecting an admitted job must send its own bounded
  error response, after rechecking the original authorization.
- Cancelling a running job does not free its reservation until execution exits.
  Cancellation is cooperative; side effects are not rolled back, and arbitrary
  Rust handlers cannot be force-stopped safely.
- Navigation, replacement sessions and close cancel old jobs. Releasing an old
  job must not remove a reservation from a newer session that reused the same ID.
- Handler panic containment, payload limits, schema checks and call-scoped stream
  completion retain their existing contracts. Panic containment is limited to
  unwinding panics, not process aborts.
- The host must retain the job's original `Authorization` and call
  `Bridge::authorize` immediately before response delivery. Completion of a
  worker does not authenticate its eventual UI destination.

## Host responsibilities

Admit calls serially in the authenticated transport's receive order, before
submitting them to a worker pool. Do not assume parallel calls preserve side
effect order: handlers that mutate shared state still need application-specific
serialization. Keep host-wide worker/queue limits across windows and session
replacement; the per-session Core budget is not a process-wide resource bound.
Never trust a frontend-supplied origin, frame flag, window ID or authorization
object. This API neither authenticates a WebView frame nor adds iframe support.

No host adapter is migrated to parallel execution in this change. In particular,
the existing Wry adapter retains its ordered worker policy. Future GPUI/Tauri
adapters remain future work, not claimed integrations.

## Verification

The public-API integration suite `crates/lumendesk-core/tests/call_admission.rs`
covers admission without execution, all six three-job execution orders, real
worker overlap, shared call/stream budgets and IDs, drop cleanup, pre-cancellation,
cancellation during running work, navigation/session replacement, close, schema
and payload failures, panic containment, repeated use and call-scoped stream
finalization. Concurrent cases use channel handshakes with bounded waits, not
sleep-based ordering assumptions.

```sh
cargo test -p lumendesk-core --test call_admission --locked
cargo test -p lumendesk-core --locked
cargo clippy -p lumendesk-core --all-targets --locked -- -D warnings
sh scripts/verify.sh
```

These are display-free contract checks, not WebView, GUI, accessibility, IME or
performance measurements. Passing compile-only desktop jobs has no broader OS
acceptance meaning. Consult the PR's exact revision and CI outcome.

## Provenance

Independently authored from this repository's Core implementation and documented
contracts, FUNCTIONAL_SPEC.md, CONTRIBUTING.md, CLEAN_ROOM.md, and Rust
standard-library API semantics. No isolated upstream toolkit implementation,
tests, examples or assets were consulted. No new third-party dependency is added.
