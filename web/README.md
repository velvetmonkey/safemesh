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

The app is static and has no backend. On a first online visit to the production build, the service worker precaches the app shell, including JavaScript, CSS, and WASM. Once installation completes, the Lab can reopen offline.

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

Lean is the SafeMesh semantic oracle. The G-Counter and OR-Set use Rust-core records, merges, and reads through WASM. The TypeScript transport simulates timing, duplication, drops, and partitions. The Lean-backed Rust core is the proof-carrying surface for modeled CRDT semantics.

- G-Counter: runs the Rust core through WASM, backed by `SafeMesh.deltaBump`, `SafeMesh.deltaGCounter_correct`, `SafeMesh.delta_dissemination_sec`, and `SafeMesh.merge_deltaState`. The Rust body is differentially tested over corpus C.
- PN-Counter: included only as a small TypeScript reference mirror of `SafeMesh.deltaBumpP`, `SafeMesh.deltaBumpN`, and `SafeMesh.deltaPNCounter_correct_P/_N`; the current UI does not center it.
- OR-Set: the Lab calls `SafeMeshStringOrSetReplica` for adds, observed removals, record delivery, log recovery, and reads. These operations delegate to the shipped Rust core; the Lab transport and UI are not claimed to be formally verified.

Do not present this PWA as the verified artifact. It is the demo skin around the proof-backed product body.

## Anti-Entropy

The simulator compares Rust-core record versions between connected peers and backfills missing G-Counter and OR-Set records with `logBytes` and `mergeLogBytes`. Guided recovery queues visible log packets before delivery; periodic recovery merges logs immediately. TypeScript schedules these exchanges; the Rust core admits records and computes state.

The Lean anchor for this backfill is `SafeMesh.merge_deltaState`: merging replicas is equivalent to receiving the union of their delta sets. `SafeMesh.delta_dissemination_sec` is the separate order/redelivery-insensitivity result once deltas are delivered. Neither theorem makes this TypeScript implementation verified.

## PWA Cache Note

The service worker uses a versioned cache and network-first navigation. Production builds derive the cache version from the emitted files and precache them during installation. Old caches are deleted on activation. Offline reopening is supported for production builds (`npm run build` followed by `npm run preview`), not the Vite development server.
