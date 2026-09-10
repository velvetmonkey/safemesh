---
title: Using SafeMesh in your code — main (unreleased)
description: Rust, C ABI, WASM/TypeScript and Python integration, with transport and persistence responsibilities.
---

This guide uses **main (unreleased)** source. These are local build paths, not registry installation promises. **The Lean proof applies to the models; bindings, bytes and transport are TESTED engineering surfaces.** Maintainer support is **UNKNOWN** for all four surfaces. [Evidence: install matrix and status](https://github.com/velvetmonkey/safemesh/blob/main/README.md#install-matrix).

## You bring the transport

SafeMesh supplies CRDT state, deltas, canonical bytes and event-log operations. Your application supplies the schema and moves records between replicas. `TransportAdapter` describes subscription, connectivity, sending batches and draining incoming envelopes; `InMemoryTransport` implements a deterministic test environment. Neither proves real delivery. [Evidence: transport interfaces](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/src/lib.rs).

For an event-log integration, follow this sequence:

1. **Create local state and its log.** Use consistent counter arity and your chosen replica identities. The Rust walkthrough's `Replica` combines a CRDT with an `EventLog`.
2. **Record and apply a local edit.** `append_with` assigns a record sequence and applies its delta through the supplied callback.
3. **Exchange canonical bytes.** Encode with `Record::to_wire_bytes`, move the bytes through your transport, then decode with `Record::from_wire_bytes` using the agreed delta type.
4. **Admit before applying.** `admit_with` applies an accepted record through its callback. Duplicate and collision outcomes leave state unchanged; do not separately apply a rejected payload.
5. **Repair gaps.** Exchange versions and use `EventLog::since` or `anti_entropy` to send missing records after connectivity returns.
6. **Restore deliberately.** Rebuild state by replaying accepted history; decoding bytes alone does not choose your application's empty-state shape or supply durable storage.

Evidence for this sequence: the public-API [`m2slice.rs` walkthrough](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/examples/m2slice.rs), [its explanation](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/README.md#persist-restore-partition-and-reconcile), and [`EventLog` implementation](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/src/lib.rs).

## Rust

After the [first-result example](/safemesh/getting-started/), create your own Cargo application and add a path dependency on your local checkout:

```toml
[dependencies]
safemesh-crdt = { path = "/absolute/path/to/safemesh/rust/crates/safemesh-crdt" }
```

Replace the path with your checkout location. The crate exposes `GCounter`, checked coordinate updates, and checked full-state merge:

```rust
use safemesh_crdt::GCounter;

fn main() {
    let mut left = GCounter::new(2);
    let mut right = GCounter::new(2);
    left.try_apply_bump(0, 3).unwrap();
    right.try_apply_bump(1, 2).unwrap();
    left.try_merge(&right).unwrap();
    right.try_merge(&left).unwrap();
    assert_eq!(left.value(), 5);
    assert_eq!(left.state(), right.state());
    println!("counter={}", left.value());
}
```

This is an in-process state merge. Tallies are cumulative, so resending the same bump does not increment again. `try_apply_bump` rejects an out-of-range coordinate; `try_merge` rejects incompatible replica counts. Handle these errors at your input boundary rather than copying the example's `unwrap` into an untrusted-input path. [Evidence: `GCounter` API](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/src/lib.rs) and [counter model](https://github.com/velvetmonkey/safemesh/blob/main/lean/SafeMesh/DeltaGCounter.lean).

For record exchange, persistence, a child-process exit and restart, follow the [durable Rust walkthrough](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/README.md#persist-restore-partition-and-reconcile). It uses `local::DurableReplica` with `local-writer` on Linux and requires filesystem locks and file/directory sync. Preserve its fence and transaction files; use restart for an existing store, not a fresh constructor. This engineering evidence does not prove storage durability. The [generated reference](/safemesh/reference/) covers the Rust API present in this build.

## C ABI

From the repository root, build the local library:

```sh
(cd rust && cargo build -p safemesh-ffi --release)
```

Include `rust/crates/safemesh-ffi/include/safemesh.h` in your C project and link the built library from `rust/target/release` using your platform's linker configuration. The header exposes opaque handles, status results, and matching release functions. This function illustrates ownership of a counter:

```c
#include "safemesh.h"

SafeMeshStatus example_counter(uint64_t *out) {
    SafeMeshGCounter *counter = safemesh_gcounter_new(2);
    SafeMeshStatus status = safemesh_gcounter_try_apply_bump(counter, 0, 3);
    if (status == Ok) {
        status = safemesh_gcounter_try_value(counter, out);
    }
    safemesh_gcounter_free(counter);
    return status;
}
```

Check status values: `Ok` is 0, `NullPointer` is 1, and `ReplicaOutOfRange` is 2 for this checked bump function. The checked read returns `ValueOverflow` (3) and leaves the output untouched when the true total does not fit `uint64_t`, so a C caller never receives a wrapped total.

Only use the output when the returned status is `Ok`; on any other status, report the error and do not produce a number. Every `uint64_t` value, including zero and `UINT64_MAX`, is a legitimate total, so there is no numeric sentinel for failure. If you copied the earlier `example_counter(void)` snippet that returned zero on error, replace it with this status-returning version and update its callers to check the status before using the output.

Release each owned handle once. OR-Set queries return owned `SafeMeshU64s` arrays, released with `safemesh_u64s_free`; sets use `safemesh_orset_free`. The caller supplies fresh set tokens. [Evidence: FFI contract](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-ffi/README.md) and [header](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-ffi/include/safemesh.h).

The C ABI has carrier operations and a G-Counter delta-to-wire helper, **no replica/event-log surface**. Do not assume the Python or WASM record-exchange examples translate directly to C. The documented repository checks call ABI functions from Rust and check header drift; those checks do not contain an external C compile/link/run test. [Evidence: root surface description](https://github.com/velvetmonkey/safemesh/blob/main/README.md) and [FFI assurance scope](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-ffi/README.md#claim-boundary).

## WASM / TypeScript

Use the Rust/WASM and Node prerequisites from the [browser example](/safemesh/examples/#before-you-start). For a Node integration, build a Node-target package from the repository root:

```sh
wasm-pack build rust/crates/safemesh-wasm --target nodejs --out-dir pkg-node --release
node rust/crates/safemesh-wasm/examples/node-convergence.mjs rust/crates/safemesh-wasm/pkg-node
```

For a bundler application, use `--target bundler` instead. The generated bundler package exports named classes and initializes WASM on import; it has no `init` export. Match the import style to the wasm-pack target. [Evidence: WASM build and quickstart](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-wasm/README.md).

For a Node-target package, the exchange at the center of an application looks like this (save beside `pkg-node` as an `.mjs` file):

```js
import { createRequire } from "node:module";
const require = createRequire(import.meta.url);
const { SafeMeshGCounterReplica } = require("./pkg-node/safemesh_wasm.js");

const left = new SafeMeshGCounterReplica(1n, 3);
const right = new SafeMeshGCounterReplica(2n, 3);
const bytes = left.appendBump(1, 5n);
right.mergeRecordBytes(bytes); // Your transport carries these bytes.
left.mergeLogBytes(right.logBytes());
console.log(left.value(), right.value()); // 5n 5n
left.free();
right.free();
```

The integer values represented as `u64` use JavaScript `bigint`. `SafeMeshStringOrSetReplica` additionally exposes UTF-8 set record/log exchange; `SafeMeshOrSet` exposes whole-state merge over numeric elements. Fresh token allocation remains the caller's responsibility. [Evidence: WASM wrapper guide](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-wasm/README.md).

For the complete TypeScript/Node file persistence and restore path, read [`PERSIST.md`](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-wasm/PERSIST.md). The Linux CI package smoke runs Node; that is not a browser/version/OS support matrix. [Evidence: package smoke](https://github.com/velvetmonkey/safemesh/blob/main/scripts/package-smoke.sh) and [WASM assurance scope](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-wasm/README.md#claim-boundary).

## Python

First [build and install the local wheel in a virtual environment](/safemesh/examples/#python-cold-chain-data-mule). Run your application with that environment's Python. The module exposes the same Rust-backed counter record/log exchange:

```python
import safemesh_python as sm

left = sm.GCounterReplica(1, 3)
right = sm.GCounterReplica(2, 3)
right.merge_record_bytes(left.append_bump(1, 5))
left.merge_log_bytes(right.log_bytes())
print(left.value(), right.value())  # 5 5
```

Here the Python calls hand bytes directly between two in-process objects; your application must move those bytes across its actual transport. `GCounter.try_apply_bump` raises `IndexError` for an invalid coordinate without changing state. Numeric `OrSet` operations take unsigned 64-bit elements and tokens; allocate fresh replica-unique tokens for adds. [Evidence: Python quickstart and token contract](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-python/README.md).

The CI distribution check installs a local wheel on Linux x64 with CPython 3.11. The `abi3-py38` setting describes artifact reach, not evidence that every Python version or platform was tested. The documented data-mule demo is an in-memory partition-and-heal walk; a Python disk/process restore walk is not documented in the root surface map. [Evidence: Python distribution scope](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-python/README.md) and [root walkthrough map](https://github.com/velvetmonkey/safemesh/blob/main/README.md#persist-restore-partition-and-reconcile-rust-and-typescriptnode).

Before committing to an integration, compare the [limits](/safemesh/limits/) with your requirements and read the [proof boundary](/safemesh/proof/#what-remains-outside).
