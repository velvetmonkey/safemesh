# SafeMesh for builders

A verified convergent-state layer you build on. Append events anywhere, sync over anything, and every replica provably converges. No duplicates, no lost events, no conflict bugs. The merge is machine-checked, and re-runnable in your CI.

SafeMesh ships small, embeddable building blocks whose correctness is machine-checked in Lean 4, not just tested. Each primitive is a dual artifact: a Lean proof of its key property, and a thin `no_std`-friendly Rust crate that is differential-tested against that proof.

SafeMesh is the builder's mesh: a convergent state fabric for infrastructure where silent divergence is expensive. You bring the transport and the application schema; SafeMesh gives you verified merge behavior for the in-house CRDT types and honest test harnesses for your own types.

## Status

**The delta-CRDT ladder is proven and the first Rust products ship against it.**

| Piece | Status |
|---|---|
| Lean delta suite (`lean/`): SEC-for-deltas + delta G-Counter, PN-Counter, OR-Set, RGA | **Proven** — zero `sorry`, axioms ⊆ {propext, Classical.choice, Quot.sound}, machine-gated (`Test/Axioms.lean`, a `lake build` default target) |
| Rust `safemesh-crdt` (`rust/`): G-Set, delta G-Counter, delta PN-Counter, delta OR-Set, RGA/Text, `no_std + alloc` (builds for `thumbv7em-none-eabihf`) | **Differentially tested** against the Lean oracle over corpus C (`tests/corpus.json`, emitted by the proven definitions via `lake exe corpus`) |
| Rust EventLog API | **Engineered** — append/merge/since/version record log with deterministic dedup; useful infrastructure, not the Lean-proven CRDT theorem itself |
| Canonical wire format | **Engineered + tested** — fixed tags, little-endian integers, length-prefixed records, and sorted set encodings; round-trip, malformed-input, and byte-stability tests cover the current Rust surface |
| C ABI spine (`safemesh-ffi`) | **Engineered + tested** — thin wrapper over the Rust core with a committed `include/safemesh.h` header and drift check |
| Laws harness (`--features laws`) | **Reusable tests** — checks merge laws and drop/dup/reorder convergence scenarios for any type implementing the SafeMesh traits; supplemental to the Lean-oracle diff |
| User-defined types | **Tested, not proven** — users can implement the same merge contract and run the laws harness, but SafeMesh does not prove arbitrary application code |

One proof, two bodies: the Lean development IS the semantics; the Rust crate is a second body of the same object, held to the first by differential conformance rather than by trust.

## Architecture

The verified core lives in the public, MIT-licensed [crdt-lean](https://github.com/velvetmonkey/crdt-lean): machine-checked state-based CvRDT convergence (SEC, conditional liveness, G-Set / G-Counter / PN-Counter / OR-Set / Sequence), zero `sorry`, standard axioms only. SafeMesh depends on that core and does not modify it.

SafeMesh adds:

- `lean/` — the delta-state (δ-CRDT) suite, PROVEN: deltas are carrier elements of a join-semilattice, so delta dissemination is order- and duplicate-insensitive with no causal-delivery assumption (`SafeMesh.delta_dissemination_sec`), and a delta replica is exactly a full-state replica over smaller payloads (`SafeMesh.delta_matches_state`). Concrete rungs, each reduced to what was shipped on the wire: G-Counter (`deltaGCounter_correct`), PN-Counter (`deltaPNCounter_correct_P/_N`), OR-Set add-wins membership (`deltaORSet_lookup`), RGA ordered read (`deltaRGA_read_mem` + inherited `read_sorted` / sequence-level SEC).
- `rust/crates/safemesh-crdt` — the product: `no_std + alloc` G-Set, delta G-Counter, delta PN-Counter, delta OR-Set, and RGA/Text state (verified to build on a bare-metal target), held to the proofs by `tests/conformance.rs`: a corpus of inputs AND expected outputs computed by the machine-checked Lean definitions (`lake exe corpus`), replayed byte-for-byte through the Rust implementation — including permutations and redeliveries, so the proven order-insensitivity is exercised, not assumed.

**TCB (honest).** The theorems are kernel-checked and universal. The Rust crate is checked against them over a **finite corpus** — evidence, not a universal theorem. The trusted bridge is: Lean's compiler evaluating the proven definitions (`lake exe corpus`, kept OFF the proof path and out of the axiom gate) → the JSON corpus → serde parsing in the std test harness. A disagreement anywhere in that loop fails the build; agreement is conformance evidence over C, no more, no less.

See `ARCHITECTURE.md`, `CLAIMS.md`, and `WHAT-IS-PROVEN.md`.

## Laws harness

Enable `--features laws` to use `safemesh_crdt::laws`. The harness checks merge commutativity, associativity, idempotence, identity, deterministic shuffle convergence, redelivery, and split/drop-then-merge recovery over supplied sample states and deltas. This is the CI bar for custom types; it does not make custom code proven.

## Wire format and C ABI

The core crate exposes `WireEncode` / `WireDecode` for the current u64-oriented wire surface and for `Record` / `EventLog` framing. The format is deliberately boring: one-byte tags, little-endian integer fields, u32 length prefixes, and BTree-backed sorted encodings for set-like state.

The FFI spine lives in `rust/crates/safemesh-ffi`. It exposes opaque G-Counter handles and a delta-to-wire helper through `include/safemesh.h`. The header is committed and checked by tests; `cbindgen.toml` is present for regeneration when `cbindgen` is installed.

## Non-goals

SafeMesh v0 is flat-first. It does not cover references between objects, trees, ordered move operations, leader election, hardware pucks, radio-delivery proofs, or a claim to be a faster Yjs. Binding glue, demos, and user-defined reducers are engineered and tested; they are not proof-carrying artifacts.

## License

AGPL-3.0-or-later. Copyright (c) 2026 Ben Cassie. See `LICENSE` and `NOTICE`.

Commercial licenses, to use SafeMesh without the AGPL network-copyleft obligations, are available. Contact the copyright holder.
