---
title: Architecture — main (unreleased)
description: Locate the architectural source for the main development branch.
---

This guide describes **main (unreleased)**. The proof is in Lean; applications call the Rust core directly or through bindings. Rust conformance is **TESTED** against a finite Lean-generated corpus. It is not a universal proof of the Rust implementation. [Evidence: verification sequence](https://github.com/velvetmonkey/safemesh/blob/main/WHAT-IS-PROVEN.md).

## The layer map

```text
Lean definitions + theorems
          │ compiled definitions emit expected results
          ▼
     JSON oracle corpus ───────► Rust differential tests
                                      │ check
                                      ▼
Application ── Rust API ───────► safemesh-crdt
Application ── C ABI ──────────► safemesh-ffi ────► core
Application ── WASM/TypeScript ► safemesh-wasm ───► core
Application ── Python ─────────► safemesh-python ─► core
```

The arrows from bindings are runtime calls; the corpus arrow is test evidence, not generated production code. [Evidence: architecture and differential bridge](https://github.com/velvetmonkey/safemesh/blob/main/ARCHITECTURE.md).

## 1. Lean: the mathematical semantics

`lean/SafeMesh/` builds on the pinned `crdt-lean` dependency. Its delta suite proves results about joining delivered deltas; the record kernel models admission, replay and fixed-writer ownership decisions. The [proof guide](/safemesh/proof/) explains named theorems and their premises. This layer does not run your transport or prove the Rust compiler correct. [Evidence: `lakefile.toml`](https://github.com/velvetmonkey/safemesh/blob/main/lean/lakefile.toml), [`SafeMesh.lean`](https://github.com/velvetmonkey/safemesh/blob/main/lean/SafeMesh.lean), and [`RecordKernel.lean`](https://github.com/velvetmonkey/safemesh/blob/main/lean/SafeMesh/RecordKernel.lean).

## 2. Rust: the implementation you embed

`safemesh-crdt` provides the five [Lean-backed carriers](/safemesh/concepts/#five-lean-backed-carriers), plus event-log and wire infrastructure. The core uses `no_std + alloc` and denies unsafe code. Its optional `local-writer` module uses `std` on Linux; the embedded-core description is not a portability promise for filesystem persistence. [Evidence: crate source and feature gates](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/src/lib.rs).

`lean/Corpus.lean` emits the expected results; `tests/conformance.rs` compares the Rust implementation with them, including reordered, repeated and split-delivery cases. This finite comparison is the bridge between the two implementations. The corpus generator is separate from the kernel-checked proof path. [Evidence: `Corpus.lean`](https://github.com/velvetmonkey/safemesh/blob/main/lean/Corpus.lean) and [`conformance.rs`](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/tests/conformance.rs).

## 3. Bindings: choose your application's language

| Surface | What you touch | What stays in Rust |
| --- | --- | --- |
| C ABI | Opaque handles, status values, owned byte/array results, and the committed header. | Carrier operations; this ABI has no replica/event-log surface. |
| WASM / TypeScript | Generated wasm-bindgen classes, including replica and record/log byte helpers. | Merge and record interpretation. |
| Python | PyO3 classes in a locally built maturin wheel, including replica and record/log byte helpers. | Merge and record interpretation. |

These are **TESTED** engineering surfaces, not separate Lean proofs. API presence, package availability, build checks, runtime checks, integration checks and maintainer support are distinct questions. [Evidence: root binding status](https://github.com/velvetmonkey/safemesh/blob/main/README.md#status), [C ABI](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-ffi/README.md), [WASM](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-wasm/README.md), and [Python](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-python/README.md).

## Where your application fits

Your application chooses the schema and transport. Records carry deltas; event logs retain them and identify missing records for exchange. Wire encoding moves those values into bytes, while the caller arranges delivery and recovery. `InMemoryTransport` supplies deterministic fault simulation for examples and tests, not a production network. [Evidence: `TransportAdapter`, `EventLog`, and `anti_entropy`](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/src/lib.rs).

### Runtime record lifecycle

Read this as a flow through caller and library operations, not seven separate calls or an end-to-end atomic transaction. Local record creation also admits and applies the delta; receiving a record enters at decode/admission.

| Step | Library-owned operation | Application-owned responsibility |
| --- | --- | --- |
| Local edit | Carrier operations produce deltas; the Linux adapter offers [`DurableReplica::bump`, `add` and `remove`](/safemesh/reference/rust/safemesh_crdt/local/struct.DurableReplica.html#method.bump). | Choose the schema, edit and writer identity; preserve exclusive writer ownership and the fixed configuration. |
| Record creation | [`EventLog::append_with`](/safemesh/reference/rust/safemesh_crdt/struct.EventLog.html#method.append_with) allocates the next record ID and routes through admission and the supplied callback. [`DurableReplica::append`](/safemesh/reference/rust/safemesh_crdt/local/struct.DurableReplica.html#method.append) uses its configured writer and allocation state. | Supply the delta and, for caller-managed logs, the writer ID and state-application callback. |
| Storage / acknowledgement | `EventLog` retains records in memory and can encode them; it is not a durable store. The Linux [`DurableReplica`](/safemesh/reference/rust/safemesh_crdt/local/struct.DurableReplica.html) replaces a transaction containing configuration, allocation sequence and history, syncing the file and directory before applying an accepted delta and returning success. | With caller-managed `EventLog`, persist history, identity and allocation metadata consistently before acknowledging or exporting edits. With the adapter, provision and retain its durable root, handle errors/restart, and acknowledge only after success. Public durable constructors/restart cover G-Counter and UTF-8 OR-Set; see [custom storage obligations](/safemesh/limits/#you-need-durable-restart-for-a-custom-crdt). |
| Transmission | [`WireEncode::to_wire_bytes`](/safemesh/reference/rust/safemesh_crdt/trait.WireEncode.html#method.to_wire_bytes) encodes records; [`TransportAdapter`](/safemesh/reference/rust/safemesh_crdt/trait.TransportAdapter.html) defines exchange operations. | Implement the production transport, delivery and retry policy. Storage success does not acknowledge peer receipt. |
| Decode / admission | [`WireDecode::from_wire_bytes`](/safemesh/reference/rust/safemesh_crdt/trait.WireDecode.html#method.from_wire_bytes) decodes the agreed record type. [`EventLog::admit_with`](/safemesh/reference/rust/safemesh_crdt/struct.EventLog.html#method.admit_with) validates against the destination carrier and checks identity: same ID/payload is `Duplicate`, same ID/different payload is `Collision`. Neither is accepted or invokes the callback. [`DurableReplica::receive`](/safemesh/reference/rust/safemesh_crdt/local/struct.DurableReplica.html#method.receive) also checks writer ownership and the local lease. | Route decoded records to admission against the correct state; handle decode errors, invalid records, refusals and verdicts. Decoding alone does not admit or apply a record. |
| State application | [`Crdt::apply_delta`](/safemesh/reference/rust/safemesh_crdt/trait.Crdt.html#tymethod.apply_delta) updates the carrier. `admit_with` invokes its callback only for `Accepted`; the durable adapter applies accepted deltas itself after persistence. | For `EventLog`, supply an infallible callback with the same delta interpretation as replay. Do not separately apply duplicate or colliding payloads. [`insert_record`](/safemesh/reference/rust/safemesh_crdt/struct.EventLog.html#method.insert_record) only admits/stores; it does not apply state. |
| Gap repair | [`EventLog::since`](/safemesh/reference/rust/safemesh_crdt/struct.EventLog.html#method.since) selects records absent from peer versions; contiguous prefixes preserve gaps. [`anti_entropy`](/safemesh/reference/rust/safemesh_crdt/fn.anti_entropy.html) sends that selection through the adapter. | Exchange peer versions, schedule repair and deliver its records through the same admission/application path. Repair selection supplies no eventual-delivery guarantee. |

The adapter's durability boundary is one local store operation, not a transaction spanning application effects, multiple stores or the network. Plain `admit_with` is an in-memory transition; the application must arrange crash recovery around it.

For code and caller obligations, continue to [using SafeMesh](/safemesh/using-safemesh/). For exclusions, read [when not to use it](/safemesh/limits/).

## Design source and assurance scope


For architecture on **main (unreleased)**, continue to the [design overview in ARCHITECTURE.md](https://github.com/velvetmonkey/safemesh/blob/main/ARCHITECTURE.md).
That source is the place to read about how the implementation layers relate to the proof work.

Use the [claims guide](/safemesh/claims/) when you need to distinguish design context from the scope of an assurance claim.
