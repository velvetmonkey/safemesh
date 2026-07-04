# SafeMesh architecture

SafeMesh follows verification-guided development (VGD): the Lean proof comes first and acts as the oracle; the shipped Rust code is differential-tested against it.

## Layers

1. **Verified core (external, public, MIT): crdt-lean.**
   Machine-checked state-based CvRDT convergence. Strong Eventual Consistency (same delivered set implies equal state), conditional liveness under fairness, and lawful instances (G-Set, G-Counter, PN-Counter, OR-Set, Sequence/RGA). Zero `sorry`; axioms limited to {propext, Classical.choice, Quot.sound}. SafeMesh consumes it as a dependency and does not modify it.

2. **Delta-state extension (this repo, `lean/`).**
   Full-state gossip does not fit constrained mesh links (LoRa duty cycles). Delta-state CRDTs (delta-mutators + delta-merge) ship only the change. The goal is a machine-checked theorem that delta dissemination converges to the same state as full-state gossip, so the bandwidth win costs nothing in correctness. Same toolchain and axiom discipline as crdt-lean.

3. **Product layer (this repo, `rust/`).**
   Thin `no_std` Rust crates implementing the CRDTs, each differential-tested against the Lean-proven semantics from layers 1 and 2. The verified core stays small; the Rust shell is guided and checked by it, not re-proved.

## Differential testing (the VGD bridge)

For each CRDT, the Rust merge and mutators are run against the same inputs as the Lean model, and the outputs are checked to agree. A disagreement is a bug in the Rust layer, caught against a proven reference rather than against hope.

## Honesty

Trusted computing base, axiom footprint, and the boundary between proven and differentially-tested behaviour are named explicitly, in the crdt-lean style.
