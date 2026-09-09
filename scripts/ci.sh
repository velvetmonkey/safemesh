#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

require_tool() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required tool: $1" >&2
    exit 1
  fi
}

require_tool cbindgen
require_tool maturin

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
  header="$(mktemp)"
  cbindgen --config crates/safemesh-ffi/cbindgen.toml \
    --crate safemesh-ffi \
    --output "$header"
  diff -u crates/safemesh-ffi/include/safemesh.h "$header"
  rm -f "$header"
)

"$repo_root/scripts/ffi-c-smoke.sh"

(
  cd "$repo_root/web"
  npm ci
  npm test
  npm run build
)

"$repo_root/scripts/package-smoke.sh"
