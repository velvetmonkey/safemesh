#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tag_commit=295b04ef6f64d11395452ddb8029747c6e03fb22
scratch="$repo_root/rust/target/older-decoder-compat"
checkout="$scratch/tagged"
generated="$scratch/current-files"
mkdir -p "$scratch" "$generated"
if [ "$(git -C "$repo_root" rev-parse 'v0.1.0^{commit}')" != "$tag_commit" ]; then
  echo 'v0.1.0 did not resolve to the pinned decoder source' >&2
  exit 1
fi

SMQ3_GENERATE="$generated" TMPDIR="$scratch" cargo test \
  --manifest-path "$repo_root/rust/Cargo.toml" -p safemesh-crdt \
  --features local-writer --test v010_compat store::generate_v010 \
  --locked -- --ignored --exact

git -C "$repo_root" worktree add --detach "$checkout" "$tag_commit"
trap 'git -C "$repo_root" worktree remove --force "$checkout"' EXIT
# The test harness names the current writer's expected values. All decoder
# product source and its Cargo manifest/lock file come from the tagged commit.
cp "$repo_root/rust/crates/safemesh-crdt/tests/v010_compat.rs" \
  "$checkout/rust/crates/safemesh-crdt/tests/v010_compat.rs"
SMQ3_FIXTURES="$generated" TMPDIR="$scratch" CARGO_TARGET_DIR="$scratch/tag-target" \
  cargo test --manifest-path "$checkout/rust/Cargo.toml" \
  -p safemesh-crdt --features local-writer --test v010_compat --locked --no-run
SMQ3_FIXTURES="$generated" TMPDIR="$scratch" CARGO_TARGET_DIR="$scratch/tag-target" \
  timeout 60 cargo test --manifest-path "$checkout/rust/Cargo.toml" \
  -p safemesh-crdt --features local-writer --test v010_compat \
  --locked -- --skip store::generate_v010
