#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

(
  cd "$repo_root/rust"
  cargo publish --dry-run -p safemesh-crdt --allow-dirty
)

if ! command -v maturin >/dev/null 2>&1; then
  if command -v pipx >/dev/null 2>&1; then
    pipx install "maturin>=1.7,<2"
  else
    python3 -m pip install --user "maturin>=1.7,<2"
  fi
  export PATH="$HOME/.local/bin:$PATH"
fi

(
  cd "$repo_root/rust/crates/safemesh-python"
  maturin build --release --features extension-module --out "$tmp_dir/wheels"
)

python3 -m venv "$tmp_dir/venv"
"$tmp_dir/venv/bin/pip" install \
  --no-index \
  --find-links "$tmp_dir/wheels" \
  safemesh-python

"$tmp_dir/venv/bin/python" - <<'PY'
import safemesh_python as sm

left = sm.GCounterReplica(1, 3)
right = sm.GCounterReplica(2, 3)
record = left.append_bump(1, 5)
assert isinstance(record, bytes)
right.merge_record_bytes(record)
assert left.merge_log_bytes(right.log_bytes()) == ["duplicate"]
assert left.value() == right.value() == 5

reg_left = sm.LwwRegisterReplica(1)
reg_right = sm.LwwRegisterReplica(2)
reg_record = reg_left.append_set(10, 1, 100)
assert isinstance(reg_record, bytes)
reg_right.merge_record_bytes(reg_record)
reg_right.append_set(10, 2, 200)
assert reg_left.merge_log_bytes(reg_right.log_bytes()) == ["duplicate", "accepted"]
assert reg_left.value_or(0) == reg_right.value_or(0) == 200

flag_left = sm.EnableWinsFlagReplica(1)
flag_right = sm.EnableWinsFlagReplica(2)
flag_record = flag_left.append_enable(10)
assert isinstance(flag_record, bytes)
flag_right.merge_record_bytes(flag_record)
flag_remove = flag_right.append_disable_observed()
flag_left.append_enable(11)
flag_left.merge_record_bytes(flag_remove)
assert flag_right.merge_log_bytes(flag_left.log_bytes()) == ["duplicate", "accepted", "duplicate"]
assert flag_left.value() == flag_right.value() is True

map_left = sm.LwwMapReplica(1)
map_right = sm.LwwMapReplica(2)
map_record = map_left.append_set(7, 10, 1, 100)
assert isinstance(map_record, bytes)
map_right.merge_record_bytes(map_record)
map_remove = map_right.append_remove(7, 11, 2)
map_left.append_set(7, 12, 1, 300)
map_left.merge_record_bytes(map_remove)
assert map_right.merge_log_bytes(map_left.log_bytes()) == ["duplicate", "accepted", "duplicate"]
assert map_left.value_or(7, 0) == map_right.value_or(7, 0) == 300

print("PYTHON_INSTALL_SMOKE=true")
PY

"$tmp_dir/venv/bin/python" \
  "$repo_root/rust/crates/safemesh-python/examples/data_mule_demo.py"

if ! command -v wasm-pack >/dev/null 2>&1; then
  cargo install wasm-pack --version 0.15.0 --locked
fi

wasm-pack build "$repo_root/rust/crates/safemesh-wasm" \
  --target bundler \
  --out-dir "$tmp_dir/wasm-pkg" \
  --release

npm pack --dry-run "$tmp_dir/wasm-pkg"

wasm-pack build "$repo_root/rust/crates/safemesh-wasm" \
  --target nodejs \
  --out-dir "$tmp_dir/wasm-node-pkg" \
  --release

node "$repo_root/rust/crates/safemesh-wasm/examples/node-convergence.mjs" \
  "$tmp_dir/wasm-node-pkg"
node "$repo_root/rust/crates/safemesh-wasm/tests/node-error-shape.mjs" \
  "$tmp_dir/wasm-node-pkg"
node "$repo_root/rust/crates/safemesh-wasm/tests/safemesh-wasm-boundary-repros.mjs" \
  "$tmp_dir/wasm-node-pkg"
