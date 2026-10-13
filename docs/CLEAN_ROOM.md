# Clean-room implementation record

This project is an independent Rust implementation based on a separately prepared functional specification. The implementation team must not inspect, clone, search, or copy the original toolkit's implementation, source code, or examples. The specification researcher and implementation team are separated.

## Allowed inputs

- The functional specification supplied by the independent documentation researcher.
- Rust's official documentation and independently developed library API documentation.
- Original requirements, implementation design, code, tests, and examples produced in this project.

## Environment and independent references

- Development began 2026-10-13 in a fresh directory on a Debian 13 cloud workspace.
- Rust installed from https://sh.rustup.rs and https://static.rust-lang.org into a workspace-local tool directory, without changing the system configuration.
- Rendering feasibility: https://docs.rs/eframe/latest/eframe/
- OS webview feasibility: https://docs.rs/wry/latest/wry/
- Windowing feasibility: https://docs.rs/tao/latest/tao/
- Linux package availability: https://packages.debian.org/trixie/libwebkit2gtk-4.1-dev

No compatibility or full-parity claim is made merely by creating this repository. Supported behavior and verified platforms must be documented explicitly.
