# SafeMesh CRDT

`safemesh-crdt` is the Rust core of SafeMesh: a `no_std + alloc` delta-state CRDT library with an engineered event log for append, merge, `since`, and version-vector sync.

## Claim boundary

The current Lean-backed surface is G-Set, G-Counter, PN-Counter, OR-Set, and RGA/Text. The Rust implementation earns the verified claim only while `tests/conformance.rs` passes against the Lean-generated oracle corpus from `lake exe corpus`.

`EventLog`, canonical wire encoding, LWW Register, Enable-wins Flag, and LWW Map are engineered and tested Rust product surfaces. They are not Lean-proven in v0.1.

See the repository `CLAIMS.md` and `WHAT-IS-PROVEN.md` for the full wording rule.

## Install

```toml
[dependencies]
safemesh-crdt = "0.1.0"
```

## Quickstart

```rust
use safemesh_crdt::{Crdt, GCounter, GCounterDelta, Mergeable};

let mut left = GCounter::new(2);
let mut right = GCounter::new(2);

let delta = GCounterDelta {
    replica: 0,
    tally: 3,
};
left.apply_delta(delta.clone());
right.apply_delta(delta);
left.merge(&right);

assert_eq!(left.value(), right.value());
```

## Persist, restore, partition and reconcile

This Rust-only walk needs Git, Cargo and a Rust toolchain; no Lean setup or
registry publication is needed. Python, WASM and the C FFI do not expose OR-Set.
Start in an empty directory where you can download and build the repository.
Run these three commands in the same shell:

```sh
git clone --quiet https://github.com/velvetmonkey/safemesh.git safemesh
cd safemesh
cargo run --quiet --manifest-path rust/Cargo.toml -p safemesh-crdt --example m2slice -- ./walk-logs
```

The first two commands produce no output. The third runs
[`examples/m2slice.rs`](examples/m2slice.rs), a small application built entirely
from public Rust APIs. It runs both choices; use its `Replica<C>` wrapper and
the corresponding `journey` call in `main` as the embedding example for your app:

- **Choose and embed (steps 1–2):** `GCounter::new(2)` tracks two replica tallies;
  `OrSet::<String, u64>::new()` tracks strings with unique add tokens. `Replica`
  owns the state and an `EventLog`. Its `local` method records each delta with
  `append_with` and applies it through `Crdt::apply_delta`, using replica IDs 0 and 1.
- **Exchange, persist and restore (steps 3–5):** `exchange` takes missing records
  with `EventLog::since`, encodes each with `Record::to_wire_bytes`, decodes it
  with `Record::from_wire_bytes`, and applies accepted records via `admit_with`.
  `persist` writes each whole log's bytes to a file and calls `sync_all`.
  The app drops both replicas, then `restart` reads and decodes each log and
  replays its deltas into an empty state. This restart happens within one process.
  The persisted log carries no schema or counter arity: the caller must supply
  the delta type and matching empty-state shape, here a two-slot counter or
  a string/token OR-Set.
- **Partition and reconcile (steps 6–8):** the app withholds exchange while each
  replica makes a local update, asserts that their states differ, and writes
  both logs. It then exchanges missing records, asserts equal states and version
  vectors, writes the reconciled logs, and checks that replay matches state and log.

The counter starts at tallies 10 and 20; independent updates produce 11 and 22,
so the reconciled value is **33**. For the set, A removes the observed token 100
for `café☕` while B adds `café☕` with fresh token 201. Token 100 remains tombstoned;
token 201 survives, so both replicas contain **`café☕` and `東京`**. Each final log
has four records. The files are under `walk-logs`: `counter-a.log`, `counter-b.log`,
`utf8-orset-a.log` and `utf8-orset-b.log`.

The example also writes separate `counter-corrupt.log` and
`utf8-orset-corrupt.log` controls. Their output shows a rejected tag mutation
and payload mutations that load changed states. Captured output:

```text
counter step=1 WORKS
counter step=2 WORKS
counter step=3 WORKS
counter step=4 WORKS
counter step=5 WORKS clean-bytes=true
counter step=6 WORKS diverged=true
counter step=7 WORKS
counter step=8 WORKS records-per-replica=4 bytes-a=173 bytes-b=173 state=GCounter { counts: [11, 22] }
utf8-orset step=1 WORKS
utf8-orset step=2 WORKS
utf8-orset step=3 WORKS
utf8-orset step=4 WORKS
utf8-orset step=5 WORKS clean-bytes=true
utf8-orset step=6 WORKS diverged=true
utf8-orset step=7 WORKS
utf8-orset step=8 WORKS records-per-replica=4 bytes-a=179 bytes-b=179 state=OrSet { adds: {("café☕", 100), ("café☕", 201), ("東京", 200)}, tombstones: {100} }
counter corrupt-tag-detected=true error=InvalidTag
counter corrupt-payload-offset=123 restart=Ok wrong-state=GCounter { counts: [10, 22] }
counter corrupt-byte-detected=false
utf8-orset corrupt-tag-detected=true error=InvalidTag
utf8-orset corrupt-payload-offset=163 restart=Ok wrong-state=OrSet { adds: {("bafé☕", 201), ("café☕", 100), ("東京", 200)}, tombstones: {100} }
utf8-orset corrupt-byte-detected=false
```

