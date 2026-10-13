# Verification summary

2026-10-13 UTC, Debian 13 x86_64, Rust/Cargo 1.99.0, Node 24.19.0,
TypeScript 7.0.2. GUI library versions and sandbox details: GUI_ENVIRONMENT.md.

## Passed

- `cargo test --workspace --all-features --offline`: 74 unit/integration tests
  (SDK 9, CLI 16, core 48 plus schema integration 1) and 1 core doctest.
- `cargo clippy --workspace --all-features --all-targets --offline -- -D warnings`
- `cargo fmt --all -- --check`
- `cargo build -p lumendesk --all-features --examples --offline`: native form,
  web form and multi-webview examples compile and link.
- `node --test tests/*.test.mjs`: 60 tests, including static asset checks.
- `npm run test:types`: strict positive fixtures pass; 13 expected-error lines
  are compiler checked; two invalid fixtures reject all 8 invalid uses.
- Both freshly scaffolded SDK apps build using actual CLI `build --offline`,
  producing release executables with embedded code/assets.
- CLI's live std-only process fixture preserves the working app through a compiler
  error and releases its TCP port before the replacement starts.
- Reviewer regressions cover queued call cancellation, post-completion stream
  creation, symlink/hard-link output safety and malformed JavaScript replies.

## Final release build metadata

Raw executable sizes only; dynamically linked libraries are excluded. These are
not startup, memory, idle, installation, or parity measurements.

- Native generated app: 18,756,648 bytes
  SHA-256: 76b929b8f23c02e8b8fffb556e339fefa3a6c922d2b931a00ce56792df57a8b9
- Web generated app: 2,620,456 bytes
  SHA-256: 3abca0813b7ab7ac6570ae5a1ea5c5f6a438569cf0f85c5e6a8d13a948a21ac3

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
