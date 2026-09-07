# SafeMesh FFI

`safemesh-ffi` is the thin C ABI spine for SafeMesh. It exposes opaque handles and byte-oriented helpers over the single Rust core instead of reimplementing merge logic.

## Claim boundary

This crate is engineered and tested glue. The verified claim belongs to the Lean-backed CRDT carriers in `safemesh-crdt`, and the Rust core must continue to pass the Lean-generated differential oracle corpus. The C ABI, generated header, and wrapper calls are covered by Rust tests and a committed-header drift check.

See the repository `CLAIMS.md` and `WHAT-IS-PROVEN.md` for the full wording rule.

## Install

Build the static or dynamic library from the workspace:

```sh
cd rust
cargo build -p safemesh-ffi --release
```

The public header is committed at `rust/crates/safemesh-ffi/include/safemesh.h`.

## Quickstart

```c
#include "safemesh.h"

SafeMeshGCounter *counter = safemesh_gcounter_new(2);
SafeMeshStatus status = safemesh_gcounter_try_apply_bump(counter, 0, 3);
/* status == Ok; ReplicaOutOfRange (2) rejects an invalid index. */
uint64_t value = safemesh_gcounter_value(counter);
safemesh_gcounter_free(counter);
```

`safemesh-ffi` has a versioned path dependency on `safemesh-crdt`. A crates.io publish dry-run for this wrapper can only pass after the core crate exists in the registry; v0.1 CI dry-runs the core crate and builds this wrapper locally.

## Checked coordinates and OR-Set

`safemesh_gcounter_try_apply_bump` returns `Ok` (0), `NullPointer` (1), or
`ReplicaOutOfRange` (2). An invalid coordinate leaves the counter unchanged.
The existing `safemesh_gcounter_apply_bump` still ignores invalid coordinates.

Create two sets with `safemesh_orset_new`, add `(element, token)` pairs with
`safemesh_orset_add`, and merge with `safemesh_orset_merge`. To remove a member,
read its `safemesh_orset_observed_tokens` and pass each token to
`safemesh_orset_apply_remove`. Read members with `safemesh_orset_elements`.
Array queries write an owned `SafeMeshU64s` on success; release it once with
`safemesh_u64s_free`. Release each set with `safemesh_orset_free`.

The set delegates to Rust `OrSet<u64, u64>`: elements and tokens are unsigned
64-bit integers (JavaScript uses `bigint`). Tokens are global to the set; use a
fresh, replica-unique token for every add to obtain add-wins behavior. Removal
persists tombstones even before an add arrives, and a reused token affects every
element carrying it. Observed tokens include tombstoned adds. Merge unions all
adds and tombstones, and reads return sorted unique live members. This is the
core's token semantics, including token reuse; the binding does not allocate IDs.
