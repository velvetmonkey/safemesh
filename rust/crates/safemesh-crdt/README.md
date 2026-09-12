# SafeMesh CRDT

**v0 scope:** G-Counter and OR-Set are **supported**, within the [language-path limits](https://velvetmonkey.github.io/safemesh/#v0-support). G-Set, PN-Counter, RGA/Text, LWW Register (`LwwRegister`), Enable-wins Flag (`EnableWinsFlag`) and LWW Map (`LwwMap`) are **experimental**, including their deltas and wrappers. Existing proof/test evidence is unchanged by release status.


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


`safemesh-crdt` is the Rust core of SafeMesh: a `no_std + alloc` delta-state CRDT library with an engineered event log for append, merge, `since`, and version-vector sync.

## Claim boundary

The current Lean-backed surface is G-Set, G-Counter, PN-Counter, OR-Set, and RGA/Text. The Rust implementation earns the verified claim only while `tests/conformance.rs` passes against the Lean-generated oracle corpus from `lake exe corpus`.

`EventLog`, canonical wire encoding, LWW Register, Enable-wins Flag, and LWW Map are engineered and tested Rust product surfaces. They are not Lean-proven in v0.1.

See the repository `CLAIMS.md` and `WHAT-IS-PROVEN.md` for the full wording rule.

## Install

```toml
[dependencies]
safemesh-crdt = { git = "https://github.com/velvetmonkey/safemesh.git", rev = "6172d7ad7b950e3238f372f378cf8617dcd86984" }
```

The pinned rev `6172d7ad` is older than current `main` and the gold path, which uses the current checkout.
This is a historical, reproducible G-Counter example. The site's generated rustdoc
uses the site's build commit and can describe APIs absent from this pin. For the
canonical current-source install and matching APIs, follow [Getting started](https://velvetmonkey.github.io/safemesh/getting-started/)
and its checkout-local path dependency. Do not mix the historical dependency with
current-main API examples; keep the checkout and generated reference at the same commit.


A registry release of `safemesh-crdt` has not yet been made. The full `rev` pins
these examples' source independently of future main changes; keep your application's
`Cargo.lock` as well. Cargo finds the package under `rust/crates/safemesh-crdt/`
by searching the Git repository for its manifest, so the Git URL needs neither a
subdirectory nor a root `Cargo.toml`. See [Cargo's Git dependency rules](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#specifying-dependencies-from-git-repositories)
and the [package manifest](Cargo.toml).

The [integration guide](https://velvetmonkey.github.io/safemesh/using-safemesh/#rust)
provides complete runnable programs and rustdoc evidence for this install,
OR-Set token recovery without `local-writer`, and the `u64`-key/`u64`-value LWW
map wire surface. The [limits guide](https://velvetmonkey.github.io/safemesh/limits/)
explains caller responsibilities for a map of PN-counters, custom CRDT restart,
nested wire layouts, and history growth without snapshots or compaction.

## Quickstart

```rust
use safemesh_crdt::{Crdt, GCounter, GCounterDelta};

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
`local-writer` enables everything this example needs; `laws` independently adds
law-checking helpers that the example does not use.

Start in an empty directory where you can download and build the repository.
Use a fresh `walk-logs` directory for this example: its fresh constructors refuse
to overwrite an existing durable store.
Run these three commands in the same shell:

```sh
git clone --quiet https://github.com/velvetmonkey/safemesh.git safemesh
cd safemesh
cargo run --quiet --manifest-path rust/Cargo.toml -p safemesh-crdt --features local-writer --example m2slice -- ./walk-logs && \
  printf 'joined child exit=' && cat ./walk-logs/joined/child.exit && printf '\n' && \
  printf 'counter delta schema=' && strings ./walk-logs/counter-a.log && \
  printf 'UTF-8 set delta schema=' && strings ./walk-logs/utf8-orset-a.log
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
  Each writer advances only its own counter slot, and reconciliation keeps the
  highest tally seen for each slot regardless of arrival order, so neither writer's
  progress is overwritten by a last writer.
  Here writer 0 sets successive tallies of 5, 9 and 12 while writer 1 sets 7,
  giving slot maxima of 12 and 7, exactly the printed `[12, 7]`.
  The members `café☕` and `東京` exercise preservation of two- and three-byte UTF-8
  characters through storage and exchange: `é` is precomposed, and both `☕` and
  the CJK characters are within the Basic Multilingual Plane, so these examples
  do not cover combining accents or four-byte characters outside that plane.
  Using character counts instead of byte lengths would split these strings at
  the wrong boundary and cause decoding to fail or recover incorrect members or
  tokens, a bug ASCII alone would not expose.
- Durable stores live in `walk-logs/joined/counter` and `walk-logs/joined/set`.
  Each `writer-<id>.transaction` holds allocation metadata and log bytes; an edit
  succeeds only after replacement and file/directory sync. Keep the directories
  and `writer-<id>.fence` files in place: each fence stores the writer configuration
  and ticket generation and supplies the exclusive lock that prevents competing
  processes from writing as the same writer. Renewal invalidates old tickets;
  restart requires the existing fence rather than creating a replacement.
  `counter-issued` and `set-issued` are audit copies, not recovery inputs.

Continuous stdout suffix of the fresh walk command above: corruption controls,
child exit, and schema-display commands. The preceding journey stages are omitted.

```text
counter corrupt-tag-detected=true error=InvalidTag
counter corrupt-payload-offset=174 restart=Err(IntegrityMismatch)
counter corrupt-byte-detected=true
utf8-orset corrupt-tag-detected=true error=InvalidTag
utf8-orset corrupt-payload-offset=212 restart=Err(IntegrityMismatch)
utf8-orset corrupt-byte-detected=true
joined child exit=77
counter delta schema=safemesh/gcounter-delta/v1
UTF-8 set delta schema=safemesh/orset-delta-utf8-u64/v1
```

### Reopen the store after the walk (including an overwrite refusal)

If you run the walk twice against the same directory, the second run refuses to
overwrite the durable store. The current harness reports a child
`RecoveryRequired` panic followed by `left: "101"` / `right: "77"`: 77 is the
intentional exit after the child's ACK; 101 is its unexpected panic exit.
Keep `walk-logs`, including every fence and transaction file. Use the recovery
command below to reopen it. To start a separate new walk, choose a different,
unused directory.

From the same repository root, copy this entire shell command. It uses the same
Linux/local-filesystem and Rust prerequisites as the walk, plus `mktemp` and
`mkdir`. It creates a uniquely named small Cargo application beside
`walk-logs` using the local crate. Run it with an absolute repository path
that contains no double quote or backslash, as that path is placed in TOML.
The walk's schema-display command above also needs `strings` (GNU binutils).

```sh
(
  walk_reopen=$(mktemp -d ./walk-reopen.XXXXXX) || exit 1
  mkdir "$walk_reopen/src" || exit 1
  cat > "$walk_reopen/Cargo.toml" <<EOF
[package]
name = "walk-reopen"
version = "0.0.0"
edition = "2021"
[workspace]
[dependencies]
safemesh-crdt = { path = "$(pwd)/rust/crates/safemesh-crdt", features = ["local-writer"] }
EOF
  cat > "$walk_reopen/src/main.rs" <<'EOF'
use safemesh_crdt::{local::DurableReplica, ownership::WriterConfig};
use std::{path::Path, process::ExitCode};

fn reopen(root: &Path) -> Result<(), String> {
    for writer in 0..2 {
        let config = WriterConfig { writers: 2, writer };
        let counter = root.join("joined/counter");
        let set = root.join("joined/set");
        let c = DurableReplica::restart_counter(&counter, config)
            .map_err(|e| format!("{} writer {writer}: {e:?}", counter.display()))?;
        println!("writer {writer} counter={:?}", c.state().state());
        let s = DurableReplica::restart_utf8_set(&set, config)
            .map_err(|e| format!("{} writer {writer}: {e:?}", set.display()))?;
        println!("writer {writer} set={:?}", s.state());
    }
    Ok(())
}
fn main() -> ExitCode {
    let root = std::env::args().nth(1).unwrap_or_else(|| "./walk-logs".into());
    match reopen(Path::new(&root)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Reopen stopped: {e}\nKeep the store and fence files. Check the path, original writer configuration and CRDT type; see the recovery notes. Do not rerun the fresh constructor here.");
            ExitCode::FAILURE
        }
    }
}
EOF
  cargo run --quiet --manifest-path "$walk_reopen/Cargo.toml" -- ./walk-logs
)
```

Stdout of the recovery command after a completed walk:

```text
writer 0 counter=[12, 7]
writer 0 set=OrSet { adds: {("café☕", 2), ("naïve", 6), ("γειά", 3), ("東京", 4)}, tombstones: {} }
writer 1 counter=[12, 7]
writer 1 set=OrSet { adds: {("café☕", 2), ("naïve", 6), ("γειά", 3), ("東京", 4)}, tombstones: {} }
```

After a completed walk, both writers print counter `[12, 7]` and a set whose adds
are `{("café☕", 2), ("naïve", 6), ("γειά", 3), ("東京", 4)}` with no
tombstones. These are the durable stores under `joined/`; the separate
`counter-a.log` / `utf8-orset-a.log` walkthrough finishes at `[11, 22]` and
different set tokens. The command appends no edits, but successful restart
acquires the writer lock and renews its fence generation; it is not a read-only
filesystem inspection. Each handle is dropped on leaving its loop iteration.

#### Recovery notes: which state is this directory in?

The command uses the walk's original configuration: two writers, IDs 0 and 1,
a counter under `joined/counter` and a UTF-8 OR-Set under `joined/set`.
It stops with exit 1 at the first error and prints the path, writer and reason;
earlier successful reopens may already have renewed their fences. It does not
repair damaged files or create replacement stores.

| State when you reach recovery | Result and next action |
| --- | --- |
| Completed walk, with or without a subsequent overwrite refusal | Both writers reopen with the values above; exit 0. Keep using restart for this store. |
| Child ACK persisted, but the parent did not finish | The same command reopens committed progress, not necessarily the final values. In the ACK-only case writer 0 has counter `[9, 0]` and `café☕` / `東京`; writer 1 is empty. Exit 0 does not certify that reconciliation finished. |
| Missing path, an existing empty `walk-logs`, only the earlier non-durable logs, or missing fence/transaction files | `Io(...NotFound...)`. Confirm the path to the original durable store. If nothing was ever persisted, start the original walk in a separate unused directory. Preserve incomplete stores for investigation; do not manufacture missing fences. |
| Another writer's fence/transaction placed at writer 0's path, or a different writer configuration | `Configuration` in the tested writer swap. Use the original application's configuration and paths; do not rename another writer's files into place. |
| A set transaction at the counter path | `History(DeltaTypeMismatch)`. Use the matching CRDT restart API and original store. A different application's store with the same schema and writer configuration may be accepted: these APIs do not establish application identity. Verify provenance before opening it. |
| Transaction truncated part way through its final record | Removing 7 bytes from the 252-byte `joined/counter/writer-0.transaction` produced `History(UnexpectedEof)`. Other corruption may report integrity, history or allocation errors. Preserve the damaged store; recover from a known-good backup with its matching ownership files, or investigate the failure. Restart does not salvage a torn record. |
| Another process holds the writer lock | `Refused`. Coordinate with that writer and retry after it releases the handle. Do not replace the fence or bypass its lock. |
| Permissions/I/O failure, malformed ownership metadata, exhausted generation, unsupported platform/filesystem, or build failure | Recovery is not established. Retain the store, address the reported environment or metadata problem, and retry only with the original configuration. Do not treat an error as permission to initialize over existing data. |

The wrong-type, wrong-writer, empty, missing-fence, lock and truncation cases above
were exercised on disposable copies. Restoring the exact seven removed bytes
from the original transaction made both writers recover the completed values
again. This is a damage-detection control, not a power-loss or backup-restore
guarantee.

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
  The persisted `EventLog` carries the delta schema identity
  (`safemesh/gcounter-delta/v1` for the counter,
  `safemesh/orset-delta-utf8-u64/v1` for the set), shown in the captured output
  above. The caller must still supply the delta type and matching empty-state
  shape, here a two-slot counter or a string/token OR-Set, because decoding
  validates the supplied type and shape rather than constructing a CRDT from the
  recorded identity.
- **Partition and reconcile (steps 6–8):** the app withholds exchange while each
  replica makes a local update, asserts that their states differ, and writes
  both logs. It then exchanges missing records, asserts equal states and version
  vectors, writes the reconciled logs, and checks that replay matches state and log.

The counter’s step 8 line shows the reconciled per-replica tallies 11 and 22;
it prints the counter state rather than their sum. Each slot keeps
its highest tally, so both replicas’ progress survives reconciliation.
For the set, A removes the observed token 100
for `café☕` while B adds `café☕` with fresh token 201. Token 100 remains tombstoned;
token 201 survives, so both replicas contain **`café☕` and `東京`**. Each final log
has four records. The files are under `walk-logs`: `counter-a.log`, `counter-b.log`,
`utf8-orset-a.log` and `utf8-orset-b.log`.

The example also writes separate `counter-corrupt.log` and
`utf8-orset-corrupt.log` controls. Their output shows a rejected tag mutation
and payload mutations rejected by the frame CRC, as shown in the fresh walk
transcript above.

### Recover a corrupt whole log from the other replica

`IntegrityMismatch` means the entire frame was refused before its records could
be replayed. The [Node recovery walk](../safemesh-wasm/PERSIST.md#recover-from-a-healthy-peer-without-the-staged-backup)
measured unchanged files and unchanged in-memory logs after that refusal. Keep
the damaged files: refusal does not itself repair them or justify deleting them.

For the earlier `EventLog` walk, a healthy B log can rebuild A without a pre-made
backup. The completed walk has already delivered every A record to B. Stop writers
and retain the original directory, including intact logs with any unsent edits.
From the repository root after the walk above, run this entire command with the
same Rust prerequisites and a path containing no double quote or backslash.
`walk-recover` and `recovered-logs` must be unused directory names. It validates
B's logs, replays them into fresh state, checks full state and log equality, writes
new A files, reopens them, then reconciles both directions with B.

This command is for `counter-b.log` and `utf8-orset-b.log`, **not** the
`joined/` durable transactions. Do not replace or recreate a durable writer's
fence or transaction from another writer's files. Recovery of a corrupt joined
transaction from a peer is not established here; preserve its ownership files
and use the durable recovery notes above.

```sh
(
  mkdir walk-recover || exit 1
  mkdir walk-recover/src || exit 1
  cat > walk-recover/Cargo.toml <<EOF
[package]
name = "walk-recover"
version = "0.0.0"
edition = "2021"
[workspace]
[dependencies]
safemesh-crdt = { path = "$(pwd)/rust/crates/safemesh-crdt" }
EOF
  cat > walk-recover/src/main.rs <<'RS'
use safemesh_crdt::{Admission, Crdt, EventLog, GCounter, OrSet, WireDecode, WireEncode, WireSchema};
use std::{fmt::Debug, fs, path::Path};

fn recover<C: Crdt + Debug + PartialEq>(name: &str, empty: impl Fn() -> C)
where C::Delta: Clone + PartialEq + WireDecode + WireEncode + WireSchema {
    let mut peer = empty();
    let source = fs::read(format!("walk-logs/{name}-b.log")).unwrap();
    let mut peer_log = EventLog::<C::Delta>::from_wire_bytes_for(&source, &peer).unwrap();
    for record in peer_log.records() { peer.apply_delta(record.delta.clone()); }
    let mut recovered = empty();
    let mut log = EventLog::for_crdt(&recovered);
    for record in peer_log.records().iter().cloned() {
        assert_eq!(log.admit_with(record, |d| recovered.apply_delta(d.clone())), Admission::Accepted);
    }
    assert_eq!(recovered, peer);
    assert_eq!(log.version(), peer_log.version());
    assert!(log == peer_log);
    println!("{name} peer={peer:?}");
    println!("{name} recovered={recovered:?}");
    let path = format!("recovered-logs/{name}-a.log");
    fs::write(&path, log.to_wire_bytes().unwrap()).unwrap();
    // Next action: reopen the recovered file and reconcile in both directions.
    log = EventLog::from_wire_bytes_for(&fs::read(&path).unwrap(), &empty()).unwrap();
    recovered = empty();
    for record in log.records() { recovered.apply_delta(record.delta.clone()); }
    for record in peer_log.records().iter().cloned() {
        assert_eq!(log.admit_with(record, |d| recovered.apply_delta(d.clone())), Admission::Duplicate);
    }
    for record in log.records().iter().cloned() {
        assert_eq!(peer_log.admit_with(record, |d| peer.apply_delta(d.clone())), Admission::Duplicate);
    }
    assert_eq!(recovered, peer);
    assert!(log == peer_log);
    println!("{name} reconciled-peer={peer:?}");
    println!("{name} reconciled-recovered={recovered:?}");
}
fn main() {
    fs::create_dir(Path::new("recovered-logs")).unwrap();
    recover("counter", || GCounter::new(2));
    recover("utf8-orset", OrSet::<String, u64>::new);
    println!("RECOVERED_AND_RECONCILED=true");
}
RS
  cargo run --quiet --manifest-path walk-recover/Cargo.toml
)
```

Stdout (exit 0):

```text
counter peer=GCounter { counts: [11, 22] }
counter recovered=GCounter { counts: [11, 22] }
counter reconciled-peer=GCounter { counts: [11, 22] }
counter reconciled-recovered=GCounter { counts: [11, 22] }
utf8-orset peer=OrSet { adds: {("café☕", 100), ("café☕", 201), ("東京", 200)}, tombstones: {100} }
utf8-orset recovered=OrSet { adds: {("café☕", 100), ("café☕", 201), ("東京", 200)}, tombstones: {100} }
utf8-orset reconciled-peer=OrSet { adds: {("café☕", 100), ("café☕", 201), ("東京", 200)}, tombstones: {100} }
utf8-orset reconciled-recovered=OrSet { adds: {("café☕", 100), ("café☕", 201), ("東京", 200)}, tombstones: {100} }
RECOVERED_AND_RECONCILED=true
```

Only `RECOVERED_AND_RECONCILED=true` certifies both types. A bad set peer
produced `InvalidTag` after the counter had succeeded, leaving partial output;
an error is not a completed recovery.

The new whole-log files are recovery candidates, not replacements installed in
`joined/`. Equality with B establishes what B holds, not whether B received every
acknowledged edit. The Node control demonstrated that a left-only record absent
from the peer is also absent after recovery. Retain any intact local histories;
do not resume writes with an old identity if lost sequences or tokens could be
reused.

Without a healthy peer, backup, or independently retained valid records, a lone
corrupt whole log cannot be recovered through the public APIs. There is no
partial-prefix salvage call: `EventLog::from_wire_bytes_for` returns an error,
not a valid prefix to replay. The individual `Record` decoder is not a salvage
API for a damaged whole-log frame. Preserve the damaged files for investigation.

Before a failure: keep verified backups with matching ownership metadata and replicate acknowledged records to another failure domain.

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

The joined walkthrough demonstrates that acknowledged edits can be recovered
from the child's committed stores after it exits without running destructors,
that writing can resume without reusing the checked record IDs or set tokens,
and that the reconciled state survives a second reopen.
It does not exercise a machine power cut or interruption partway through a commit,
so it does not establish recovery from those failures.
