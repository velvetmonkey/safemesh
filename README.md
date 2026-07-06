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
| LWW Register | **Tested, not proven** — flat max-register over `(timestamp, replica, value)`, covered by laws and wire tests but not by the Lean oracle corpus |
| Rust EventLog API | **Engineered** — append/merge/since/version record log with deterministic dedup; useful infrastructure, not the Lean-proven CRDT theorem itself |
| Canonical wire format | **Engineered + tested** — fixed tags, little-endian integers, length-prefixed records, and sorted set encodings; round-trip, malformed-input, and byte-stability tests cover the current Rust surface |
| C ABI spine (`safemesh-ffi`) | **Engineered + tested** — thin wrapper over the Rust core with a committed `include/safemesh.h` header and drift check |
| WASM / TypeScript binding (`safemesh-wasm`) | **Engineered + tested** — wasm-bindgen wrapper over the Rust core, with G-Counter replica/event-log exchange, committed TypeScript surface, and wasm32 build check |
| Python binding (`safemesh-python`) | **Engineered + tested** — PyO3/maturin wrapper over the Rust core, with G-Counter replica/event-log exchange, pyproject metadata, and Rust-side binding tests |
| Break-it demo (`cargo run -p safemesh-crdt --example break_it`) | **Runnable showpiece** — deterministic partition/drop/duplicate/reorder/heal scenario that exits nonzero unless the replicas converge |
| Transport coverage contract | **Engineered + tested** — `TransportAdapter`, `InMemoryTransport`, and `anti_entropy` exercise subscribe/send/connectivity under drop, duplicate, reorder, partition, and heal campaigns |
| Cold-chain kill-test (`cargo run -p safemesh-crdt --example cold_chain_kill_test`) | **Engineered evaluation** — software-only field-science vertical using the flat CRDT carriers and event-log exchange under transport faults |
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

## CI Gate

Run `./scripts/ci.sh` to execute the same full gate used by the repository workflow: Lean build, Rust formatting/tests/features/examples, the break-it and cold-chain demos, embedded and WASM target builds, and the web test/build pair.

## Laws harness

Enable `--features laws` to use `safemesh_crdt::laws`. The harness checks merge commutativity, associativity, idempotence, identity, deterministic shuffle convergence, redelivery, and split/drop-then-merge recovery over supplied sample states and deltas. This is the CI bar for custom types; it does not make custom code proven.

## Wire format and C ABI

The core crate exposes `WireEncode` / `WireDecode` for the current u64-oriented wire surface and for `Record` / `EventLog` framing. The format is deliberately boring: one-byte tags, little-endian integer fields, u32 length prefixes, and BTree-backed sorted encodings for set-like state.

The FFI spine lives in `rust/crates/safemesh-ffi`. It exposes opaque G-Counter handles and a delta-to-wire helper through `include/safemesh.h`. The header is committed and checked by tests; `cbindgen.toml` is present for regeneration when `cbindgen` is installed.

## Bindings

The first bindings are thin wrappers over the same Rust core:

- `rust/crates/safemesh-wasm` exposes G-Counter, `GCounterReplica`, LWW Register, `LwwRegisterReplica`, canonical record/log bytes, and merge-from-bytes through wasm-bindgen, with a committed TypeScript declaration file.
- `rust/crates/safemesh-python` exposes the same G-Counter and LWW Register replica/event-log surface through PyO3, with `pyproject.toml` configured for maturin.

Neither binding reimplements merge logic. Both are engineered/tested glue around the Rust core, and both have tests that exchange canonical record/log bytes and converge through that core. The G-Counter binding rides the Lean-backed surface; the LWW Register binding remains tested-not-proven.

## Break-it demo

Run:

```sh
cd rust
cargo run -p safemesh-crdt --example break_it
```

The demo partitions four replicas, drops cross-partition packets, delivers same-partition packets in reverse order, duplicates a packet, then heals with anti-entropy. It prints `CONVERGED=true ...` and exits nonzero if convergence fails.

## Integrity Vertical Kill-Test

`KILL-TEST.md` records the first software-only integrity vertical: field-science cold-chain sample custody. Run `cargo run -p safemesh-crdt --example cold_chain_kill_test` to exercise `EventLog`, `InMemoryTransport`, and the flat CRDT carriers through drop, duplicate, reorder, partition, and heal. This is an engineered evaluation artifact, not proof of sensors, custody law, storage durability, or real network delivery.

## Transport Coverage Contract

The core crate exposes `TransportAdapter`, `InMemoryTransport`, and `anti_entropy`. The adapter contract covers peer subscription, link connectivity, sending record batches, draining subscribed inboxes, and version-vector anti-entropy via `EventLog::since`. Versions advertise contiguous per-replica prefixes, so an out-of-order later record cannot hide earlier missing records.

The in-memory adapter is for CI fault campaigns. It can drop the next send, duplicate the next send, reverse pending delivery for a peer, partition a link, and heal it. These tests show the engineered adapter meets the coverage contract; they do not prove a real radio or network delivers packets.

## Non-goals

SafeMesh v0 is flat-first. It does not cover references between objects, trees, ordered move operations, leader election, hardware pucks, radio-delivery proofs, or a claim to be a faster Yjs. Binding glue, demos, and user-defined reducers are engineered and tested; they are not proof-carrying artifacts.

`LwwRegister` is included as a tested flat type for builders who need it, but it is intentionally outside the current Lean-proven surface. Its docs and claims must stay in the tested-not-proven bucket unless a Lean proof and oracle corpus are added.

## License

AGPL-3.0-or-later. Copyright (c) 2026 Ben Cassie. See `LICENSE` and `NOTICE`.

Commercial licenses, to use SafeMesh without the AGPL network-copyleft obligations, are available. Contact the copyright holder.
