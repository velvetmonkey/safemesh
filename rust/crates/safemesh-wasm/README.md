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
