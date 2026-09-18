#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ $# -lt 1 || $# -gt 2 ]]; then
  echo "Usage: $0 {bundler|nodejs} [output-directory]" >&2
  exit 2
fi
case "$1" in
  bundler) default_dir="$repo_root/rust/crates/safemesh-wasm/pkg" ;;
  nodejs) default_dir="$repo_root/rust/crates/safemesh-wasm/pkg-node" ;;
  *) echo "Unsupported target: $1 (expected bundler or nodejs)" >&2; exit 2 ;;
esac
mkdir -p "${2:-$default_dir}"
out_dir="$(cd "${2:-$default_dir}" && pwd)"

# wasm-pack runs unlocked cargo metadata before forwarding --locked to its build.
# Reject a stale workspace lock before that metadata call can regenerate it.
cargo metadata --locked --format-version 1 \
  --manifest-path "$repo_root/rust/crates/safemesh-wasm/Cargo.toml" >/dev/null

wasm-pack build "$repo_root/rust/crates/safemesh-wasm" \
  --target "$1" \
  --out-dir "$out_dir" \
  --release -- --locked >&2

# wasm-pack 0.15 omits inline-JS snippets from its npm files allowlist.
# Include them in the artifact before packing; never repair the consumer install.
node --input-type=module - "$out_dir/package.json" <<'JS'
import { readFileSync, writeFileSync } from "node:fs";
const path = process.argv[2];
const manifest = JSON.parse(readFileSync(path, "utf8"));
manifest.files = [...new Set([...manifest.files, "snippets/"])];
writeFileSync(path, JSON.stringify(manifest, null, 2) + "\n");
JS

# Leave the installable tarball beside the generated bindings; stdout is pack JSON.
npm pack "$out_dir" --pack-destination "$out_dir" --json
