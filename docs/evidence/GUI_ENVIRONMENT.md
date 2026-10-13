# Isolated Linux GUI build dependencies

Prepared in `/workspace/scratch/c7703109bdb3/.cleanroom-tools` for Debian 13 x86_64. No system packages were installed and no system/security settings were changed. No application upstream source was accessed.

## Build environment

```bash
source /workspace/scratch/c7703109bdb3/.cleanroom-tools/gui-env.sh
cargo build
```

The source script adds the isolated `sysroot/usr/bin` to PATH, sets `PKG_CONFIG_SYSROOT_DIR`, adds the isolated architecture/share pkg-config directories to `PKG_CONFIG_PATH`, and adds the isolated architecture library directory to `LD_LIBRARY_PATH` and `LIBRARY_PATH`. Existing values are preserved. Source it only for this build shell.

Verified pkg-config versions:
- GTK 3: 3.24.49
- WebKitGTK 4.1: 2.52.6
- JavaScriptCoreGTK 4.1: 2.52.6
- libsoup 3: 3.6.5

Xvfb package: 2:21.1.16-1.3+deb13u4. Optional xdotool and libxdo3 packages were also downloaded/extracted before runtime display restrictions were established.

## Verification

- `gtk-smoke.c` compiled and linked successfully with `pkg-config --cflags --libs gtk+-3.0`.
- `webkit-smoke.c` compiled and linked successfully with `pkg-config --cflags --libs webkit2gtk-4.1`.
- `ldd` reports no missing libraries for the WebKit smoke binary, WebKitGTK shared library, or Xvfb.
- A real GTK window launch was attempted, but failed because a usable display cannot be started in this environment.
- Xvfb's normal local Unix display sockets are denied with `EPERM`, including with the supported escalated execution request. An independent AF_UNIX capability probe confirms the same restriction.
- The first escalated attempt also encountered an intermittent infrastructure `bwrap` mount error before execution; its exact retry ran but confirmed the socket denial.
- No TCP display, sandbox disabling, access-control disabling, firewall change, or other workaround of this restriction was attempted.
- Consequently, native window rendering, webview child-process launch/sandboxing, OS input, and screenshots remain unverified. GUI smoke binaries are build checks, not a claim of successful runtime tests.

## Provenance and reproducibility

APT downloaded official Debian packages against these pinned snapshot sources:
- https://snapshot.debian.org/archive/debian/20260925T181338Z/ (trixie main)
- https://snapshot.debian.org/archive/debian-security/20260925T181338Z/ (trixie-security main)

APT verified signed Release metadata with the pre-existing Debian archive keyring and package hashes from that metadata. The snapshot sources use `check-valid-until=no` because they are immutable historical snapshots; signature/hash checks were retained. Packages were extracted with `dpkg-deb --extract`; no maintainer scripts were run.

`debian-packages-manifest.json` records all 105 downloaded package filenames, versions/architectures, and SHA-256 hashes. `apt/apt.conf`, `apt/sources.list`, and downloaded package archives are retained. APT state/cache/log paths are inside this tools directory.

This is a current-workspace build sysroot, not a standalone redistributable Linux runtime. Development symlinks whose corresponding runtime already exists on the host were bridged to those host shared libraries; `host-runtime-links.txt` records those links. The existing host compiler/libc, GTK and most system runtime libraries are reused. Runtime helper discovery in the extracted WebKit layout has not been tested because the display restriction blocks GUI execution.

Diagnostics: `runtime/xvfb.log`, `runtime/xvfb-diagnostic.log`. No Xvfb remains running. The test Xauthority cookie is private to the tools directory and has not been shared.
