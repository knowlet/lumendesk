# Native X11 acceptance and WebKit runtime boundary

Date: 2026-10-25. Repository: `knowlet/lumendesk` (1416312729).
Baseline/main: `b0ef9ccfa2d8155425b8cf62d62b5781c3242fe2`.

This session runs in Codex Cloud on Debian 13.6, Linux x86_64. It does not
use a user's computer. The checkout was clean; neither root/repository
`AGENTS.md` nor checkout `.agents/skills` existed. `/workspace/.agents` was
empty. The packaging and no-read fixes already on main were left alone.

## Environment and evidence layers

- Rust 1.99.0, Cargo 1.99.0, Node 24.19.0.
- GTK 3.24.49; WebKitGTK/JavaScriptCore 2.54.0 development libraries.
- eframe/egui 0.36.2, Wry 0.57.0, Tao 0.35.3, as pinned by `Cargo.lock`.
- Xvfb 21.1.16, Openbox 3.6.1, 1280×900×24 X11 desktop, 800×600 native app.
- Normal Unix display sockets, per-display Xauthority authentication and
  `-nolisten tcp`. No access-control override or sandbox disabling.
- Rust and Debian packages were installed/extracted only under
  `/tmp/lumendesk-tools`; no system package installation, maintainer scripts,
  security/permission/certificate changes, release, deployment or merge.
  APT verified official Debian signed metadata and package hashes. The
  sysroot reuses host compiler/libc libraries and is not redistributable.
  [Package versions and hashes](debian-packages.json) preserve the 132-package inventory.

The native acceptance uses **real eframe windows and X11 pointer/key input**,
with screenshots and the existing application's smoke-state JSON. It does not
inject egui events or replace the application. Unit tests below are a separate,
display-free regression layer.

## Confirmed defect and minimal correction

With `LUMENDESK_CANCEL_FIRST_CLOSE=1` and **no smoke timer**, minimize the
native window and send a normal window-manager close request. On main the
process exits and the window disappears on the first request, without logging
`first close cancelled`. See [before result](minimized-close-before.json).

`FormApp` handled close cancellation only in `App::ui`. eframe intentionally
skips painting while minimized/occluded and calls only `App::logic`, so that
handler never ran. The correction moves lifecycle processing to `App::logic`
and leaves rendering in `ui`. No public API or dependency changes are needed.

The implementation decision was checked against the independent dependency,
not the original toolkit. The published eframe 0.36.2 crate identifies egui
commit `49682f8baa058bf49e011035cfbd6e825f88a5ef`; its MIT/Apache-2.0 notices
were inspected. [`App::logic`](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/eframe/src/epi.rs)
documents hidden-window callbacks, and
[`EpiIntegration::update_logic_only`](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/eframe/src/native/epi_integration.rs)
reads `CancelClose` from the resulting viewport commands. Reuse that lifecycle
contract; no dependency implementation was copied and no new framework added.

## Acceptance steps and results

The optional [X11 runner](../../../scripts/verify-native-x11.py) repeats the
following cases against the existing executable. It requires a dedicated,
authenticated X11 desktop, an EWMH window manager, `xdotool`, `wmctrl`, `xprop`,
Python 3 and ImageMagick `import` (`tesseract` is optional). Coordinates target
this example's 800×600 viewport at 96 DPI; geometry is checked, but this is not a
general GUI automation framework. Do not interact with that display during a run.

```sh
cargo build -p lumendesk --features native --example native_form --locked
python3 scripts/verify-native-x11.py all --output target/native-x11-evidence
```

For an already provisioned Xvfb, set `DISPLAY` and `XAUTHORITY`. This Cloud
session used a 1280×900×24 screen with Openbox and `Xvfb -auth <private-file>
-screen 0 1280x900x24 -nolisten tcp`. Cookies are intentionally not published.

| Case | Real X11 action and observed result |
|---|---|
| Disabled | Click the empty button; enter three spaces and click again. Final state has three spaces, no greeting, zero submissions. |
| Repeated | Type Orion, click twice; replace with `  Luna  `, Tab/Space toggle enthusiasm, Tab/Space submit. Visible greeting `Hello, Luna!`, final state agrees, submissions = 3. |
| Close | With first-close cancellation enabled and **no auto-exit timer**, send EWMH close twice. First preserves process/window and logs cancellation; second exits 0 within three seconds and removes window. |
| Minimized close | Confirm `_NET_WM_STATE_HIDDEN`, send the same two requests while minimized. After the fix, first preserves process/window; second exits 0 within three seconds and removes window. Main failed the first request. |

