#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 || ! -d "$1" ]]; then
  echo 'release install: pass an asset directory' >&2
  exit 2
fi
assets="$(cd "$1" && pwd)"
if [[ ! -f "$assets/SHA256SUMS" ]]; then
  echo 'release install: SHA256SUMS missing' >&2
  exit 1
fi
(
  cd "$assets"
  sha256sum --check SHA256SUMS
) || { echo 'release install: SHA256SUMS check failed' >&2; exit 1; }

work="$(mktemp -d "${TMPDIR:-${RUNNER_TEMP:-$PWD}}/safemesh-release-install.XXXXXXXX")"
cleanup() { python3 - "$work" <<'PY'
import shutil, sys
shutil.rmtree(sys.argv[1])
PY
}
trap cleanup EXIT
if ! python3 - "$assets/SHA256SUMS" > "$work/names" <<'PY'
from pathlib import Path
import re
import sys
lines = Path(sys.argv[1]).read_text().splitlines()
if len(lines) != 3:
    raise SystemExit('release install: expected 3 packages in SHA256SUMS')
patterns = {
    'crate': re.compile(r'safemesh-crdt-(\d+\.\d+\.\d+)\.crate'),
    'wheel': re.compile(r'safemesh_python-(\d+\.\d+\.\d+)-[^/]+\.whl'),
    'wasm': re.compile(r'safemesh-wasm-(\d+\.\d+\.\d+)\.tgz'),
}
found = {}
for line in lines:
    entry = re.fullmatch(r'[0-9a-f]{64}  ([^/\s]+)', line)
    if not entry:
        raise SystemExit('release install: invalid SHA256SUMS entry')
    name = entry.group(1)
    matches = [(kind, match.group(1)) for kind, pattern in patterns.items()
               if (match := pattern.fullmatch(name))]
    if len(matches) != 1 or matches[0][0] in found:
        raise SystemExit(f'release install: expected 3 packages, invalid or duplicate asset {name}')
    found[matches[0][0]] = (name, matches[0][1])
if set(found) != set(patterns):
    raise SystemExit('release install: expected 3 packages: crate, wheel, WASM')
if len({version for _, version in found.values()}) != 1:
    raise SystemExit('release install: package version mismatch')
print(next(iter({version for _, version in found.values()})))
for kind in ('wheel', 'wasm', 'crate'):
    print(found[kind][0])
PY
then
  cat "$work/names" >&2
  exit 1
fi
mapfile -t names < "$work/names"
version="${names[0]}"
wheel="${names[1]}"
wasm="${names[2]}"
crate="${names[3]}"

mkdir "$work/python"
python3 -m venv "$work/python/venv"
if ! "$work/python/venv/bin/pip" install --no-index "$assets/$wheel"; then
  echo "release install: wheel $wheel install failed" >&2
  exit 1
fi
if ! ( cd "$work/python" && "$work/python/venv/bin/python" -I - <<'PY'
import pathlib
import safemesh_python as sm
print('PYTHON MODULE', sm.__file__)
assert pathlib.Path(sm.__file__).resolve().is_relative_to(pathlib.Path(__import__('sys').prefix).resolve())
left = sm.GCounterReplica(1, 3)
right = sm.GCounterReplica(2, 3)
left.append_bump(1, 5)
right.append_bump(2, 7)
assert right.merge_log_bytes(left.log_bytes()) == ['accepted']
assert left.merge_log_bytes(right.log_bytes()) == ['accepted', 'duplicate']
assert left.value() == right.value() == 12
PY
); then
  echo "release install: wheel $wheel smoke failed" >&2
  exit 1
fi
echo "INSTALL safemesh_python $version ok"

mkdir "$work/wasm"
if ! ( cd "$work/wasm" && npm install --offline --no-audit --no-fund "$assets/$wasm" ); then
  echo "release install: WASM package $wasm install failed" >&2
  exit 1
fi
if ! ( cd "$work/wasm" && node --experimental-wasm-modules --input-type=module - <<'JS'
import assert from 'node:assert/strict';
import { SafeMeshGCounterReplica } from 'safemesh-wasm';
const left = new SafeMeshGCounterReplica(1n, 3);
const right = new SafeMeshGCounterReplica(2n, 3);
left.appendBump(1, 5n);
right.appendBump(2, 7n);
assert.deepEqual(right.mergeLogBytes(left.logBytes()), ['accepted']);
assert.deepEqual(left.mergeLogBytes(right.logBytes()), ['accepted', 'duplicate']);
assert.equal(left.value(), 12n);
assert.equal(right.value(), 12n);
left.free();
right.free();
JS
); then
  echo "release install: WASM package $wasm smoke failed" >&2
  exit 1
fi
echo "INSTALL safemesh-wasm $version ok"

mkdir "$work/rust"
tar -xzf "$assets/$crate" -C "$work/rust"
if [[ ! -f "$work/rust/safemesh-crdt-$version/Cargo.toml" ]]; then
  echo "release install: crate $crate has no package manifest" >&2
  exit 1
fi
cp -R "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/examples/packaged-consumer" "$work/rust/consumer"
python3 - "$work/rust/consumer/Cargo.toml" "$version" <<'PY'
from pathlib import Path
import re
import sys
path = Path(sys.argv[1])
text = path.read_text()
text, count = re.subn(r'path = "../safemesh-crdt-[^"]+"',
                      f'path = "../safemesh-crdt-{sys.argv[2]}"', text)
if count != 1:
    raise SystemExit('release install: consumer path missing')
path.write_text(text)
PY
if ! CARGO_TARGET_DIR="$work/rust/target" cargo metadata --offline --locked --format-version 1 --manifest-path "$work/rust/consumer/Cargo.toml" > "$work/rust/metadata.json"; then
  echo "release install: crate $crate metadata failed" >&2
  exit 1
fi
if ! python3 - "$work/rust/metadata.json" "$work/rust/safemesh-crdt-$version/Cargo.toml" <<'PY'
import json
from pathlib import Path
import sys
metadata = json.loads(Path(sys.argv[1]).read_text())
actual = [Path(package['manifest_path']).resolve() for package in metadata['packages']
          if package['name'] == 'safemesh-crdt']
expected = Path(sys.argv[2]).resolve()
if actual != [expected]:
    raise SystemExit(f'packaged consumer source mismatch: {actual} != {[expected]}')
PY
then
  echo "release install: crate $crate source mismatch" >&2
  exit 1
fi
if ! CARGO_TARGET_DIR="$work/rust/target" cargo build --offline --locked --manifest-path "$work/rust/consumer/Cargo.toml"; then
  echo "release install: crate $crate build failed" >&2
  exit 1
fi
if ! "$work/rust/target/debug/safemesh-packaged-consumer" "$work/rust/store" > "$work/rust/output"; then
  cat "$work/rust/output" >&2
  echo "release install: crate $crate smoke failed" >&2
  exit 1
fi
cat "$work/rust/output"
if [[ "$(tail -n 1 "$work/rust/output")" != 'PACKAGED RUST CONSUMER PASS checks=4' ]]; then
  echo "release install: crate $crate final line missing" >&2
  exit 1
fi
echo "INSTALL safemesh-crdt $version ok"
echo 'RELEASE INSTALL 3 of 3'
