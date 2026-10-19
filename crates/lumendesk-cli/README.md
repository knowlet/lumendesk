# LumenDesk CLI

Independent Rust project tooling for the LumenDesk SDK. This crate does not import
upstream application templates or promise compatibility with another framework.

## Commands

```sh
cargo run -p lumendesk-cli -- --help
cargo run -p lumendesk-cli -- new hello-native --mode native
cargo run -p lumendesk-cli -- new hello-web --mode web
lumendesk doctor --project hello-web
lumendesk bindings --project hello-web
lumendesk dev --project hello-web
lumendesk build --project hello-web
lumendesk package --kind source --project hello-web --out hello-source.tar.gz
lumendesk package --kind binary --project hello-web --out hello-linux.tar.gz
```

Install the command from this workspace with `cargo install --path
crates/lumendesk-cli --locked`. `new` accepts an explicit `--sdk-path` for a local
SDK checkout. When run from this source workspace it records the SDK's absolute
local path in the generated Cargo manifest. Use `package --kind source` to bundle and rewrite that dependency before moving
the project, or update the path manually. If the source workspace is absent, the fallback dependency is
`lumendesk = "0.1"`; this milestone does **not** claim that package is published.
Pass `--sdk-path` for a checkout rather than relying on a registry release.

Both templates are real desktop applications. The native template starts the
SDK's editable native form; it enables no webview feature and needs no Node or
frontend build tool. The web template embeds its own `web/index.html` with
`include_str!` and uses the SDK's reference greeting service. All scaffold text,
HTML, styles, and configuration were independently authored. An ordinary browser
tab cannot supply the desktop bridge.

`new` only accepts new or empty real directories. It refuses populated directories
and destination symlinks. Its parent directory must exist. It creates each file
exclusively and rolls back only files it created if a write fails. It never offers
a force-overwrite flag.

## Configuration

`lumendesk.toml` uses this independent schema:

```toml
schema_version = 1

[app]
name = "hello-web"
id = "dev.example.hello"
version = "0.1.0"
mode = "web"

[build]
manifest = "Cargo.toml"
binary = "hello-web"

[bindings]
schema = "bindings.json"
output = "web/lumendesk.generated.ts"
```

Unknown fields and schema versions are rejected. Configured paths must be relative
without parent-directory traversal. Generated bindings output rejects symlink components
inside the project; an explicit `--out` also rejects a final symlink. Generated files
are replaced atomically, so existing hard links do not modify another file. Native/web are the only accepted modes.
The identity and version describe the project and portable archive; they do not
create platform installers or replace Cargo package metadata.

## Building and development

`build` runs an actual host `cargo build --release` and obtains the executable path
from Cargo's compiler-artifact messages. Missing tools, invalid configuration,
failed builds, and missing artifacts return nonzero status. `--offline` asks Cargo
to use only locally cached dependencies. Cross-compilation, platform installers, installation, signing, publishing, and
updater installation are not implemented. Portable source and binary archives are
available separately with `package`.

`dev` compiles into `target/lumendesk-dev`, copies successful executables into
`.lumendesk/run`, and starts the staged executable. A content-based polling watcher
ignores `target`, `.git`, `.lumendesk`, `node_modules`, `Cargo.lock`, and symlinks.
There is no external dependency watching. `--poll-ms` accepts 50–60000 ms; the
default is 500 ms. Source changes during a build are picked up on the next pass.

An already running application remains alive while replacement compilation runs,
and remains alive after a failed compile or invalid changed configuration. After
a successful build and staging, the CLI forcibly terminates and waits for the old
child before launching the replacement. This is **not a graceful save/quit hook**;
unsaved in-memory application work can be lost during a reload. Descendant-process
supervision is not implemented. If spawning the replacement fails, the CLI tries
to restart the last launched executable. A replacement that starts successfully
and then crashes is an application failure, not a failed compilation.

Development children receive:

- `LUMENDESK_ENV=development`
- `LUMENDESK_APP_ID=<app.id>.development`
- `LUMENDESK_DATA_DIR=<project>/.lumendesk/development-data`

The SDK uses the development identity where supported and does not persist user
form state in this milestone. Custom persistence must honor the provided data
path. The CLI does not intercept arbitrary filesystem writes from application
code. Ctrl-C stops the direct child. `dev --once` builds once, starts the app, and
waits for its exit; a failing app exit gives the CLI a failing exit status.

