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
safemesh_gcounter_increment(counter, 0, 3);
uint64_t value = safemesh_gcounter_value(counter);
safemesh_gcounter_free(counter);
```

`safemesh-ffi` has a versioned path dependency on `safemesh-crdt`. A crates.io publish dry-run for this wrapper can only pass after the core crate exists in the registry; v0.1 CI dry-runs the core crate and builds this wrapper locally.
