---
title: Using SafeMesh in your code — main (unreleased)
description: Rust, C ABI, WASM/TypeScript and Python integration, with transport and persistence responsibilities.
---

This guide uses **main (unreleased)** source. Rust consumers can pin the Git source; the other surfaces use local builds. No registry release is available. **The Lean proof applies to the models; bindings, bytes and transport are TESTED engineering surfaces.** Maintainer support is **UNKNOWN** for all four surfaces. [Evidence: install matrix and status](https://github.com/velvetmonkey/safemesh/blob/main/README.md#install-matrix).

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

After the [first-result example](/safemesh/getting-started/), create an application (Git, Cargo and a Rust toolchain required):

```sh
cargo new --bin --vcs none safemesh-app
cd safemesh-app
```

Add this dependency to the generated `Cargo.toml` (it already has a `[dependencies]` heading):

```toml
[dependencies]
safemesh-crdt = { git = "https://github.com/velvetmonkey/safemesh.git", rev = "6172d7ad7b950e3238f372f378cf8617dcd86984" }
```

The full `rev` pins the source used by these examples: an unpinned Git dependency can resolve to newer main on a fresh resolution or update. Keep the application's `Cargo.lock` too. This is unreleased source, not a registry install.

Cargo searches the Git repository for the package named `safemesh-crdt`; it finds `rust/crates/safemesh-crdt/Cargo.toml` even though there is no root manifest. Use the repository URL, without a subdirectory or `path` field. Evidence: [Cargo's Git dependency rules](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#specifying-dependencies-from-git-repositories), [the pinned package manifest](https://github.com/velvetmonkey/safemesh/blob/6172d7ad7b950e3238f372f378cf8617dcd86984/rust/crates/safemesh-crdt/Cargo.toml), and the [`GCounter` rustdoc item](/safemesh/reference/rust/safemesh_crdt/struct.GCounter.html) exercised below.

Replace `src/main.rs` with this complete program, then run `cargo run --quiet` from `safemesh-app`. It prints `counter=5`:

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

### OR-Set tokens across restart without `local-writer`

Raw `OrSet` and `EventLog` do not allocate or durably reserve add tokens for you. Choose a fixed writer count and distinct writer IDs in `0..writers`. One possible application policy is `token = sequence * writers + writer`, using checked arithmetic and the next local **record** sequence. Removes consume record sequences too. Never change this assignment within the same set's history.

On restart, decode the complete retained log, replay every delta (including removes), and recover the next sequence from **all** records by this writer, not just visible elements or the version vector's contiguous prefix. This keeps removed tokens from being reused. The following program uses writer 0 of 2 and no optional features. Replace `src/main.rs` in the application above, then run `cargo run --quiet` twice. The first run adds and removes token 2; the second replays that history and uses token 6. Run it in a fresh application directory with no `tokens.log`:

```rust
use safemesh_crdt::{Crdt, EventLog, OrSet, OrSetDelta, WireEncode};
use std::{fs, io::ErrorKind};

fn main() {
    // Persist these assignments as application metadata; never reassign a writer.
    let (writers, writer) = (2_u64, 0_u64);
    assert!(writer < writers);
    let mut state = OrSet::<String, u64>::new();
    let mut log = match fs::read("tokens.log") {
        Ok(bytes) => EventLog::from_wire_bytes_for(&bytes, &state).unwrap(),
        Err(e) if e.kind() == ErrorKind::NotFound => EventLog::for_crdt(&state),
        Err(e) => panic!("read failed: {e}"),
    };
    for record in log.records() {
        state.apply_delta(record.delta.clone());
    }
    let sequence = log.records().iter()
        .filter(|record| record.id.replica == writer)
        .map(|record| record.id.sequence).max().unwrap_or(0)
        .checked_add(1).expect("record sequence exhausted");
    let token = sequence.checked_mul(writers)
        .and_then(|n| n.checked_add(writer)).expect("token space exhausted");
    let id = log.append_with(writer, OrSetDelta::Add {
        element: "water".to_owned(), token,
    }, |delta| state.apply_delta(delta.clone())).unwrap();
    assert_eq!(id.sequence, sequence);
    log.append_with(writer, OrSetDelta::Remove { tokens: vec![token] },
        |delta| state.apply_delta(delta.clone())).unwrap();
    // Demonstrates ordinary process restart only; not a durable commit protocol.
    fs::write("tokens.log", log.to_wire_bytes().unwrap()).unwrap();
    println!("allocated token={token}; records={}", log.records().len());
}
```

Only treat a missing file as a new set when provisioning a genuinely new writer/store. For an existing store, missing or corrupt history must stop writes. The application must exclusively own its writer, validate incoming ownership, and persist identity, allocation state and history consistently **before acknowledging or exporting edits**. The simple `fs::write` above does not provide that durability or a lock. Complete history means every previously allocated/exported token is accounted for; a stale backup or a peer missing local edits is insufficient. Preserve a durable high-water mark if tokens can be reserved outside the log; never reset it after loss. Without trustworthy history or allocation metadata, do not resume with the old writer identity.

Evidence: [`EventLog::append_with`](/safemesh/reference/rust/safemesh_crdt/struct.EventLog.html#method.append_with), [`EventLog::records`](/safemesh/reference/rust/safemesh_crdt/struct.EventLog.html#method.records), [`OrSetDelta`](/safemesh/reference/rust/safemesh_crdt/enum.OrSetDelta.html), and the checked adapter's [`DurableReplica::add`](/safemesh/reference/rust/safemesh_crdt/local/struct.DurableReplica.html#method.add). This is caller policy using those APIs, not a public allocator or generic durable restart API. See the [custom restart limit](/safemesh/limits/#you-need-durable-restart-for-a-custom-crdt).

### LWW map wire types

The built-in `WireEncode`, `WireDecode` and `WireSchema` implementations support **`LwwMap<u64, u64>` and `LwwMapDelta<u64, u64>` only**. String keys or values can be used in the generic in-memory map (with its `Ord`/`Clone` bounds), but have no built-in wire codec. For strings on the wire, define an application-owned wrapper/schema and codec, or an agreed stable numeric-ID mapping; do not assign IDs independently at each replica. LWW picks a winning value per key; it does not merge concurrent counter increments.

Replace `src/main.rs` in the application above with this program and run `cargo run --quiet`. It prints `map wire roundtrip=42`:

```rust
use safemesh_crdt::{Crdt, LwwMap, LwwMapDelta, WireDecode, WireEncode};

fn main() {
    let delta = LwwMapDelta::Set {
        key: 7_u64, timestamp: 9, replica: 0, value: 42_u64,
    };
    let bytes = delta.to_wire_bytes().unwrap();
    let decoded = LwwMapDelta::<u64, u64>::from_wire_bytes(&bytes).unwrap();
    let mut map = LwwMap::<u64, u64>::new();
    map.apply_delta(decoded);
    let restored = LwwMap::<u64, u64>::from_wire_bytes(
        &map.to_wire_bytes().unwrap()).unwrap();
    assert_eq!(restored, map);
    assert_eq!(restored.get(&7), Some(&42));
    println!("map wire roundtrip=42");
}
```

Evidence: the trait implementations on [`LwwMap`](/safemesh/reference/rust/safemesh_crdt/struct.LwwMap.html) and [`LwwMapDelta`](/safemesh/reference/rust/safemesh_crdt/enum.LwwMapDelta.html), and [`lww_map_wire_is_stable_and_roundtrips`](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/tests/wire.rs). This map is TESTED, not Lean-proved.

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
const admissions = left.mergeLogBytes(right.logBytes());
console.assert(admissions.join() === "duplicate");
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
admissions = left.merge_log_bytes(right.log_bytes())
assert admissions == ["duplicate"]
print(left.value(), right.value())  # 5 5
```

The batch merge calls return one `accepted`, `duplicate`, or `collision`
verdict per input record, in order, so a caller can identify every applied
record even when a later record collides.

Here the Python calls hand bytes directly between two in-process objects; your application must move those bytes across its actual transport. `GCounter.try_apply_bump` raises `IndexError` for an invalid coordinate without changing state. Numeric `OrSet` operations take unsigned 64-bit elements and tokens; allocate fresh replica-unique tokens for adds. [Evidence: Python quickstart and token contract](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-python/README.md).

The CI distribution check installs a local wheel on Linux x64 with CPython 3.11. The `abi3-py38` setting describes artifact reach, not evidence that every Python version or platform was tested. The documented data-mule demo is an in-memory partition-and-heal walk; a Python disk/process restore walk is not documented in the root surface map. [Evidence: Python distribution scope](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-python/README.md) and [root walkthrough map](https://github.com/velvetmonkey/safemesh/blob/main/README.md#persist-restore-partition-and-reconcile-rust-and-typescriptnode).

Before committing to an integration, compare the [limits](/safemesh/limits/) with your requirements and read the [proof boundary](/safemesh/proof/#what-remains-outside).
