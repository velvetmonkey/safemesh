# SafeMesh architecture

SafeMesh follows verification-guided development (VGD): the Lean proof comes first and acts as the oracle; the shipped Rust code is differential-tested against it.

## Layers

1. **Verified core (external, public, MIT): crdt-lean.**
   Machine-checked state-based CvRDT convergence. Strong Eventual Consistency (same delivered set implies equal state), conditional liveness under fairness, and lawful instances (G-Set, G-Counter, PN-Counter, OR-Set, Sequence/RGA). Zero `sorry`; axioms limited to {propext, Classical.choice, Quot.sound}. SafeMesh consumes it as a dependency and does not modify it.

2. **Delta-state extension (this repo, `lean/`) — DONE, proven.**
   Full-state gossip does not fit constrained mesh links (LoRa duty cycles). Delta-state CRDTs ship only the change. Because the carrier is a join-semilattice, deltas are just carrier elements joined in: `SafeMesh.Delta` proves delta dissemination is order/duplicate-insensitive with no causal-delivery assumption and equals full-state gossip whenever the joins agree (the bandwidth win costs nothing). Four concrete rungs lift the whole crdt-lean instance suite to deltas: G-Counter, PN-Counter, OR-Set (add-wins membership over shipped deltas), RGA (ordered read over shipped deltas; ordering obligations inherited from crdt-lean's `read_sorted` / `read_strong_eventual_consistency`). Same toolchain and axiom discipline as crdt-lean, machine-gated by `Test/Axioms.lean` as a `lake build` default target.

3. **Product layer (this repo, `rust/`) — flat CRDT carriers shipped.**
   `safemesh-crdt`: `no_std + alloc` G-Set, delta G-Counter, delta PN-Counter, delta OR-Set, and RGA/Text state (builds for `thumbv7em-none-eabihf`), mirroring the Lean models. G-Counter merge is pointwise max, PN value is ΣP − ΣN, OR-Set is observed-token add-wins, and RGA read is sorted live positions. The EventLog API is engineered append/merge/since/version infrastructure around those deltas. `LwwRegister` and `EnableWinsFlag` are present as tested-not-proven flat types.

4. **Bindings, demos, and adapters — engineered.**
   Canonical wire encoding, C ABI, WASM, Python, web demos, storage adapters, and transport adapters call or mirror the core, but they are not theorem-proven. They must be tested and labelled as engineered glue. The verified claim stays with the Lean-backed in-house CRDT types and the Rust bodies that pass the Lean-generated oracle corpus.

## Differential testing (the VGD bridge)

`lean/Corpus.lean` (`lake exe corpus`) instantiates the proven definitions and emits a deterministic JSON corpus: delivered deltas plus the expected converged state/read, computed by the machine-checked model. `rust/crates/safemesh-crdt/tests/conformance.rs` replays every case through the Rust implementation and asserts byte-for-byte agreement for G-Set, G-Counter, PN-Counter, OR-Set, and RGA, including permutation and redelivery cases (the proven order-insensitivity is exercised) and a split-delivery merge case (the gossip step). A disagreement is a bug in the Rust layer, caught against a proven reference rather than against hope. Regenerate with `cd lean && lake exe corpus > ../rust/crates/safemesh-crdt/tests/corpus.json`.

This differential test is the honesty artifact. Property tests and fuzzers can find more bugs, but they only prove the code agrees with itself unless the Lean-generated oracle remains in the loop.

## Wire and ABI

The canonical Rust wire format uses fixed one-byte tags, little-endian integer fields, u32 length prefixes, and sorted BTree-backed encodings for set-like state. This is the cross-language byte contract; content-addressed dedup depends on these bytes staying stable.

`rust/crates/safemesh-ffi` is the first C ABI spine. It exposes opaque G-Counter handles and a delta-to-wire helper through `include/safemesh.h`. The crate carries `cbindgen.toml`; the committed header is protected by a drift test so ABI changes are explicit.

## Bindings

`rust/crates/safemesh-wasm` and `rust/crates/safemesh-python` are the first language bindings. They expose G-Counter operations, G-Counter replica/event-log exchange, and canonical record/log bytes by calling the same Rust core. They do not reimplement merge logic, and they stay in the engineered/tested bucket.

## Break-it demo

`cargo run -p safemesh-crdt --example break_it` is the CI-friendly showpiece. It drives G-Counter, OR-Set, and RGA state through partition, drop, duplicate, reorder, and heal phases, then exits nonzero unless all replicas converge.

## Integrity Vertical Kill-Test

`cargo run -p safemesh-crdt --example cold_chain_kill_test` is the first software-only vertical check. It models field-science cold-chain sample custody with `EventLog`, `InMemoryTransport`, `GSet`, `OrSet`, `Rga`, and `GCounter`, then runs through drop, duplicate, reorder, partition, and heal. This validates that the current flat-first surface can express one load-bearing workflow slice; it is not a proof of domain procedure, sensor truth, durability, or real transport delivery.

## Transport Coverage Contract

`TransportAdapter` is the engineered boundary for send/subscribe/connectivity. `anti_entropy` uses `EventLog::since(remote_version)` to resend missing records once a link is available. Event-log versions advance only over contiguous per-replica prefixes, so receiving a later sequence before an earlier one does not mask the gap. `InMemoryTransport` is the deterministic CI adapter for fault campaigns: drop, duplicate, reverse delivery, partition, and heal. This validates adapter behavior under the coverage contract; it is not a proof of real network delivery.

## Honesty

- **Proven (universal):** everything in `lean/SafeMesh/` — kernel-checked, zero `sorry`, axioms ⊆ {propext, Classical.choice, Quot.sound}, pinned per-theorem by `#guard_msgs` in `Test/Axioms.lean` (a default build target, so the gate cannot silently not-run).
- **Differentially tested (finite evidence over corpus C):** the Rust crate. The corpus is finite; conformance is evidence, not a universal theorem.
- **Laws-tested (finite evidence over generated scenarios):** custom/user types, `LwwRegister`, `EnableWinsFlag`, and unproven implementation glue. The optional `laws` module checks merge laws, redelivery, shuffled delivery, and split/drop-then-merge recovery. This checks the merge contract but does not earn a Lean-proven label.
- **Trusted (the named TCB of the bridge):** Lean's compiler evaluating the proven definitions in `lake exe corpus` (kept off the proof path — not a default target, imported by nothing in the proof tree, invisible to the axiom gate), the emitted JSON, and serde parsing in the std test harness. The `no_std` library itself has no dependencies and forbids `unsafe`.

One proof, two bodies: the Lean development is the semantics; the Rust crate is a second body of the same object, held to the first by conformance.
