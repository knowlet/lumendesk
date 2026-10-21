# Acceptance traceability

Status date: 2026-10-19. The independently prepared functional specification has
18 proposed black-box cases. This document keeps their original scope: a passing
unit test is not silently promoted to a passing OS acceptance test.

PR #1 baseline `a90f4c2` passed Linux, macOS and Windows CI on 2026-10-19
([run 39188660594](https://github.com/knowlet/lumendesk/actions/runs/39188660594)).
Those jobs built and linked desktop code and ran display-free tests; they did
not run the GUI, Wry IPC, engine iframe/navigation boundaries or desktop
integration cases. The independent PR review adds targeted regressions and
retains all of those engine/manual gaps. Check the latest commit's CI separately.

Legend: **PASS** means the stated scoped acceptance was exercised; **PARTIAL**
means implemented/tested contracts or build evidence exist but the full case is
not established; **NOT IMPLEMENTED** and **NOT RUN** are explicit gaps.

| Case | Status | Evidence and remaining work |
|---|---|---|
| A01 lifecycle | PARTIAL | Core readiness-once, late listener and ordered lifecycle tests pass. Real adapter readiness/quit integration is not validated. |
| A02 close | PARTIAL | Core cancellable close contract tests pass. Web host has `on_close`; examples can reject first close. Actual window persistence/disappearance not run. |
| A03 multiwindow | PARTIAL | Core session isolation and closing-one-window tests pass; `web_multiwindow` compiles. Multiple OS windows not run. Mixed native+web windows in one process are not implemented. |
| A04 native interaction | PARTIAL | Shared actual egui view receives synthetic text "Orion", pointer click and keyboard Space; model/output agree. Disabled button does not activate. OS input/rendering not run. |
| A05 focus/accessibility | PARTIAL | Headless Tab order moves name → checkbox → button; Space toggles/activates. Accessible text labels are authored. Screen-reader names, icon-only control, IME and host focus not verified. |
| A06 web call | PARTIAL | Typed Rust dispatch and JavaScript runtime tests cover Unicode/nested JSON, malformed input, missing method, backend error, panic and malformed reply. Actual Wry IPC round trip not run. |
| A07 authorization | PARTIAL | Core exact-origin, spoof/lookalike, subframe flag, token/replay and navigation cancellation tests pass. Runtime top-level guard tested with mocks. Real iframe isolation is unverified; Linux Wry lacks frame-authenticating IPC URL metadata. See SECURITY.md. |
| A08 events | PARTIAL | Core targeted/broadcast/once/unsubscribe/readiness contracts, host schema/queue admission, and JS listener semantics pass. Real cross-window frontend delivery not run. |
| A09 streams | PARTIAL | Rust registration/queues and frontend async iterators are wired through a pull-credit host. Shared Rust/JS wire fixtures, slow consumer, terminal ordering, cancellation, host timeout and resource caps pass. Actual OS-webview streaming not run. |
| A10 assets | PARTIAL | Embedded HTML/CSS/JS/SVG and route fallback tested; encoded traversal and missing binary resources rejected. Release web executable links embedded assets. Offline release page loading not run. |
| A11 generated client | PASS | Deterministic generator tests pass. Strict TypeScript 7.0.2 accepts valid calls/events and rejects invalid fixtures; optional/null/safe integer schemas are tested. TypeScript cannot statically enforce the safe-integer numeric range; Rust validates it. |
| A12 CLI safety | PASS | Native and web scaffolds in empty directories, populated/symlink refusal, diagnostics, unknown flags/config/commands, real builds and symlink/hard-link output protection tested. Both generated SDK projects compile into real executables. |
| A13 rebuild | PASS, CLI process scope | Live process fixture survives a deliberate compilation failure, then replacement binds the old process's TCP port only after kill/wait. This uses a std-only app, not graphical OS resource tests. Restart is forced; graceful app shutdown is not implemented. |
| A14 geometry | NOT IMPLEMENTED | Initial dimensions validated; persisted geometry and monitor reconciliation not implemented. |
| A15 package | PARTIAL | Portable source/binary archives have deterministic manifests, SHA-256 inventories, dependency/license notices and path/symlink safeguards. Relocated source rebuild and display-free binary execution are tested. No installer/app bundle, clean-VM GUI install, identity/icon packaging or uninstall test. |
| A16 runtime separation | PARTIAL | Dev app identity/data environment separation is tested; production bridge only accepts embedded app origin and inspector is configured off. Real engine inspector/data behavior not run; no dev-server loader exists. |
| A17 native independence | PARTIAL | Native-only generated project builds without webview feature or frontend tools. Native shared view tests pass. Visible native rendering and OS input remain blocked. |
| A18 performance | NOT RUN | No credible startup/idle/RSS measurement without a running GUI. Release executable sizes are recorded as raw file size only, not runtime performance or parity claims. |

## Test layers

- Core Rust tests exercise independent contracts with no windows.
- SDK tests use the same native `form_ui` as the desktop app, with synthetic egui
  input; web host tests validate assets, exact origins and bounded event queues.
- CLI integration tests execute processes and compile real fixture applications.
- Generated native and web projects are separately built in development/release
  profiles using the actual SDK and embedded assets.
- Node tests use a VM mock of the DOM/IPC boundary, including delayed scripts
  against a replacement session and exact JSON data preservation. They are not
  browser or Wry tests.
- TypeScript compiler fixtures check generated declarations with strict settings.
- GTK/WebKit C smoke binaries compile/link, but display creation fails with EPERM.

## Deferred milestone work

A shared native+web event loop and state demo, persisted
window geometry, desktop integrations, single-instance/deep links, native installers,
updater/signing, and three-OS GUI verification remain. This source bundle does not
claim completion of the full public toolkit scope or clean-room legal clearance.

## Evidence

See `evidence/` for the original local test summaries, compiler diagnostics and
build/environment records, and the dated platform/redistribution review for
subsequent evidence. The original GUI socket restriction was respected; those
historical logs do not describe a completed engine test or the current PR state.
