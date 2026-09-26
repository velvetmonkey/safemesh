---
title: Concepts — main (unreleased)
description: Replicas, convergence, deltas and the five Lean-backed CRDT carriers.
---

**v0 scope:** G-Counter and OR-Set are the **v0 focus** types, within the [language-path limits](/safemesh/#v0-support). G-Set, PN-Counter, RGA/Text, LWW Register (`LwwRegister`), Enable-wins Flag (`EnableWinsFlag`) and LWW Map (`LwwMap`) are **experimental**, including their deltas and wrappers. Existing proof/test evidence is unchanged by release status.


These concepts describe **main (unreleased)**. The five carriers below have Lean models and finite Rust differential evidence; the Rust implementation is not itself a Lean proof. [Evidence: assurance scope](https://github.com/velvetmonkey/safemesh/blob/main/WHAT-IS-PROVEN.md).

## CRDT

A conflict-free replicated data type combines independently received state with a merge operation. In SafeMesh's Lean model, merge is a **join**: it is commutative (order does not matter), associative (grouping does not matter), and idempotent (repetition does not matter). These laws explain why replaying a delta can leave modeled state unchanged. A custom type must satisfy the same contract; passing the Rust laws harness is **TESTED**, not **PROVED**. [Evidence: `Delta.lean`](https://github.com/velvetmonkey/safemesh/blob/main/lean/SafeMesh/Delta.lean) and [laws harness](https://github.com/velvetmonkey/safemesh/blob/main/WHAT-IS-PROVEN.md#laws-harness).

## Replica

A replica holds a local copy of state. Two replicas may temporarily differ because they have received different updates. A G-Counter has a fixed number of replica coordinates; each coordinate stores a tally. A record also has an identity consisting of a replica ID and a sequence number. Keep those record identities distinct from the application values being synchronized. [Evidence: `GCounter`, `RecordId`, and `EventLog` in the Rust core](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/src/lib.rs).

## Convergence

Here, strong eventual consistency means that replicas which have joined the same delta set hold equal state, regardless of order or repeated delivery. It does not mean replicas agree during a partition. **ASSUMED for eventual recovery:** missing deltas eventually reach the replicas, through delivery or merge/anti-entropy. The theorem does not make that happen. [Evidence: `delta_dissemination_sec`](https://github.com/velvetmonkey/safemesh/blob/main/lean/SafeMesh/Delta.lean) and [claim boundary](https://github.com/velvetmonkey/safemesh/blob/main/CLAIMS.md).

## Delta dissemination

A delta is a mergeable piece of state representing an update; dissemination is its delivery to replicas. For a G-Counter, a delta says “coordinate 0 has tally 3,” not “add 3 again.” Joining that delta twice still leaves that coordinate at 3. `deltaGCounter_correct` states that each coordinate ends at the maximum of its received tallies. [Evidence: `DeltaGCounter.lean`](https://github.com/velvetmonkey/safemesh/blob/main/lean/SafeMesh/DeltaGCounter.lean).

## Anti-entropy

Anti-entropy repairs missing records by exchanging what a peer lacks. SafeMesh's `anti_entropy` uses `EventLog::since(remote_version)` to send records beyond the peer's version. Event-log versions track contiguous per-replica prefixes, so a later record arriving early does not conceal an earlier gap. This is **TESTED** infrastructure; the real transport must still deliver the repair. [Evidence: transport coverage contract](https://github.com/velvetmonkey/safemesh/blob/main/ARCHITECTURE.md#transport-coverage-contract).

For example, a sender holds valid records 1, 2 and 3 from one author A, in that order; a receiver starts with an empty `EventLog`. Here the numbers are A's positive record sequences, not delta values; there is no sequence-zero record. Each arrival is admitted with `EventLog::admit_with` and an application callback that applies the accepted delta.

| Receiver step | Retained sequences (arrival order) | Advertised prefix `receiver.version().get(A)` | Sender's `since(receiver.version())` selects |
| --- | --- | --- | --- |
| Initially | none | 0 | 1, 2, 3 |
| Receive 1 | 1 | 1 | 2, 3 |
| Receive 3 before 2 | 1, 3 | 1 | 2, 3 |
| Receive repair record 2 | 1, 3, 2 | 3 | none |
| Receive the repair's identical copy of 3 | 1, 3, 2 | 3 | none |

Receiving 3 does not acknowledge 2: the prefix covers every positive sequence through its value, and cannot cross the missing 2. Advertising prefix 1 therefore selects both 2 and 3 for repair, even though 3 is already retained. The application must exchange the receiver's version, invoke `anti_entropy` with that version, and have its transport actually deliver the selected records to the receiver for admission. Selection alone does not close the gap. In this trace, the repair delivers 2 then 3: admitting 2 advances the prefix through the already-held 3; the identical resent 3 returns `Admission::Duplicate`, stores no second copy and does not invoke the apply callback. The last two rows show what a fresh `since` call would select, not cancellation of the repair already sent.

This illustrates existing behavior, not a new proof. [Code: `VersionVector`, `EventLog` admission/prefix tracking and `since`, and `anti_entropy`](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/src/lib.rs).

## Five Lean-backed carriers

| Type | In plain words | State and merge | Use the semantics deliberately |
| --- | --- | --- | --- |
| **G-Set** (`GSet`) | A list that only gains items. | A grow-only set; merge takes the union. | Insert members; this type has no removal operation. |
| **G-Counter** (`GCounter`) | A total that can only rise. | Per-replica tallies; merge takes each coordinate's maximum. | Read the sum; deltas carry cumulative tallies. |
| **PN-Counter** (`PnCounter`) | A total that can rise and fall. | Two G-Counters, for increments and decrements. | Read the increment total minus the decrement total. |
| **OR-Set** (`OrSet`) | A list that allows additions and removals; a new simultaneous add survives a remove. | Tagged adds plus removed-token tombstones; merge unions both. | A remove names observed tokens; a concurrent add with a fresh token survives. |
| **RGA/Text** (`Rga`) | An ordered list whose entries can be deleted. | Positioned elements plus deleted-position tombstones. | Read live positions in sorted order; the caller supplies position identifiers. |

Evidence: [Rust carrier implementations](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/src/lib.rs), [OR-Set model](https://github.com/velvetmonkey/safemesh/blob/main/lean/SafeMesh/DeltaORSet.lean), [RGA model](https://github.com/velvetmonkey/safemesh/blob/main/lean/SafeMesh/DeltaRGA.lean), and [the differential test](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/tests/conformance.rs).

A **tombstone** is a record that an item was removed, so an older copy of that item does not come back when copies merge. It records a removal while retaining information needed when older adds arrive later. OR-Set tokens are global to the set: reusing a token can affect every element carrying it. The numeric bindings do not allocate tokens for you. [Evidence: binding token contract](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-ffi/README.md#checked-coordinates-and-or-set).

## Other types have a different assurance label

`LwwRegister`, `EnableWinsFlag`, and `LwwMap` are also present, but are **TESTED, not PROVED**. They are not in the current Lean CRDT oracle corpus. `EventLog` supplies record infrastructure; it is not a sixth Lean-backed application-state carrier. [Evidence: claims ceiling](https://github.com/velvetmonkey/safemesh/blob/main/CLAIMS.md).

Next: see [which layer you call](/safemesh/architecture/) or [choose an integration](/safemesh/using-safemesh/).
