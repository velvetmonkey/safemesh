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
sbom_files=(
  "$repo_root/rust/crates/safemesh-crdt/safemesh-crdt.cdx.json"
  "$repo_root/rust/crates/safemesh-ffi/safemesh-ffi.cdx.json"
  "$repo_root/rust/crates/safemesh-python/safemesh-python.cdx.json"
  "$repo_root/rust/crates/safemesh-python/safemesh-python.cyclonedx.json"
  "$repo_root/rust/crates/safemesh-wasm/safemesh-wasm.cdx.json"
)
for sbom_file in "${sbom_files[@]}"; do
  if [[ -e "$sbom_file" ]]; then
    echo "SBOM output path is already in use: $sbom_file" >&2
    exit 1
  fi
done
trap 'rm -f -- "${sbom_files[@]}"' EXIT
if [[ "$(cargo cyclonedx --version 2>/dev/null || true)" != "cargo-cyclonedx-cyclonedx 0.5.9" ]]; then
  cargo install cargo-cyclonedx --version 0.5.9 --locked --root "$repo_root/rust/target/release-tools"
  export PATH="$repo_root/rust/target/release-tools/bin:$PATH"
fi
source_date_epoch="$(git -C "$repo_root" log -1 --format=%ct)"
(cd "$repo_root/rust" && SOURCE_DATE_EPOCH="$source_date_epoch" cargo cyclonedx \
  --manifest-path crates/safemesh-python/Cargo.toml --target all --spec-version 1.5 --format json)
python3 - "$repo_root" <<'PY'
from pathlib import Path
import json
import sys

root = Path(sys.argv[1]).resolve()
sbom = root / 'rust/crates/safemesh-python/safemesh-python.cdx.json'
data = sbom.read_text()
source_prefix = 'path+file://' + str(root) + '/'
if source_prefix not in data:
    raise SystemExit('SBOM has no source path references to normalize')
data = data.replace(source_prefix, 'path+repo:')
if str(root) in data or 'path+file://' in data:
    raise SystemExit('SBOM still contains an absolute source path')
json.loads(data)
(sbom.parent / 'safemesh-python.cyclonedx.json').write_text(data)
sbom.unlink()
PY
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