## Portable archives

```sh
lumendesk package --kind source --project hello-web --out hello-source.tar.gz --offline
lumendesk package --kind binary --project hello-web --out hello-binary.tar.gz --offline
```

`source` is the default kind. `--out` is required, must end in `.tar.gz`, and its
parent must already exist. Existing output files are never overwritten. Packaging
uses Cargo metadata to resolve dependencies and create/update the application's
`Cargo.lock`; `--offline` restricts this to the existing Cargo cache. Source mode
does not compile the application or run its build scripts. Binary mode performs
an actual release build for `rustc`'s host target, even if Cargo configuration
specifies another default target; it never starts the application.

Every archive contains:

- `MANIFEST.json`: application identity, kind, host/target, runtime limitations,
  and every payload file's byte length, mode and SHA-256. The manifest itself is
  excluded from its own checksum to avoid self-reference.
- `DEPENDENCIES.json`: resolved dependency versions, declared license expressions,
  registry/source identifiers, included notice paths and provenance, exact registry
  archive hashes, source-verification status and unresolved license review items.
  Source archives describe the complete resolved graph. Binary archives select the
  host target's normal/build dependency closure, excluding development-only edges;
  this is a conservative build-input inventory, not a linked-object analysis.
- `dependency-licenses/`: available LICENSE, LICENCE, COPYING, NOTICE, AUTHORS,
  COPYRIGHT and UNLICENSE files, including bounded recursive inspection and
  explicitly declared Cargo license files. Exact package/checksum-bound supplements
  preserve reviewed upstream texts omitted from published crates. A changed version,
  registry, license expression or archive checksum cannot inherit those supplements.
- `DEPENDENCY-SOURCES.txt`: verified versioned registry source locations and hashes.
  Expected but unverified locations and their status remain in `DEPENDENCIES.json`.
  Reviewed supplements and verified source-availability statements require checking
  both the cached `.crate` hash and its local source tree. Modified, extra or missing
  source files remain visible as unresolved review items; the only ignored extraction
  marker is a checked, bounded regular `.cargo-ok` file.
- `PACKAGE-NOTES.txt`: rebuild/run instructions and runtime/license boundaries.

Empty notice lists and unresolved review items must be examined before release.
An included notice is not proof that every obligation has been met, and archive
creation leaves `license_review_required` and `redistribution_review_required`
true. See [the redistribution review](../../docs/REDISTRIBUTION.md) for the pinned
275-dependency web inventory, the 31 original notice gaps, Microsoft loader and
native font notices, and outstanding Apple/system-library/native release questions.

Archive entries are sorted; file modes are normalized, owners and timestamps are
zeroed, and the gzip header has no filename/time/host variation. Identical payloads
produce identical bytes. This does **not** promise that separate Rust builds on
different machines produce identical executable bytes. No signature or attestation
is added; the checksums detect content changes, not an untrusted distributor.

Source mode supports a single root `Cargo.toml` application. It includes the
application and its local `lumendesk`/`lumendesk-core` dependencies under
`_sdk/crates/`, with SDK root assets in `_sdk/assets/`. Workspace-inherited crate
metadata is made explicit and dependency paths are rewritten relative to the
archive. SDK/core license notices are copied alongside their source. The bundled
crates are excluded from the application's workspace membership, retaining the
original application's dependency/feature resolution and locked rebuild.

Extract the source archive into an empty directory, then run:

```sh
cargo build --release --locked
```

The original checkout is unnecessary. Registry dependencies are **not vendored**:
network access or all locked packages in Cargo's local cache are required; add
`--offline` only in the latter case. To prepare an air-gapped rebuild separately,
use Cargo's `cargo vendor --locked vendor` and its emitted Cargo source configuration,
reviewing licenses for that separately prepared source bundle. This CLI does not
silently copy the user's entire Cargo cache or credentials. Rust 1.95+ and the
platform development libraries remain required. Other local path crates,
workspace-inherited dependency tables, Cargo patch/replace overrides, nested app
manifests, and custom external resource layouts are rejected or unsupported.

The source allowlist is deliberately bounded:

- Root Cargo/config/bindings manifests, build.rs, package manifests, tsconfig.json,
  README and license/notice files
- Files under `src/`, `web/`, `assets/`, `examples/` and `tests/` with source,
  web/font/image/shader, JSON, Markdown or text extensions
