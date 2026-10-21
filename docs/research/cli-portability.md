# CLI registry and source portability corrections

Decision date: 2026-10-20. Scope: the CLI schema adapter and relocation of an
existing application, not a new packaging format or dependency upgrade.

- [Serde field attributes](https://serde.rs/field-attrs.html) document defaulting
  absent fields. Add an explicit, defaulted stream map and reuse the existing
  runtime registration/validation engine; keep `deny_unknown_fields`.
- [Cargo resolver reference](https://doc.rust-lang.org/cargo/reference/resolver.html)
  and [`Workspace::set_resolve_behavior` in Cargo 0.90.0](https://github.com/rust-lang/cargo/blob/0.90.0/src/cargo/core/workspace.rs)
  establish workspace-root resolver selection: explicit resolver first, root
  package edition default otherwise, resolver 1 for virtual roots. A member's
  edition must not select its workspace's resolver. Cargo is MIT/Apache-2.0;
  its [MIT license](https://github.com/rust-lang/cargo/blob/0.90.0/LICENSE-MIT)
  was inspected. Use documented behavior only; no upstream source is copied.
- Keep the existing `_sdk/crates/<name>` plus `_sdk/assets` archive layout.
  Recognize that exact bundled layout before looking for the original SDK
  workspace. Retain bounded regular-file reads and symlink rejection.

No new library is needed. The independent CLI/runtime implementation and the
Cargo/Serde documentation are sufficient; no original-toolkit source was read.
Regression evidence must include actual CLI stream generation, resolver-1
feature-unification release builds with the archived lockfile, and two successive
source archive relocations with asset integrity and locked release builds.

## Validation

Against PR head `c02ea70e728e302ba8fb7206b2514434f08be874`, the new regressions
first failed with an unknown `streams` field, resolver-1 archive rebuild E0425
(missing dev-unified feature), and a missing `_sdk/assets/message.txt` on the
second archive. After the correction:

- `cargo test -p lumendesk-cli -p lumendesk-core --locked --offline` passed,
  including CLI fixture generation, stream-name/duplicate validation, two-hop
  archive builds, and an eight-case effective-resolver matrix with locked
  release builds. The repack test distinguishes app assets from SDK assets and
  rejects a symlink substituted for the bundled asset directory on Unix.
- `cargo clippy -p lumendesk-cli -p lumendesk-core --all-targets --locked --offline -- -D warnings`,
  `cargo fmt --all -- --check`, and `git diff --check` passed.
- `node --test tests/*.test.mjs` passed all 135 tests. `npm run test:types`
  passed its positive fixtures and rejected the expected negative fixtures.

Local Rust was 1.99.1. The real WebKit source-archive test remains ignored locally
because GTK/WebKit development libraries are absent. These checks do not claim
GUI/WebView, signing, distribution, or real-device acceptance; full-platform CI
and real desktop acceptance remain distinct requirements.
