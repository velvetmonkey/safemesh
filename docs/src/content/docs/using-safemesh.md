---
title: Using SafeMesh in your code — main (unreleased)
description: Rust, C ABI, WASM/TypeScript and Python integration, with transport and persistence responsibilities.
---

**v0 scope:** G-Counter and OR-Set are **supported**, within the [language-path limits](/safemesh/#v0-support). G-Set, PN-Counter, RGA/Text, LWW Register (`LwwRegister`), Enable-wins Flag (`EnableWinsFlag`) and LWW Map (`LwwMap`) are **experimental**, including their deltas and wrappers. Existing proof/test evidence is unchanged by release status.


SafeMesh's Rust crate floor for consumers is **Rust 1.89**. For the source builds,
demos and locked wasm-pack 0.15.0 installation on this page, use **Rust 1.96.1**,
the full-gate CI version. Install rustup first (Linux/Bash, with curl and a native
C compiler/linker), then select that toolchain:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.96.1
. "$HOME/.cargo/env"
rustup default 1.96.1
```

The default applies to your user account; the repository's `rust-toolchain.toml`
also selects 1.96.1 inside this checkout. An outside application's toolchain remains
its own choice; consuming the crate requires at least 1.89.


This guide uses **main (unreleased)** source. These are local build paths, not registry installation promises. **The Lean proof applies to the models; bindings, bytes and transport are TESTED engineering surfaces.** Maintainer support is **UNKNOWN** for all four surfaces. [Evidence: install matrix and status](https://github.com/velvetmonkey/safemesh/blob/main/README.md#install-matrix).

## You bring the transport

SafeMesh supplies CRDT state, deltas, canonical bytes and event-log operations. Your application supplies the schema and moves records between replicas. `TransportAdapter` describes subscription, connectivity, sending batches and draining incoming envelopes; `InMemoryTransport` implements a deterministic test environment. Neither proves real delivery. [Evidence: transport interfaces](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/src/lib.rs).

Choose a Rust API layer by the task and the storage/identity responsibilities you want it to handle:

| API layer | Intended task and operations | Storage and identity ownership | Existing example |
| --- | --- | --- | --- |
| Raw carriers, such as `GCounter` | Merge in-memory state directly with `try_apply_bump` and `try_merge`. | The caller chooses counter width and writer coordinates (or fresh OR-Set tokens), and supplies any persistence. No event log or writer fencing is added. | [Direct G-Counter merge below](#pinned-git-dependency-for-a-separate-application) (historically pinned example). |
| Caller-managed `EventLog` plus a carrier | Add record sequencing, duplicate/collision admission and history exchange with `append_with` / `admit_with`. | The log assigns record sequences; the caller owns writer identities, OR-Set tokens, storage and replay into the correctly shaped state. Admission is an in-memory transition. | [`m2slice.rs` caller-managed journey](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/examples/m2slice.rs). |
| Linux `local::DurableReplica` (`local-writer` feature) | Use the supported Linux restart path for G-Counter or UTF-8 OR-Set, with `bump`, `add` and `receive` over a carrier and event log. | The caller fixes `WriterConfig` and a shared store directory. The adapter fences writers, allocates sequences/add tokens, commits transactions with file/directory sync before acknowledging accepted edits, and validates/replays on restart. | [Rust gold path below](#rust) and [durable process walkthrough](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/README.md#persist-restore-partition-and-reconcile). |

The `Replica<C>` in `m2slice.rs` is an example-defined helper, not a public library type. It combines public carrier and `EventLog` APIs; the same file's Linux durable journey uses the public `local::DurableReplica` instead. All three layers leave transport to the application.

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

Run the [Rust gold path](/safemesh/persist-and-restart/#rust-gold-path) first. Its
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
Ordinary `restart_utf8_set` budgets collection counts from the locally committed
transaction length, including old stores with a Remove of more than 4,096 tokens.
Peer wire decoding still rejects collections above 4,096 elements by default;
check the provenance of a copied store file before treating it as local history.
These are tested engineering contracts, not a proof of storage durability.
See the [longer durable walkthrough](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/README.md#persist-restore-partition-and-reconcile)
and the [generated Rust reference](/safemesh/reference/).

### Pinned Git dependency for a separate application

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

The pinned rev `6172d7ad` is older than the CI-tested revision in the
[pinned-source install recipe](/safemesh/pinned-source/), current `main`, and
the gold path, which uses the current checkout.
This is a historical, reproducible G-Counter example. The site's generated rustdoc
uses the site's build commit and can describe APIs absent from this pin. For the
canonical current-source install and matching APIs, follow [Getting started](https://velvetmonkey.github.io/safemesh/getting-started/)
and its checkout-local path dependency. Do not mix the historical dependency with
current-main API examples; keep the checkout and generated reference at the same commit.


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
    let id = log.append_with(&mut state, writer, OrSetDelta::Add {
        element: "water".to_owned(), token,
    }, |state, delta| state.apply_delta(delta.clone())).unwrap();
    assert_eq!(id.sequence, sequence);
    log.append_with(&mut state, writer, OrSetDelta::Remove { tokens: vec![token] },
        |state, delta| state.apply_delta(delta.clone())).unwrap();
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

The C ABI has carrier operations and a G-Counter delta-to-wire helper, **no replica/event-log surface**. Do not assume the Python or WASM record-exchange examples translate directly to C. The repository checks call ABI functions from Rust, check header drift, and run `scripts/ffi-c-smoke.sh`, which compiles, links and runs an external C caller against the built library on Linux; this is not a platform matrix. [Evidence: root surface description](https://github.com/velvetmonkey/safemesh/blob/main/README.md) and [FFI assurance scope](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-ffi/README.md#claim-boundary).

## WASM / TypeScript

For new UTF-8 OR-Set writers, use the allocated path. SafeMesh fixes the writer
count and author at creation and allocates each add token through the Rust core.
Follow the [TypeScript gold path](/safemesh/persist-and-restart/#typescript-gold-path-node)
to build the Node package and run the complete program below in two processes.
It saves an identity file, imports it in the new process, allocates another add,
and saves and checks the latest identity before releasing the handles.

**Storage and ownership contract:** persist the latest complete identity export
with your storage after every local or peer edit, before acknowledging the edit
or exporting its record/log bytes to peers. The API does **not** make storage
atomic, and a **self-consistent stale snapshot is not detected** on import.
Live-author checks apply only within the same WASM instance; the caller must
ensure **one live writer per author** across instances, tabs and processes.
v0 provides no shared browser lock or cross-tab/process fencing, and exporting
an identity does not stop its live writer.

`exportIdentity()` includes writer count, author, next sequence and the complete
log in one local storage container. It does not change record or log wire bytes.
The **allocation/history consistency check** on `importIdentity(bytes)` validates
configuration, the core log's shape/integrity, every add's author/sequence/token,
complete local history and the saved next sequence. Any mismatch throws before a
live writer is returned; it never starts a fresh identity. Imports also refuse an
author already held by an allocated writer in the same WASM instance. Use
`logBytes()` and record bytes for peer exchange, not identity exports.

Writer count and author must be unsigned 64-bit `bigint` values, with writer
count greater than zero and author below it. Token overflow and an exhausted
sequence refuse before an allocated add changes state; allocated writes at next
sequence `u64::MAX` refuse, while a consistent exhausted identity can still be
exported and restored. Empty and Unicode strings are supported. Incoming records
on allocated replicas must follow the same writer configuration and token rule;
new records claiming the local author refuse. Invalid ownership in a whole log
is rejected before any of that batch is applied.

Legacy instances keep the caller-token behavior of `appendAdd(element, token)`.
Allocated instances reject caller tokens immediately; use
`appendAllocatedAdd(element)` for allocated adds, including after
`exportIdentity()` and `importIdentity(bytes)`. Python and C numeric OR-Set paths
continue to use caller-owned tokens.

The legacy constructor and `appendAdd(element, token)` are an alternative for
callers who intentionally own token allocation, not the recommended path for new
writers. The [caller-token persistence walkthrough](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-wasm/PERSIST.md)
demonstrates that alternative.

The following gold path exercises the recommended allocated-writer API:

Run the [TypeScript gold path](/safemesh/persist-and-restart/#typescript-gold-path-node)
to build the local Node package, compile this program with `tsc` and execute it.
It imports the real generated declarations and calls the Rust core through WASM.
The checkout’s `examples/gold-path/typescript/` contains these exact files,
`package.json` and the npm lockfile. Copy the complete program below or edit
`main.ts` directly.

<details>
<summary>Complete allocated-writer TypeScript program</summary>

<!-- gold:source ts:typescript/main.ts -->
```ts
import { strict as assert } from "node:assert";
import { mkdirSync, readFileSync, writeFileSync, unlinkSync, rmdirSync } from "node:fs";
import { SafeMeshGCounterReplica as Counter, SafeMeshStringOrSetReplica as Members } from "./pkg/safemesh_wasm";

// tsc checks these imports against the generated package's .d.ts, with no `any` shim.
const step = process.argv[2];
const root = ".gold-typescript";
assert(["persist", "restart"].includes(step), "pass persist or restart");
let counter: Counter | undefined;
let members: Members | undefined;
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
  counter = new Counter(0n, 2);
  if (step === "persist") {
    mkdirSync(root); // refuse to overwrite an existing exercise
    members = Members.createAllocated(2n, 0n);
    counter.appendBump(0, 3n);
    members.appendAllocatedAdd("compass"); // Rust allocates token 2
    writeFileSync(`${root}/counter.log`, counter.logBytes());
    writeFileSync(`${root}/members.identity`, members.exportIdentity());
    assert.equal(counter.value(), 3n);
    assert.deepEqual(members.elements(), ["compass"]);
    console.log("saved counter=3 members=[compass]");
  } else {
    assert.equal(step, "restart", "pass persist or restart");
    // No fallback to a new identity if the saved file is missing or invalid.
    members = Members.importIdentity(readFileSync(`${root}/members.identity`));
    accepted(counter.mergeLogBytes(readFileSync(`${root}/counter.log`)));
    assert.equal(counter.value(), 3n);
    assert.deepEqual(members.elements(), ["compass"]);
    console.log("restored counter=3 members=[compass]");
    counter.appendBump(0, 4n); // new cumulative tally, not +4
    members.appendAllocatedAdd("map"); // restored allocation produces token 4
    writeFileSync(`${root}/counter.log`, counter.logBytes());
    writeFileSync(`${root}/members.identity`, members.exportIdentity());
    let peerCounter: Counter | undefined;
    let peerMembers: Members | undefined;
    try {
      peerCounter = new Counter(1n, 2);
      peerMembers = Members.createAllocated(2n, 1n);
      peerCounter.appendBump(1, 2n);
      peerMembers.appendAllocatedAdd("rope"); // Rust allocates token 3
      writeFileSync(`${root}/peer.identity`, peerMembers.exportIdentity());
      // Your transport delivers these Uint8Arrays; this exercise hands them across.
      accepted(peerCounter.mergeLogBytes(counter.logBytes()));
      accepted(peerMembers.mergeLogBytes(members.logBytes()));
      writeFileSync(`${root}/peer.identity`, peerMembers.exportIdentity());
      accepted(counter.mergeLogBytes(peerCounter.logBytes()));
      accepted(members.mergeLogBytes(peerMembers.logBytes()));
      writeFileSync(`${root}/counter.log`, counter.logBytes());
      writeFileSync(`${root}/members.identity`, members.exportIdentity());
      assert.equal(counter.value(), 6n);
      assert.deepEqual(members.elements(), ["compass", "map", "rope"]);
      assert(counter.sameStateAs(peerCounter));
      // Logs retain arrival order. Compare all records by duplicate admission,
      // and compare carrier metadata separately, rather than comparing byte order.
      assert.deepEqual(counter.mergeLogBytes(peerCounter.logBytes()), ["duplicate", "duplicate", "duplicate"]);
      assert.deepEqual(members.mergeLogBytes(peerMembers.logBytes()), ["duplicate", "duplicate", "duplicate"]);
      assert.deepEqual(memberState(members), memberState(peerMembers));
      assert.deepEqual(memberState(members), { adds: ["compass:2", "map:4", "rope:3"], tombstones: [] });
      for (const replica of [counter, members, peerCounter, peerMembers]) {
        assert.equal(replica.versionFor(0n), 2n);
        assert.equal(replica.versionFor(1n), 1n);
      }
      console.log("synced counter=6 members=[compass,map,rope] records=3+3");
      const before = counter.logBytes();
      const malformedTarget = counter;
      assert.throws(() => malformedTarget.mergeRecordBytes(new Uint8Array()), (error: unknown) => {
        assert(error instanceof Error);
        assert.equal(error.name, "SafeMeshError");
        assert.equal(error.message, "failed to decode record");
        console.log(`malformed record: ${error.name}: ${error.message}`);
        return true;
      });
      assert.deepEqual(counter.logBytes(), before);
    } finally {
      peerCounter?.free();
      peerMembers?.free();
    }
    // Release the claim, then verify the latest file, including the post-restart add.
    const latest = memberState(members);
    members.free();
    members = undefined;
    members = Members.importIdentity(readFileSync(`${root}/members.identity`));
    assert.deepEqual(memberState(members), latest);
    assert.equal(members.versionFor(0n), 2n);
    assert.equal(members.versionFor(1n), 1n);
    console.log("saved latest identity members=[compass,map,rope] local-sequence=2");
    counter.free();
    counter = undefined;
    members.free();
    members = undefined;
    unlinkSync(`${root}/counter.log`);
    unlinkSync(`${root}/members.identity`);
    unlinkSync(`${root}/peer.identity`);
    rmdirSync(root);
    console.log("cleaned exercise stores");
  }
} finally {
  counter?.free();
  members?.free();
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

The legacy constructor does not acquire an allocated-author claim.

Unlike the Rust durable adapter, these replica classes do not provide writer
fencing or atomic disk persistence. The fixture assumes one live process per
writer, a complete identity export from an ordinary exit and fixed counter width.
Rust allocates the set tokens from the saved writer configuration and sequence. It does not authorize restoring an old backup
or cloning a live writer. Check every batch's admission verdicts: a collision can
follow an already accepted prefix. Never apply rejected payloads yourself.

The [longer Node persistence walkthrough](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-wasm/PERSIST.md)
explores partition and recovery. For a browser/bundler entry point, see the
[browser example](/safemesh/examples/#browser-interactive-convergence) and the
[WASM wrapper guide](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-wasm/README.md).
Node execution does not establish browser support; consult the
[measured environments and exclusions](/safemesh/evaluate-guarantees/#environments-run).

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

The Python CI matrix builds and installs a local wheel and runs binding and demo/sync tests on Linux x64 with CPython 3.8–3.14; the full distribution check also runs on CPython 3.11. The `abi3-py38` setting describes artifact reach, not evidence that every Python version or platform was tested. Python's data-mule demo is an in-memory partition-and-heal walk. [Fieldcheck](https://github.com/velvetmonkey/safemesh/blob/main/examples/fieldcheck/README.md), a Python CLI and Rust service, walks through disk-backed stores, SIGKILL/reopen recovery, and loopback reconnect. [Evidence: Python distribution scope](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-python/README.md) and [root walkthrough map](https://github.com/velvetmonkey/safemesh/blob/main/README.md#persist-restore-partition-and-reconcile-rust-and-typescriptnode).

Before committing to an integration, compare the [limits](/safemesh/limits/) with your requirements and read the [proof boundary](/safemesh/proof/#what-remains-outside).
