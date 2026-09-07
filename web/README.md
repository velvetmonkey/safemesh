# SafeMesh web convergence hero

Interactive browser demo for the SafeMesh "watch it converge" story.

![Captured web hero showing four replicas and convergence controls](../demos/assets/web-hero-desktop.png)

## Run

```sh
npm install
npm run dev
npm run build
npm run preview
npm run test
```

The app is static and has no backend. After a first load, the service worker caches the app shell so it can reopen offline.

## What It Demonstrates

- Simulated replicas with local CRDT state.
- Five use-case buttons: normal delivery, duplicate replay, out-of-order delivery, dropped-message recovery, and partition heal.
- Scenario steps that all start from the same message and end at the same visible state: `vaccine` plus audit count `1`.
- A collapsed Advanced lab with partition/heal, latency, drop, duplicate, and reorder controls.
- Periodic anti-entropy between connected peers so dropped one-shot messages can be recovered.
- G-Counter and OR-Set divergence under partition and convergence after reconnect.
- A live convergence indicator that compares raw states and reads.
- A technical/plain-language claim toggle and reduced-motion mode.

The scenario flow is intentionally manual: choose what can go wrong, then use the Previous / Replay / Next buttons directly under the replica picture. The Advanced lab keeps the free-form controls available without making them the first thing a new viewer has to understand.

## Honesty Boundary

Lean is the SafeMesh semantic oracle. The G-Counter runs the Rust core through WASM; the OR-Set runs a TypeScript demo mirror, which is not proof-carrying verified code. The Lean-backed Rust core is the proof-carrying surface for modeled CRDT semantics.

- G-Counter: runs the Rust core through WASM, backed by `SafeMesh.deltaBump`, `SafeMesh.deltaGCounter_correct`, `SafeMesh.delta_dissemination_sec`, and `SafeMesh.merge_deltaState`. The Rust body is differentially tested over corpus C.
- PN-Counter: included only as a small TypeScript reference mirror of `SafeMesh.deltaBumpP`, `SafeMesh.deltaBumpN`, and `SafeMesh.deltaPNCounter_correct_P/_N`; the current UI does not center it.
- OR-Set: the Lab’s TypeScript implementation mirrors Lean-proven `SafeMesh.orAddDelta`, `SafeMesh.orRemoveDelta`, and `SafeMesh.deltaORSet_lookup` directly. Rust OR-Set ships in `safemesh-crdt` and is differentially tested over corpus C, but the Lab does not use it.

Do not present this PWA as the verified artifact. It is the demo skin around the proof-backed product body.

## Anti-Entropy

The simulator periodically exchanges compact state digests between connected peers and backfills missing G-Counter coordinates, OR-Set add tokens, and OR-Set tombstones. This is a transport/re-gossip behavior in the demo, not a new CRDT.

The Lean anchor for this backfill is `SafeMesh.merge_deltaState`: merging replicas is equivalent to receiving the union of their delta sets. `SafeMesh.delta_dissemination_sec` is the separate order/redelivery-insensitivity result once deltas are delivered. Neither theorem makes this TypeScript implementation verified.

## PWA Cache Note

The service worker uses a versioned cache and network-first navigation. Old caches are deleted on activation so development rebuilds are not pinned behind a stale app shell.
