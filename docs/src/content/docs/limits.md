---
title: When not to use SafeMesh — main (unreleased)
description: Decide whether the semantics and assurance boundary fit your application.
---

This guide follows **main (unreleased)**. Use it with the [claims ceiling](/safemesh/claims/), not as a release support policy.

## You need guaranteed delivery

SafeMesh's convergence claim is conditional on missing deltas eventually being recovered. It does not prove network or radio delivery. A partition-and-heal example exercises modeled delivery faults; it cannot establish that your real transport will repair every gap. [Evidence: `CLAIMS.md`](https://github.com/velvetmonkey/safemesh/blob/main/CLAIMS.md) and [example scope](/safemesh/examples/).

## You need consensus or leader election

These are explicitly outside v0's coverage. Equal state after joining the same updates does not provide a leader-election protocol. [Evidence: not covered in v0](https://github.com/velvetmonkey/safemesh/blob/main/CLAIMS.md#not-covered-in-v0).

## Your data model needs trees, moves or object references

References between objects, trees and ordered move semantics are outside the stated v0 scope. The shipped RGA model reads live positioned elements; it does not supply identifier allocation machinery. [Evidence: scope exclusions](https://github.com/velvetmonkey/safemesh/blob/main/CLAIMS.md#not-covered-in-v0) and [RGA scope](https://github.com/velvetmonkey/safemesh/blob/main/lean/SafeMesh/DeltaRGA.lean).

## You need a proof of all your application code

The universal results are about Lean definitions. Rust is checked against a finite corpus. Bindings, canonical wire encoding and adapters are outside the Lean claim, and arbitrary user reducers are not proved by SafeMesh. The laws harness supplies tests rather than a theorem prover. [Evidence: tested-not-proven boundary](https://github.com/velvetmonkey/safemesh/blob/main/CLAIMS.md#tested-not-proven).

## Your requirement is a proved flag, map or register

`EnableWinsFlag`, `LwwMap` and `LwwRegister` exist, but are **TESTED, not PROVED**. They have Rust laws/wire evidence and are not in the current Lean CRDT oracle corpus. Do not transfer the five carriers' proof label to these types. [Evidence: claims](https://github.com/velvetmonkey/safemesh/blob/main/CLAIMS.md).

## You need a map of PN-counters

There is no built-in map of PN-counters keyed by SKU names. `LwwMap` chooses a winning value; using it for a stock total loses concurrent increments. A caller can define an application `Crdt` containing a `BTreeMap<String, PnCounter>`, with per-key deltas, consistent replica arity and explicit key creation/removal rules, then supply its wire schema and codecs. Alternatively, keep separate PN-counter logs per stable SKU and route them in the application. Neither composition inherits a SafeMesh proof. Evidence: [`PnCounter`](/safemesh/reference/rust/safemesh_crdt/struct.PnCounter.html), [`LwwMap`](/safemesh/reference/rust/safemesh_crdt/struct.LwwMap.html), and the extension contract [`Crdt`](/safemesh/reference/rust/safemesh_crdt/trait.Crdt.html).

## You need durable restart for a custom CRDT

`DurableReplica` has public constructors and restart entry points for `GCounter` and `OrSet<String, u64>` only. It provides no public generic durable constructor/restart or PN-counter restart. A custom CRDT caller must implement storage and writer ownership: persist its shaped `EventLog`, identity and allocation metadata consistently, decode with `EventLog::from_wire_bytes_for` against the correct empty state, then replay records. Handle file replacement/sync, locks and ambiguous failures before acknowledging edits; `EventLog` encoding alone is not a durable store. Evidence: [`DurableReplica`](/safemesh/reference/rust/safemesh_crdt/local/struct.DurableReplica.html), [`EventLog::from_wire_bytes_for`](/safemesh/reference/rust/safemesh_crdt/struct.EventLog.html#method.from_wire_bytes_for), and [`Crdt::apply_delta`](/safemesh/reference/rust/safemesh_crdt/trait.Crdt.html#tymethod.apply_delta).

## You need a helper to nest wire layouts

There is no compose/map combinator or public `WireCursor` write helper. Callers define their outer framing and a unique `WireSchema`. One approach encodes each inner delta with `to_wire_bytes`, prefixes its checked `u32` byte length in little-endian order, then uses `read_len`, `read_exact`, and the inner `from_wire_bytes` to decode that bounded slice. Reject overflow, truncation and trailing bytes; define field order and enum tags in your own schema. You need not copy private PN-counter or OR-set layouts. Direct composition via `encode_wire` and `decode_wire` on a shared cursor is also available, but the outer schema remains yours. Evidence: [`WireEncode`](/safemesh/reference/rust/safemesh_crdt/trait.WireEncode.html), [`WireDecode`](/safemesh/reference/rust/safemesh_crdt/trait.WireDecode.html), [`WireCursor`](/safemesh/reference/rust/safemesh_crdt/struct.WireCursor.html), and [`WireSchema`](/safemesh/reference/rust/safemesh_crdt/trait.WireSchema.html).

## You need snapshots or compaction

There is no public EventLog snapshot/compaction API. Saving `EventLog` retains every record and payload; `since` selects missing records for exchange but does not shrink stored history. Callers must budget for growth and retain or archive complete history with a recovery plan. Some carriers can encode whole state, but that does not preserve record admission history, writer allocation or a protocol for peers that missed edits. Do not discard old records or tombstones as an optimization. If bounded history is required, design and validate an application checkpoint/epoch and peer-retirement protocol, or choose storage with that facility. Evidence: [`EventLog::records`](/safemesh/reference/rust/safemesh_crdt/struct.EventLog.html#method.records), [`EventLog::since`](/safemesh/reference/rust/safemesh_crdt/struct.EventLog.html#method.since), and its [`WireEncode` implementation](/safemesh/reference/rust/safemesh_crdt/struct.EventLog.html#trait-implementations).

## You need physical or legal truth

Cold-chain examples model software state. They do not prove sensor truth, legal custody, hardware puck behavior, or storage durability. A converged temperature alert does not prove that the sensor reading was true. [Evidence: evaluation limits](https://github.com/velvetmonkey/safemesh/blob/main/CLAIMS.md) and [Python example scope](/safemesh/examples/).

## You need unconditional crash or storage guarantees

Record-kernel proofs cover modeled atomic transitions and replay. They do not prove disk durability or crash atomicity. The Rust durable journey requires Linux and a local filesystem supporting locks and file/directory sync; its checks are not a general power-loss guarantee. [Evidence: record obligations](https://github.com/velvetmonkey/safemesh/blob/main/lean/SafeMesh/RecordKernel.lean) and [durable walkthrough](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/README.md).

## You need established distribution or maintainer support

The source-building examples do not establish registry availability. The root install matrix labels maintainer support **UNKNOWN** for all four surfaces. A generated WASM package running in Node does not establish a browser/OS matrix; a Linux Python wheel smoke test does not establish every interpreter/platform combination. [Evidence: install and status matrix](https://github.com/velvetmonkey/safemesh/blob/main/README.md#status).

If these limits fit your requirements, [try the local Rust example](/safemesh/getting-started/) and then [choose your integration](/safemesh/using-safemesh/).
