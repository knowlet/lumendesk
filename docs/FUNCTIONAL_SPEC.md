# Independent Rust desktop toolkit: public functional specification

Research date: 2026-10-13 UTC. Intended recipient: implementation team isolated from upstream implementation source.

## Evidence and boundary

This document paraphrases public-facing documentation into functional goals. No upstream repository was cloned, implementation file opened, binary executed, account created, or code copied into this artifact. Documentation contains public example snippets; these were viewed incidentally but are not reproduced. Repository root file names were visible. Some public release notes and guides contain implementation detail; such detail is deliberately excluded from implementation requirements. This is a practical separation procedure, not certification of legal clean-room status.

Evidence labels:
- D: upstream documentation states behavior; not independently executed.
- O: directly observed metadata on public pages.
- P: independent proposed Rust behavior or acceptance criterion.
- U: unknown or not tested.

Do not claim source, API, ABI, wire-format, visual, or performance compatibility. Use independently chosen names and Rust-appropriate types. Do not acquire upstream implementation, tests, generated output, assets, or templates. Public documentation URLs below identify evidence, not permission to import code.

## Public scope baseline

O: The landing page and releases list identify v0.4.8; the latest listed release is dated October 12. A window guide footer still identifies v0.4.6, so these rolling docs are not a fully version-pinned contract. [Landing](https://mygo.egoist.dev/) · [Releases](https://github.com/egoist/mygo/releases)

D: The core product offers macOS, Linux, and Windows desktop applications using either system web rendering or a UI authored in the host language. Mixed-mode apps are in scope. It advertises small binaries, low idle usage, typed frontend integration, and shipment tooling. These are upstream claims, not measured properties of the proposed Rust product. [README](https://github.com/egoist/mygo#readme)

D: Native mode means host-language controls rendered without HTML, JavaScript, or a webview. It does not mean every control is an operating-system widget. Native UI includes layout, text, input, collections, overlays, styling, animation, accessibility and embedded web content. [Native UI](https://mygo.egoist.dev/docs/ui)

D: Web mode uses WKWebView, WebKitGTK and WebView2 on the respective desktop systems, rather than shipping a browser. The getting-started guide requires GTK 3 on Linux and WebKitGTK 4.1 for web content, and calls out WebView2 Runtime on Windows 10. Rust dependencies and OS minimums must be separately established; the upstream Go toolchain requirement is not applicable to Rust. [Getting started](https://mygo.egoist.dev/docs/getting-started)

## Documented capabilities, translated into outcomes

### Application lifetime

D: There is a blocking application event loop, readiness notification, window creation after readiness, activation and final-window notifications, cancellable quit, immediate exit, and restart. Listeners can be removed. A late readiness callback runs immediately. The default final-window behavior is exit, unless overridden. UI listeners must remain responsive while backend work runs. Single-instance behavior can forward a subsequent launch's arguments to the original process. Deep links and file-open delivery include launch-time inputs. [Application](https://mygo.egoist.dev/docs/app)

P: The Rust facade should expose explicit ownership and fallible operations, with typed handles for cross-thread requests. A test-only lifecycle implementation must be clearly named and cannot count as actual desktop support.

### Windows and pages

D: Windows have titles, device-independent geometry, resize constraints, visibility/focus states, minimize/maximize/fullscreen, decoration choices, parent/modal relationships and persistent placement. Closing is cancellable; destruction bypasses cancellation. Events include creation, visibility, focus, geometry, state transitions and first display readiness. Page-specific operations include navigation history, loading/reloading, zoom, navigation interception and load/title notifications. The documented default window is 800 by 600, visible, resizable and centered. Wayland may ignore requested positioning. [Windows](https://mygo.egoist.dev/docs/windows)

P: Reject nonsensical dimensions explicitly. Invalid/closed handles return errors, never fabricate success. Each backend must declare unsupported capabilities. Persisted geometry must not strand a window outside currently available monitors.

### Frontend loading

D: Development can load a configured server; production embeds built frontend assets. Extensionless missing paths support client-side routing through the entry page. External URLs, local files and HTML content can also load. Runtime information exposes host platform/window identity and controls for the current window. Ordinary browser tabs cannot successfully invoke the desktop runtime. Production inspector access is disabled unless explicitly enabled. [Frontend](https://mygo.egoist.dev/docs/frontend)

P: Use an independently named app origin and runtime namespace. Asset lookup must normalize paths and reject traversal, including encoded variants. Missing binary assets must not silently become HTML. Development origins must never become blanket production permissions.

### Calls, generated types, events, streaming

D: Pages call registered backend services asynchronously with JSON-compatible arguments/results. Backend failures reject calls. Generated TypeScript describes methods, structures and typed events. Calls can identify their originating window and cancel on page navigation/closure. Events can target a window or all windows, queue until page readiness, support once/unsubscribe, and fail for a closed destination. Streams deliver in order before call completion, terminate with the call, and cancel when the consumer leaves. Only trusted top-level pages may call the backend; embedded frames are barred. [Bindings](https://mygo.egoist.dev/docs/bindings)

P: Prefer explicit schema/registration in Rust over assumed runtime reflection. Document integer precision and null/optional handling. Validate method existence, payload size and argument types. Give requests unique IDs and isolate window sessions. Deny external origins by default; use exact parsed origin matching, never string-prefix matching. Navigation invalidates earlier call/session authority. Bounded queues should reject or backpressure deliberately. Specify cancellation semantics independently; do not copy transport internals.

### Native UI and testing

D: A native view is derived from application state; actions alter state and redraw. Persistent control identity matters when lists reorder. Background state changes require a safe route into the UI. Public documentation supports both inline action observation and callbacks. [Views](https://mygo.egoist.dev/docs/ui/views)

D: Public test tooling can run views without windows, find controls by text/labels, synthesize pointer and keyboard input, change size/theme/scale and examine rendered output. It can inspect focus and accessibility announcements. [Testing](https://mygo.egoist.dev/docs/ui/testing)

P: Initial native controls should be text, button, editable text, checkbox, row/column and scrolling, with keyboard focus and observable model updates. An independent headless model and a real renderer should use shared semantic tests. Headless output alone is insufficient evidence that native windows render or receive OS input.

### Desktop services

D: The public desktop API covers file/message dialogs, notification delivery, text/image/HTML clipboard, shell opening, display metadata, theme changes, power and global shortcuts. Some features are conditional: macOS notifications require a packaged app and permission; Linux depends on the desktop's notification service. [Desktop APIs](https://mygo.egoist.dev/docs/native)

P: Stage these services. The first release can include file dialogs and clipboard, while returning explicit unsupported errors for omitted features. Never expose shell execution to arbitrary frontend requests as a convenience binding.

### CLI and distribution

D: Public commands cover project creation, development/reload, build, binding generation, dependency diagnosis and version output. Native and web templates differ. Development preserves the last working process if compilation fails and isolates development app identity from production. Browser-targeted native UI exists as additional scope. [CLI](https://mygo.egoist.dev/docs/cli)

D: Shipping formats include macOS app bundles/disk images, Windows executables/installers, and Linux desktop assets, installation archives and Debian packages. Configuration supplies identity, version, icons and resources. Cross-compilation is distinct from packaging/signing: macOS signing and disk-image creation need macOS; installer creation has tool dependencies. [Distribution](https://mygo.egoist.dev/docs/distribution)

D: The updater accepts signed releases, verifies against an embedded public key and supports release hosting, delta artifacts and an update UI. This is a large optional subsystem. [Updates](https://mygo.egoist.dev/docs/updates)

P: Build commands must create runnable artifacts, not merely packaging manifests. Unsupported targets fail clearly. Signing, notarization, publishing, persistent credentials and self-update installation should be out of the first milestone. Do not use upstream CLI names, templates or icons. Rust cross-build feasibility depends on chosen dependencies and target SDKs and must be tested independently.

## Proposed coherent milestone

The first milestone is a small Rust SDK and CLI, not feature-complete parity:

1. Independently named Rust workspace with core contracts, desktop adapters, optional native renderer, optional webview adapter, bridge schema/runtime and CLI.
2. One real desktop application can open a native control window and a system-webview window. A supported-host matrix says which combinations were actually built and run.
3. A native editable form changes shared Rust state; a web form invokes a Rust service through generated TypeScript; a typed event reflects the state in both windows.
4. Cancellation, close handling, origin restrictions and resource path rules are tested before adding integrations.
5. CLI creates both project modes in empty directories, emits bindings deterministically, diagnoses native prerequisites and builds release executables with embedded assets.
6. Documentation explains installation, examples, security model, scope limits and exact test commands. All sample labels/content/assets are newly authored.

If the current environment cannot execute a graphical backend, deliver a compiled core with honest test evidence and mark desktop acceptance pending. Do not call a headless command simulation an implemented desktop framework. If only one OS is validated, market the milestone as that OS plus unverified adapters, not cross-platform parity.

## Black-box acceptance suite

These are independently proposed cases, not copied upstream tests. Status begins NOT RUN.

- A01, lifecycle: readiness fires once; a callback registered afterward completes without waiting for another startup. Running and shutdown produce an ordered, documented event log.
- A02, close: cancel the first close request, verify the actual window remains, permit a second request and verify it disappears.
- A03, multiwindow: open two distinct windows; closing one does not invalidate the other's handle or bridge.
- A04, native interaction: enter “Orion” into an editable field, activate a button by mouse and keyboard, verify visible output and Rust state agree. Disabled controls do not activate.
- A05, focus: tab through controls predictably; a label-only icon control has a discoverable accessible name. Test composition input separately on each OS.
- A06, web call: submit a Unicode string and nested JSON record, verify typed successful result; malformed arguments, missing methods and backend failures produce rejected calls without a process crash.
- A07, authorization: trusted app top-level page succeeds; an unrelated HTTPS page, a lookalike host and every embedded frame fail. Navigate a trusted page away while a call is pending and verify cancellation and session invalidation.
- A08, events: targeted delivery reaches only its destination; broadcast reaches all eligible windows; once listeners receive once; unsubscribed listeners receive nothing further. Check event-before-ready behavior against the documented local contract.
- A09, streams if included: send numbered values, verify order and completion after final value; close the consumer early and verify producer cancellation. Slow the consumer and verify bounded memory.
- A10, assets: run a release build without development server/network; load entry page, CSS and images. Verify client-side route fallback. Attempt parent-directory and encoded traversal; no external file is disclosed.
- A11, generated client: run generation twice and compare bytes; compile positive TypeScript usage and expected compile-failure fixtures. Unknown/optional/null/integer types match the declared serialization contract.
- A12, CLI safety: scaffold to an empty directory; refuse a populated destination without overwriting user files. Missing configuration, tools and unknown commands produce actionable diagnostics and nonzero status.
- A13, rebuild: introduce a compiler error; keep last working app. Repair it; run the replacement only after the old process releases its resources.
- A14, geometry: resize and restart; verify bounds restore where supported. Disconnect a saved monitor and verify visible placement. Mark compositor-enforced exceptions.
- A15, package: install onto a clean target VM and launch without developer toolchains. Verify icons, identity, resources and uninstall behavior for each claimed package type.
- A16, runtime separation: production app does not trust the development server and has no inspector unless explicitly enabled. Development data does not overwrite production data.
- A17, native independence: native-only example does not require frontend tools or initialize a webview; it still visibly renders and responds to OS input.
- A18, idle/performance: record machine, OS, renderer, build profile, app scenario, startup definition, memory methodology and idle duration. Report measurements; do not adopt upstream marketing figures as thresholds.

## Platform verification and unknowns

Required real-machine or VM checks: macOS event-loop behavior, window focus/activation, app bundle layout and WKWebView; Windows WebView2 presence, focus/geometry/DPI behavior and executable resources; Linux GTK/WebKit runtime detection, X11 and Wayland differences, multi-monitor scaling, desktop services. Native GPU rendering, fonts, IME, screen readers, keyboard layouts and mixed native/web compositing need direct visual/input validation.

Cross-compilation checks do not establish runtime correctness. Unit tests do not establish accessibility. A screenshot does not establish keyboard input. A packaging directory does not establish installability. Unsupported environments need recorded skip reasons, not passing results.

U: No upstream functionality was executed. Exact undocumented defaults, wire protocol, renderer fidelity, minimum supported OS versions, plugin completeness and claimed memory/binary figures remain unverified. Latest release notes add advanced editor/diff functionality, demonstrating that full parity extends beyond this MVP; omit those features explicitly. [Release notes](https://github.com/egoist/mygo/releases)

## License observation

O: The repository license page is MIT, attributed to MyGo contributors, 2026. It permits broad use subject to preserving the required notice in copies/substantial portions and disclaims warranty. No upstream code is included here. Dependency licenses and any later copied material require their own review. This observation is not legal advice or a claim of legal clearance. [License](https://github.com/egoist/mygo/blob/main/LICENSE)

## Access provenance

Read-only public web retrieval only, 2026-10-13 UTC:
- https://github.com/egoist/mygo — root and rendered README; repository listing visible, source files not opened.
- https://github.com/egoist/mygo/blob/main/LICENSE — license text.
- https://github.com/egoist/mygo/releases — release metadata/notes; no commit/diff/PR links opened.
- https://mygo.egoist.dev/ — public landing page.
- https://mygo.egoist.dev/docs/getting-started
- https://mygo.egoist.dev/docs/frontend
- https://mygo.egoist.dev/docs/ui
- https://mygo.egoist.dev/docs/bindings
- https://mygo.egoist.dev/docs/native
- https://mygo.egoist.dev/docs/distribution
- https://mygo.egoist.dev/docs/app
- https://mygo.egoist.dev/docs/windows
- https://mygo.egoist.dev/docs/cli
- https://mygo.egoist.dev/docs/ui/views
- https://mygo.egoist.dev/docs/ui/testing
- https://mygo.egoist.dev/docs/updates

The docs-directory listing at https://github.com/egoist/mygo/tree/main/docs failed to load. Some direct documentation opens initially failed but succeeded through verified site links or cached page references. Architecture/rendering/internal-source links were deliberately not opened. No public source-code API listing was needed.
