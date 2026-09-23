---
title: Connect replicas — main (unreleased)
description: Exchange encoded records and recover a missing record with a bounded peer version.
---

SafeMesh gives you records and merge rules; your application moves the bytes.
After [Persist and restart](/safemesh/persist-and-restart/), this in-memory exercise
shows that boundary without pretending to test a physical network.

## Exchange and repair

Run the complete example below from the source checkout described in
[Try a merge](/safemesh/getting-started/#before-you-start).
`to_wire_bytes()` produces each outgoing record. At the receiving boundary,
check its byte length, decode it, then use `admit_with` to apply only an accepted
record. A duplicate must not apply the edit again.

The peer reports both its positive contiguous prefixes and its independent
sequence-zero acknowledgements. Reconstruct its claim with
[`VersionVector::from_peer_prefixes_with_limits`](/safemesh/reference/rust/safemesh_crdt/struct.VersionVector.html#method.from_peer_prefixes_with_limits)
and `VersionVectorLimits`: the opt-in author and zero-replica budgets are checked
**before prefix validation and before either collection is cloned**. `None` is
unbounded. These budgets do not bound the version message's decoding allocation:
your decoder must enforce byte and entry limits before constructing either collection.
The built-in version wire codec can decode with explicit entry budgets. This
exercise passes the collections directly between objects; it does not use that
codec.

<!-- gold:source rust:rust/examples/connect_replicas.rs -->
```rust
use safemesh_crdt::{
    Admission, Crdt, EventLog, GCounter, GCounterDelta, Record, VersionVector, VersionVectorLimits,
    WireDecode, WireEncode,
};

fn main() {
    let mut left = GCounter::new(2);
    let mut right = GCounter::new(2);
    let mut sent = EventLog::for_crdt(&left);
    let mut received = EventLog::for_crdt(&right);
    for tally in [4, 6] {
        sent.append_with(
            &mut left,
            0,
            GCounterDelta { replica: 0, tally },
            |state, delta| state.apply_delta(delta.clone()),
        )
        .unwrap();
    }
    // Bytes leave the library here. An application would send these frames.
    let frames: Vec<_> = sent
        .records()
        .iter()
        .map(|record| record.to_wire_bytes().unwrap())
        .collect();
    let receive = |bytes: &[u8], state: &mut GCounter, log: &mut EventLog<GCounterDelta>| {
        assert!(bytes.len() <= 1024); // Check before decoding each record.
        let record = Record::<GCounterDelta>::from_wire_bytes(bytes).unwrap();
        log.admit_with(state, record, |state, delta| {
            state.apply_delta(delta.clone())
        })
    };
    // The first frame is lost; receiving sequence 2 must not acknowledge the gap.
    assert_eq!(
        receive(&frames[1], &mut right, &mut received),
        Admission::Accepted
    );
    assert_eq!(received.version().get(0), 0);
    // This example passes both version collections directly between objects.
    // Enforce byte/entry budgets in that decoder before allocating these collections.
    let peer = VersionVector::from_peer_prefixes_with_limits(
        received.version().entries(),
        received.version().zero_replicas(),
        VersionVectorLimits {
            max_authors: Some(2),
            max_zero_replicas: Some(2),
        },
    )
    .unwrap();
    let missing = sent.since(&peer);
    assert_eq!(missing.len(), 2); // Contiguous prefix conservatively resends sequence 2.
    let repair: Vec<_> = missing
        .iter()
        .map(|record| record.to_wire_bytes().unwrap())
        .collect();
    assert_eq!(
        receive(&repair[0], &mut right, &mut received),
        Admission::Accepted
    );
    assert_eq!(
        receive(&repair[1], &mut right, &mut received),
        Admission::Duplicate
    );
    assert_eq!(received.version().get(0), 2);
    assert_eq!(received.records().len(), 2);
    assert_eq!(right.value(), 6);
    assert_eq!(left.state(), right.state());
    assert!(sent.since(received.version()).is_empty());
    println!("recovered records=2 total=6 duplicate=unchanged");
}
```
<!-- /gold -->

<!-- gold:commands connect_replicas -->
```sh
cargo run --quiet --locked --manifest-path examples/gold-path/rust/Cargo.toml --example connect_replicas
```
<!-- /gold -->

Expected output:

<!-- gold:output connect_replicas -->
```text
recovered records=2 total=6 duplicate=unchanged
```
<!-- /gold -->

`since(peer_version)` selects records the peer has not acknowledged. Receiving
sequence 2 before sequence 1 leaves the prefix at zero, so repair sends both;
sequence 2 is a harmless duplicate. Version claims must come from records actually
admitted: fabricated acknowledgements can hide missing records. Retry repair after
lost messages; eventual delivery remains an application assumption. `since` retains
all history and is not compaction.

## What the application owns

| Responsibility | Application work |
| --- | --- |
| Transport | Framing, byte and entry budgets before decoding, authentication, encryption, routing, retry and exchanging both version collections. SafeMesh does not provide a network or discovery service. |
| Storage | Persist accepted history before durable acknowledgement; choose transaction and recovery behavior. The Linux durable adapter covers the Rust paths, while the Node journey owns its file writes. Retain history: there is no public snapshot/compaction API. |
| Identity allocation | Assign exclusive writer identities and consistent replica counts; preserve allocation on restart, and never run a cloned writer or old backup as a live writer. |

For WASM OR-Sets created with `createAllocated`, [`appendAdd`](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-wasm/src/lib.rs) rejects caller-supplied tokens, so use `appendAllocatedAdd` and preserve `exportIdentity()` / `importIdentity()` state across restart.

Read the [complete integration contracts](/safemesh/using-safemesh/) and
[input and history limits](/safemesh/limits/), then
[Evaluate guarantees](/safemesh/evaluate-guarantees/#proof-boundary).
