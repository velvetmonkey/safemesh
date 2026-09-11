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

For an application-owned record containing multiple deltas, see [Nesting deltas](/safemesh/nesting-deltas/) for a compiled length-prefix pattern and its decode errors.

## Rust

Run the [Rust gold path](/safemesh/getting-started/#rust-gold-path) first. Its
standalone Cargo project consumes the crate by path. This is its complete source,
including imports, assertions, malformed-input handling and exercise cleanup.
The checkout’s `examples/gold-path/rust/src/main.rs` and displayed block are
checked together in documentation CI. Copy the complete program below or edit
that file directly.

<details>
<summary>Complete Rust program</summary>

<!-- gold:source rust:rust/src/main.rs -->
```rust
use safemesh_crdt::{
    local::DurableReplica, ownership::WriterConfig, Admission, GCounter, GCounterDelta, OrSet,
    OrSetDelta, Record, WireDecode, WireEncode, WireError,
};
use std::{fs, path::Path};

type Counter = DurableReplica<GCounter>;
type Members = DurableReplica<OrSet<String, u64>>;

fn main() {
    let step = std::env::args().nth(1).expect("pass persist or restart");
    let root = Path::new(".gold-rust");
    let config = WriterConfig {
        writers: 2,
        writer: 0,
    };
    let counter_dir = root.join("counter");
    let members_dir = root.join("members");
    if step == "persist" {
        fs::create_dir(root).expect("use a fresh directory; never reset an existing writer");
        fs::create_dir(&counter_dir).unwrap();
        fs::create_dir(&members_dir).unwrap();
        let mut counter = Counter::counter(&counter_dir, config).unwrap();
        let mut members = Members::utf8_set(&members_dir, config).unwrap();
        // Each call commits its own transaction before returning.
        counter.bump(counter.ticket(), 3).unwrap();
        members.add(members.ticket(), "compass".into()).unwrap();
        assert_eq!(counter.state().value(), 3);
        assert_eq!(
            members.state().elements(),
            ["compass".to_string()].into_iter().collect()
        );
        println!("saved counter=3 members=[compass]");
    } else {
        assert_eq!(step, "restart");
        // This process obtains a new ticket and restores both state and allocation.
        let mut counter = Counter::restart_counter(&counter_dir, config).unwrap();
        let mut members = Members::restart_utf8_set(&members_dir, config).unwrap();
        assert_eq!(counter.state().value(), 3);
        assert_eq!(
            members.state().elements(),
            ["compass".to_string()].into_iter().collect()
        );
        println!("restored counter=3 members=[compass]");
        counter.bump(counter.ticket(), 4).unwrap(); // cumulative tally, not +4
        members.add(members.ticket(), "map".into()).unwrap(); // allocates a fresh token
        let peer = WriterConfig {
            writers: 2,
            writer: 1,
        };
        let mut other_counter = Counter::counter(&counter_dir, peer).unwrap();
        let mut other_members = Members::utf8_set(&members_dir, peer).unwrap();
        other_counter.bump(other_counter.ticket(), 2).unwrap();
        other_members
            .add(other_members.ticket(), "rope".into())
            .unwrap();
        // In an application, your transport carries these encoded records.
        let counter_bytes: Vec<_> = counter
            .log()
            .records()
            .iter()
            .map(|r| r.to_wire_bytes().unwrap())
            .collect();
        let member_bytes: Vec<_> = members
            .log()
            .records()
            .iter()
            .map(|r| r.to_wire_bytes().unwrap())
            .collect();
        for bytes in counter_bytes {
            let record = Record::<GCounterDelta>::from_wire_bytes(&bytes).unwrap();
            assert_eq!(
                other_counter
                    .receive(other_counter.ticket(), record)
                    .unwrap(),
                Admission::Accepted
            );
        }
        for bytes in member_bytes {
            let record = Record::<OrSetDelta<String, u64>>::from_wire_bytes(&bytes).unwrap();
            assert_eq!(
                other_members
                    .receive(other_members.ticket(), record)
                    .unwrap(),
                Admission::Accepted
            );
        }
        for r in other_counter.log().records() {
            let r = Record::<GCounterDelta>::from_wire_bytes(&r.to_wire_bytes().unwrap()).unwrap();
            assert_ne!(
                counter.receive(counter.ticket(), r).unwrap(),
                Admission::Collision
            );
        }
        for r in other_members.log().records() {
            let r = Record::<OrSetDelta<String, u64>>::from_wire_bytes(&r.to_wire_bytes().unwrap())
                .unwrap();
            assert_ne!(
                members.receive(members.ticket(), r).unwrap(),
                Admission::Collision
            );
        }
        assert_eq!(counter.state().value(), 6);
        assert_eq!(
            members.state().elements(),
            ["compass".to_string(), "map".to_string(), "rope".to_string()]
                .into_iter()
                .collect()
        );
        assert_eq!(counter.state(), other_counter.state());
        assert_eq!(members.state(), other_members.state()); // includes tokens and tombstones
        assert_eq!(counter.log().version(), other_counter.log().version());
        assert_eq!(members.log().version(), other_members.log().version());
        assert_eq!(counter.log().records().len(), 3);
        assert_eq!(members.log().records().len(), 3);
        println!("synced counter=6 members=[compass,map,rope] records=3+3");
        let before = counter.log().to_wire_bytes().unwrap();
        let error = Record::<GCounterDelta>::from_wire_bytes(&[]).unwrap_err();
        assert_eq!(error, WireError::UnexpectedEof);
        assert_eq!(counter.log().to_wire_bytes().unwrap(), before);
        println!("malformed record: {error:?}");
        // Only discard this disposable exercise after all writers release their locks.
        drop((counter, members, other_counter, other_members));
        for dir in [&counter_dir, &members_dir] {
            for entry in fs::read_dir(dir).unwrap() {
                fs::remove_file(entry.unwrap().path()).unwrap();
            }
            fs::remove_dir(dir).unwrap();
        }
        fs::remove_dir(root).unwrap();
        println!("cleaned exercise stores");
    }
}
```
<!-- /gold -->

</details>

The Linux durable adapter requires filesystem locks and file/directory sync.
All instances for the same primitive and writer configuration must use the same
store directory. Preserve the fence and transaction files of real stores; the
fixture deletes only its disposable exercise after releasing every writer. Use
restart constructors for existing stores. A failed restart grants no writer.
These are tested engineering contracts, not a proof of storage durability.
See the [longer durable walkthrough](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/README.md#persist-restore-partition-and-reconcile)
and the [generated Rust reference](/safemesh/reference/).

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

Run the [TypeScript gold path](/safemesh/getting-started/#typescript-gold-path-node)
to build the local Node package, compile this program with `tsc` and execute it.
It imports the real generated declarations and calls the Rust core through WASM.
The checkout’s `examples/gold-path/typescript/` contains these exact files,
`package.json` and the npm lockfile. Copy the complete program below or edit
`main.ts` directly.

<details>
<summary>Complete TypeScript program</summary>

<!-- gold:source ts:typescript/main.ts -->
```ts
import { strict as assert } from "node:assert";
import { mkdirSync, readFileSync, writeFileSync, unlinkSync, rmdirSync } from "node:fs";
import { SafeMeshGCounterReplica as Counter, SafeMeshStringOrSetReplica as Members } from "./pkg/safemesh_wasm";

// tsc checks these imports against the generated package's .d.ts, with no `any` shim.
const step = process.argv[2];
const root = ".gold-typescript";
const counter = new Counter(0n, 2);
const members = new Members(0n);
function memberState(replica: Members) {
  const entries = replica.addEntries();
  try {
    return {
      adds: entries.map(entry => `${entry.element()}:${entry.token()}`).sort(),
      tombstones: Array.from(replica.tombstones(), String).sort(),
    };
  } finally {
    entries.forEach(entry => entry.free());
  }
}
function accepted(verdicts: ("accepted" | "duplicate" | "collision")[]) {
  assert(!verdicts.includes("collision"), `record collision: ${verdicts}`);
}
try {
  if (step === "persist") {
    mkdirSync(root); // refuse to overwrite an existing exercise
    counter.appendBump(0, 3n);
    members.appendAdd("compass", 10n); // application-owned unique token
    writeFileSync(`${root}/counter.log`, counter.logBytes());
    writeFileSync(`${root}/members.log`, members.logBytes());
    assert.equal(counter.value(), 3n);
    assert.deepEqual(members.elements(), ["compass"]);
    console.log("saved counter=3 members=[compass]");
  } else {
    assert.equal(step, "restart", "pass persist or restart");
    // Single-writer exercise: restore the complete logs before allocating new edits.
    accepted(counter.mergeLogBytes(readFileSync(`${root}/counter.log`)));
    accepted(members.mergeLogBytes(readFileSync(`${root}/members.log`)));
    assert.equal(counter.value(), 3n);
    assert.deepEqual(members.elements(), ["compass"]);
    console.log("restored counter=3 members=[compass]");
    counter.appendBump(0, 4n); // new cumulative tally, not +4
    members.appendAdd("map", 11n); // different from every previous add token
    const peerCounter = new Counter(1n, 2);
    const peerMembers = new Members(1n);
    try {
      peerCounter.appendBump(1, 2n);
      peerMembers.appendAdd("rope", 20n);
      // Your transport delivers these Uint8Arrays; this exercise hands them across.
      accepted(peerCounter.mergeLogBytes(counter.logBytes()));
      accepted(peerMembers.mergeLogBytes(members.logBytes()));
      accepted(counter.mergeLogBytes(peerCounter.logBytes()));
      accepted(members.mergeLogBytes(peerMembers.logBytes()));
      assert.equal(counter.value(), 6n);
      assert.deepEqual(members.elements(), ["compass", "map", "rope"]);
      assert(counter.sameStateAs(peerCounter));
      // Logs retain arrival order. Compare all records by duplicate admission,
      // and compare carrier metadata separately, rather than comparing byte order.
      assert.deepEqual(counter.mergeLogBytes(peerCounter.logBytes()), ["duplicate", "duplicate", "duplicate"]);
      assert.deepEqual(members.mergeLogBytes(peerMembers.logBytes()), ["duplicate", "duplicate", "duplicate"]);
      assert.deepEqual(memberState(members), memberState(peerMembers));
      assert.deepEqual(memberState(members), { adds: ["compass:10", "map:11", "rope:20"], tombstones: [] });
      for (const replica of [counter, members, peerCounter, peerMembers]) {
        assert.equal(replica.versionFor(0n), 2n);
        assert.equal(replica.versionFor(1n), 1n);
      }
      console.log("synced counter=6 members=[compass,map,rope] records=3+3");
      const before = counter.logBytes();
      assert.throws(() => counter.mergeRecordBytes(new Uint8Array()), (error: unknown) => {
        assert(error instanceof Error);
        assert.equal(error.name, "SafeMeshError");
        assert.equal(error.message, "failed to decode record");
        console.log(`malformed record: ${error.name}: ${error.message}`);
        return true;
      });
      assert.deepEqual(counter.logBytes(), before);
    } finally {
      peerCounter.free();
      peerMembers.free();
    }
    unlinkSync(`${root}/counter.log`);
    unlinkSync(`${root}/members.log`);
    rmdirSync(root);
    console.log("cleaned exercise stores");
  }
} finally {
  counter.free();
  members.free();
}
```
<!-- /gold -->

</details>

<details>
<summary>TypeScript compiler configuration</summary>

<!-- gold:source json:typescript/tsconfig.json -->
```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "Node16",
    "moduleResolution": "Node16",
    "strict": true,
    "noEmitOnError": true,
    "types": ["node"]
  },
  "files": ["main.ts"]
}
```
<!-- /gold -->

</details>

The Node-target package is CommonJS; TypeScript's `Node16` module mode compiles
this project's imports to `require`. WASM loads synchronously. Keep the generated
package directory whole. Handles require `free()`; `finally` releases them even
when a check throws. The locked build dependencies are installed by the first-use
commands; there is no SafeMesh registry dependency.

Unlike the Rust durable adapter, these replica classes do not provide writer
fencing or atomic disk persistence. The fixture assumes one live process per
writer, a complete log from an ordinary exit, fixed counter width and unique add
tokens chosen by the application. It does not authorize restoring an old backup
or cloning a live writer. Check every batch's admission verdicts: a collision can
follow an already accepted prefix. Never apply rejected payloads yourself.

The [longer Node persistence walkthrough](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-wasm/PERSIST.md)
explores partition and recovery. For a browser/bundler entry point, see the
[browser example](/safemesh/examples/#browser-interactive-convergence) and the
[WASM wrapper guide](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-wasm/README.md).
Node execution does not establish browser support; consult the
[measured environments and exclusions](/safemesh/getting-started/#environments-run).

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
