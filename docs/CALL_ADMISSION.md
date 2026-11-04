# Two-phase ordinary-call admission

Status: implementation in progress on this branch; no new runtime acceptance is
claimed. This work is independent of native PR #3 and WebKit runtime PR #4.

## Motivation

Frontend request IDs describe admission order, not worker completion order.
`Bridge::begin_stream` already reserves a stream before the host schedules its
producer. Ordinary `Bridge::dispatch` instead reserves its request when the
handler is executed. Passing ordinary requests straight to parallel workers can
therefore reject a valid earlier request as `ReusedRequestId` when a later worker
starts first.

The proposed `Bridge::begin_call` / `Bridge::begin_call_json` methods return an
owned, one-shot `CallJob`. The trusted host admits jobs in transport order, then
runs them on its own bounded workers. No framework, executor, thread pool or UI
renderer becomes a dependency of `lumendesk-core`.

## Contract

- Admission checks authorization, request size/shape, method registration,
  monotonic request ID, pre-cancellation and the shared per-session budget.
  It does not execute application code. Method argument/result schemas continue
  to be checked on execution, as with the existing stream API.
- Calls and frontend streams share the same request-ID namespace and budget.
  Worker execution and completion may occur in a different order after admission.
- `CallJob::run(self)` executes at most once and returns the existing
  `CallResponse`. `dispatch` remains the synchronous convenience path.
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

## Verification plan

Display-free public-API integration tests will cover admission without execution,
reverse worker order, shared call/stream budgets and IDs, drop cleanup,
pre-cancellation, cancellation during running work, navigation/session replacement,
close, schema and payload failures, panic containment, repeated use and
call-scoped stream finalization. Use bounded channel handshakes rather than sleep
or timing-sensitive assertions for concurrent cases.

Run the existing workspace verification and current-revision CI. These checks
are not evidence of WebView, GUI, accessibility, IME or performance behavior.
No host adapter is migrated to parallel execution in this change.

## Provenance

Independently authored from this repository's Core implementation and documented
contracts, CONTRIBUTING.md, CLEAN_ROOM.md, and Rust standard-library API
semantics. No isolated upstream toolkit implementation, tests, examples or assets
were consulted. No new third-party dependency is required.
