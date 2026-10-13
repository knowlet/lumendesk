#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo fmt --all -- --check
cargo test --workspace --all-features --locked
cargo clippy --workspace --all-features --all-targets --locked -- -D warnings
cargo build -p lumendesk --all-features --examples --locked
node --test tests/*.test.mjs
npm run test:types
