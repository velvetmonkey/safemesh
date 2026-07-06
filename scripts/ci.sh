#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

rustup target add thumbv7em-none-eabihf wasm32-unknown-unknown

(
  cd "$repo_root/lean"
  lake build
)

(
  cd "$repo_root/rust"
  cargo fmt --all --check
  cargo test --workspace
  cargo test -p safemesh-crdt --no-default-features
  cargo test -p safemesh-crdt --features laws
  cargo test -p safemesh-crdt --examples
  cargo run -p safemesh-crdt --example break_it
  cargo run -p safemesh-crdt --example cold_chain_kill_test
  cargo build -p safemesh-crdt --target thumbv7em-none-eabihf
  cargo build -p safemesh-wasm --target wasm32-unknown-unknown
)

(
  cd "$repo_root/web"
  npm ci
  npm test
  npm run build
)
