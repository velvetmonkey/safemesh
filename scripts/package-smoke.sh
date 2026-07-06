#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

(
  cd "$repo_root/rust"
  cargo package -p safemesh-crdt --allow-dirty --no-verify
)

python3 -m pip wheel "$repo_root/rust/crates/safemesh-python" \
  --wheel-dir "$tmp_dir/wheels" \
  --no-deps

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
left.merge_log_bytes(right.log_bytes())
assert left.value() == right.value() == 5

reg_left = sm.LwwRegisterReplica(1)
reg_right = sm.LwwRegisterReplica(2)
reg_record = reg_left.append_set(10, 1, 100)
assert isinstance(reg_record, bytes)
reg_right.merge_record_bytes(reg_record)
reg_right.append_set(10, 2, 200)
reg_left.merge_log_bytes(reg_right.log_bytes())
assert reg_left.value_or(0) == reg_right.value_or(0) == 200

print("PYTHON_INSTALL_SMOKE=true")
PY