## Verify locally

```sh
cd rust
cargo publish --dry-run -p safemesh-crdt --allow-dirty
cargo test -p safemesh-crdt --features laws
```

### EventLog persistence frame

`EventLog` uses tag `0x03`, replacing the old `0x02` format without a compatibility
path. The tag is followed by a little-endian u32 body length, its bitwise
complement, the body, and a little-endian CRC-32/ISO-HDLC. The body contains the
shape header described below, the record count and every length-prefixed record,
including each payload. The CRC
covers both length fields and the entire body. The length complement is checked
before using the length; the CRC is checked before decoding records. Tag changes
return `WireError::InvalidTag`; length-pair or checksum mismatches return
`WireError::IntegrityMismatch`. Truncated frames return `UnexpectedEof`.

This detects accidental corruption, including any single changed byte anywhere
in a frame. It is not authentication: a forger can recompute the checksum, and
multiple-byte corruption can have CRC collisions. Record and delta encodings
remain separate wire types and do not gain this frame integrity check.


The shape-bearing layout retains tag `0x03` and the integrity envelope, but
**changes persisted EventLog bytes**. The body starts with little-endian
`u32::MAX` (an impossible record count for a valid old frame), a u32-length-prefixed
stable UTF-8 schema identity, and an arity discriminator: `0` for an unbounded
domain, or `1` followed by the little-endian u64 replica count. The count can be
zero. The existing record count and records follow. All shape fields are covered
by the existing CRC. Schemas describe complete delta types, including enum variants
and element types; nested log/record identities recursively include their payload
schema. Individual record and delta bytes are unchanged.

Construct persisted logs with `EventLog::for_crdt(&state)` (or
`with_replica_count(n)` for an explicitly fixed domain). `EventLog::new()` remains
available for in-memory logs and unbounded CRDTs; encoding an unshaped G-Counter
or PN-Counter log returns `WireError::MissingShape`. Decode at a destination with
`EventLog::from_wire_bytes_for(bytes, &state)` before applying any records.
A different delta schema returns `DeltaTypeMismatch`; a different fixed arity
returns `ReplicaCountMismatch { expected, actual }`; fixed/unbounded disagreement
returns `ArityKindMismatch`. Plain `from_wire_bytes` decodes the log, checks its
delta schema, and retains the saved arity; it does not apply records to a CRDT.
The Python/WASM replica loaders and the `m2slice` persistence example use the
destination-aware decoder.

Existing `0x03` files without the shape header now return `MissingShape`.
There is no automatic migration: old bytes cannot establish the original arity,
including replicas that never emitted a delta. Preserve old files and use the
old release plus independently verified original schema/arity to recover and
re-encode records with the new API. Do not infer arity from the largest coordinate.
New files are also incompatible with old decoders. A G-Counter frame grows by
43 bytes; other overhead is 9 plus the schema byte length, plus 8 for fixed arity.
External payloads persisted in EventLog must implement `WireSchema` with a stable,
unique identity; external fixed-domain CRDTs must implement `Crdt::replica_count`.


## Checked local writers (Linux)

Enable the optional `local-writer` feature to use `local::LocalReplica::counter`
or `local::LocalReplica::utf8_set`. Configure every replica with the same fixed
`WriterConfig::writers` and a distinct `WriterConfig::writer` in `0..writers`.
All local processes for that replica set must use the same storage directory.
The adapter holds an exclusive file lock until drop; keep its fence files in
place. A contending instance is read-only and returns `LocalError::Refused` on
writes. Unsupported locking and filesystem I/O return errors.

Capture `ticket()` and pass it to `bump`, `add`, `remove`, or `receive`. `renew`
revokes older tickets. Counter edits use the configured writer's coordinate;
set adds allocate `sequence * writers + writer` with checked arithmetic. Incoming
adds must carry that record author's token. Removes may reference other writers'
observed tokens. Validation precedes the existing log admission and uses the
ownership rules tested against the Lean-generated corpus. Use `receive` for
individual decoded records; duplicate and reordered valid delivery is supported.

This adapter acknowledges in memory. It does not yet commit edits durably or
recover an existing store: opening a previously initialized store with the lock
available returns `RecoveryRequired`. The default core remains `no_std`; raw
carriers and `EventLog` do not themselves grant an exclusive writer lease.

### Durable local commits (packet B)

With `local-writer`, use `local::DurableReplica::counter` or `utf8_set` for
acknowledged durable edits. Supply an existing, durably created directory on the
supported Linux local filesystem and keep it and its fence files in place.
The same ticket, ownership, append, and receive rules apply. Each accepted edit
replaces one file containing allocation/ownership metadata and the unchanged log
wire bytes; temporary-file write, file sync, rename, and directory sync finish
before success reaches the caller. The example and adapter share this replacement
routine. Any commit I/O error disables further writes and ticket renewal on that
instance; an ambiguous failure may have committed the transaction despite the
error, so callers must not assume it was rolled back.

`CommittedTransaction::read` reopens the committed metadata and log bytes without
granting a write lease. It is a storage read, not checked history validation.
Writable restart remains packet C: durable constructors return `RecoveryRequired`
for an existing store. This path does not claim general power-loss certification.
