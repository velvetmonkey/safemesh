---
title: Getting started — main (unreleased)
description: Add SafeMesh to a Rust or TypeScript program, save an edit, restart and sync a second replica.
---

**v0 scope:** G-Counter and OR-Set are **supported**, within the [language-path limits](/safemesh/#v0-support). G-Set, PN-Counter, RGA/Text, LWW Register (`LwwRegister`), Enable-wins Flag (`EnableWinsFlag`) and LWW Map (`LwwMap`) are **experimental**, including their deltas and wrappers. Existing proof/test evidence is unchanged by release status.


Build a small field kit: an observation counter and a membership set. Save `3` and
`compass`, exit the process, restore them, then make independent edits and exchange
bytes with a second replica. The final count is `6`; membership is `compass`, `map`,
`rope`. Both programs assert those values, retained history and the error outcome.

These paths use **main (unreleased)** source and a locally built WASM package.
There is no published SafeMesh package to install. The Rust and TypeScript programs
are complete consumer fixtures, separate from the library workspace; their code,
commands and stdout are checked by the existing documentation CI job.

## Before you start

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

Use Linux x86_64, Bash, Git, internet access, Rust/Cargo through rustup and a native
C compiler/linker. The TypeScript path also needs Node.js and npm. The measured
versions and exclusions are in [environments run](#environments-run). No Lean is
needed. The first build downloads build dependencies and may take several minutes.

These fixtures are on `main`. Obtain the source in an empty working directory:

```sh
git clone --branch main https://github.com/velvetmonkey/safemesh.git
cd safemesh
```

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
replay the existing store while reacquiring the writer. After restart, a tally of
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
`AlreadyExists` on a second `persist` means the exercise store already exists:
keep it and run `restart` with its original writer configuration, or use a fresh
checkout for a separate exercise. Never reset an existing writer's fence/history.
`History(UnexpectedEof)` on restart means the stored transaction is truncated or
incomplete. Retain the damaged store for inspection; restart does not salvage a
torn record. Investigate the failed write/transfer and recover only from a known
consistent history with its original identity and allocation metadata. Do not
initialize over the damaged store. These are tested engineering outcomes, not
proofs of crash safety. See the [recovery evidence](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/README.md).

## Predict, then run: a concurrent add and remove

Predict whether `milk` remains after one replica removes its observed token while
another adds `milk` with a fresh token. Then replace the Rust gold-path fixture's
`examples/gold-path/rust/src/main.rs` in your disposable checkout with this complete
program. Run it from the checkout root with
`cargo run --quiet --locked --manifest-path examples/gold-path/rust/Cargo.toml`.
It uses the same supported OR-Set carrier without touching the exercise stores.

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

Output:

```text
milk remains=true; observed token removed, fresh token survives
```

Token `100` was observed and removed; concurrent token `201` survives. Tokens here
are fixed for this one-shot example. A real mutable writer must allocate unique
tokens and preserve identity/allocation on restart as described in the
[OR-Set lifecycle guide](/safemesh/using-safemesh/#rust).

## Decrement with supported types

PN-Counter is **experimental** in v0. Use two supported G-Counters for stock:
one totals additions and the other totals removals. Replace the same disposable
fixture's `src/main.rs` with this program and run the same Cargo command above.
Each writer owns a distinct coordinate; each bump is its cumulative tally, not
an increment amount. Merge both counters, including after offline edits.

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
    let stock = i128::try_from(added_a.value()).unwrap()
        .checked_sub(i128::try_from(removed_a.value()).unwrap()).unwrap();
    assert_eq!(stock, 8);
    println!("added={} removed={} stock={stock}", added_a.value(), removed_a.value());
}
```

Output:

```text
added=12 removed=4 stock=8
```

This demonstrates supported carrier semantics, not a proven inventory schema.
Persist and exchange both totals; two durable stores do not commit atomically.
Keyed stock, membership policy, overflow handling and nonnegative-stock enforcement
remain application responsibilities. Offline decrements can oversell: convergence
does not enforce a global availability constraint. PN/custom durable restart is
outside v0; Rust's Linux durable paths cover G-Counter and UTF-8 OR-Set only.
Restore the original fixture source before running its `persist`/`restart` journey again.

## TypeScript gold path (Node)

The complete application is in `examples/gold-path/typescript/`. Build the Node
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
malformed record: SafeMeshError: failed to decode record
cleaned exercise stores
```
<!-- /gold -->

Edit `examples/gold-path/typescript/main.ts` to adapt the tally and membership.
The import loads the generated Node CommonJS package synchronously; there is no
`init` call. Keep the whole `pkg/` directory, including `.wasm`, declarations and
`snippets/`. This path uses that directory directly, not an npm SafeMesh tarball.
`tsc` emits `main.js` only if compilation succeeds. Values passed to Rust `u64`
parameters are `bigint` (`3n`), and exchanged bytes are `Uint8Array`.

This Node program writes complete logs with `writeFileSync` and reloads them in a
new process before making more edits. It demonstrates an **ordinary, single-writer
restart**, with no crash-safe commit or writer fencing. The application owns
storage and globally unique add tokens (`10n`, `11n`, `20n` here); never clone a
writer or resume an old backup as a live writer. Rust's durable adapter is not
exported by this binding. Two log writes are not an atomic pair.

The fixture passes an empty `Uint8Array` to `mergeRecordBytes`, catches the real
`SafeMeshError: failed to decode record`, and asserts no log mutation. Reject the
malformed input and check your record framing. Every WASM handle is freed in
`finally`. Read the [complete TypeScript source and project configuration](/safemesh/using-safemesh/#wasm--typescript).

## Environments run

Measured on 11 September 2026; this is execution evidence, not a support pledge.
The checkout’s `scripts/check-gold-paths.py` compares values, output and the
source blocks on these pages in the
[documentation workflow](https://github.com/velvetmonkey/safemesh/blob/main/.github/workflows/docs.yml).

| Support field | Rust path | TypeScript path |
| --- | --- | --- |
| API present | Counter, UTF-8 OR-Set, durable local writer | Counter and UTF-8 OR-Set record/log binding |
| Artifact available | Source path dependency, no registry release | Locally built Node WASM directory, no registry release |
| Build checked | Linux x86_64, Rust 1.98.1 | Linux x86_64, Rust 1.98.1, wasm-pack 0.15.0, TypeScript 6.0.3 |
| Runtime tested | Ubuntu 24.04.4, native executable | Ubuntu 24.04.4, Node 22.22.3, npm 10.9.8 |
| Integration tested | Edit, disk commit, process restart, new edit, byte exchange, malformed record | Edit, file save, process reload, new edit, byte exchange, malformed record |
| Maintainer-supported | Unknown | Unknown |

Not run for these paths: Windows/PowerShell, macOS, ARM64, musl, embedded hardware,
browsers (including Safari), Deno, Bun, or other Rust/Node versions. Python and C
are separate routes, not equivalent persistence APIs. A WASM build is not browser
runtime evidence. These fixtures hand bytes between objects after restart; they
do not test a physical network, sudden power loss or storage hardware guarantees.

## Next steps

- [Use the complete sources and connect your transport](/safemesh/using-safemesh/).
- [Explore lost, reordered and repeated delivery in the examples](/safemesh/examples/).
- [Understand convergence and its assumptions](/safemesh/concepts/#convergence).
