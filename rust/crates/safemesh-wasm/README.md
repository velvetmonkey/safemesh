# SafeMesh WASM

`safemesh-wasm` is the wasm-bindgen wrapper over the SafeMesh Rust core. It exposes replica/event-log helpers for browser and Node users while keeping merge behavior in one Rust implementation.

## Claim boundary

This crate is engineered and tested binding glue. The G-Counter path reaches the Lean-backed Rust carrier; LWW Register, Enable-wins Flag, and LWW Map remain tested-not-proven. The binding itself is not a separate proof.

See the repository `CLAIMS.md` and `WHAT-IS-PROVEN.md` for the full wording rule.

## Install

The v0.1 distribution flow builds an npm package with `wasm-pack`; it does not publish to npm.

```sh
wasm-pack build rust/crates/safemesh-wasm --target bundler --release
```

## Quickstart

```js
import init, { GCounterReplica } from "./pkg/safemesh_wasm.js";

await init();
const left = new GCounterReplica(1, 3);
const right = new GCounterReplica(2, 3);

right.merge_record_bytes(left.append_bump(1, 5));
left.merge_log_bytes(right.log_bytes());

console.log(left.value(), right.value());
```

Run `./scripts/package-smoke.sh` from the repository root to build the wasm package and run `npm pack --dry-run` without publishing.
