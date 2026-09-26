---
title: Persist and restart — main (unreleased)
description: Add SafeMesh to a Rust or TypeScript program, save an edit, restart and sync a second replica.
---

**v0 scope:** G-Counter and OR-Set are the **v0 focus** types, within the [language-path limits](/safemesh/#v0-support). G-Set, PN-Counter, RGA/Text, LWW Register (`LwwRegister`), Enable-wins Flag (`EnableWinsFlag`) and LWW Map (`LwwMap`) are **experimental**, including their deltas and wrappers. Existing proof/test evidence is unchanged by release status.


Build a small field kit: an observation counter and a membership set. Save `3` and
`compass`, exit the process, restore them, then make independent edits and exchange
bytes with a second replica. The final count is `6`; membership is `compass`, `map`,
`rope`. Both programs assert those values, retained history and the error outcome.

These paths use **main (unreleased)** source and a locally built WASM package.
There is no published SafeMesh package to install. The Rust and TypeScript programs
are complete consumer fixtures, separate from the library workspace; their code,
commands and stdout are checked by the existing documentation CI job.

Choose the [Rust path](#rust-gold-path) for Linux durable storage or the
[TypeScript path (Node)](#typescript-gold-path-node) for caller-owned Node storage.
Complete [Before you start](#before-you-start) for either route, then jump directly
to your chosen path; the optional Rust exercises are separate from the Node route.

## Before you start

SafeMesh's Rust crate floor for consumers is **Rust 1.89**. For the source builds,
demos and locked wasm-pack 0.15.0 installation on this page, use **Rust 1.96.1**,
the prescribed toolchain for this guide and the full-gate CI version. Install
rustup first (Linux/Bash, with curl and a native C compiler/linker), then select
that toolchain:

Run `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.96.1`, then `. "$HOME/.cargo/env"` and `rustup default 1.96.1`.

The default applies to your user account; the repository's `rust-toolchain.toml`
also selects 1.96.1 inside this checkout. An outside application's toolchain remains
its own choice; consuming the crate requires at least 1.89.

Use Linux x86_64, Bash, Git, internet access, Rust/Cargo through rustup and a native
C compiler/linker. The TypeScript path also needs Node.js and npm. The measured
versions and exclusions are in [environments run](/safemesh/evaluate-guarantees/#environments-run). No Lean is
needed. The first build downloads build dependencies and may take several minutes.

These fixtures are on `main`. Obtain the source in an empty working directory:

Run `git clone --branch main https://github.com/velvetmonkey/safemesh.git`, then `cd safemesh`.

Run the chosen path from this repository root and stop if a command fails. Each
`persist` invocation needs its exercise directory to be absent. `restart` runs in
a new process and cleans up that exercise's stores on success. A failed run leaves
them for inspection; use a fresh checkout to retry rather than erasing a real
writer's fence/history. Build outputs remain for reuse.

## Rust gold path

The complete application is in `examples/gold-path/rust/`. It already adds the
local crate by path, including its Linux `local-writer` feature:

<!-- gold:source toml:rust/Cargo.toml -->
```toml
[package]
name = "safemesh-first-use"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
safemesh-crdt = { path = "../../../rust/crates/safemesh-crdt", features = ["local-writer"] }
```
<!-- /gold -->

To adapt it, edit `examples/gold-path/rust/src/main.rs`: the first bump's `3` is a
cumulative observation tally; `compass` is the member to add. Update the matching
assertions for your own application. Keep the two writer IDs distinct (`0` and `1`)
and their configured writer count equal (`2`).

<!-- gold:commands rust -->
```sh
cargo run --quiet --locked --manifest-path examples/gold-path/rust/Cargo.toml -- persist
cargo run --quiet --locked --manifest-path examples/gold-path/rust/Cargo.toml -- restart
```
<!-- /gold -->

The first command prints `saved`; the second process prints the remaining lines.
Stdout is exactly:

<!-- gold:output rust -->
```text
saved counter=3 members=[compass]
restored counter=3 members=[compass]
synced counter=6 members=[compass,map,rope] records=3+3
malformed record: UnexpectedEof
cleaned exercise stores
```
<!-- /gold -->

`DurableReplica::counter` / `utf8_set` create the stores. Each successful edit
commits its own transaction; `restart_counter` / `restart_utf8_set` validate and
replay the existing store while reacquiring the writer. For an existing counter,
`restart_counter_from_store(root, writer)` reads the committed count under that
writer's lock and runs the same checked replay, while explicit `restart_counter`
still rejects a mismatched count. Their collection budget
comes from the locally stored transaction length, so an existing OR-Set store
with a Remove of more than 4,096 tokens still uses ordinary `restart_utf8_set`.
Peer wire decoding keeps the 4,096-element default. A copied store file has no
authenticated provenance; validate its source before using it as local history.
After restart, a tally of
`4` and the peer's `2` yield `6`. The membership writer allocates a fresh token for
each add. Counter and membership use separate stores; there is no transaction
across both objects.

The malformed-record line comes from decoding an empty byte slice before admission:
`UnexpectedEof` is the exact Rust debug error. The fixture asserts that the log is
unchanged. Reject that input and inspect the sender/framing; do not apply its
payload or reset your store.

Read the [complete Rust source and storage boundary](/safemesh/using-safemesh/#rust).

## Diagnose a failed Rust exercise

The fixture uses `expect`/`unwrap`, so failures appear as a Rust panic (exit 101).
On a second `persist`, an existing store returns `RecoveryRequired` (Display:
`local store requires recovery`). Check whether `.gold-rust` existed before this
run: if it did, keep that store and run `restart` with its original writer
configuration, or use a fresh checkout for a separate exercise.

`Refused` on `restart` (Display: `local writer operation refused by ownership or
lease checks`) means another process still holds this writer's fence lock. Stop
that owner and wait for it to release the lock, then retry `restart` with the
same store and writer configuration. Keep the fence file in place; deleting it
does not release the other owner's lock or restore ownership safely.

A truncated writer fence on `restart` returns the same `RecoveryRequired` and
`local store requires recovery`. If the store was already present and this was
a `restart`, inspect its `writer-0.fence` files: each must be 24 bytes. Retain a
damaged store for inspection and recover only from a known consistent copy with
its original writer identity and allocation metadata. Never reset an existing
writer's fence/history.

`Configuration` on `restart` (Display: `invalid or mismatched local writer
configuration`) means the requested writer configuration disagrees with stored
identity or allocation metadata. In your own application, check the `writer` and
`writers` fields of the `WriterConfig` passed to restart against the values used
when the store was created, then retry with the original values. This gold-path
example fixes them at `writer: 0` and `writers: 2`; its commands do not take a
writer flag. If the stored fence or transaction metadata itself differs,
retain the store for inspection and recover only from a known consistent copy
with its original writer identity and allocation metadata. Do not initialize
over the affected store.

`Io(Os { code: 20, kind: NotADirectory, message: "Not a directory" })` on
`restart` (Display: `local store I/O failed: Not a directory (os error 20)`)
can mean an expected store directory is a file on Linux. Check that the store
path is a directory. Retain the affected path for inspection and recover the
directory only from a known consistent copy with its original writer identity
and allocation metadata; then retry `restart`. Do not initialize over it.

`Io(Os { code: 2, kind: NotFound, message: "No such file or directory" })`
on `restart` (Display: `local store I/O failed: No such file or directory
(os error 2)`) can mean a stored writer transaction is missing. Check that
both the writer fence and transaction are present in each store. Retain what
remains for inspection and recover the missing file only from a known consistent
copy with its original writer identity and allocation metadata; then retry
`restart`. Do not initialize over the store.

`InvalidHistory` on `restart` (Display: `local history failed replay or
sequence validation`) can mean the transaction's stored sequence does not
match its replayed history. Retain the store for inspection; recover only from
a known consistent copy with its original writer identity and allocation
metadata, then retry `restart`. Do not initialize over the store.

`Exhausted` on `restart` (Display: `local writer sequence, generation, or token
allocation exhausted`) can mean the stored fence generation cannot advance.
Retain the store for inspection and check its generation and allocation
metadata. Recover only from a known consistent copy with its original writer
identity and allocation metadata, then retry `restart`; do not reset the fence
or initialize over the store.

`History(UnexpectedEof)` on `restart` (Display: `local history wire validation
failed: unexpected end of wire input`) means the stored transaction is truncated
or incomplete. `History(IntegrityMismatch)` (Display: `local history wire
validation failed: wire frame integrity check failed`) means a stored
transaction failed its integrity check. Retain either damaged store for
inspection; restart does not salvage a torn or corrupted record. Investigate
the failed write or transfer, and recover only from a known consistent history
with its original identity and allocation metadata. Do not initialize over the
damaged store. These are tested engineering outcomes, not proofs of crash safety.
`History(TrailingBytes)` (Display: `local history wire validation failed:
unexpected trailing bytes after wire value`) means the stored transaction has
bytes after its complete wire value. `History(InvalidTag)` (Display: `local
history wire validation failed: unexpected wire tag`) means its wire tag is
invalid. `History(DeltaTypeMismatch)` (Display: `local history wire validation
failed: wire delta schema does not match the expected type`) means the stored
transaction has the wrong delta schema. For each, retain the damaged store for
inspection; restart does not salvage the record. Investigate the failed write
or transfer, and recover only from a known consistent history with its original
identity and allocation metadata. Do not initialize over the damaged store.
`LegacyEventLogFrame { found: Tag02 }` or `LegacyEventLogFrame { found:
Tag03Unshaped }` from an `EventLog` loader (Display: `legacy EventLog frame:
found tag 0x02 (no CRC, no shape header), expected tag 0x03 with shape header;
migrate once with EventLog::migrate_legacy_wire_bytes_for(bytes, &destination)
...`) means an earlier `safemesh-crdt` wrote the file, before the frame gained
its CRC or its shape header. The bytes are intact and nothing was applied. Keep
the file, then run the explicit migration once with the original replica count,
reload the result, and replace the file:
see "Migrating a legacy EventLog" in the
[safemesh-crdt README](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/README.md).
Loaders never migrate on open. Durable stores postdate the shape header, so
`restart` meets this only for a hand-edited transaction.
See the [recovery evidence](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/README.md).
The [wire compatibility and upgrades](#wire-compatibility-and-upgrades) section
is the policy for that refusal, the explicit migration, the toolchains current
`main` checks, and the upgrade behaviours that are not yet promised.

<a id="predict-then-run-a-concurrent-add-and-remove"></a>

## Predict, then run: a concurrent add and remove (optional)

Predict whether `milk` remains after one replica removes its observed token while
another adds `milk` with a fresh token. Then run this complete
program from the checkout root with the command below.
It uses the same v0-focus OR-Set carrier without touching the exercise stores.

<!-- gold:source rust:rust/examples/concurrent_add_remove.rs -->
```rust
use safemesh_crdt::OrSet;

fn main() {
    let mut left = OrSet::<String, u64>::new();
    let milk = "milk".to_owned();
    left.add(milk.clone(), 100);
    let mut right = left.clone();
    right.apply_remove(right.observed_tokens(&milk));
    left.add(milk.clone(), 201);
    left.merge(&right);
    right.merge(&left);
    assert!(left.contains(&milk) && right.contains(&milk));
    assert_eq!(left.elements(), right.elements());
    println!("milk remains=true; observed token removed, fresh token survives");
}
```
<!-- /gold -->

<!-- gold:commands concurrent_add_remove -->
```sh
cargo run --quiet --locked --manifest-path examples/gold-path/rust/Cargo.toml --example concurrent_add_remove
```
<!-- /gold -->

Expected output:

<!-- gold:output concurrent_add_remove -->
```text
milk remains=true; observed token removed, fresh token survives
```
<!-- /gold -->

Token `100` was observed and removed; concurrent token `201` survives. Tokens here
are fixed for this one-shot example. A real mutable writer must allocate unique
tokens and preserve identity/allocation on restart as described in the
[OR-Set lifecycle guide](/safemesh/using-safemesh/#rust).

<a id="decrement-with-v0-focus-types"></a>
<a id="decrement-with-supported-types"></a>

## Decrement with v0-focus types (optional)

PN-Counter is **experimental** in v0. Use two v0-focus G-Counters for stock:
one totals additions and the other totals removals. Run this complete
program with its command below.
Each writer owns a distinct coordinate; each bump is its cumulative tally, not
an increment amount. Merge both counters, including after offline edits.

<!-- gold:source rust:rust/examples/paired_counters.rs -->
```rust
use safemesh_crdt::GCounter;

fn main() {
    let mut added_a = GCounter::new(2);
    let mut removed_a = GCounter::new(2);
    added_a.try_apply_bump(0, 10).unwrap();
    let mut added_b = added_a.clone();
    let mut removed_b = removed_a.clone();
    removed_a.try_apply_bump(0, 3).unwrap();
    added_b.try_apply_bump(1, 2).unwrap();
    removed_b.try_apply_bump(1, 1).unwrap();
    added_a.try_merge(&added_b).unwrap();
    removed_a.try_merge(&removed_b).unwrap();
    added_b.try_merge(&added_a).unwrap();
    removed_b.try_merge(&removed_a).unwrap();
    assert_eq!(added_a.state(), added_b.state());
    assert_eq!(removed_a.state(), removed_b.state());
    // Checked conversion/subtraction: counter totals themselves are u128.
    let stock = i128::try_from(added_a.value())
        .unwrap()
        .checked_sub(i128::try_from(removed_a.value()).unwrap())
        .unwrap();
    assert_eq!(stock, 8);
    println!(
        "added={} removed={} stock={stock}",
        added_a.value(),
        removed_a.value()
    );
}
```
<!-- /gold -->

<!-- gold:commands paired_counters -->
```sh
cargo run --quiet --locked --manifest-path examples/gold-path/rust/Cargo.toml --example paired_counters
```
<!-- /gold -->

Expected output:

<!-- gold:output paired_counters -->
```text
added=12 removed=4 stock=8
```
<!-- /gold -->

This demonstrates v0-focus carrier semantics, not a proven inventory schema.
Persist and exchange both totals; two durable stores do not commit atomically.
Keyed stock, membership policy, overflow handling and nonnegative-stock enforcement
remain application responsibilities. Offline decrements can oversell: convergence
does not enforce a global availability constraint. PN/custom durable restart is
outside v0; Rust's Linux durable paths cover G-Counter and UTF-8 OR-Set only.
The separate example leaves the original `persist`/`restart` fixture source intact.

## TypeScript gold path (Node)

The recommended allocated-writer application is in `examples/gold-path/typescript/`. Build the Node
WASM package locally, install the fixture's locked TypeScript build dependencies,
compile against the generated declarations, and run the two processes:

<!-- gold:commands typescript -->
```sh
cargo install wasm-pack --version 0.15.0 --locked
rustup target add wasm32-unknown-unknown
wasm-pack build rust/crates/safemesh-wasm --target nodejs --out-dir "$PWD/examples/gold-path/typescript/pkg" --release
npm --prefix examples/gold-path/typescript ci
npm --prefix examples/gold-path/typescript run build
node examples/gold-path/typescript/main.js persist
node examples/gold-path/typescript/main.js restart
```
<!-- /gold -->

Build tools print progress separately. The two `node` commands' stdout is exactly:

<!-- gold:output typescript -->
```text
saved counter=3 members=[compass]
restored counter=3 members=[compass]
synced counter=6 members=[compass,map,rope] records=3+3
malformed record: SafeMeshError: failed to decode record: unexpected end of wire input
saved latest identity members=[compass,map,rope] local-sequence=2
cleaned exercise stores
```
<!-- /gold -->

Edit `examples/gold-path/typescript/main.ts` to adapt the tally and membership.
The import loads the generated Node CommonJS package synchronously; there is no
`init` call. Keep the whole `pkg/` directory, including `.wasm`, declarations and
`snippets/`. This path uses that directory directly, not an npm SafeMesh tarball.
`tsc` emits `main.js` only if compilation succeeds. Values passed to Rust `u64`
parameters are `bigint` (`3n`), and exchanged bytes are `Uint8Array`.

This Node program writes the counter log and the set's complete
`exportIdentity()` bytes with `writeFileSync`. The second process calls
`importIdentity()` before adding `map`: Rust allocates token `4` after
`compass`'s token `2`; the peer's `rope` gets token `3`. After every set
edit or peer merge, the program saves the latest identity before acknowledging
or exchanging edits. It reloads the final saved identity to check that both the
new add and peer history were persisted, then releases all handles and deletes
only these disposable exercise files.

It demonstrates an **ordinary, single-writer restart**, with no crash-safe commit
or writer fencing. The application owns storage and exclusive author assignment;
never clone a writer or resume an old backup as a live writer. Missing or invalid
identity data stops the program; do not fall back to `createAllocated()`.
Rust's durable adapter is not exported by this binding. The separate file writes
are not an atomic transaction, and `writeFileSync` is not a crash-safe commit.
The [full storage and refusal contract](/safemesh/using-safemesh/#wasm--typescript)
also applies. The legacy caller-token API is an alternative for applications
that intentionally own token allocation; it is not this recommended path.

The fixture passes an empty `Uint8Array` to `mergeRecordBytes`, catches the real
`SafeMeshError: failed to decode record: unexpected end of wire input`, and
asserts no log mutation. Reject the malformed input and check your record
framing. Every WASM handle is freed in `finally`. Read the [complete TypeScript source and project configuration](/safemesh/using-safemesh/#wasm--typescript).

<a id="wire-compatibility-and-upgrades"></a>

## Wire compatibility and upgrades

Current `main` already refuses older EventLog frames, keeps a retained corpus of
those frames, and runs named toolchain and platform jobs. This section is that
policy. It is not a support window.

### Newer SafeMesh, older log

A current EventLog loader refuses a log written with tag `0x02` or with unshaped
tag `0x03` before it decodes any record. The error is
`WireError::LegacyEventLogFrame`, naming the frame found.
[`tests/legacy_event_log.rs`](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/tests/legacy_event_log.rs)
asserts that outcome on `from_wire_bytes`, `from_wire_bytes_for`,
`records_from_wire_bytes_for`, and the bounded loader, then asserts lossless
explicit migration and each file's pinned SHA-256.

Python `merge_log_bytes` raises `ValueError` with
`failed to decode event log: ` in front of that same core text, and does not
apply the bytes
([`safemesh-python` binding test](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-python/src/lib.rs)).
WASM `mergeLogBytes` throws `SafeMeshError` code 1 with that same core text; a
saved identity whose history is a legacy frame is refused the same way
([`safemesh-wasm` binding test](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-wasm/src/lib.rs)).
The generated Node package is exercised by
[`node-error-shape.mjs`](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-wasm/tests/node-error-shape.mjs),
which [`package-smoke.sh`](https://github.com/velvetmonkey/safemesh/blob/main/scripts/package-smoke.sh)
runs from the full gate
([`scripts/ci.sh`](https://github.com/velvetmonkey/safemesh/blob/main/scripts/ci.sh)).

Loaders never migrate on open. Migration is an explicit step. The crate README
section [Migrating a legacy EventLog](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/README.md#migrating-a-legacy-eventlog)
is the single writer for the frame table, the replica-count rule, and the
`migrate_event_log` commands.

The retained corpus
[`tests/fixtures/legacy-event-log/`](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/tests/fixtures/legacy-event-log/README.md)
holds each earlier frame for every built-in delta that could be persisted then.
CI regenerates those files at the commits the fixture README claims, in
[Regenerate legacy EventLog fixtures at claimed sources](https://github.com/velvetmonkey/safemesh/blob/main/.github/workflows/ci.yml),
which runs
[`check-legacy-event-log-fixtures.sh`](https://github.com/velvetmonkey/safemesh/blob/main/scripts/check-legacy-event-log-fixtures.sh).
A separate retained bootstrap corpus is regenerated in
[Regenerate bootstrap fixtures at claimed source](https://github.com/velvetmonkey/safemesh/blob/main/.github/workflows/ci.yml).
Those files are last-writer commits on `main`, not release tags.

### Older SafeMesh, newer log

See [Not yet promised](#not-yet-promised). Current `main` has no job that runs
an older decoder against a current EventLog frame.

### Toolchains and platforms current main checks

The checkout selects Rust **1.96.1** in
[`rust-toolchain.toml`](https://github.com/velvetmonkey/safemesh/blob/main/rust-toolchain.toml).
The crate floor is Rust **1.89**, from
[`rust/Cargo.toml`](https://github.com/velvetmonkey/safemesh/blob/main/rust/Cargo.toml)
`workspace.package.rust-version`. CI
[Install declared Rust floor](https://github.com/velvetmonkey/safemesh/blob/main/.github/workflows/ci.yml),
then [FLOOR COMPILE](https://github.com/velvetmonkey/safemesh/blob/main/.github/workflows/ci.yml)
and [FLOOR TEST](https://github.com/velvetmonkey/safemesh/blob/main/.github/workflows/ci.yml)
at that version. The
[msrv](https://github.com/velvetmonkey/safemesh/blob/main/.github/workflows/ci.yml)
job [Test at declared MSRV](https://github.com/velvetmonkey/safemesh/blob/main/.github/workflows/ci.yml)
runs `cargo test --workspace --locked` at that same declared version.

The
[clippy](https://github.com/velvetmonkey/safemesh/blob/main/.github/workflows/ci.yml)
job and the full-gate step
[Check laws on a non-Linux target](https://github.com/velvetmonkey/safemesh/blob/main/.github/workflows/ci.yml)
`cargo check` the `laws` feature for `x86_64-apple-darwin`. Those jobs run on
`ubuntu-latest`; they are not a macOS runner. Clippy also builds `safemesh-crdt`
for `thumbv7em-none-eabihf` and `safemesh-wasm` for `wasm32-unknown-unknown`.
The full gate
[builds those same targets](https://github.com/velvetmonkey/safemesh/blob/main/scripts/ci.sh).

The
[python](https://github.com/velvetmonkey/safemesh/blob/main/.github/workflows/ci.yml)
job matrix is CPython 3.8–3.14 on `ubuntu-latest`, including
`cargo test -p safemesh-python`. The full-gate job runs on `ubuntu-latest` and
sets Node 24. The
[documentation workflow](https://github.com/velvetmonkey/safemesh/blob/main/.github/workflows/docs.yml)
uses Node 24. wasm-pack **0.15.0** is the locked installer in both the floor
job and the full gate.

These jobs are what current `main` checks. They are not a supported-platform
matrix.

### Not yet promised

Before a first release, the project does not promise:

- **Fixtures from previous releases.** No GitHub release exists
  ([`release-status.mjs`](https://github.com/velvetmonkey/safemesh/blob/main/docs/release-status.mjs)
  reads the GitHub releases list at docs build). The retained EventLog corpus is
  generated from the last main-branch commit that wrote each frame, as
  [`legacy-event-log/README.md`](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/tests/fixtures/legacy-event-log/README.md)
  states.
- **Reproducible release builds.** There is no release build.
- **A version-numbering promise**, including semantic versioning or a support
  window. `safemesh-crdt`
  [`Cargo.toml`](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/Cargo.toml)
  currently records `0.1.0` as an unpublished crate version; that number is not
  a versioning promise.
- **That an older decoder can read a current EventLog frame.** Current `main`
  has no job that runs an older decoder against a new file.
- **Maintainer support.** The root status matrix labels it UNKNOWN for all four
  surfaces
  ([README status](https://github.com/velvetmonkey/safemesh/blob/main/README.md#status)).
- **Runtime on Windows, macOS hosts, ARM64 hosts, browsers, or Python/OS
  combinations outside the jobs named above.**

`WireError` is `#[non_exhaustive]` in
[`codec.rs`](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/src/codec.rs).
The crate README records the matching rule for the first release; this section
does not add a stability window for existing variants.

<a id="history-retention"></a>

## History retention

SafeMesh keeps every committed record forever: restart replays a writer's whole
history, and 5,000 records survive durable persist and restart
([`retention_durable_persist_restart_all_5000`](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/tests/bootstrap.rs)) as well as save, load,
restore and merge replay (the `retention_*_all_5000` tests in
[`record_admission.rs`](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/tests/record_admission.rs)).
There is no compaction in the first release, so a durable store only grows; a
store that a restart budget refused still reopens with every record
([`restart_budget_opens_supported_size_and_refuses_one_more`](https://github.com/velvetmonkey/safemesh/blob/0db39d63d076a6e6d85ff2712355ffadac05be20/rust/crates/safemesh-crdt/src/local.rs)).

### Supported size

The supported history is **100,000 records per writer store**, exported as
[`local::SUPPORTED_MAX_RECORDS`](/safemesh/reference/rust/safemesh_crdt/local/constant.SUPPORTED_MAX_RECORDS.html); a
store of exactly 100,000 records restarts under that budget
([`restart_budget_opens_supported_size_and_refuses_one_more`](https://github.com/velvetmonkey/safemesh/blob/0db39d63d076a6e6d85ff2712355ffadac05be20/rust/crates/safemesh-crdt/src/local.rs)).
The limit comes from write amplification: each durable append rewrites the
whole committed transaction, so one appended record writes the whole store,
about 4.2 MB for a G-Counter and 5.2 MB for a UTF-8 OR-Set at 100,000 records
and ten times that at 1,000,000 ([benchmark results](https://github.com/velvetmonkey/safemesh/blob/0db39d63d076a6e6d85ff2712355ffadac05be20/evidence/retention/results.md)).
On the benchmark machine one acknowledged append at 100,000 records took 50 ms
(G-Counter) and 64 ms (UTF-8 OR-Set); at 1,000,000 records it took 582 ms and
741 ms ([benchmark results](https://github.com/velvetmonkey/safemesh/blob/0db39d63d076a6e6d85ff2712355ffadac05be20/evidence/retention/results.md)).
Restart at 100,000 records took 85 ms and 153 ms with a peak RSS of 22 MiB and
42 MiB; at 1,000,000 records it took 0.9 s and 1.6 s with 199 MiB and 394 MiB
([benchmark results](https://github.com/velvetmonkey/safemesh/blob/0db39d63d076a6e6d85ff2712355ffadac05be20/evidence/retention/results.md)).
Every measured cost grows linearly with the record count, and 100,000 is the
largest measured size at which an acknowledged append stays under a tenth of a
second, so it is the supported size ([benchmark results](https://github.com/velvetmonkey/safemesh/blob/0db39d63d076a6e6d85ff2712355ffadac05be20/evidence/retention/results.md)).
The store occupies 42 bytes per G-Counter record and 52 bytes per UTF-8 OR-Set
add of a 14-byte element ([benchmark results](https://github.com/velvetmonkey/safemesh/blob/0db39d63d076a6e6d85ff2712355ffadac05be20/evidence/retention/results.md)).
These figures come from one machine and toolchain, which the results file
records; run [`retention_bench`](https://github.com/velvetmonkey/safemesh/blob/0db39d63d076a6e6d85ff2712355ffadac05be20/rust/crates/safemesh-crdt/examples/retention_bench.rs) to measure your own.

### Above the supported size

Pass the budget to refuse a larger store by name:
[`restart_counter_with_limits`](/safemesh/reference/rust/safemesh_crdt/local/struct.DurableReplica.html#method.restart_counter_with_limits),
[`restart_counter_from_store_with_limits`](/safemesh/reference/rust/safemesh_crdt/local/struct.DurableReplica.html#method.restart_counter_from_store_with_limits)
and
[`restart_utf8_set_with_limits`](/safemesh/reference/rust/safemesh_crdt/local/struct.DurableReplica.html#method.restart_utf8_set_with_limits)
with `DecodeLimits { max_records: Some(SUPPORTED_MAX_RECORDS), max_collection_elements: None }`
return
[`LocalError::RecordLimitExceeded`](/safemesh/reference/rust/safemesh_crdt/local/enum.LocalError.html#variant.RecordLimitExceeded)
`{ max_records: 100000 }` for a store of 100,001 records (Display: `local
history exceeds restart record budget: RecordLimitExceeded: 100000`)
([`restart_budget_opens_supported_size_and_refuses_one_more`](https://github.com/velvetmonkey/safemesh/blob/0db39d63d076a6e6d85ff2712355ffadac05be20/rust/crates/safemesh-crdt/src/local.rs)).
The check reads only the record count the committed transaction declares,
before any record is read, decoded or replayed
([`restart_budget_refuses_before_reading_records`](https://github.com/velvetmonkey/safemesh/blob/0db39d63d076a6e6d85ff2712355ffadac05be20/rust/crates/safemesh-crdt/src/local.rs)).
The refused store's fence and transaction keep their bytes and modification
times ([`restart_budget_opens_supported_size_and_refuses_one_more`](https://github.com/velvetmonkey/safemesh/blob/0db39d63d076a6e6d85ff2712355ffadac05be20/rust/crates/safemesh-crdt/src/local.rs)).
The count is read before the frame's integrity check, so a damaged count can be
refused by budget; within budget, the full checked read reports the damage as a
`History` error ([`restart_budget_refuses_before_reading_records`](https://github.com/velvetmonkey/safemesh/blob/0db39d63d076a6e6d85ff2712355ffadac05be20/rust/crates/safemesh-crdt/src/local.rs)).
The budget is opt-in: `restart_counter`, `restart_counter_from_store` and
`restart_utf8_set` apply no record budget and still open a store above 100,000
records, outside the supported size
([`restart_budget_opens_supported_size_and_refuses_one_more`](https://github.com/velvetmonkey/safemesh/blob/0db39d63d076a6e6d85ff2712355ffadac05be20/rust/crates/safemesh-crdt/src/local.rs)).
SafeMesh deletes no record to get under the limit; keep a refused store, and
restart it without the budget only if you accept costs beyond those measured
([`restart_budget_opens_supported_size_and_refuses_one_more`](https://github.com/velvetmonkey/safemesh/blob/0db39d63d076a6e6d85ff2712355ffadac05be20/rust/crates/safemesh-crdt/src/local.rs)).
Python and WASM expose no durable restart, so this budget has no binding
surface; their log loaders already take `max_records`
([Python binding tests](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-python/src/lib.rs), [WASM binding tests](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-wasm/src/lib.rs)).

### Not yet promised: compaction, pruning, snapshots

- **Compaction.** No API folds committed records into a smaller history; a
  refused store reopens with every record
  ([`restart_budget_opens_supported_size_and_refuses_one_more`](https://github.com/velvetmonkey/safemesh/blob/0db39d63d076a6e6d85ff2712355ffadac05be20/rust/crates/safemesh-crdt/src/local.rs)).
- **Pruning.** No API drops records by age, count or acknowledgement; the
  retention tests assert every record identity survives
  ([`bootstrap.rs`](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/tests/bootstrap.rs), [`record_admission.rs`](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/tests/record_admission.rs)).
- **Snapshots.** Restart writes and reads no state snapshot; it replays the full
  history, and each benchmark restart asserts it replayed every retained record
  ([`retention_bench`](https://github.com/velvetmonkey/safemesh/blob/0db39d63d076a6e6d85ff2712355ffadac05be20/rust/crates/safemesh-crdt/examples/retention_bench.rs)).

Continue to [Connect replicas](/safemesh/connect-replicas/), or review the
[proof boundary](/safemesh/evaluate-guarantees/#proof-boundary).
