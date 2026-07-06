# SafeMesh architecture

SafeMesh follows verification-guided development (VGD): the Lean proof comes first and acts as the oracle; the shipped Rust code is differential-tested against it.

## Layers

1. **Verified core (external, public, MIT): crdt-lean.**
   Machine-checked state-based CvRDT convergence. Strong Eventual Consistency (same delivered set implies equal state), conditional liveness under fairness, and lawful instances (G-Set, G-Counter, PN-Counter, OR-Set, Sequence/RGA). Zero `sorry`; axioms limited to {propext, Classical.choice, Quot.sound}. SafeMesh consumes it as a dependency and does not modify it.

2. **Delta-state extension (this repo, `lean/`) — DONE, proven.**
   Full-state gossip does not fit constrained mesh links (LoRa duty cycles). Delta-state CRDTs ship only the change. Because the carrier is a join-semilattice, deltas are just carrier elements joined in: `SafeMesh.Delta` proves delta dissemination is order/duplicate-insensitive with no causal-delivery assumption and equals full-state gossip whenever the joins agree (the bandwidth win costs nothing). Four concrete rungs lift the whole crdt-lean instance suite to deltas: G-Counter, PN-Counter, OR-Set (add-wins membership over shipped deltas), RGA (ordered read over shipped deltas; ordering obligations inherited from crdt-lean's `read_sorted` / `read_strong_eventual_consistency`). Same toolchain and axiom discipline as crdt-lean, machine-gated by `Test/Axioms.lean` as a `lake build` default target.

3. **Product layer (this repo, `rust/`) — counters shipped, sets/sequence deferred.**
   `safemesh-crdt`: `no_std + alloc` delta G-Counter and PN-Counter (builds for `thumbv7em-none-eabihf`), mirroring the Lean model exactly (merge = pointwise max, delta = single-coordinate bump, PN value = ΣP − ΣN). Each public operation's doc comment names the Lean theorem it realises. OR-Set and RGA/Text are the named next step — their Lean side is already proven, but they are not proven product surface until the Rust body and corpus bridge land.

4. **Bindings, demos, and adapters — engineered.**
   C ABI, WASM, Python, web demos, storage adapters, and transport adapters call or mirror the core, but they are not theorem-proven. They must be tested and labelled as engineered glue. The verified claim stays with the Lean-backed in-house CRDT types and the Rust bodies that pass the Lean-generated oracle corpus.

## Differential testing (the VGD bridge)

`lean/Corpus.lean` (`lake exe corpus`) instantiates the proven definitions at `ι = Fin 4` and emits a deterministic JSON corpus: bump sequences in delivery order plus the expected converged state and value, computed by the machine-checked model. `rust/crates/safemesh-crdt/tests/conformance.rs` replays every case through the Rust implementation and asserts byte-for-byte agreement, including permutation and redelivery cases (the proven order-insensitivity is exercised) and a split-delivery merge case (the gossip step). A disagreement is a bug in the Rust layer, caught against a proven reference rather than against hope. Regenerate with `cd lean && lake exe corpus > ../rust/crates/safemesh-crdt/tests/corpus.json`.

This differential test is the honesty artifact. Property tests and fuzzers can find more bugs, but they only prove the code agrees with itself unless the Lean-generated oracle remains in the loop.

## Honesty

- **Proven (universal):** everything in `lean/SafeMesh/` — kernel-checked, zero `sorry`, axioms ⊆ {propext, Classical.choice, Quot.sound}, pinned per-theorem by `#guard_msgs` in `Test/Axioms.lean` (a default build target, so the gate cannot silently not-run).
- **Differentially tested (finite evidence over corpus C):** the Rust crate. The corpus is finite; conformance is evidence, not a universal theorem.
- **Laws-tested (finite evidence over generated scenarios):** custom/user types and unproven implementation glue. This checks the merge contract but does not earn a Lean-proven label.
- **Trusted (the named TCB of the bridge):** Lean's compiler evaluating the proven definitions in `lake exe corpus` (kept off the proof path — not a default target, imported by nothing in the proof tree, invisible to the axiom gate), the emitted JSON, and serde parsing in the std test harness. The `no_std` library itself has no dependencies and forbids `unsafe`.

One proof, two bodies: the Lean development is the semantics; the Rust crate is a second body of the same object, held to the first by conformance.
