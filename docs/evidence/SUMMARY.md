# Verification summary

2026-10-13 UTC, Debian 13 x86_64, Rust/Cargo 1.99.0, Node 24.19.0,
TypeScript 7.0.2. GUI library versions and sandbox details: GUI_ENVIRONMENT.md.

## Passed

- `cargo test --workspace --all-features --offline`: 106 unit/integration tests
  (SDK 14, CLI 24, core 48 plus stream integration 19 and schema integration 1)
  and 2 core doctests. The platform-dependent extraction regression is ignored
  in this aggregate command and separately executed successfully.
- `cargo clippy --workspace --all-features --all-targets --offline -- -D warnings`
- `cargo fmt --all -- --check`
- `cargo build -p lumendesk --all-features --examples --offline`: native form,
  web form, multi-webview and bounded-stream examples compile and link.
- `node --test tests/*.test.mjs`: 125 tests, including static asset checks and shared Rust/JS wire fixtures.
- `npm run test:types`: strict positive fixtures pass; 18 expected-error lines
  are compiler checked; two invalid fixtures reject all 10 invalid uses.
- Both freshly scaffolded SDK apps build using actual CLI `build --offline`,
  producing release executables with embedded code/assets.
- CLI's live std-only process fixture preserves the working app through a compiler
  error and releases its TCP port before the replacement starts.
- Reviewer regressions cover queued call cancellation, post-completion stream
  creation, symlink/hard-link output safety, malformed JavaScript replies, exact
  streaming wire shape, explicit host timeout rejection and portable SDK fixtures.
- Portable archive tests verify deterministic bytes, all file checksums, path/link
  safety, exclusions, output preservation, a relocated source rebuild and a real
  extracted display-free binary. A real web source archive also rebuilds offline
  in release mode; its extracted SDK passes all 12 webview-only tests.
- The opt-in `real_web_source_archive_builds_and_retains_sdk_test_fixtures` test
  was explicitly executed with `--ignored` and passed.
- The compiled stream example emitted generated client types using
  `LUMENDESK_PRINT_TYPES=1`, without opening a display.

## Final release build metadata

Raw executable sizes only; dynamically linked libraries are excluded. These are
not startup, memory, idle, installation, or parity measurements.

- Native generated app: 18,756,648 bytes
  SHA-256: 76b929b8f23c02e8b8fffb556e339fefa3a6c922d2b931a00ce56792df57a8b9
- Web generated app: 2,776,920 bytes
  SHA-256: 4e6bc7f0403acee729022ad7972ab011ca7ee72f513cb43adbf988eefbb59117

These final rebuilds supersede earlier raw size/hash entries in the CLI's
verification history. The source archive excludes binaries, targets, toolchains,
and workspace-specific sysroot libraries.

## Not established

AF_UNIX display sockets are explicitly denied by this cloud sandbox. No native
window, OS input, live webview IPC/CSP/frame isolation, screen reader/IME, or
real-desktop performance result is claimed. GTK/WebKit compilation is not runtime
verification. No macOS/Windows build/run, mixed native+web event loop, installer,
update/signing workflow or clean-VM installation was performed.

No remote repository, push, account change, or deployment was made.
