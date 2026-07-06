# SafeMesh for builders: interactive web convergence hero

This is the visual demo: replicas as live nodes, deltas moving across the wire, a partition that makes state split, and a heal path that makes every modeled replica settle on the same read.

![Captured web hero showing four replicas and convergence controls](../assets/web-hero-desktop.png)

![Captured mobile web hero with separated replica field and narrative](../assets/web-hero-mobile.png)

## 30-second run

From the repository root:

```sh
cd web
npm install
npm run dev
```

Open the local Vite URL. The first screen defaults to **Guided timeline**.

## What It Shows

- Live replica state for G-Counter and OR-Set demo data.
- Visible packet chips carrying a compact wire sketch.
- A manual timeline scrubber with authored steps for add, duplicate, reorder, drop, partition, heal, and final inspection.
- A separate sandbox mode with controls for partition, heal, drop, duplicate, reorder, and reset.
- Narrative and event log panels next to each other, with controls kept in the bottom dock.
- A technical/plain-language toggle so the honest claim boundary stays visible without making the first screen feel like docs.
- A calm-motion toggle for reduced-motion users.

## Honest Claim Boundary

The TypeScript app is a demo mirror. It is not the verified artifact.

The Lean-backed Rust core is the proof-carrying surface for the in-house CRDT semantics. The web demo makes the behavior inspectable and interactive; it does not prove browser code, real network delivery, storage durability, arbitrary reducers, or sensor truth.

## Why This Matters

A verified library still has to show its work. This demo makes the core idea visible without asking a builder to read the proof first: delivery gets ugly, replicas diverge for a while, and once the modeled records meet, the state settles.
