# Security model

This milestone is a prototype SDK, not a hardened production browser container.
No OS-webview security claim has been validated against a running engine here.

## Trust boundary

The privileged host supports only application-authored embedded assets. Arbitrary untrusted pages and hard isolation of untrusted iframes are unsupported. Only application-authored embedded assets are trusted. Do not embed untrusted
HTML, interpolate unescaped user content into scripts, or register general shell
execution, filesystem, credential, or process-control services. Backend methods
are explicit allowlisted registrations; schemas validate input and output in
addition to Rust serde types. Unknown fields can be denied by object schemas and
Rust `deny_unknown_fields`. Integers are limited to JavaScript's safe integer
range; nullable values and optional properties have different schema types.

Bridge sessions have 256 bits of OS randomness and are isolated by window and
parsed exact origin. Requests have increasing per-session IDs. Navigation/close
invalidate sessions and cancel active work; UI delivery checks the session again
so a response cannot be installed into a replacement page. Cancellation is
cooperative: long Rust handlers must call `check_cancelled`. It does not roll back
side effects already performed. Pre-cancel tombstones prevent timed-out requests
queued by the host from subsequently executing.

The host accepts only its embedded custom origin. External navigation, new-window
requests, downloads and permission requests are denied. Inspector access is off.
Production does not trust a development server, and this milestone has no
URL-loading convenience mode. The JavaScript runtime is injected after page load
with a private session closure and does not install in an ordinary browser or
when `window.top !== window`.

## Frame limitation

Wry on Linux reports the main-page URL for iframe IPC. The adapter therefore
cannot independently attest frame identity from that URL. It uses a private
session token and an embedded-page CSP that denies all frames. Runtime unit tests
prove the top-level installation guard, **not** engine-level iframe isolation.
Same-origin frames are not a supported isolation boundary: do not create them or
allow application code to expose parent runtime functions to other realms.
External content must never be included in trusted application assets. A07
remains partial until actual engine tests cover frames and navigation.

The CSP permits inline scripts/styles for self-contained application-authored
HTML templates, denies frames, child contexts, workers, framing ancestors, network connections and external scripts, and sets
`object-src`, `base-uri`, and `form-action` restrictions. This is not an HTML
sanitizer. A malicious trusted script has the app's bridge privileges.

## Bounds and lifetime

Wire messages are capped at 1 MiB. The frontend permits 32 pending calls. Each
web window has a bounded, ordered 32-job worker queue. Timeout requests cancellation
and drops the frontend promise. There is one backend worker per web window;
handlers must return cooperatively and must not block forever. Rust cannot safely
force-stop arbitrary user code.

Host event schemas are validated before enqueue; per-window queues hold at most
64 events. Broadcast admission is all-or-nothing, closed targets fail, and full
queues return errors. Events queue before readiness; navigation discards the old
page's queued events and listeners. Event callbacks should remain short.

Rust streams are bounded, preserve FIFO order, cancel when the consumer drops,
and terminate when their originating call returns. They are not wired to the
JavaScript host transport in this milestone. The core stream contract is not a
claim of browser-stream support.

Embedded assets are served from an in-memory allowlist, never from the filesystem.
Traversal (including repeated percent encoding), NULs and backslashes are rejected.
Only missing extensionless routes may fall back to the HTML entry point; missing
CSS/images return errors. OS/network errors are not hidden as successful work.

## Reporting

Do not publish credentials or sensitive application data in bug reports. Include
a minimal reproduction, platform/engine versions, expected behavior, and the
relevant authorization or lifecycle boundary. No disclosure inbox or hosted
security service is configured for this unpublished source milestone.
