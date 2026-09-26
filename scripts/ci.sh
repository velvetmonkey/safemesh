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
require_tool python3

python3 "$repo_root/scripts/check-readme-quickstart.py" \
  --work-dir "$repo_root/rust/target/readme-quickstart"

rustup target add thumbv7em-none-eabihf wasm32-unknown-unknown

(
  cd "$repo_root/lean"
  lake build
)

(
  cd "$repo_root/rust"
  cargo fmt --all --check
  cargo test --workspace --locked
  cargo test -p safemesh-crdt --no-default-features --locked
  cargo test -p safemesh-crdt --features laws --locked
  cargo test -p safemesh-crdt --examples --locked
  cargo run -p safemesh-crdt --example break_it --locked
  cargo run -p safemesh-crdt --example cold_chain_kill_test --locked
  cargo build -p safemesh-crdt --target thumbv7em-none-eabihf --locked
  cargo build -p safemesh-wasm --target wasm32-unknown-unknown --locked
  header="$(mktemp)"
  cbindgen --config crates/safemesh-ffi/cbindgen.toml \
    --crate safemesh-ffi \
    --output "$header"
  diff -u crates/safemesh-ffi/include/safemesh.h "$header"
  rm -f "$header"
)

# Exercise the reference application alongside the existing product gate.
cargo fmt --manifest-path "$repo_root/examples/fieldcheck/Cargo.toml" --check
cargo clippy --manifest-path "$repo_root/examples/fieldcheck/Cargo.toml" --locked --all-targets -- -D warnings
cargo test --locked --manifest-path "$repo_root/examples/fieldcheck/Cargo.toml"
cargo build --locked --manifest-path "$repo_root/examples/fieldcheck/Cargo.toml"
"$repo_root/scripts/fieldcheck-journey.py"

"$repo_root/scripts/ffi-c-smoke.sh"

(
  cd "$repo_root/web"
  npm ci
  npm test
  npm run build
)

"$repo_root/scripts/package-smoke.sh"
