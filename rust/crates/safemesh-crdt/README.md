# SafeMesh CRDT

`safemesh-crdt` is the Rust core of SafeMesh: a `no_std + alloc` delta-state CRDT library with an engineered event log for append, merge, `since`, and version-vector sync.

## Claim boundary

The current Lean-backed surface is G-Set, G-Counter, PN-Counter, OR-Set, and RGA/Text. The Rust implementation earns the verified claim only while `tests/conformance.rs` passes against the Lean-generated oracle corpus from `lake exe corpus`.

`EventLog`, canonical wire encoding, LWW Register, Enable-wins Flag, and LWW Map are engineered and tested Rust product surfaces. They are not Lean-proven in v0.1.

See the repository `CLAIMS.md` and `WHAT-IS-PROVEN.md` for the full wording rule.

## Install

```toml
[dependencies]
safemesh-crdt = { git = "https://github.com/velvetmonkey/safemesh.git" }
```

A registry release of `safemesh-crdt` has not yet been made.

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
registry publication is needed. Python exposes `OrSet`, WASM exposes
`SafeMeshOrSet`, and the C FFI exposes the `safemesh_orset_*` functions; each wraps
`OrSet<u64, u64>` (u64 elements and caller-supplied u64 tokens). The Rust walk uses
`OrSet<String, u64>` for UTF-8 strings.
The durable journey requires Linux and a local filesystem supporting file locks
and file/directory sync. On macOS or Windows, the command below fails to compile:
the example imports `local`, but that module is gated on both `local-writer` and
Linux. Omitting `--features local-writer` runs only the earlier `EventLog`
walkthrough with a same-process restart, without the durable journey.
`local-writer` enables everything this example needs; `laws` also enables it but
adds law-checking helpers that the example does not use.

Start in an empty directory where you can download and build the repository.
Use a fresh `walk-logs` directory for this example: its fresh constructors refuse
to overwrite an existing durable store.
Run these three commands in the same shell:

```sh
git clone --quiet https://github.com/velvetmonkey/safemesh.git safemesh
cd safemesh
cargo run --quiet --manifest-path rust/Cargo.toml -p safemesh-crdt --features local-writer --example m2slice -- ./walk-logs
```

The first two commands produce no output. The third runs
[`examples/m2slice.rs`](examples/m2slice.rs), a small application built entirely
from public Rust APIs. It first runs the joined durable journey for both a counter
and a UTF-8 set, using `local::DurableReplica`:

- A real child process creates two writers for each CRDT, commits offline edits,
  prints `joined ACK`, and exits with code 77 without running destructors. The
  parent saves and checks that code in `walk-logs/joined/child.exit`, then restarts
  the child's stores through `DurableReplica::restart_counter` and
  `DurableReplica::restart_utf8_set`. Both entry points lock the store, validate
  and replay its history, check allocation metadata, and renew the write ticket.
- The parent commits more edits, checks that record IDs and set tokens were not
  reused, and exchanges records. Both delivery orders converge to counter tallies
  `[12, 7]` and a set containing `café☕`, `東京`, `naïve`, and `γειά`. It drops and
  reopens the replicas again before printing `joined HAPPY`.
- Durable stores live in `walk-logs/joined/counter` and `walk-logs/joined/set`.
  Each `writer-<id>.transaction` holds allocation metadata and log bytes; an edit
  succeeds only after replacement and file/directory sync. Keep the directories
  and `writer-<id>.fence` files in place: each fence stores the writer configuration
  and ticket generation and supplies the exclusive lock that prevents competing
  processes from writing as the same writer. Renewal invalidates old tickets;
  restart requires the existing fence rather than creating a replacement.
  `counter-issued` and `set-issued` are audit copies, not recovery inputs.

The example then runs the earlier `EventLog` walkthrough for both choices. Its
`Replica<C>` wrapper and corresponding `journey` calls illustrate these lower-level
APIs; use `DurableReplica` and `joined::run` for the durable embedding example:

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
  The persisted `EventLog` carries tag `0x03` and a shape header with body sentinel
  `u32::MAX`, the delta schema identity (`safemesh/gcounter-delta/v1` for the counter,
  `safemesh/orset-delta-utf8-u64/v1` for the set), and arity (fixed at 2 for the
  counter, unbounded for the set); the caller must still supply the delta type and
  matching empty-state shape, here a two-slot counter or a string/token OR-Set,
  because decoding validates the supplied type and shape rather than constructing
  a CRDT from the recorded identity.
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
and payload mutations rejected by the frame CRC. Captured output:

```text
joined ACK counter-successive-tallies=5,9 set=café☕,東京 loss=false

counter IDs before=2 after=2 overlap=0 planted-duplicate=detected
set IDs before=2 after=2 overlap=0 planted-duplicate=detected
set tokens before=2 after=2 overlap=0 planted-duplicate=detected
joined reconciled counter=[12, 7] set=OrSet { adds: {("café☕", 2), ("naïve", 6), ("γειά", 3), ("東京", 4)}, tombstones: {} }
joined HAPPY: all acknowledged records survive; both exchange orders converge; second restart survives
counter step=1 WORKS
counter step=2 WORKS
counter step=3 WORKS
counter step=4 WORKS
counter step=5 WORKS clean-bytes=true
counter step=6 WORKS diverged=true
counter step=7 WORKS
counter step=8 WORKS records-per-replica=4 bytes-a=228 bytes-b=228 state=GCounter { counts: [11, 22] }
utf8-orset step=1 WORKS
utf8-orset step=2 WORKS
utf8-orset step=3 WORKS
utf8-orset step=4 WORKS
utf8-orset step=5 WORKS clean-bytes=true
utf8-orset step=6 WORKS diverged=true
utf8-orset step=7 WORKS
utf8-orset step=8 WORKS records-per-replica=4 bytes-a=232 bytes-b=232 state=OrSet { adds: {("café☕", 100), ("café☕", 201), ("東京", 200)}, tombstones: {100} }
counter corrupt-tag-detected=true error=InvalidTag
counter corrupt-payload-offset=174 restart=Err(IntegrityMismatch)
counter corrupt-byte-detected=true
utf8-orset corrupt-tag-detected=true error=InvalidTag
utf8-orset corrupt-payload-offset=212 restart=Err(IntegrityMismatch)
utf8-orset corrupt-byte-detected=true
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
With `local-writer` on Linux, use `local::DurableReplica::restart_counter` or
`local::DurableReplica::restart_utf8_set` to reopen an existing durable store with
its `WriterConfig`. They reacquire the exclusive fence lock, validate and replay
the committed history, check allocation metadata, and renew the write ticket
before returning a writable replica. Any error returns no replica or write ticket.
The fresh `counter` and `utf8_set` constructors still return `RecoveryRequired`
for an existing store. This path does not claim general power-loss certification.
