# Persist, restore, partition and reconcile (TypeScript / Node)

This example persists allocated OR-Set identities across processes. For a shorter
allocated-writer save/new-process restore path, follow the [TypeScript gold path](https://velvetmonkey.github.io/safemesh/persist-and-restart/#typescript-gold-path-node).

**v0 scope:** G-Counter and OR-Set are **supported**, within the [language-path limits](https://velvetmonkey.github.io/safemesh/#v0-support).


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

Before the WASM builds, install their target and build tool:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-pack --version 0.15.0 --locked
```


This page walks the same four steps as the [Rust walkthrough](../safemesh-crdt/README.md#persist-restore-partition-and-reconcile),
in Node, against the `safemesh-wasm` package that `wasm-pack` builds for the
`nodejs` target. Two replicas each hold a G-Counter and a UTF-8 OR-Set. You write
counter logs and complete set identities to real files, rebuild the replicas from those files in a fresh
process, let the two sides edit offline until they disagree, then merge each
side's restored log into the other and watch them agree again.

The original persist/restore/partition/reconcile walk was run on Node v22.22.3
and again on Node v24.20.0, with identical output. The allocated-identity
transcripts below were refreshed after the example switched identity formats. The root README does not name a Node
version; the Lab in `web/` asks for Node 24.x and the repository CI uses Node 24.
The package was built with `wasm-pack 0.15.0` (the version `scripts/package-smoke.sh`
installs when none is present) over `rustc 1.96.1`.

## Claim boundary

Integration tested: this walkthrough exercises the generated `safemesh-wasm` Node package through disk persist, process restore, partition, and reconciliation. Browser/version/OS matrix coverage is not established by this Node walkthrough. Every value printed below
is computed by the Rust core (`safemesh-crdt`): the G-Counter and OR-Set carriers
it merges are the Lean-backed ones, while the event log, the wire bytes and this
binding are outside the separate Lean proof claim. See the repository
`CLAIMS.md` and `WHAT-IS-PROVEN.md`.

## Allocated OR-Set writes

For new applications, use the [allocated identity lifecycle](README.md#string-or-set-replica-with-an-event-log):
`createAllocated(writers, author)`, `appendAllocatedAdd(element)`,
`exportIdentity()` and checked `importIdentity(bytes)`. Persist the whole identity
export, and refuse a failed import instead of creating a fresh writer. The caller
must run one live writer per author across WASM instances; there is no cross-tab
or cross-process fencing, and a self-consistent stale snapshot is not detected.
The walkthrough below uses allocated writes and persists each set’s complete
identity, including its allocation cursor. Older caller-token set logs are not
identity exports; use a fresh directory for this version.

## What you need

- Node. This page used v22.22.3 and v24.20.0.
- A Rust toolchain with the `wasm32-unknown-unknown` target and `wasm-pack`, used
  once to build the package. Nothing is fetched from the npm registry.
- A POSIX shell. This page ran on Linux. The commands use `git`, `mkdir`, `cp`,
  `printf` and `dd`.

## 1. Build the package

Start in an empty directory. This clones the repository beside a new `walk`
directory and builds the `nodejs` package into `walk/pkg`:

```sh
git clone --quiet https://github.com/velvetmonkey/safemesh.git safemesh
mkdir walk
wasm-pack build safemesh/rust/crates/safemesh-wasm --target nodejs --out-dir "$PWD/walk/pkg" --release
```

`walk/pkg` now holds `safemesh_wasm.js`, `safemesh_wasm_bg.wasm`, `safemesh_wasm.d.ts`
and a `snippets/` directory. Keep the directory whole: `safemesh_wasm.js` requires a
file under `snippets/` on its second line, and `npm pack --dry-run ./pkg` lists six
files with no `snippets/` entry, so a packed tarball of this target is not what this
page uses.

The `nodejs` package is CommonJS. It has no `init` export and nothing to await: the
module loads the `.wasm` file synchronously when it is required. From an ES module,
load it with `createRequire`:

```js
import { createRequire } from "node:module";
const require = createRequire(import.meta.url);
const { SafeMeshGCounterReplica, SafeMeshStringOrSetReplica } = require("./pkg/safemesh_wasm.js");
```

## 2. The program

Change into `walk`. Everything below runs from there, and `walk` needs nothing but
`pkg/` and the files this page tells you to create. Save the following as
`walk/node-persist.mjs`. The same file is committed at
[`examples/node-persist.mjs`](examples/node-persist.mjs).

```js
#!/usr/bin/env node
// SafeMesh persist / restore / partition / reconcile walk for Node.
//
// Usage: node node-persist.mjs <package-dir> <log-dir> <step>
//   <package-dir> is a `wasm-pack build --target nodejs` output directory.
//   <log-dir>     is where the two counter logs and two set identities live.
//   <step>        is one of: persist, restore, partition, reconcile.
//
// Each step is a separate process. State survives only through the stored files.
// Set identities include the complete log and allocation cursor. Older manual-token
// set logs cannot be imported as identities; start this version in a fresh directory.
// Run phases sequentially: these files do not provide concurrent-process fencing.

import { createRequire } from "node:module";
import { existsSync, lstatSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

const [packageDir, logDir, step] = process.argv.slice(2);
if (!packageDir || !logDir || !step) {
  console.error("usage: node node-persist.mjs <package-dir> <log-dir> <step>");
  process.exit(64);
}

const require = createRequire(import.meta.url);
const { SafeMeshGCounterReplica, SafeMeshStringOrSetReplica } = require(
  join(resolve(packageDir), "safemesh_wasm.js"),
);

// Two replicas, ids 1 and 2. A G-Counter replica may only bump the counter
// slot whose index equals its own replica id, so the counter width must be
// larger than the largest replica id in use: width 3 here, slot 0 unused.
const WIDTH = 3;
const SIDES = { left: 1n, right: 2n };

const logPath = (side, kind) =>
  join(logDir, `${side}-${kind}.${kind === "set" ? "identity" : "log"}`);

function fresh(side) {
  return {
    counter: new SafeMeshGCounterReplica(SIDES[side], WIDTH),
    set: SafeMeshStringOrSetReplica.createAllocated(BigInt(WIDTH), SIDES[side]),
  };
}

function save(side, replica) {
  mkdirSync(logDir, { recursive: true });
  for (const kind of ["counter", "set"]) {
    const bytes = kind === "set" ? replica.set.exportIdentity() : replica.counter.logBytes();
    writeFileSync(logPath(side, kind), bytes, { flag: step === "persist" ? "wx" : "w" });
    console.log(`  wrote ${logPath(side, kind)} (${bytes.length} bytes)`);
  }
}

function mergeLog(replica, bytes) {
  const admissions = replica.mergeLogBytes(bytes);
  if (admissions.includes("collision")) {
    throw new Error(`batch collision after admissions=${JSON.stringify(admissions)}`);
  }
  return admissions;
}

function load(side) {
  const replica = { counter: new SafeMeshGCounterReplica(SIDES[side], WIDTH) };
  for (const kind of ["counter", "set"]) {
    const path = logPath(side, kind);
    if (!existsSync(path)) {
      console.error(`RESTORE FAILED file=${path} error=missing (run the persist step first)`);
      process.exit(2);
    }
    try {
      if (kind === "set") {
        replica.set = SafeMeshStringOrSetReplica.importIdentity(readFileSync(path));
      } else {
        mergeLog(replica.counter, readFileSync(path));
      }
    } catch (error) {
      console.error(`RESTORE FAILED file=${path} error=${error.name}: ${error.message}`);
      process.exit(2);
    }
  }
  return replica;
}

function show(label, replica) {
  const versions = `v1=${replica.set.versionFor(1n)} v2=${replica.set.versionFor(2n)}`;
  console.log(
    `  ${label}: counter=${replica.counter.value()} set=${JSON.stringify(replica.set.elements())} (${versions})`,
  );
}

function same(a, b) {
  return (
    a.counter.sameStateAs(b.counter) &&
    JSON.stringify(a.set.elements()) === JSON.stringify(b.set.elements())
  );
}

function exchange(a, b) {
  for (const kind of ["counter", "set"]) {
    mergeLog(a[kind], b[kind].logBytes());
    mergeLog(b[kind], a[kind].logBytes());
  }
}

switch (step) {
  case "persist": {
    // Check every store before writing any: even a partial history must survive.
    // lstat also detects dangling symlinks at a store path.
    const paths = Object.keys(SIDES).flatMap(side =>
      ["counter", "set"].map(kind => logPath(side, kind)),
    );
    const existing = paths.filter(path => lstatSync(path, { throwIfNoEntry: false }));
    const missing = paths.filter(path => !existing.includes(path));
    if (existing.length) {
      console.error(
        `PERSIST REFUSED: existing store files: ${existing.join(", ")}. ` +
        (missing.length
          ? `missing store files: ${missing.join(", ")}. ` +
            "Partial store: this walkthrough cannot restore an incomplete history. " +
            "Move the files aside or choose a different, empty directory for a new exercise."
          : "All four store paths exist; use restore to read a complete, valid history. " +
            "Restore will still reject invalid files. Use a different, empty directory for a new exercise."),
      );
      process.exit(2);
    }
    console.log("persist: two fresh replicas, one edit each, full exchange, then write stores");
    const left = fresh("left");
    const right = fresh("right");
    // appendBump(slot, tally): tally is the slot's new running total, not an increment.
    left.counter.appendBump(1, 5n);
    left.set.appendAllocatedAdd("vaccine");
    right.counter.appendBump(2, 7n);
    right.set.appendAllocatedAdd("insulin");
    exchange(left, right);
    show("left ", left);
    show("right", right);
    save("left", left);
    save("right", right);
    process.exit(same(left, right) ? 0 : 1);
  }
  case "restore": {
    console.log("restore: fresh process, replicas rebuilt from the stored files alone");
    const left = load("left");
    const right = load("right");
    show("left ", left);
    show("right", right);
    const ok = same(left, right) && left.counter.value() > 0n;
    console.log(`RESTORED=${ok}`);
    process.exit(ok ? 0 : 1);
  }
  case "partition": {
    console.log("partition: both sides edit offline; nothing is exchanged");
    const left = load("left");
    const right = load("right");
    left.counter.appendBump(1, 8n); // slot 1 total 5 -> 8
    left.set.appendRemoveObserved("vaccine"); // removes every vaccine token left has seen
    right.counter.appendBump(2, 11n); // slot 2 total 7 -> 11
    right.set.appendAllocatedAdd("vaccine"); // restored cursor allocates a fresh token
    right.set.appendAllocatedAdd("gauze");
    show("left ", left);
    show("right", right);
    save("left", left);
    save("right", right);
    const diverged = !same(left, right);
    console.log(`DIVERGED=${diverged}`);
    process.exit(diverged ? 0 : 1);
  }
  case "reconcile": {
    console.log("reconcile: each side merges the other's restored log, then both are written back");
    const left = load("left");
    const right = load("right");
    exchange(left, right);
    show("left ", left);
    show("right", right);
    save("left", left);
    save("right", right);
    const converged = same(left, right);
    console.log(`CONVERGED=${converged}`);
    process.exit(converged ? 0 : 1);
  }
  default:
    console.error(`unknown step ${JSON.stringify(step)}; expected persist, restore, partition or reconcile`);
    process.exit(64);
}
```

Each step is its own process. The only thing that carries from one step to the
next is the four files under `logs/`: one counter log and one set identity per side.

Three facts about the calls the program makes:

- `appendBump(slot, tally)`: a G-Counter replica may only bump the slot whose
  index equals its own replica id, and `tally` is that slot's new running total,
  not an increment. Bumping slot 1 to 5 and later to 8 reads 8, not 13. A wrong
  slot throws `counter coordinate out of range or not owned by record author`.
- `appendAllocatedAdd(element)` allocates a fresh token from the restored
  identity cursor. `appendRemoveObserved(element)` tombstones only the tokens
  this replica has already seen, so a concurrent add with a fresh token survives.
  That is the add-wins rule. Persist `exportIdentity()` and reopen it with
  `importIdentity(bytes)` to retain the cursor across processes.
- `logBytes()` is the whole log; `mergeLogBytes(bytes)` admits every record in it
  that the replica does not already hold. Records already held are duplicates and
  do not move state, so merging the same file twice is harmless.
  The return value contains an ordered `accepted`, `duplicate`, or `collision`
  verdict for every input record, including any accepted prefix before a collision.

`persist` initializes a new exercise and refuses if any of the four store paths
already exists, including a partial history. For all four existing paths, the
refusal suggests `restore` to read a complete, valid history; restore still
rejects invalid files. For a partial store (one to three paths), the refusal
lists both existing and missing paths and explains that this walkthrough cannot
restore an incomplete history. Keep the files by moving them aside, or choose a
different, empty directory for a new exercise. This does not recover missing
history. There is no reset command.

## 3. Persist

```sh
node node-persist.mjs ./pkg ./logs persist
```

```text
persist: two fresh replicas, one edit each, full exchange, then write stores
  left : counter=12 set=["insulin","vaccine"] (v1=1 v2=1)
  right: counter=12 set=["insulin","vaccine"] (v1=1 v2=1)
  wrote logs/left-counter.log (144 bytes)
  wrote logs/left-set.identity (177 bytes)
  wrote logs/right-counter.log (144 bytes)
  wrote logs/right-set.identity (177 bytes)
```

Both sides made one edit each and exchanged logs in memory, so the four files
describe two replicas that already agree. Exit code 0.

## 4. Restore

This is a new process. Nothing survives from step 3 except the files.

```sh
node node-persist.mjs ./pkg ./logs restore
```

```text
restore: fresh process, replicas rebuilt from the stored files alone
  left : counter=12 set=["insulin","vaccine"] (v1=1 v2=1)
  right: counter=12 set=["insulin","vaccine"] (v1=1 v2=1)
RESTORED=true
```

Exit code 0. `v1`/`v2` are `versionFor(1n)` and `versionFor(2n)`: how many records
from each author the set log holds.

## 5. Partition

Both sides are restored from disk, edit without talking to each other, and are
written back. The left side raises its counter slot and removes `vaccine`. The
right side raises its own slot, re-adds `vaccine` under a fresh token and adds
`gauze`.

```sh
node node-persist.mjs ./pkg ./logs partition
```

```text
partition: both sides edit offline; nothing is exchanged
  left : counter=15 set=["insulin"] (v1=2 v2=1)
  right: counter=16 set=["gauze","insulin","vaccine"] (v1=1 v2=3)
  wrote logs/left-counter.log (186 bytes)
  wrote logs/left-set.identity (215 bytes)
  wrote logs/right-counter.log (186 bytes)
  wrote logs/right-set.identity (265 bytes)
DIVERGED=true
```

Exit code 0. The two sides now disagree on both the counter and the set.

## 6. Reconcile

Each side merges the other side's restored logs. No step is repeated and no record
is applied twice.

```sh
node node-persist.mjs ./pkg ./logs reconcile
```

```text
reconcile: each side merges the other's restored log, then both are written back
  left : counter=19 set=["gauze","insulin","vaccine"] (v1=2 v2=3)
  right: counter=19 set=["gauze","insulin","vaccine"] (v1=2 v2=3)
  wrote logs/left-counter.log (228 bytes)
  wrote logs/left-set.identity (303 bytes)
  wrote logs/right-counter.log (228 bytes)
  wrote logs/right-set.identity (303 bytes)
CONVERGED=true
```

Exit code 0. The counter reads 8 + 11 = 19 on both sides. `vaccine` survives
because the right side allocated a fresh add token, which the left side had
never observed when it removed the element.

## 7. When a file is bad

Corrupt exactly one byte of one persisted identity and try to restore. This keeps a
good copy first, then overwrites byte 40 of the left set identity:

```sh
cp logs/left-set.identity left-set.identity.good
printf '\231' | dd of=logs/left-set.identity bs=1 seek=40 count=1 conv=notrunc status=none
node node-persist.mjs ./pkg ./logs restore
```

Stdout:

```text
restore: fresh process, replicas rebuilt from the stored files alone
```

Stderr (`console.error`):

```text
RESTORE FAILED file=logs/left-set.identity error=SafeMeshError: failed to decode event log: wire frame integrity check failed
```

The program exits 2. The identity import refuses the integrity-invalid file.
Restore does not write the store files. The counter loaded earlier in the process
is separate; this is not a transaction across all four files.

### Missing or damaged identities

This walkthrough has no repair command. `restore` requires all four valid store
files; a partial store is not a restorable history. Keep the remaining bytes and
any verified backups. Move the files aside or choose a different, empty directory
only to start a new exercise, not to recover the old one.

The earlier caller-token version of this guide demonstrated rebuilding logs from
a healthy peer. That recipe does not restore a lost allocated identity: the peer's
log does not supply the original writer's allocation cursor. Importing records
under a fresh identity can reject records claiming its local author, and starting
a fresh writer is not a safe substitute for recovering its identity. A peer also
cannot supply edits it never received. Do not resume writes under the old identity
without its original allocation information.

Before a failure: keep verified backups and replicate acknowledged records to another failure domain.

## 8. The errors you will meet

Every error the binding throws is a `SafeMeshError` (an `Error` subclass) with a
numeric `code` and a `message`. Save this as `walk/errors.mjs` and run it from
`walk` after step 6, because two of the probes read the log files:

```js
import { createRequire } from "node:module";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";

const require = createRequire(import.meta.url);
const w = require(join(resolve(process.argv[2] ?? "./pkg"), "safemesh_wasm.js"));

function probe(label, fn) {
  try {
    console.log(`${label}: returned ${JSON.stringify(fn())}`);
  } catch (error) {
    console.log(`${label}: threw ${error.name} code=${error.code} message=${JSON.stringify(error.message)}`);
  }
}

const counter = new w.SafeMeshGCounter(2);
probe("SafeMeshGCounter(2).tryApplyBump(2, 1n)", () => counter.tryApplyBump(2, 1n));

const replica = new w.SafeMeshGCounterReplica(1n, 3);
probe("SafeMeshGCounterReplica(1n, 3).appendBump(2, 1n)", () => replica.appendBump(2, 1n));
probe("SafeMeshGCounterReplica.mergeRecordBytes(Uint8Array [0])", () => replica.mergeRecordBytes(new Uint8Array([0])));

const set = new w.SafeMeshStringOrSetReplica(1n);
const record = set.appendAdd("x", 1n);
probe("SafeMeshStringOrSetReplica.mergeRecordBytes(logBytes())", () => set.mergeRecordBytes(set.logBytes()));
probe("SafeMeshStringOrSetReplica.mergeLogBytes(<record bytes>)", () => set.mergeLogBytes(record));

const other = new w.SafeMeshStringOrSetReplica(2n);
probe("first mergeRecordBytes of one record", () => other.mergeRecordBytes(record));
probe("second mergeRecordBytes of the same record", () => other.mergeRecordBytes(record));

const narrow = new w.SafeMeshGCounterReplica(1n, 2);
probe("width-2 replica .mergeLogBytes(logs/left-counter.log written at width 3)", () => narrow.mergeLogBytes(readFileSync("logs/left-counter.log")));
probe("SafeMeshStringOrSetReplica.mergeLogBytes(logs/left-counter.log)", () => new w.SafeMeshStringOrSetReplica(1n).mergeLogBytes(readFileSync("logs/left-counter.log")));
```

```sh
node errors.mjs ./pkg
```

```text
SafeMeshGCounter(2).tryApplyBump(2, 1n): threw SafeMeshError code=2 message="replica out of range"
SafeMeshGCounterReplica(1n, 3).appendBump(2, 1n): threw SafeMeshError code=2 message="counter coordinate out of range or not owned by record author"
SafeMeshGCounterReplica.mergeRecordBytes(Uint8Array [0]): threw SafeMeshError code=1 message="failed to decode record: unexpected wire tag"
SafeMeshStringOrSetReplica.mergeRecordBytes(logBytes()): threw SafeMeshError code=1 message="failed to decode record: unexpected wire tag"
SafeMeshStringOrSetReplica.mergeLogBytes(<record bytes>): threw SafeMeshError code=1 message="failed to decode event log: unexpected wire tag"
first mergeRecordBytes of one record: returned "accepted"
second mergeRecordBytes of the same record: returned "duplicate"
width-2 replica .mergeLogBytes(logs/left-counter.log written at width 3): threw SafeMeshError code=1 message="replica count mismatch"
SafeMeshStringOrSetReplica.mergeLogBytes(logs/left-counter.log): threw SafeMeshError code=1 message="delta type mismatch"
```

`mergeRecordBytes` takes one record, as returned by `appendBump`, `appendAdd` or
`appendRemoveObserved`. `mergeLogBytes` takes a whole log, as returned by
`logBytes`. Handing one to the other is the most likely way to see
`failed to decode record: unexpected wire tag` or `failed to decode event log: unexpected wire tag`.
A G-Counter log only restores into a replica built with the same width; a
different width throws `replica count mismatch`. A counter log will not restore
into a set replica or the reverse; that throws `delta type mismatch`.

## Running the committed copy from a clone

If you are already inside a clone and have built the `nodejs` package with the
root README's one-liner, which leaves it at `$tmp/pkg`, the committed example runs
the same four steps from the repository root, in the same shell that set `$tmp`:

```sh
node rust/crates/safemesh-wasm/examples/node-persist.mjs "$tmp/pkg" ./walk-logs persist &&
node rust/crates/safemesh-wasm/examples/node-persist.mjs "$tmp/pkg" ./walk-logs restore &&
node rust/crates/safemesh-wasm/examples/node-persist.mjs "$tmp/pkg" ./walk-logs partition &&
node rust/crates/safemesh-wasm/examples/node-persist.mjs "$tmp/pkg" ./walk-logs reconcile
```

It prints the same transcript as steps 3 to 6 and leaves the four store files in
`./walk-logs`.
