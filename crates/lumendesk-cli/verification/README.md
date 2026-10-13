# CLI verification record

Recorded 2026-10-13 UTC on the provided Linux x86-64 cloud workspace with Rust
1.99.0. This records CLI behavior and build artifacts, not graphical runtime,
performance, installer, accessibility, or other-platform acceptance.

- `cargo test -p lumendesk-cli`: 2 unit tests and 22 integration tests passed
  (one platform-prerequisite test is opt-in), including real Cargo
  release building, failing/successful live recompiles, development isolation,
  process resource release, safe scaffolding, and symlink/hard-link regressions.
- `cargo clippy -p lumendesk-cli --all-targets --no-deps -- -D warnings`: passed.
- `lumendesk doctor --mode native` and `--mode web`: passed with the extracted GUI
  sysroot configured. Both correctly noted that this shell had no display.
- Fresh `lumendesk new ... --mode native` and `--mode web` projects both compiled
  offline into real dev-profile executables.
- `lumendesk build --project ... --offline` successfully created release-profile
  executables for both freshly generated projects.

The real rebuild test starts a Rust child that owns a TCP listening socket and
prints periodic heartbeats. It introduces a compiler error, observes that the
same app keeps running, repairs the source, and verifies that the replacement
can bind the socket and no old-version heartbeats occur afterward. The CLI uses
forced child termination plus wait; this is not a graceful application quit test.

## Build environment and commands

From the repository root:

```sh
export CARGO_HOME=/workspace/scratch/c7703109bdb3/.cleanroom-tools/cargo
export RUSTUP_HOME=/workspace/scratch/c7703109bdb3/.cleanroom-tools/rustup
export PATH="$CARGO_HOME/bin:$PATH"
source /workspace/scratch/c7703109bdb3/.cleanroom-tools/gui-env.sh
export CARGO_TARGET_DIR="$PWD/target"

mkdir -p target/cli-scaffold-smoke
for mode in native web; do
  project="target/cli-scaffold-smoke/lumendesk-cli-${mode}-smoke"
  target/debug/lumendesk new "$project" --mode "$mode"
  cargo build --offline --manifest-path "$project/Cargo.toml"
  target/debug/lumendesk build --project "$project" --offline
done
```

The environment script points pkg-config, linker, and runtime lookup at extracted
Linux GUI development/runtime libraries. It is environment-specific and is not a
claim that those libraries were installed system-wide or bundled in the result.
A different host must install its own prerequisites; use `lumendesk doctor`.

## Release artifact identity

Both results are unstripped, dynamically linked, Linux x86-64 ELF executables.
The generated projects use Cargo's ordinary optimized release profile. These
numbers are raw on-disk executable sizes, exclude system libraries, and are not
performance or deployment-size comparisons.

- `target/release/lumendesk-cli-native-smoke`: 18,756,648 bytes
  - SHA-256: `76b929b8f23c02e8b8fffb556e339fefa3a6c922d2b931a00ce56792df57a8b9`
- `target/release/lumendesk-cli-web-smoke`: 2,620,256 bytes
  - SHA-256: `2564aaee0de94e5468e9d6b724d8c5341f350a76833fc04eff8eb256e5a55656`

The web scaffold embeds its own newly authored `web/index.html`. No frontend
server or TypeScript compiler was needed to produce either executable. Generated
TypeScript describes the registered reference bridge and is available alongside
the HTML. Separate core generator tests cover the TypeScript type contract.

Full command outputs are saved in the neighboring `.txt` files. Build durations
appearing in Cargo's output are uncalibrated command logs and not benchmarks.

## Portable archive verification

`package-tests.txt` records the full CLI suite after adding portable packages.
Six regular packaging integration tests verify deterministic archive bytes,
per-file SHA-256, identity/runtime metadata, sensitive/cache exclusions, symlink
and hardlink rejection, unsafe/escaping paths, existing-output preservation,
unsupported local dependency diagnostics, source relocation after deleting the
original application **and** fixture SDK, and a built executable extracted and
run as a display-free fixture.

`package-real-web-regression.txt` records the explicitly enabled real-web test:

```sh
source /workspace/scratch/c7703109bdb3/.cleanroom-tools/gui-env.sh
cargo test -p lumendesk-cli --test package --offline -- --ignored
```

It scaffolds and archives a real web application, verifies all payload checksums,
asserts bundled SDK/core sources, license files, root assets and the shared stream
fixture exist, removes the original application, extracts elsewhere, then builds
with `--release --locked --offline` and runs the extracted SDK's display-free
tests. The scaffold's dependency enables the webview feature; the test checks
that webview and stream test names actually appear. It uses the existing Cargo
cache and configured platform development libraries, not the original SDK path.

Separate `package-web-source-build.txt` and `package-web-sdk-tests.txt` logs
record an extracted real web scaffold release build and all 12 SDK tests passing.
The shared `tests/stream-frames.json` fixture is included beneath the SDK crate.
Cargo's `--features` switch cannot select features for an excluded workspace
dependency, so SDK tests use `cargo test -p lumendesk --locked --offline` from the
web application's dependency graph. The application activates `webview` there.

`package-web-binary-build.txt` records the real web executable archive build for
`x86_64-unknown-linux-gnu`. No desktop executable was launched during packaging
verification. The archive contains the executable, checksummed inventory,
license metadata/notices and explicit GTK 3/WebKitGTK 4.1 runtime requirements;
it does not bundle those system libraries. Dependency notice lists can be empty
when an upstream crate distribution does not provide a recognized notice file.
The inventory flags license review as required; this is not redistribution or
clean-machine runtime certification.

Portable archives and additional machine-generated checks are under the ignored
`dist/package-verification/` directory. Those artifacts are examples, not
installed software. The package command does not change installed system files.

`package-archive-metadata.json` records final archive sizes, SHA-256 hashes,
verified payload counts, dependency notice coverage, identity and target. The
final source archive was generated twice with identical bytes. The real web
binary was inspected with `file` and `readelf`, without running it.
