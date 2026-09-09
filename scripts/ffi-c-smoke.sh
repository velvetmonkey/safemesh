#!/usr/bin/env bash
# External C compile, link and run test for the SafeMesh C ABI (rust/crates/safemesh-ffi).
# Builds the library, compiles tests/c/gcounter_smoke.c against include/safemesh.h with a C
# compiler, links it against the built shared library, runs it, and reads its exit code from a
# file. The C program uses valid ownership only.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
crate_dir="$repo_root/rust/crates/safemesh-ffi"
target_dir="${CARGO_TARGET_DIR:-$repo_root/rust/target}"
lib_dir="$target_dir/release"
cc_bin="${CC:-cc}"

if ! command -v "$cc_bin" >/dev/null 2>&1; then
  echo "missing required tool: $cc_bin" >&2
  exit 1
fi

(
  cd "$repo_root/rust"
  cargo build -p safemesh-ffi --release
)

tmp_dir="$(mktemp -d)"
cleanup() {
  rm -f "$tmp_dir"/gcounter_smoke "$tmp_dir"/exit-code
  rmdir "$tmp_dir"
}
trap cleanup EXIT

"$cc_bin" -std=c99 -Wall -Wextra -Wpedantic -Werror \
  -I "$crate_dir/include" \
  "$crate_dir/tests/c/gcounter_smoke.c" \
  -L "$lib_dir" -lsafemesh_ffi -Wl,-rpath,"$lib_dir" \
  -o "$tmp_dir/gcounter_smoke"

set +e
"$tmp_dir/gcounter_smoke"
echo $? > "$tmp_dir/exit-code"
set -e

exit_code="$(cat "$tmp_dir/exit-code")"
if [ "$exit_code" != "0" ]; then
  echo "safemesh C ABI smoke test exited $exit_code" >&2
  exit 1
fi
echo "safemesh C ABI smoke test exited $exit_code"
