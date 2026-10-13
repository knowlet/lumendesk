# CLI verification record

Recorded 2026-10-13 UTC on the provided Linux x86-64 cloud workspace with Rust
1.99.0. This records CLI behavior and build artifacts, not graphical runtime,
performance, installer, accessibility, or other-platform acceptance.

- `cargo test -p lumendesk-cli`: 16 integration tests passed, including real Cargo
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
