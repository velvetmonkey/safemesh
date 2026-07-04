# SafeMesh

Formally verified coordination primitives for ad-hoc mesh networks (LoRa, Wi-Fi mesh, BLE, off-grid / disaster / community).

SafeMesh ships small, embeddable building blocks whose correctness is machine-checked in Lean 4, not just tested. Each primitive is a dual artifact: a Lean proof of its key property, and a thin `no_std`-friendly Rust crate that is differential-tested against that proof.

## Status

**The delta-CRDT ladder is proven and the first Rust products ship against it.**

| Piece | Status |
|---|---|
| Lean delta suite (`lean/`): SEC-for-deltas + delta G-Counter, PN-Counter, OR-Set, RGA | **Proven** — zero `sorry`, axioms ⊆ {propext, Classical.choice, Quot.sound}, machine-gated (`Test/Axioms.lean`, a `lake build` default target) |
| Rust `safemesh-crdt` (`rust/`): delta G-Counter + PN-Counter, `no_std + alloc` (builds for `thumbv7em-none-eabihf`) | **Differentially tested** against the Lean oracle over corpus C (`tests/corpus.json`, emitted by the proven definitions via `lake exe corpus`) |
| Rust OR-Set + RGA | **Deferred** — the Lean side is proven; the crates are the named next step |

One proof, two bodies: the Lean development IS the semantics; the Rust crate is a second body of the same object, held to the first by differential conformance rather than by trust.

## Architecture

The verified core lives in the public, MIT-licensed [crdt-lean](https://github.com/velvetmonkey/crdt-lean): machine-checked state-based CvRDT convergence (SEC, conditional liveness, G-Set / G-Counter / PN-Counter / OR-Set / Sequence), zero `sorry`, standard axioms only. SafeMesh depends on that core and does not modify it.

SafeMesh adds:

- `lean/` — the delta-state (δ-CRDT) suite, PROVEN: deltas are carrier elements of a join-semilattice, so delta dissemination is order- and duplicate-insensitive with no causal-delivery assumption (`SafeMesh.delta_dissemination_sec`), and a delta replica is exactly a full-state replica over smaller payloads (`SafeMesh.delta_matches_state`). Concrete rungs, each reduced to what was shipped on the wire: G-Counter (`deltaGCounter_correct`), PN-Counter (`deltaPNCounter_correct_P/_N`), OR-Set add-wins membership (`deltaORSet_lookup`), RGA ordered read (`deltaRGA_read_mem` + inherited `read_sorted` / sequence-level SEC).
- `rust/crates/safemesh-crdt` — the product: `no_std + alloc` delta G-Counter and PN-Counter (verified to build on a bare-metal target), doc-commented with the Lean theorem each operation realises, and held to the proofs by `tests/conformance.rs`: a corpus of inputs AND expected outputs computed by the machine-checked Lean definitions (`lake exe corpus`), replayed byte-for-byte through the Rust implementation — including permutations and redeliveries, so the proven order-insensitivity is exercised, not assumed.

**TCB (honest).** The theorems are kernel-checked and universal. The Rust crate is checked against them over a **finite corpus** — evidence, not a universal theorem. The trusted bridge is: Lean's compiler evaluating the proven definitions (`lake exe corpus`, kept OFF the proof path and out of the axiom gate) → the JSON corpus → serde parsing in the std test harness. A disagreement anywhere in that loop fails the build; agreement is conformance evidence over C, no more, no less.

See `ARCHITECTURE.md`.

## License

AGPL-3.0-or-later. Copyright (c) 2026 Ben Cassie. See `LICENSE` and `NOTICE`.

Commercial licenses, to use SafeMesh without the AGPL network-copyleft obligations, are available. Contact the copyright holder.