Results: [disabled](disabled-result.json), [repeated](repeated-result.json),
[visible close](close-result.json), [minimized close](minimized-close-result.json).
Screenshots: [disabled whitespace](disabled-whitespace.png),
[third submission](repeated-keyboard-third.png),
[visible first close cancelled](close-first-cancelled.png).

The JSON results and screenshots in this directory retain the individual commands,
exit codes, timestamps and binary hashes. After-results were generated from the
modified working tree rooted at `b0ef9cc`, **not clean main**; the recorded source
hash fixes that distinction:

```text
native.rs after SHA-256: 13d0f28d0f05f066a63ac31e14f157ce01f1ad36fee5c53559f9bc22c2cd9a05
Cargo.lock SHA-256:      2b63bc73de295dff584dbabc348672048a30470587756802bb4fb88159ffef0b
```

A preliminary runner's post-close `/proc` children read raced with process exit;
that observation was removed before rerunning the final runner. This was a test
harness failure, not an application regression. Final cleanup assertions use the
waited process result and X11 window absence. Minimized windows are kept hidden
between close requests; no screenshot attempt or reactivation disturbs that case.

## Program-boundary verification

- [Red unit regression](unit-before.txt): new logic-only close test fails on the
  old implementation with no `CancelClose`, exit 101.
- [Green native tests](unit-after.txt): both new lifecycle regressions and the
  existing input tests pass. Tests call `App::logic` and `App::ui` directly;
  hidden-path coverage does not paint a UI.
- [Full verification](full-verification.txt): `sh scripts/verify.sh`, exit 0,
  against the modified Rust source above. Formatting; 136 Rust tests plus two
  doctests (one archive test ignored by default); all-feature Clippy/all targets
  with warnings denied; all examples built and linked; 135 Node tests;
  TypeScript positive and expected-negative fixtures passed.
- [Relocated archive regression](relocated-archive.txt): separate
  `cargo test -p lumendesk-cli --test package --locked
  real_web_source_archive_builds_and_retains_sdk_test_fixtures -- --ignored --exact`,
  exit 0. This is a headless rebuild/SDK test, not a WebKit content launch.
- Independent review reran the four native tests and found no blocker.

## Web examples: execution blocked before content loads

Both actual `web_form` and `web_stream` executables were launched on the X11
desktop. They could not reach page content or IPC because the production Debian
WebKit library tries to spawn:

```text
/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/WebKitNetworkProcess
```

That host path is absent. Helpers exist in the extracted sysroot, but the
production WebKit library does not honor the developer-build relocation
variable. A standalone GTK/WebKit C probe fails at the same boundary.
This is **ENOENT**, not a sandbox denial. We did not patch runtime binaries,
intercept helper spawning, install into system paths or disable sandboxing.

[Launch results](launch-results.json) retain both initial attempts: `web_form`
timed out after 12 seconds and was terminated, while `web_stream` aborted with
SIGTRAP (Python return code -5). The unchanged-runtime
[25-second diagnostic follow-up](web_form-diagnostic-result.json) established
the same missing-helper error for `web_form`, also SIGTRAP. Compilation and
linking succeeded, but **web UI, real Wry IPC, repeated web submits, real stream
cancel/restart, web close and WebKit child cleanup were not accepted**.

Node VM runtime tests and Rust Bridge/StreamBook tests exercise the respective
program boundaries, including cancellation, pagehide, late delivery and bounded
resource bookkeeping. They do not replace the blocked engine acceptance.

## Remaining scope

ASCII keyboard/mouse and X11 window-manager behavior are this run's scope.
The extracted environment reports a missing `C.UTF-8` Compose file. Unicode OS
input/IME, screen readers, Wayland, hardware GPU drivers, macOS/Windows GUI,
multiwindow behavior and native/WebView integration remain unverified. Process
exit and window disappearance establish only that native application's observed
cleanup, not a driver-level leak audit or WebKit subprocess cleanup.
