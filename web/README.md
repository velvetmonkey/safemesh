# SafeMesh PWA MVP

Installable, offline-first browser demo for SafeMesh delta-state CRDT convergence under simulated mesh partition and reconnect.

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

- N simulated peers with local CRDT state.
- Delta delivery with configurable partition/heal, latency, and drop rate.
- G-Counter divergence under partition and convergence after reconnect.
- OR-Set add-wins behavior using observed-token remove tombstones.
- A live convergence indicator that compares raw states and reads.

## Honesty Boundary

Lean is the SafeMesh semantic oracle. The TypeScript in this PWA is not proof-carrying verified code; it is a browser demo mirror.

- G-Counter: mirrors `SafeMesh.deltaBump`, `SafeMesh.deltaGCounter_correct`, `SafeMesh.delta_dissemination_sec`, and `SafeMesh.merge_deltaState`. This CRDT also has a shipped Rust body that is differentially tested over corpus C.
- PN-Counter: included only as a small TypeScript reference mirror of `SafeMesh.deltaBumpP`, `SafeMesh.deltaBumpN`, and `SafeMesh.deltaPNCounter_correct_P/_N`; the current UI does not center it.
- OR-Set: mirrors Lean-proven `SafeMesh.orAddDelta`, `SafeMesh.orRemoveDelta`, and `SafeMesh.deltaORSet_lookup` directly. Rust OR-Set is not shipped yet.

Do not present this PWA as the verified artifact. It is the demo skin that a future Rust/WASM verified product body can replace.

## PWA Cache Note

The service worker uses a versioned cache and network-first navigation. Old caches are deleted on activation so development rebuilds are not pinned behind a stale app shell.
