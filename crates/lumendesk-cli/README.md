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
```

Install the command from this workspace with `cargo install --path
crates/lumendesk-cli --locked`. `new` accepts an explicit `--sdk-path` for a local
SDK checkout. When run from this source workspace it records the SDK's absolute
local path in the generated Cargo manifest. That path needs updating if the
project moves. If the source workspace is absent, the fallback dependency is
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
The identity and version describe the project; they do not create platform
packages or replace Cargo package metadata.

## Building and development

`build` runs an actual host `cargo build --release` and obtains the executable path
from Cargo's compiler-artifact messages. Missing tools, invalid configuration,
failed builds, and missing artifacts return nonzero status. `--offline` asks Cargo
to use only locally cached dependencies. Cross-compilation, platform packages,
installation, signing, publishing, and updater installation are not implemented.

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
```

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
