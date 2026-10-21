# LumenDesk

An independently authored Rust desktop toolkit milestone with GPU-drawn native
controls, system-webview windows, a typed JSON bridge, and project tooling.

**This is a working first milestone with incomplete desktop acceptance.** CI at
PR #1 commit `a90f4c2` passed Linux, macOS and Windows builds, linking and
display-free contracts. Actual window rendering, OS input, Wry IPC and WebView
security boundaries have not been verified. Simultaneous native and webview
windows in one process are not implemented yet. See the dated evidence in
[platform notes](docs/PLATFORMS.md) and the current commit's Actions results.

## Start here

Rust 1.95+ and a C compiler are required; this build was checked with Rust 1.99.
On Debian/Ubuntu, system-webview builds need GTK 3 and WebKitGTK 4.1 development
packages. A normal host setup is:

```sh
sudo apt install build-essential pkg-config libgtk-3-dev libwebkit2gtk-4.1-dev
cargo test --workspace
cargo run -p lumendesk --example native_form --features native
cargo run -p lumendesk --example web_form --features webview
cargo run -p lumendesk --example web_stream --features webview
```

The native feature uses egui/eframe's OpenGL renderer and does not initialize a
webview or need Node. The webview feature uses Wry/Tao with the OS web engine.
Linux runtime needs an actual X11/Wayland desktop and matching installed shared
libraries. Build flags do not prove runtime support. See [platform notes](docs/PLATFORMS.md).

## Create an app

```sh
cargo run -p lumendesk-cli -- new hello-native --mode native
cargo run -p lumendesk-cli -- new hello-web --mode web
cargo run -p lumendesk-cli -- doctor --project hello-web
cargo run -p lumendesk-cli -- bindings --project hello-web
cargo run -p lumendesk-cli -- build --project hello-web
cargo run -p lumendesk-cli -- dev --project hello-web
```

Scaffolds reference the SDK's local absolute path. Move/update that path with your
project; no crates.io publication is claimed. `new` refuses populated directories.
`build` actually invokes Cargo and reports its executable. `dev` retains the last
working child on compilation failure, then stops it before starting its successor.
See [CLI documentation](crates/lumendesk-cli/README.md).

## Library surface

- `lumendesk-core`: display-free contracts; lifecycle, schemas, bridge, assets,
  typed events, call-bound bounded streams, cancellation and security tests
- `lumendesk`: validated window options; opt-in `native` and `webview` hosts
- `lumendesk::native::run_native`: run your own `eframe::App`; egui is re-exported
- `lumendesk::webview::WebApplication`: embed your assets, register your bridge,
  open multiple web windows, validate/broadcast events, cancel close requests
- `lumendesk-cli`: `new`, `doctor`, `bindings`, `build`, `dev`, `package`, version and help

The reference forms share the same `FormState` definition, but run as separate
applications. The JavaScript runtime offers promise calls, on/once/unsubscribe listeners, and
pull-driven typed async-iterator streams. Stream queues are bounded and cancellable;
terminal results follow the final consumed item. See [streaming](docs/STREAMING.md).
The Rust/JavaScript wire fixture is shared by both test suites; actual OS-webview
stream execution remains unverified.

## Portable archives

The CLI can create source or host-binary archives with checksums and dependency
notices. Source archives bundle the local SDK with relative dependency paths.
Registry crates are not vendored. Binary archives require the target desktop
runtime libraries; these are not signed installers. See the CLI guide for
`package --kind source|binary` and safety limits.
The [redistribution review](docs/REDISTRIBUTION.md) distinguishes dependency
notice inventory, target-specific material and outstanding release obligations.
Creating an archive does not grant approval to redistribute it.

## Verification

```sh
cargo test --workspace --all-features
cargo clippy --workspace --all-features --all-targets -- -D warnings
cargo fmt --all -- --check
node --test tests/*.test.mjs
npm ci --ignore-scripts
npm run test:types
```

All-feature checks require Linux GUI development libraries even though unit tests
do not open windows. Node is optional and used only for JavaScript runtime tests,
not for native apps. [Acceptance traceability](docs/ACCEPTANCE.md) records the 18
functional cases, evidence, and gaps. [Security model](docs/SECURITY.md) explains
what the bridge guarantees and what application authors must keep trusted.

The [2026-10-19 independent PR review](docs/PR1-INDEPENDENT-REVIEW.md) records
reproduced defects, regression fixes, package inspections and the remaining
maintainer approval and real WebView/GUI acceptance conditions.

## Deliberate gaps

Mixed native/web event-loop integration, full desktop service APIs, persisted
monitor-aware geometry, single-instance/deep links, hot reload without forced
termination, installers, signing/notarization, updating, full widget
parity, and platform-level accessibility/IME verification
remain future work. Unsupported capabilities are not represented as successful.
No compatibility or performance parity is claimed.

## Provenance and licensing

Implementation was separated from upstream research and based on a paraphrased
functional specification. No upstream implementation, tests, generated output,
assets, or templates were read or copied by the implementation team. This is a
practical separation procedure, not legal certification. See
[clean-room record](docs/CLEAN_ROOM.md). Dependencies remain under their own
licenses. LumenDesk's newly authored code is dual-licensed MIT OR Apache-2.0.
