# Build and platform status

## Original local development environment

- Linux x86_64, Debian 13 (trixie)
- Rust 1.99.0 / Cargo 1.99.0; declared minimum 1.95 due to egui 0.36
- GTK 3.24.49; WebKitGTK/JavaScriptCore 2.52.6; libsoup 3.6.5
- Native renderer: egui/eframe 0.36.2, OpenGL via glow
- Webview host: Wry 0.57.0, Tao 0.35.3
- Node 24 for standalone runtime tests only

GTK/WebKit headers and missing libraries were extracted from signed official
Debian packages into a workspace-local sysroot. No system installation was done.
This sysroot reuses installed host libraries and is not a redistributable runtime.
See [environment evidence](evidence/GUI_ENVIRONMENT.md).

## Status by platform as of 2026-10-19

| Platform | Core tests | Native compiled | Webview compiled | Real windows/input |
|---|---|---|---|---|
| Debian 13 x86_64 | Verified | Verified | Verified | Blocked by sandbox |
| Ubuntu 24.04 x86_64 CI | Passed | Built/linked | Built/linked | Not run |
| Linux Wayland | Core is platform-neutral | Adapter selected | GTK adapter selected | Not run |
| macOS 15 CI | Passed | Built/linked | Built/linked | Not run |
| Windows 2022 CI | Passed | Built/linked | Built/linked | Not run |

The hosted results above refer specifically to PR #1 head
`a90f4c2d247c8b0cedd337ac911c12903f6faca8`,
[CI run 39188660594](https://github.com/knowlet/lumendesk/actions/runs/39188660594),
completed 2026-10-19. Later review fixes require their own successful CI run.
The independent review environment uses Ubuntu 24.04, Rust 1.99.0 and installed
GTK/WebKit development packages for compilation and display-free tests only.

Dependency support is not equivalent to validated SDK support. Cross-compilation,
IME, screen readers, DPI/multimonitor handling, focus/activation, GPU drivers,
clipboard integration and application packaging all need host-specific testing.

## Run on a normal Linux desktop

Install Rust from rustup's official distribution and install your distribution's
GTK 3/WebKitGTK 4.1 development packages plus a C compiler and pkg-config. Then:

```sh
cargo run -p lumendesk --example native_form --features native
cargo run -p lumendesk --example web_form --features webview
```

`LUMENDESK_SMOKE_MS=2000` requests an automatic exit for a real-window smoke test.
`LUMENDESK_CANCEL_FIRST_CLOSE=1` lets the reference apps reject the first close
request. This switch is a test aid; web application code can install `on_close`.
Native application code can handle close events through egui viewport commands.

## Why graphical verification is pending

A virtual X11 display was attempted, but this sandbox explicitly denies AF_UNIX
socket creation with EPERM, including escalated execution. Xvfb cannot start its
normal local display. We did not switch to a TCP display, disable sandboxing, or
use a user's device. This also leaves WebKit child-process startup unverified.
Headless egui input and JavaScript VM tests do not replace OS input tests.

## Hosted cross-platform compile checks

The PR CI matrix includes standard `macos-15` (Apple Silicon) and
`windows-2022` (x86_64/MSVC) GitHub-hosted runners. Each runs the display-free core
and SDK library tests, builds the CLI, and compiles/links all native and webview
examples with Rust 1.99.0. It never launches an application window, requests
signing credentials, or publishes an artifact. A configured job is not a verified
result: consult the current commit's Actions run before claiming a platform pass.
The table separates the original Debian baseline from the observed hosted CI
results. The review revision also runs CLI library and portable-archive tests on
macOS/Windows, including Windows hardlink/reparse-point regressions. This expands
filesystem contract coverage and does not execute a GUI acceptance case.

PR-title validation is a separate small workflow. Bot edits to a PR description
therefore do not cancel and restart expensive platform builds.