- No hidden paths, `.env`, `.git`, `target`, `dist`, `build`, caches,
  `node_modules`, existing vendor trees, key/certificate files, or names containing
  secret/credential/password components

Unknown root files and file extensions are omitted. Declared Cargo source/readme/
license/build paths must be present in the allowlist. Embedded `include_str!` or
`include_bytes!` dependencies outside this layout need restructuring first. The
manifest makes the exact payload reviewable; an allowlist cannot identify secrets
someone embeds in otherwise ordinary Rust/JSON/text files.

Selected source files, local dependency paths and output path components reject
symlinks and Windows reparse points. Selected configuration is inspected before
parsing or invoking Cargo. Actual opened files are checked before bounded reads;
source hardlinks and special files are rejected on the supported desktop systems.
Parent traversal, backslashes, absolute archive paths, Windows reserved names and
invalid characters, file/directory prefix conflicts, and case or canonically
equivalent Unicode component collisions are rejected. This normalization does not
claim to model every filesystem's name-equivalence rules. Cargo's normal hard-linked executable artifacts are read as bytes
and archived as a regular executable file; no archive hardlinks/symlinks are emitted.
Do not modify an input tree concurrently with packaging: this is a snapshot utility,
not a hardened sandbox against a malicious process changing the filesystem during
reads. Archive extraction is performed by the user's standard archive tool into a
fresh directory; this CLI provides no extraction command.

Binary mode includes `bin/<name>` with executable mode plus notices/metadata. It
is a portable **archive**, not a self-contained runtime: Linux binaries dynamically
link system libraries. Web apps need GTK 3/WebKitGTK 4.1 and their dependencies;
native apps need their window-system/graphics stack. macOS uses Cocoa/WKWebView,
and Windows web apps require WebView2. The manifest records the host target and
these requirements. No installer, signing, desktop registration, runtime library
bundling, clean-machine installation certification or license-compliance
certification is supplied. Custom resources not embedded in the executable are
not automatically bundled.

## Deterministic TypeScript

`bindings` uses the core runtime's explicit schema generator, with sorted method,
event, and field maps. It emits no timestamps or absolute paths. The output is a
typed client interface, not a JavaScript bundler or runtime injection script.

```sh
lumendesk bindings --sample
lumendesk bindings --schema bindings.json --out web/lumendesk.generated.ts
```

The JSON input has `methods` and optional `events` maps. Each method contains a
matching `name`, `arguments`, and `result`. Schemas are tagged with `kind`: `json`,
`null`, `boolean`, `number`, `integer`, `string`, `array`, `nullable`, or `object`.
For example:

```json
{
  "methods": {
    "echo": {
      "name": "echo",
      "arguments": { "kind": "string" },
      "result": { "kind": "string" }
    }
  },
  "events": {}
}
```

Object fields have `schema` and `optional`; optionality differs from nullability.
Integer schemas use JavaScript-safe integers (±(2^53−1)); encode larger integers
as strings. Generated output must describe the services actually registered by
the app. The reference form registry uses `greet` and the `greeting` event.

## Verification

```sh
cargo test -p lumendesk-cli
# After configuring the host webview development libraries:
cargo test -p lumendesk-cli --test package -- --ignored
```

Packaging tests additionally verify byte determinism, every payload checksum,
allowlist exclusions, symlink/hardlink/path rejection, preservation of existing
outputs, a source rebuild after deleting the original app and SDK, and execution
of an extracted display-free binary. A real extracted web scaffold is also built
with the local platform development libraries; its GUI is not launched.
The path/link and archive integrity tests run on Linux, macOS and Windows CI,
including Windows junctions and Unix FIFO configuration rejection where supported.
Unit fixtures also check target/development dependency selection, nested notice
collection, pinned supplement identities and modified registry-source detection.

Integration tests cover both scaffold modes, preserving existing data, symlink
rejection, argument/config errors, deterministic generation, a real offline
release executable, development environment isolation, and app exit status. A
Unix integration test uses real Cargo builds and a TCP socket to establish that:

1. A first app remains active during and after a compiler error.
2. A repaired app replaces it.
3. The old process releases its socket before the replacement starts.

These CLI tests do not prove a desktop window is visible, graphical input works,
or a platform package is installable. See the workspace verification matrix for
actual desktop runtime evidence.
