# Persist, restore, partition and reconcile (TypeScript / Node)

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
their event logs to real files, rebuild the replicas from those files in a fresh
process, let the two sides edit offline until they disagree, then merge each
side's log file into the other and watch them agree again.

The original persist/restore/partition/reconcile walk was run on Node v22.22.3
and again on Node v24.20.0, with identical output. The peer-recovery additions
in section 7 were measured on Node v22.22.3. The root README does not name a Node
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
The walkthrough below retains the legacy caller-token API and its original
complete-log, single-writer assumptions.

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
//   <log-dir>     is where the four log files live.
//   <step>        is one of: persist, restore, partition, reconcile.
//
// Each step is a separate process. State survives only through the log files.

import { createRequire } from "node:module";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
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

const logPath = (side, kind) => join(logDir, `${side}-${kind}.log`);

function fresh(side) {
  return {
    counter: new SafeMeshGCounterReplica(SIDES[side], WIDTH),
    set: new SafeMeshStringOrSetReplica(SIDES[side]),
  };
}

function save(side, replica) {
  mkdirSync(logDir, { recursive: true });
  for (const kind of ["counter", "set"]) {
    const bytes = replica[kind].logBytes();
    writeFileSync(logPath(side, kind), bytes);
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
  const replica = fresh(side);
  for (const kind of ["counter", "set"]) {
    const path = logPath(side, kind);
    if (!existsSync(path)) {
      console.error(`RESTORE FAILED file=${path} error=missing (run the persist step first)`);
      process.exit(2);
    }
    try {
      mergeLog(replica[kind], readFileSync(path));
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
    console.log("persist: two fresh replicas, one edit each, full exchange, then write logs");
    const left = fresh("left");
    const right = fresh("right");
    // appendBump(slot, tally): tally is the slot's new running total, not an increment.
    left.counter.appendBump(1, 5n);
    left.set.appendAdd("vaccine", 11n);
    right.counter.appendBump(2, 7n);
    right.set.appendAdd("insulin", 21n);
    exchange(left, right);
    show("left ", left);
    show("right", right);
    save("left", left);
    save("right", right);
    process.exit(same(left, right) ? 0 : 1);
  }
  case "restore": {
    console.log("restore: fresh process, replicas rebuilt from the log files alone");
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
    left.set.appendRemoveObserved("vaccine"); // removes the token left has seen (11)
    right.counter.appendBump(2, 11n); // slot 2 total 7 -> 11
    right.set.appendAdd("vaccine", 22n); // concurrent re-add with a fresh token
    right.set.appendAdd("gauze", 23n);
    show("left ", left);
    show("right", right);
    save("left", left);
    save("right", right);
    const diverged = !same(left, right);
    console.log(`DIVERGED=${diverged}`);
    process.exit(diverged ? 0 : 1);
  }
  case "reconcile": {
    console.log("reconcile: each side merges the other's log file, then both are written back");
    const left = load("left");
    const right = load("right");
    for (const kind of ["counter", "set"]) {
      mergeLog(left[kind], readFileSync(logPath("right", kind)));
      mergeLog(right[kind], readFileSync(logPath("left", kind)));
    }
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
next is the four files under `logs/`: one counter log and one set log per side.

Three facts about the calls the program makes:

- `appendBump(slot, tally)`: a G-Counter replica may only bump the slot whose
  index equals its own replica id, and `tally` is that slot's new running total,
  not an increment. Bumping slot 1 to 5 and later to 8 reads 8, not 13. A wrong
  slot throws `counter coordinate out of range or not owned by record author`.
- `appendAdd(element, token)` and `appendRemoveObserved(element)`: tokens are
  caller-supplied `bigint`s and must be unique per add across all replicas. A
  remove tombstones only the tokens this replica has already seen, so a
  concurrent add with a fresh token survives it. That is the add-wins rule.
- `logBytes()` is the whole log; `mergeLogBytes(bytes)` admits every record in it
  that the replica does not already hold. Records already held are duplicates and
  do not move state, so merging the same file twice is harmless.
  The return value contains an ordered `accepted`, `duplicate`, or `collision`
  verdict for every input record, including any accepted prefix before a collision.

## 3. Persist

```sh
node node-persist.mjs ./pkg ./logs persist
```

```text
persist: two fresh replicas, one edit each, full exchange, then write logs
  left : counter=12 set=["insulin","vaccine"] (v1=1 v2=1)
  right: counter=12 set=["insulin","vaccine"] (v1=1 v2=1)
  wrote logs/left-counter.log (144 bytes)
  wrote logs/left-set.log (148 bytes)
  wrote logs/right-counter.log (144 bytes)
  wrote logs/right-set.log (148 bytes)
```

Both sides made one edit each and exchanged logs in memory, so the four files
describe two replicas that already agree. Exit code 0.

## 4. Restore

This is a new process. Nothing survives from step 3 except the files.

```sh
node node-persist.mjs ./pkg ./logs restore
```

```text
restore: fresh process, replicas rebuilt from the log files alone
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
  wrote logs/left-set.log (186 bytes)
  wrote logs/right-counter.log (186 bytes)
  wrote logs/right-set.log (236 bytes)
DIVERGED=true
```

Exit code 0. The two sides now disagree on both the counter and the set.

## 6. Reconcile

Each side merges the other side's log files. No step is repeated and no record
is applied twice.

```sh
node node-persist.mjs ./pkg ./logs reconcile
```

```text
reconcile: each side merges the other's log file, then both are written back
  left : counter=19 set=["gauze","insulin","vaccine"] (v1=2 v2=3)
  right: counter=19 set=["gauze","insulin","vaccine"] (v1=2 v2=3)
  wrote logs/left-counter.log (228 bytes)
  wrote logs/left-set.log (274 bytes)
  wrote logs/right-counter.log (228 bytes)
  wrote logs/right-set.log (274 bytes)
CONVERGED=true
```

Exit code 0. The counter reads 8 + 11 = 19 on both sides. `vaccine` survives
because the right side's add carried token 22, which the left side had never
observed when it removed the element.

## 7. When a file is bad

Corrupt exactly one byte of one persisted log and try to restore. This keeps a
good copy first, then overwrites byte 40 of the left set log:

```sh
cp logs/left-set.log left-set.log.good
printf '\231' | dd of=logs/left-set.log bs=1 seek=40 count=1 conv=notrunc status=none
node node-persist.mjs ./pkg ./logs restore
```

Stdout:

```text
restore: fresh process, replicas rebuilt from the log files alone
```

Stderr (`console.error`):

```text
RESTORE FAILED file=logs/left-set.log error=SafeMeshError: failed to decode event log: IntegrityMismatch
```

The program exits 2. The core refused this integrity-invalid file before applying
any record from it. In this run, all four on-disk logs were byte-for-byte unchanged
after refusal; merging the bad file into both an empty and a populated set also
left their serialized logs unchanged. The counter loaded earlier in the process
is separate; this is not a transaction across all four files.

### Recover from a healthy peer, without the staged backup

Keep `logs/` intact and stop local writers while taking a consistent copy of the
healthy peer's logs. For this walk, that peer is `logs/right-counter.log` and
`logs/right-set.log`, with replica ID 2 and counter width 3. Step 6 already sent
all left-side records to it. No command below reads `left-set.log.good` or the
corrupt left set log.

From `walk`, with `pkg/` and `node-persist.mjs` from steps 1–2, run the following.
It loads the healthy peer alone, merges its complete logs into fresh replica ID 1,
compares counter slots, set adds, tombstones, both version vectors and log bytes,
and writes a separate `recovered-logs/` directory. That directory must not already
exist. Keep the originals, including any intact logs containing unsent edits.

```sh
node --input-type=commonjs <<'JS'
const { readFileSync, writeFileSync, mkdirSync } = require('node:fs');
const assert = require('node:assert/strict');
const { SafeMeshGCounterReplica: Counter, SafeMeshStringOrSetReplica: SetReplica } = require('./pkg/safemesh_wasm.js');
const peer = { counter: new Counter(2n, 3), set: new SetReplica(2n) };
const recovered = { counter: new Counter(1n, 3), set: new SetReplica(1n) };
function mergeLog(replica, bytes) {
  const admissions = replica.mergeLogBytes(bytes);
  assert(!admissions.includes('collision'), `batch collision after admissions=${JSON.stringify(admissions)}`);
  return admissions;
}
function state(r) {
  return {
    counter: Array.from(r.counter.state(), String),
    elements: r.set.elements(),
    adds: r.set.addEntries().map(e => [e.element(), String(e.token())]),
    tombstones: Array.from(r.set.tombstones(), String),
    counterVersions: [String(r.counter.versionFor(1n)), String(r.counter.versionFor(2n))],
    setVersions: [String(r.set.versionFor(1n)), String(r.set.versionFor(2n))],
  };
}
try {
  for (const kind of ['counter', 'set']) {
    mergeLog(peer[kind], readFileSync(`logs/right-${kind}.log`));
    mergeLog(recovered[kind], peer[kind].logBytes());
  }
  console.log('peer=' + JSON.stringify(state(peer)));
  console.log('recovered=' + JSON.stringify(state(recovered)));
  assert.deepStrictEqual(state(recovered), state(peer));
  for (const kind of ['counter', 'set']) {
    assert.deepStrictEqual(recovered[kind].logBytes(), peer[kind].logBytes());
  }
  mkdirSync('recovered-logs'); // Refuse to overwrite an earlier recovery.
  for (const kind of ['counter', 'set']) {
    writeFileSync(`recovered-logs/left-${kind}.log`, recovered[kind].logBytes());
    writeFileSync(`recovered-logs/right-${kind}.log`, peer[kind].logBytes());
  }
  console.log('RECOVERED=true');
} catch (error) {
  console.error('RECOVERY FAILED: ' + error.message);
  process.exitCode = 1;
}
JS
```

Stdout (exit 0):

```text
peer={"counter":["0","8","11"],"elements":["gauze","insulin","vaccine"],"adds":[["gauze","23"],["insulin","21"],["vaccine","11"],["vaccine","22"]],"tombstones":["11"],"counterVersions":["2","2"],"setVersions":["2","3"]}
recovered={"counter":["0","8","11"],"elements":["gauze","insulin","vaccine"],"adds":[["gauze","23"],["insulin","21"],["vaccine","11"],["vaccine","22"]],"tombstones":["11"],"counterVersions":["2","2"],"setVersions":["2","3"]}
RECOVERED=true
```

`RECOVERED=true` means equality with the supplied peer, not proof that the peer
has every edit the damaged replica ever acknowledged. In an additional run, a
left-only add of `unreplicated` with token 100 was absent after this recovery:
left's set version was `(3,3)`, but the available peer and recovery had `(2,3)`.
Do not resume writes under the old identity if lost record sequences or tokens
could be reused; that requires the application's original allocation information.
This recipe demonstrates recovery and reconciliation of the already-synced walk.

Bring the rebuilt replica back together with its peer, then reopen the result:

```sh
node node-persist.mjs ./pkg ./recovered-logs reconcile
node node-persist.mjs ./pkg ./recovered-logs restore
```

Stdout (each command exits 0):

```text
reconcile: each side merges the other's log file, then both are written back
  left : counter=19 set=["gauze","insulin","vaccine"] (v1=2 v2=3)
  right: counter=19 set=["gauze","insulin","vaccine"] (v1=2 v2=3)
  wrote recovered-logs/left-counter.log (228 bytes)
  wrote recovered-logs/left-set.log (274 bytes)
  wrote recovered-logs/right-counter.log (228 bytes)
  wrote recovered-logs/right-set.log (274 bytes)
CONVERGED=true
restore: fresh process, replicas rebuilt from the log files alone
  left : counter=19 set=["gauze","insulin","vaccine"] (v1=2 v2=3)
  right: counter=19 set=["gauze","insulin","vaccine"] (v1=2 v2=3)
RESTORED=true
```

### If there is no healthy peer

There is no public core or binding call that salvages the prefix of an
integrity-invalid whole log. Without a healthy peer, a backup, or independently
retained valid records, the data in a lone corrupt log cannot be recovered through
these APIs. Keep the damaged bytes for investigation; do not delete the store.
A peer can restore only records it received. The staged `left-set.log.good` copy
in this exercise is not something a real failure automatically provides.

Deleting the left set log made this program exit 2 with `error=missing`, not
restart successfully. A fresh empty replacement had no adds, no tombstones and
versions `(0,0)`: it lacked the two initial adds, left's remove of token 11, and
right's adds of tokens 22 and 23. In this already-reconciled walk all five records
were available from the peer; unsent edits would not be.

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
SafeMeshGCounterReplica.mergeRecordBytes(Uint8Array [0]): threw SafeMeshError code=1 message="failed to decode record"
SafeMeshStringOrSetReplica.mergeRecordBytes(logBytes()): threw SafeMeshError code=1 message="failed to decode record: InvalidTag"
SafeMeshStringOrSetReplica.mergeLogBytes(<record bytes>): threw SafeMeshError code=1 message="failed to decode event log: InvalidTag"
first mergeRecordBytes of one record: returned "accepted"
second mergeRecordBytes of the same record: returned "duplicate"
width-2 replica .mergeLogBytes(logs/left-counter.log written at width 3): threw SafeMeshError code=1 message="replica count mismatch"
SafeMeshStringOrSetReplica.mergeLogBytes(logs/left-counter.log): threw SafeMeshError code=1 message="delta type mismatch"
```

`mergeRecordBytes` takes one record, as returned by `appendBump`, `appendAdd` or
`appendRemoveObserved`. `mergeLogBytes` takes a whole log, as returned by
`logBytes`. Handing one to the other is the most likely way to see
`failed to decode record: InvalidTag` or `failed to decode event log: InvalidTag`.
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

It prints the same transcript as steps 3 to 6 and leaves the four log files in
`./walk-logs`.
