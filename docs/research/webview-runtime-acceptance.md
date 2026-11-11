# Real WebView acceptance scope (2026-11-04)

The next milestone is evidence for the existing Wry adapter, not a new UI
framework or a Tauri/GPUI migration. Core and Node mock passes do not establish
real engine IPC, navigation or cancellation behavior.

The existing dependency Wry 0.57.0 is independently developed and MIT/Apache-2.0.
Its [builder and platform API](https://github.com/tauri-apps/wry/blob/792d0359ba6501a4fc360ece17de2ae42329a47c/src/lib.rs)
requires a real event loop, uses `build_gtk` for Linux, invokes IPC handlers with
engine request metadata and reports page-load lifecycle events. These APIs are
already used by LumenDesk. This work uses that dependency directly; it does not
copy Wry examples or consult the original toolkit's implementation.

Decision: add an application-level acceptance fixture using the public bridge
and `WebApplication` APIs, plus a bounded X11 process runner. Keep production
CSP, sessions, navigation policy and WebKit sandbox unchanged. CI already
installs standard GTK/WebKit runtime packages; add Xvfb/Openbox for this test.
The assistant cloud's missing system-visible WebKit helpers are not worked
around by changing system paths or disabling sandboxing.

The fixture must prove Unicode repeated IPC, schema rejection, ordered stream
completion, cancellation/restart, bounded slow-consumer behavior, same-origin
navigation cancelling old work, external navigation rejection, frame CSP denial,
and window-close cancellation of a live producer. A timer, process exit alone,
or a DOM/IPC mock cannot report success. Native input, macOS/Windows GUI, IME,
accessibility, GPU performance and exhaustive hostile-frame coverage remain
separate work. Compare other adapters only after this baseline is credible.

## Reproduce

Use standard distro GTK3/WebKitGTK 4.1 development/runtime packages, Xvfb,
Xauth, Openbox, wmctrl and D-Bus. No sandbox override is supported.

```sh
cargo build -p lumendesk --all-features --example web_acceptance --locked
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests -p 'webview_runner_test.py' -v
dbus-run-session -- xvfb-run -a sh -eu -c '
  openbox >/tmp/lumendesk-openbox.log 2>&1 &
  wm_pid=$!
  trap "kill $wm_pid 2>/dev/null || true" EXIT
  python3 scripts/verify-webview-x11.py --output /tmp/lumendesk-webview.log
'
```

The runner requires the real application to complete all assertions, then sends
normal WM close to its PID-owned window and requires both clean exit and the
post-close cancellation marker. Timeouts, smoke timers, early PASS or missing
assertions fail. Python subprocess fixtures test this handshake; they are not
WebKit tests. The real engine fixture covers CSP frame denial, not arbitrary
hostile iframe IPC. The untrusted navigation target is local `about:blank` to
avoid external network/DNS dependence. Event delivery includes quotes,
backslashes, Unicode separators and HTML-looking text.

## Defect found by the first real-engine run

[CI run 41784512294](https://github.com/knowlet/lumendesk/actions/runs/41784512294)
compiled on all three platforms and passed the existing suites, but the first
Linux runtime attempt timed out before any bridge assertion. GTK reported that
`GtkApplicationWindow` already contained a `GtkBox` and could not accept a second
child. Tao creates that box by default; `WindowExtUnix::default_vbox` exposes it.
The Linux adapter must pass the existing box to Wry `build_gtk`, rather than
trying to insert a WebView as another direct child of the window. The new
acceptance fixture is the regression test; no sandbox or CSP policy is relaxed.

The lifecycle trace in [run 41786420674](https://github.com/knowlet/lumendesk/actions/runs/41786420674)
then confirmed the page, injected runtime and transport were loaded, but RPC
progress stalled while GTK was idle. The exact locked
[Tao 0.35.3 Linux event loop](https://github.com/tauri-apps/tao/blob/tao-v0.35.3/src/platform_impl/linux/event_loop.rs)
checks `WaitUntil` before a blocking GTK iteration without arming a wake timer.
IPC had only queued an mpsc command. The adapter now sends a coalesced proxy wake
on accepted IPC and uses a stoppable 30ms Linux wake ticker only while streams are live.
Rust events wake the loop directly; idle applications park without periodic work.
At most one wake is pending; teardown unparks and joins the ticker. This preserves
the existing 30ms polling intent without a busy `ControlFlow::Poll` loop.


## Review follow-up

The initial fixed ticker preserved the prior 30ms polling intent but needlessly
woke idle windows. It now parks with no active streams (except an explicitly
requested timed smoke run); IPC and Rust events request their own coalesced wake.
A display-free test observes no ticks before activation and deterministic thread
shutdown. The engine fixture stages a probe payload, returns from `emit-probe`,
then delivers it only after the frontend acknowledges with `deliver-probe`.
That proves payload fidelity without claiming the UI entered an idle wait before
delivery. Deterministic isolated idle-event wake acceptance remains future work;
the helper tests separately verify parking, reactivation, and wake coalescing.

The runner subprocess audit has three boundaries: `wmctrl -lp` uses a trusted
PATH executable with fixed argv; the acceptance binary is a caller-selected,
trusted local executable, resolved as one literal argv item; `wmctrl -ic` uses
only a PID-matched, validated unsigned 32-bit hexadecimal window ID. All calls
use `shell=False`. Tests execute a filename with shell metacharacters literally,
reject malformed/signed/option-like IDs, exclude unrelated PIDs, and require the
fake WM to receive the exact close option and expected ID before signaling.
These are command-construction checks, not a sandbox for untrusted executables.

The runner also bounds pipe draining to 200ms after the application exits:
WebKit helpers may inherit stdout/stderr, so helper EOF is not application
completion. An actual inherited-pipe helper reproduces the old false close
timeout; tests now require success only with exit zero and the full handshake,
and separately reject missing PASS and nonzero exit. Owned-group cleanup stops
the helper in all three cases. The wake tests additionally cover active to idle
to active transitions, with no periodic tick during the idle interval.
