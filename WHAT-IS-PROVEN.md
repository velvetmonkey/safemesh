# What is proven

SafeMesh follows verification-guided development:

1. Lean definitions state the semantics.
2. Lean theorems prove convergence properties over those definitions.
3. A Lean executable emits an oracle corpus from the same definitions.
4. Rust product code replays that corpus and must match it byte-for-byte.

The proof is in Lean. The Rust crate is a second implementation held to the proof by differential conformance.

## Lean proof surface

The SafeMesh Lean suite proves:

- Delta dissemination is order-insensitive, duplicate-insensitive, and convergent when replicas receive the same delta set.
- Delta-state replicas agree with full-state replicas when they have joined the same updates.
- Delta G-Counter state matches the max tally per replica coordinate.
- Delta PN-Counter state matches the componentwise increment/decrement G-Counters and read value.
- OR-Set add-wins membership follows from observed add tokens and remove tombstones.
- RGA/Text read order depends on the live positioned set, not delivery order.

The upstream `crdt-lean` corpus also proves the state-based CvRDT laws, conditional liveness under explicit fairness and quiescence assumptions, G-Set, G-Counter, PN-Counter, OR-Set, and Sequence/RGA convergence.

## Rust product surface today

`rust/crates/safemesh-crdt` currently ships:

- `GSet`
- `GCounter`
- `PnCounter`
- `OrSet`
- `Rga`
- `EventLog`

The CRDT carriers are `no_std + alloc`, forbid `unsafe`, and are checked by `tests/conformance.rs` against `tests/corpus.json`. `EventLog` is engineered infrastructure and is tested by Rust unit tests; it is not by itself a Lean-proven application-state convergence theorem.

## The honesty boundary

The Lean theorems are universal over their mathematical models. The Rust implementation is not itself proven in Lean. It is differentially tested against the Lean-generated corpus. This corpus bridge is the honesty artifact that keeps "verified" tied to the proof.

Property tests, fuzzers, and demos are useful, but they cannot by themselves justify a proven claim.
