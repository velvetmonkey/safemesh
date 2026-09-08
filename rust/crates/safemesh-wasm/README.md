# SafeMesh WASM

`safemesh-wasm` is the wasm-bindgen wrapper over the SafeMesh Rust core. It exposes replica/event-log helpers for browser and Node users while keeping merge behavior in one Rust implementation.

## Claim boundary

This crate is engineered and tested binding glue. The G-Counter path reaches the Lean-backed Rust carrier; LWW Register, Enable-wins Flag, and LWW Map remain tested-not-proven. The binding itself is not a separate proof.

See the repository `CLAIMS.md` and `WHAT-IS-PROVEN.md` for the full wording rule.

## Install

The v0.1 distribution flow builds an npm package with `wasm-pack`; it does not publish to npm.

```sh
cd rust/crates/safemesh-wasm
wasm-pack build . --target bundler --release
npm pack --dry-run pkg
```

## Quickstart

```js
import init, { SafeMeshGCounterReplica } from "./pkg/safemesh_wasm.js";

await init();
const left = new SafeMeshGCounterReplica(1n, 3);
const right = new SafeMeshGCounterReplica(2n, 3);

right.mergeRecordBytes(left.appendBump(1, 5n));
left.mergeLogBytes(right.logBytes());

console.log(left.value(), right.value());
```

## Demos

Run the Node convergence demo against a freshly generated Node-target package:

```sh
cd rust/crates/safemesh-wasm
wasm-pack build . --target nodejs --out-dir pkg-node --release
node examples/node-convergence.mjs pkg-node
```

Run the browser demo by building a web-target package and serving this crate directory with any static file server:

```sh
cd rust/crates/safemesh-wasm
wasm-pack build . --target web --out-dir pkg --release
python3 -m http.server 8000
```

Then open `http://127.0.0.1:8000/examples/browser-convergence.html`.

Run `./scripts/package-smoke.sh` from the repository root to build the bundler package, run `npm pack --dry-run`, and execute the Node convergence demo without publishing.

## Checked coordinates and OR-Set

`SafeMeshGCounter.tryApplyBump(replica, tally)` throws an `Error` with a message such as
`ReplicaOutOfRange { replica: 2, replica_count: 2 }` for an invalid coordinate
without changing state. `applyBump` retains its silent behavior.

```js
const left = new SafeMeshOrSet(), right = new SafeMeshOrSet();
left.add(10n, 101n);
right.merge(left);
left.applyRemove(left.observedTokens(10n));
right.add(10n, 201n); // Concurrent add uses a fresh token.
left.merge(right);
console.log(left.elements()); // BigUint64Array [10n]
left.free(); right.free();
```

Import `SafeMeshOrSet` from the generated package alongside the counter classes.

The set delegates to Rust `OrSet<u64, u64>`: elements and tokens are unsigned
64-bit integers (JavaScript uses `bigint`). Tokens are global to the set; use a
fresh, replica-unique token for every add to obtain add-wins behavior. Removal
persists tombstones even before an add arrives, and a reused token affects every
element carrying it. Observed tokens include tombstoned adds. Merge unions all
adds and tombstones, and reads return sorted unique live members. This is the
core's token semantics, including token reuse; the binding does not allocate IDs.

## String OR-Set replica with an event log

`SafeMeshStringOrSetReplica` carries an `OrSet<String, u64>` behind an
`EventLog`, so records can be replayed, deduplicated and repaired from a log
the same way `SafeMeshGCounterReplica` does. It sits beside `SafeMeshOrSet`,
which is unchanged and still exposes whole-state merge over `u64` elements.

```js
const left = new SafeMeshStringOrSetReplica(1n);
const right = new SafeMeshStringOrSetReplica(2n);
const add = left.appendAdd("vaccine", 11n);
right.mergeRecordBytes(add); // "accepted"
right.mergeRecordBytes(add); // "duplicate": same record identity and payload, state unchanged
right.mergeRecordBytes(left.appendRemoveObserved("vaccine"));
console.log(right.elements()); // []
console.log(right.observedTokens("vaccine")); // BigUint64Array [11n]
const view = SafeMeshStringOrSetReplica.inspectRecordBytes(add);
console.log(view.replica(), view.sequence(), view.deltaKind(), view.element(), view.token());
```

`mergeRecordBytes` returns the core's admission verdict; a record whose
identity is already known with a different payload throws
`record ID collision`, and bytes the core cannot decode throw
`failed to decode record: <reason>`. `inspectRecordBytes` decodes record bytes
through the same core decoder without admitting them anywhere. Every value
these classes return is computed by `safemesh-crdt`; the binding holds no
set or log logic of its own. Token semantics are the core's, exactly as for
`SafeMeshOrSet` above.
