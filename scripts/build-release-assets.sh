#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ $# -ne 1 ]]; then
  echo "Usage: $0 OUTPUT_DIRECTORY" >&2
  exit 2
fi
mkdir -p "$1"
out="$(cd "$1" && pwd)"
version="$(python3 - "$repo_root/rust/crates/safemesh-crdt/Cargo.toml" <<'PY'
import sys, tomllib
with open(sys.argv[1], 'rb') as source:
    print(tomllib.load(source)['package']['version'])
PY
)"

# These are the crate, wheel and WASM forms exercised by package-smoke.sh.
(cd "$repo_root/rust" && cargo package -p safemesh-crdt --locked --allow-dirty --no-verify)
cp "$repo_root/rust/target/package/safemesh-crdt-$version.crate" "$out/"
maturin build --manifest-path "$repo_root/rust/crates/safemesh-python/Cargo.toml" \
  --release --locked --features extension-module --out "$out"
"$repo_root/scripts/package-wasm.sh" bundler "$out/wasm-package" > "$out/wasm-pack.json"
cp "$out/wasm-package/safemesh-wasm-$version.tgz" "$out/"
rm "$out/wasm-pack.json"
python3 - "$out" "$version" <<'PY'
import hashlib
from pathlib import Path
import sys

out = Path(sys.argv[1])
version = sys.argv[2]
assets = sorted([out / f'safemesh-crdt-{version}.crate',
                 out / f'safemesh-wasm-{version}.tgz',
                 *out.glob(f'safemesh_python-{version}-*.whl')])
if len(assets) != 3 or any(not asset.is_file() for asset in assets):
    raise SystemExit('Release assets must contain one crate, one WASM package and one wheel')
with (out / 'SHA256SUMS').open('w') as checksums:
    for asset in assets:
        checksums.write(f'{hashlib.sha256(asset.read_bytes()).hexdigest()}  {asset.name}\n')
PY
