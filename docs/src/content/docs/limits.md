---
title: When not to use SafeMesh — main (unreleased)
description: Decide whether the semantics and assurance boundary fit your application.
---

**v0 scope:** G-Counter and OR-Set are the **v0 focus** types, within the [language-path limits](/safemesh/#v0-support). G-Set, PN-Counter, RGA/Text, LWW Register (`LwwRegister`), Enable-wins Flag (`EnableWinsFlag`) and LWW Map (`LwwMap`) are **experimental**, including their deltas and wrappers. Existing proof/test evidence is unchanged by release status.


This guide follows **main (unreleased)**. Use it with the [claims ceiling](/safemesh/claims/), not as a release support policy.

## A shopping list versus a bank ledger

An offline shared shopping list fits OR-Set's v0-focus add-wins membership semantics:
a concurrent fresh add survives an observed remove. A bank ledger that must never
accept an unauthorised transfer needs application authorisation and coordination
of its financial invariants. SafeMesh convergence supplies neither transfer
authorisation nor a no-overdraft guarantee; equal merged state does not mean a
transfer was permitted. Paired counters can represent stock changes, but cannot
prevent concurrent offline decrements from overselling.

## Fit check

Use these questions for an initial go/no-go decision, then follow the links for
the detailed limits. Proceed only if your requirements fit the stated scope and
you can own the application work; convergence alone does not close these gaps.

| Requirement question | Library boundary | Decision consequence |
| --- | --- | --- |
| Do you need guaranteed delivery or coordinated decisions? | [Delivery is not proved](#you-need-guaranteed-delivery); [consensus and leader election are outside v0](#you-need-consensus-or-leader-election). | Own missing-delta recovery, authorisation and coordination in the application. If you require SafeMesh alone to guarantee delivery or enforce cross-writer invariants, it does not fit. |
| Must stored history stay bounded? | There is [no public snapshot/compaction API](#you-need-snapshots-or-compaction); saving an EventLog retains all records and payloads. | Budget for complete history, or design and validate a checkpoint/epoch and peer-retirement protocol. If you need built-in bounded history, choose storage with that facility. |
| Do you need durable restart on your target platform and CRDT? | [Public durable restart covers only `GCounter` and `OrSet<String, u64>`](#you-need-durable-restart-for-a-custom-crdt). The Rust journey requires Linux and suitable local filesystem locks/sync; [proofs do not establish disk durability or crash atomicity](#you-need-unconditional-crash-or-storage-guarantees). | Validate the storage/platform assumptions. Custom CRDT storage and writer ownership are yours; if you need generic restart or unconditional crash guarantees from the library, it does not fit. |
| Does your data model require more than the supplied semantics? | [Trees, moves and object references are outside v0](#your-data-model-needs-trees-moves-or-object-references); there is [no built-in map of PN-counters](#you-need-a-map-of-pn-counters) or [compose/map combinator](#you-need-a-helper-to-nest-wire-layouts). | Use a fitting supplied type, or own the composition, consistency rules and wire schema without inheriting a proof. If you require these models ready-made, SafeMesh does not fit. |
| Does your assurance requirement extend beyond the proved Lean definitions? | [Application code, bindings and adapters are outside the Lean claim](#you-need-a-proof-of-all-your-application-code); [flag, map and register are tested, not proved](#your-requirement-is-a-proved-flag-map-or-register). [Physical and legal truth are not proved](#you-need-physical-or-legal-truth). | Supply the additional assurance your application needs. If acceptance requires SafeMesh's proofs to cover these areas, it does not fit. |
| Do you require established packages, platform coverage or maintainer support? | [Source examples do not establish registry availability or a platform matrix; maintainer support is UNKNOWN](#you-need-established-distribution-or-maintainer-support). | Verify distribution and your target environment, and arrange the support you need. If adoption requires established availability or support, defer until those requirements are evidenced. |

## You need guaranteed delivery

SafeMesh's convergence claim is conditional on missing deltas eventually being recovered. It does not prove network or radio delivery. A partition-and-heal example exercises modeled delivery faults; it cannot establish that your real transport will repair every gap. [Evidence: `CLAIMS.md`](https://github.com/velvetmonkey/safemesh/blob/main/CLAIMS.md) and [example scope](/safemesh/examples/).

## You decode collection state from untrusted input

Core collection wire decoding defaults to a **4,096-entry ceiling per count**.
G-Set checks elements; RGA checks placements and tombstones; both OR-Set state
types check add pairs and tombstones; LWW Map checks entries and removals;
Enable-wins Flag checks enables and tombstones. OR-Set Remove and Enable-wins
Disable deltas check token counts. A declared count of 4,097 is rejected before
decoding or allocating those entries. Rust callers with larger trusted states
can use `WireDecode::from_wire_bytes_with_collection_limits` with
`CollectionLimits { max_elements: Some(n) }` (G-Set and RGA also have inherent
methods). `None` removes the ceiling for direct collection decoding. These controls change
decoding only; canonical encoded bytes are unchanged. Cap input bytes as well,
since the element ceiling does not bound the size of the input buffer.

An EventLog whose built-in collection record payload
exceeds the default fails to decode before replay. Rust callers can use
`EventLog::from_wire_bytes_with_limits` with `DecodeLimits::max_collection_elements`
to raise the nested ceiling. `max_records` also bounds records in a nested
EventLog payload. Each record count is checked separately:

```rust
use safemesh_crdt::{DecodeLimits, EventLog, GSet, Record, RecordId};
let records = [Record {
    id: RecordId { replica: 0, sequence: 1 },
    delta: GSet::<u64>::new(),
}];
let mut bytes = Vec::new();
EventLog::encode_records(None, &records, &mut bytes).unwrap();
let log = EventLog::<GSet<u64>>::from_wire_bytes_with_limits(
    &bytes,
    DecodeLimits { max_records: Some(1), max_collection_elements: Some(5_000) },
).unwrap();
assert_eq!(log.records().len(), 1);
```

Python `merge_log_bytes(max_records=n)` and WASM `mergeLogBytes(bytes, undefined, n)`
limit top-level input records before admission.
Rust legacy migration accepts `migrate_legacy_wire_bytes_for_with_limits(bytes,
state, DecodeLimits { max_records: Some(n), ..DecodeLimits::default() })`.
WASM `SafeMeshStringOrSetReplica.importIdentity(bytes, n)` bounds the saved
history before creating a live writer. Omitting either budget retains the
previous unbounded behavior.

For a destination CRDT, use `EventLog::from_wire_bytes_for_with_limits`
to validate its shape before replay. `None` for `max_collection_elements`
retains the 4,096 default for peer bytes. The Linux durable adapter's ordinary
`restart_utf8_set(root, config)` and `restart_counter(root, config)` instead
budget locally committed history from its stored byte length. This lets stores
written before the collection ceiling restart even when a historical OR-Set
Remove contains more than 4,096 tokens. The explicit
`restart_utf8_set_with_max_collection_elements(root, config, n)` remains available
to set a smaller or larger local restart ceiling. A state with more than 4,096
live tokens built from individual Add records also restarts normally: the log
stores those Add deltas, not a serialized whole-state collection. Treat a copied
or imported store file as trusted local input only after validating its source;
the restart budget does not authenticate store provenance. Likewise, a caller
that constructs a `Record` directly or explicitly raises peer decode limits
can pass a larger delta to `DurableReplica::receive`; it is then committed as
local history and ordinary restart will replay it.

## You exchange peer versions from untrusted input

`VersionVector::from_peer_prefixes` accepts positive prefixes through `u64::MAX`, including 1,000,001. It copies the prefix map and independent sequence-zero acknowledgements directly, taking O(r + z) time and additional space for r prefix entries and z zero acknowledgements. Prefix magnitude does not determine reconstruction work. A zero-valued prefix remains noncanonical; carry sequence-zero possession in `zero_replicas`.

Keep three application-owned limits at the receiving boundary; `VersionVector::from_peer_prefixes_with_limits` accepts a `VersionVectorLimits` with optional `max_authors` and `max_zero_replicas` budgets and rejects oversized collections before validating prefixes or cloning either collection (`None` remains unbounded):

- **Author-entry budget:** cap the number of prefix entries before reconstruction to bound validation and map cloning.
- **Zero-acknowledgement budget:** cap the zero-author set before reconstruction to bound set cloning; the prefix-entry budget does not cover it.
- **Encoded-byte budget:** cap the version message before decoding to bound parsing and input allocation, including repeated entries that a map or set would deduplicate.

SafeMesh has a built-in version wire codec with default ceilings of 4,096 authors and 4,096 zero acknowledgements. Applications can choose their own budgets with `VersionVector::from_wire_bytes_with_limits` and should bound encoded bytes before decoding. The peer-prefix constructors retain their caller-selected budgets. These limits bound input cost without imposing a writer-lifetime ceiling. Peer versions remain possession claims: reconstruction does not verify that the sender holds the records. Evidence: [`VersionVector`](/safemesh/reference/rust/safemesh_crdt/struct.VersionVector.html).

## You need consensus or leader election

These are explicitly outside v0's coverage. Equal state after joining the same updates does not provide a leader-election protocol. [Evidence: not covered in v0](https://github.com/velvetmonkey/safemesh/blob/main/CLAIMS.md#not-covered-in-v0).

## Your data model needs trees, moves or object references

References between objects, trees and ordered move semantics are outside the stated v0 scope. The shipped RGA model reads live positioned elements; it does not supply identifier allocation machinery. [Evidence: scope exclusions](https://github.com/velvetmonkey/safemesh/blob/main/CLAIMS.md#not-covered-in-v0) and [RGA scope](https://github.com/velvetmonkey/safemesh/blob/main/lean/SafeMesh/DeltaRGA.lean).

## You need a proof of all your application code

The universal results are about Lean definitions. Rust is checked against a finite corpus. Bindings, canonical wire encoding and adapters are outside the Lean claim, and arbitrary user reducers are not proved by SafeMesh. The laws harness supplies tests rather than a theorem prover. [Evidence: tested-not-proven boundary](https://github.com/velvetmonkey/safemesh/blob/main/CLAIMS.md#tested-not-proven).

## Your requirement is a proved flag, map or register

`EnableWinsFlag`, `LwwMap` and `LwwRegister` exist, but are **TESTED, not PROVED**. They have Rust laws/wire evidence and are not in the current Lean CRDT oracle corpus. Do not transfer the five carriers' proof label to these types. [Evidence: claims](https://github.com/velvetmonkey/safemesh/blob/main/CLAIMS.md).

## You need a map of PN-counters

PN-Counter is **experimental** in v0. Prefer [paired v0-focus G-Counters](/safemesh/persist-and-restart/#decrement-with-v0-focus-types) for added/removed totals, optionally with an OR-Set for membership. The keyed schema and its consistency rules remain application-owned. There is no built-in map of PN-counters keyed by SKU names. `LwwMap` chooses a winning value; using it for a stock total loses concurrent increments. A caller can define an application `Crdt` containing a `BTreeMap<String, PnCounter>`, with per-key deltas, consistent replica arity and explicit key creation/removal rules, then supply its wire schema and codecs. Alternatively, keep separate PN-counter logs per stable SKU and route them in the application. Neither composition inherits a SafeMesh proof. Evidence: [`PnCounter`](/safemesh/reference/rust/safemesh_crdt/struct.PnCounter.html), [`LwwMap`](/safemesh/reference/rust/safemesh_crdt/struct.LwwMap.html), and the extension contract [`Crdt`](/safemesh/reference/rust/safemesh_crdt/trait.Crdt.html).

## You need durable restart for a custom CRDT

`DurableReplica` has public constructors and restart entry points for `GCounter` and `OrSet<String, u64>` only. It provides no public generic durable constructor/restart or PN-counter restart. A custom CRDT caller must implement storage and writer ownership: persist its shaped `EventLog`, identity and allocation metadata consistently, decode with `EventLog::from_wire_bytes_for` against the correct empty state, then replay records. Handle file replacement/sync, locks and ambiguous failures before acknowledging edits; `EventLog` encoding alone is not a durable store. Evidence: [`DurableReplica`](/safemesh/reference/rust/safemesh_crdt/local/struct.DurableReplica.html), [`EventLog::from_wire_bytes_for`](/safemesh/reference/rust/safemesh_crdt/struct.EventLog.html#method.from_wire_bytes_for), and [`Crdt::apply_delta`](/safemesh/reference/rust/safemesh_crdt/trait.Crdt.html#tymethod.apply_delta).

## You need a helper to nest wire layouts

There is no compose/map combinator or public `WireCursor` write helper. Callers define their outer framing and a unique `WireSchema`. One approach encodes each inner delta with `to_wire_bytes`, prefixes its checked `u32` byte length in little-endian order, then uses `read_len`, `read_exact`, and the inner `from_wire_bytes` to decode that bounded slice. Reject overflow, truncation and trailing bytes; define field order and enum tags in your own schema. You need not copy private PN-counter or OR-set layouts. Direct composition via `encode_wire` and `decode_wire` on a shared cursor is also available, but the outer schema remains yours. Evidence: [`WireEncode`](/safemesh/reference/rust/safemesh_crdt/trait.WireEncode.html), [`WireDecode`](/safemesh/reference/rust/safemesh_crdt/trait.WireDecode.html), [`WireCursor`](/safemesh/reference/rust/safemesh_crdt/struct.WireCursor.html), and [`WireSchema`](/safemesh/reference/rust/safemesh_crdt/trait.WireSchema.html).

## You need snapshots or compaction

There is no public EventLog snapshot/compaction API. Saving `EventLog` retains every record and payload; `since` selects missing records for exchange but does not shrink stored history. Callers must budget for growth and retain or archive complete history with a recovery plan. Some carriers can encode whole state, but that does not preserve record admission history, writer allocation or a protocol for peers that missed edits. Do not discard old records or tombstones as an optimization. If bounded history is required, design and validate an application checkpoint/epoch and peer-retirement protocol, or choose storage with that facility. Evidence: [`EventLog::records`](/safemesh/reference/rust/safemesh_crdt/struct.EventLog.html#method.records), [`EventLog::since`](/safemesh/reference/rust/safemesh_crdt/struct.EventLog.html#method.since), and its [`WireEncode` implementation](/safemesh/reference/rust/safemesh_crdt/struct.EventLog.html#trait-implementations).

### Illustrative capacity estimate (not a throughput benchmark)

For one retained log, let **R** be the number of new, unique records per day,
**S** the average encoded record size in bytes/record (including its length
prefix inside the log), and **D** the retention duration in days. Then:

- Retained records: **N = R × D**.
- Encoded-log size: **B = N × S + H bytes**, where **H** is the log frame's fixed overhead.

A concrete size basis is the current G-Counter wire layout: a delta occupies
17 bytes (1-byte tag + 8-byte replica + 8-byte tally). Its record adds
21 bytes (1-byte tag + 8-byte replica + 8-byte sequence + 4-byte delta length),
giving **38 bytes/record**. EventLog adds a 4-byte length prefix per record,
so **S = 42 bytes/record**. Its G-Counter frame adds **H = 60 bytes** for the
tag, lengths, shape marker, schema, arity, record count and checksum.
These sizes are derived from the current encoder, not an assumed average for
other payloads. See the [wire implementation](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/src/lib.rs)
and [canonical-byte tests](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/tests/wire.rs).
The [two-application journey's recorded measurement](https://github.com/velvetmonkey/safemesh/blob/main/demos/two-app-inventory/EVIDENCE.md)
also reports 228,038 bytes of record payloads for 6,001 G-Counter records:
228,038 bytes / 6,001 records = 38 bytes/record, excluding EventLog framing and
SQLite overhead. That journey is not a throughput benchmark.

**Illustrative workload, not measured:** choose R = 10,000 records/day and
D = 365 days. Starting from an empty log:

- N = 10,000 records/day × 365 days = **3,650,000 records**.
- B = 3,650,000 records × 42 bytes/record + 60 bytes = **153,300,060 bytes**,
  or about **153.3 MB** (1 MB = 1,000,000 bytes), for one encoded log copy.

Substitute your own workload and retention duration, and measure encoded sizes
from representative payloads with `to_wire_bytes`. Variable-length strings and
remove-token sets can change the average substantially. Add existing history
to N, and budget separately for replicas, backups, database/filesystem overhead
and temporary copies during saving. This calculation covers encoded-log bytes;
it is not a RAM estimate. Measure in-memory records, carrier state, tombstones,
admission/version indexes and peak encode/decode allocations separately.

Replay is another capacity measurement: this example requires reading and
admitting 3,650,000 records to reconstruct from complete history. Measure
end-to-end recovery time and peak memory on your target hardware, storage and
payload mix at the planned history size; no replay records/second or recovery
time is established by this arithmetic or by the journey's total elapsed time.

The 365-day duration is a sizing horizon, **not permission to expire records**.
Archival does not authorize deleting active history or restoring from incomplete
history. Without a validated checkpoint/epoch and peer-retirement protocol,
continue budgeting for all retained history beyond that horizon and preserve
complete recovery history.

## You need physical or legal truth

Cold-chain examples model software state. They do not prove sensor truth, legal custody, hardware puck behavior, or storage durability. A converged temperature alert does not prove that the sensor reading was true. [Evidence: evaluation limits](https://github.com/velvetmonkey/safemesh/blob/main/CLAIMS.md) and [Python example scope](/safemesh/examples/).

## You need unconditional crash or storage guarantees

Record-kernel proofs cover modeled atomic transitions and replay. They do not prove disk durability or crash atomicity. The Rust durable journey requires Linux and a local filesystem supporting locks and file/directory sync; its checks are not a general power-loss guarantee. [Evidence: record obligations](https://github.com/velvetmonkey/safemesh/blob/main/lean/SafeMesh/RecordKernel.lean) and [durable walkthrough](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/README.md).

## You need established distribution or maintainer support

The source-building examples do not establish registry availability. The root install matrix labels maintainer support **UNKNOWN** for all four surfaces. A generated WASM package running in Node does not establish a browser/OS matrix; a Linux Python wheel smoke test does not establish every interpreter/platform combination. [Evidence: install and status matrix](https://github.com/velvetmonkey/safemesh/blob/main/README.md#status).
The [wire compatibility and upgrades](/safemesh/persist-and-restart/#wire-compatibility-and-upgrades) section lists the toolchain and platform jobs current `main` runs, and names the upgrade behaviours that are not yet promised.

If these limits fit your requirements, [try the local Rust example](/safemesh/getting-started/) and then [choose your integration](/safemesh/using-safemesh/).
