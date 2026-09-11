# What is proven

SafeMesh follows verification-guided development:

1. Lean definitions state the semantics.
2. Lean theorems prove convergence properties over those definitions.
3. A Lean executable emits an oracle corpus from the same definitions.
4. Rust product code replays that JSON corpus and compares state vectors, sets, read vectors, numeric values, ownership decisions and optional allocation/sequence results with the parsed expectations. **TESTED** — Evidence: [`conformance`](rust/crates/safemesh-crdt/tests/conformance.rs); run from `rust/`: `cargo test -p safemesh-crdt --test conformance --locked`.

The proof is in Lean. The Rust crate is a second implementation held to the proof by differential conformance.

<span id="what-you-can-rely-on-in-the-model"></span>
<span id="named-results-translated"></span>

## Lean proof surface

The SafeMesh Lean suite proves the following model properties. The general delta model assumes a join-semilattice with bottom and equal sets of delivered deltas; it does not assume causal delivery or supply missing deltas. Eventual recovery is a delivery assumption. OR-Set membership uses observed tokens; RGA positions are caller supplied, and identifier allocation is outside its model. [Model and assumptions](lean/SafeMesh/Delta.lean).

- Delta dissemination is order-insensitive, duplicate-insensitive, and convergent when replicas receive the same delta set. **PROVEN** — Evidence: [`delta_dissemination_sec`](lean/SafeMesh/Delta.lean).
- Delta-state replicas agree with full-state replicas when they have joined the same updates. **PROVEN** — Evidence: [`delta_matches_state`](lean/SafeMesh/Delta.lean).
- Delta G-Counter state matches the max tally per replica coordinate. **PROVEN** — Evidence: [`deltaGCounter_correct`](lean/SafeMesh/DeltaGCounter.lean).
- Delta PN-Counter state matches the componentwise increment/decrement G-Counters and read value. **PROVEN** — Evidence: [`deltaPNCounter_correct_P; deltaPNCounter_correct_N; deltaPNCounter_value_matches`](lean/SafeMesh/DeltaPNCounter.lean).
- OR-Set add-wins membership follows from observed add tokens and remove tombstones. **PROVEN** — Evidence: [`deltaORSet_lookup`](lean/SafeMesh/DeltaORSet.lean).
- RGA/Text read order depends on the live positioned set, not delivery order. **PROVEN** — Evidence: [`deltaRGA_read_sec; deltaRGA_read_mem; deltaRGA_read_sorted`](lean/SafeMesh/DeltaRGA.lean).

<span id="why-these-count-as-checked-proofs"></span>

### Record kernel

`lean/SafeMesh/RecordKernel.lean`, namespace `SafeMesh.RecordKernel`, declares 21 theorems. The module is imported by `lean/SafeMesh.lean`, so the default `SafeMesh` target builds it, and each of the 21 has its axiom set pinned by `#guard_msgs` in `Test/Axioms.lean`, itself a default target; `lake build` in CI therefore fails on any new axiom, `sorry` or `native_decide` in any of them. At source revision `279926bbe05eb6cc592af54c1c4c28a9f21f84e9` the [SafeMesh CI run 34390575569](https://github.com/velvetmonkey/safemesh/actions/runs/34390575569) built that gate successfully. This is commit-specific evidence, not certification of every later commit.

Read the theorems against the model the file defines. A `Replica` is a log of `(id, payload)` records plus a cached live state; `outcome` classifies a record as accepted, duplicate or collision by comparing the full payload, not a hash; `step` mutates only on acceptance; `Reachable` is what `step` can produce from `empty`. Payloads range over any bottomed join-semilattice, so the same theorems apply to every carrier.

- `admission`: for a replica whose provenance equals its log, the log changes if and only if the record is accepted, the live state changes if and only if the record is accepted, and the live value after the step is the old value joined with the payload when the record is accepted and the old value unchanged otherwise; a redundant payload can leave the value equal in both cases, so the value alone does not characterize acceptance. `replayAgrees` supplies that provenance premise for every reachable replica. **PROVEN** — Evidence: [`admission`](lean/SafeMesh/RecordKernel.lean).
- `idempotence`: `step (step s r) r = step s r` for every replica `s`, including one with an arbitrary cache. **PROVEN** — Evidence: [`idempotence`](lean/SafeMesh/RecordKernel.lean).
- `collision`: an id already present in an id-unique log with a different payload yields the `collision` outcome and leaves the replica unchanged; and any two reachable replicas with equal logs have equal live state. **PROVEN** — Evidence: [`collision`](lean/SafeMesh/RecordKernel.lean).
- `replayAgrees`: every reachable replica's live state equals `replay` of its log; the retired `legacyStep` transition really produces the measured Rust defect (`RustDefect legacyWitness`, checked by the kernel rather than by native evaluation); and no reachable replica exhibits that defect. **PROVEN** — Evidence: [`replayAgrees`](lean/SafeMesh/RecordKernel.lean).
- `tokenOwner`, `tokenDisjoint`, `tokenSequence`, `allocatedOwned`, `sequenceFresh`: a token is defined as `sequence * writers + author`; modulo recovers the author, so two distinct valid authors mint distinct tokens at every pair of sequences; equal tokens from one author force equal sequences when the writer count is positive; a successful `allocateToken` implies a valid author, a positive sequence and a result at most `maxWord`; a successful `nextSequence` strictly increases and stays at most `maxWord`. **PROVEN** — Evidence: [`tokenOwner; tokenDisjoint; tokenSequence; allocatedOwned; sequenceFresh`](lean/SafeMesh/RecordKernel.lean).
- `validPermitted`, `ownedWriter`, `ownedCoordinate`, `competingRefused`, `staleRefused`, `foreignCoordinateRefused`, `foreignTokenRefused`, `refusedUnchanged`, `ownedReachable`, `ownedConvergence`: `refuses` is defined as the negation of the decidable `permitted` predicate, so exactly the permitted domain passes by that definition, and `validPermitted` is the one-way positive control that a permitted write is not refused; a passing write has an author below the writer count and, for a local write, equal to the configured writer, and a passing counter payload names the author's own coordinate; a write is refused when the lock is not held, when the generation differs from the current one, when a counter coordinate is foreign, or when an add's token is not the one allocated to that author and sequence; a refused `ownedStep` returns its input unchanged; a guarded step from a reachable replica reaches a reachable replica; and equal logs still give equal live state. **PROVEN** — Evidence: [`validPermitted; ownedWriter; ownedCoordinate; competingRefused; staleRefused; foreignCoordinateRefused; foreignTokenRefused; refusedUnchanged; ownedReachable; ownedConvergence`](lean/SafeMesh/RecordKernel.lean).
- `replayOwnedReachable`, `checkedRestartSound`: replaying a history through the guard from a reachable state yields a reachable state, so a successful `checkedRestart` returns a replica whose live state equals replay of its log and whose allocation mark equals the saved mark. `replayOwned` and `checkedRestart` are defined to return `none` on a refused or non-accepted record and on a mismatched mark; the theorem speaks only about the `some` case. **PROVEN** — Evidence: [`replayOwnedReachable; checkedRestartSound`](lean/SafeMesh/RecordKernel.lean).

What the record kernel does not establish. Its transitions are atomic because they are functions, which says nothing about a crash between a log append and a cache update in a real process. Equal *accepted* sets determine state; equal *input* sets with conflicting payloads do not, because rejection makes the accepted set depend on arrival order. `held`, `generation` and `currentGeneration` are inputs, so the theorems prove the decision taken on those facts, not that the OS lock is exclusive or that the generation was read correctly. `checkedRestart` takes an already-decoded history, so frame validation, lock reacquisition, `fsync`, rename and power-loss recovery are outside it. No theorem here covers version vectors, cursors, envelopes, authentication, or how a collision is reported to a peer.

**PROVEN** — The upstream `crdt-lean` corpus also proves the state-based CvRDT laws, conditional liveness under explicit fairness and quiescence assumptions, G-Set, G-Counter, PN-Counter, OR-Set, and Sequence/RGA convergence. Evidence at the pinned upstream revision: [`Crdt.strong_eventual_consistency`](https://github.com/velvetmonkey/crdt-lean/blob/7d9016164c7b9208b93bbb1eef2f5c018d626a32/Crdt/Convergence.lean), [`Crdt.DeliverySystem.eventual_agreement`](https://github.com/velvetmonkey/crdt-lean/blob/7d9016164c7b9208b93bbb1eef2f5c018d626a32/Crdt/Liveness.lean), [G-Set and counter instances](https://github.com/velvetmonkey/crdt-lean/blob/7d9016164c7b9208b93bbb1eef2f5c018d626a32/Crdt/Instances.lean), [`Crdt.ORSet.strong_eventual_consistency`](https://github.com/velvetmonkey/crdt-lean/blob/7d9016164c7b9208b93bbb1eef2f5c018d626a32/Crdt/ORSet.lean), and [`Crdt.Sequence.read_strong_eventual_consistency`](https://github.com/velvetmonkey/crdt-lean/blob/7d9016164c7b9208b93bbb1eef2f5c018d626a32/Crdt/Sequence.lean).

<span id="how-the-proof-reaches-rust"></span>

## Rust product surface today

`rust/crates/safemesh-crdt` currently ships:

- `GSet` **TESTED** — Evidence: [`conformance`](rust/crates/safemesh-crdt/tests/conformance.rs); run from `rust/`: `cargo test -p safemesh-crdt --test conformance --locked`.
- `GCounter` **TESTED** — Evidence: [`conformance`](rust/crates/safemesh-crdt/tests/conformance.rs); run from `rust/`: `cargo test -p safemesh-crdt --test conformance --locked`.
- `PnCounter` **TESTED** — Evidence: [`conformance`](rust/crates/safemesh-crdt/tests/conformance.rs); run from `rust/`: `cargo test -p safemesh-crdt --test conformance --locked`.
- `OrSet` **TESTED** — Evidence: [`conformance`](rust/crates/safemesh-crdt/tests/conformance.rs); run from `rust/`: `cargo test -p safemesh-crdt --test conformance --locked`.
- `Rga` **TESTED** — Evidence: [`conformance`](rust/crates/safemesh-crdt/tests/conformance.rs); run from `rust/`: `cargo test -p safemesh-crdt --test conformance --locked`.
- `EventLog` **TESTED** — Evidence: [`record_1_1_live_and_replay; duplicates_and_collisions_do_not_invoke_application`](rust/crates/safemesh-crdt/tests/record_admission.rs); run from `rust/`: `cargo test -p safemesh-crdt --features laws --test record_admission --locked`.
- `LwwRegister` **TESTED** — Evidence: [`wire`](rust/crates/safemesh-crdt/tests/wire.rs); run from `rust/`: `cargo test -p safemesh-crdt --test wire --locked`. **TESTED** — Evidence: [`merge_laws_hold_for_in_house_samples; convergence_harness_hammers_delta_delivery_shapes`](rust/crates/safemesh-crdt/tests/laws.rs); run from `rust/`: `cargo test -p safemesh-crdt --features laws --test laws --locked`.
- `EnableWinsFlag` **TESTED** — Evidence: [`wire`](rust/crates/safemesh-crdt/tests/wire.rs); run from `rust/`: `cargo test -p safemesh-crdt --test wire --locked`. **TESTED** — Evidence: [`merge_laws_hold_for_in_house_samples; convergence_harness_hammers_delta_delivery_shapes`](rust/crates/safemesh-crdt/tests/laws.rs); run from `rust/`: `cargo test -p safemesh-crdt --features laws --test laws --locked`.
- `LwwMap` **TESTED** — Evidence: [`wire`](rust/crates/safemesh-crdt/tests/wire.rs); run from `rust/`: `cargo test -p safemesh-crdt --test wire --locked`. **TESTED** — Evidence: [`merge_laws_hold_for_in_house_samples; convergence_harness_hammers_delta_delivery_shapes`](rust/crates/safemesh-crdt/tests/laws.rs); run from `rust/`: `cargo test -p safemesh-crdt --features laws --test laws --locked`.

**TESTED** — The Lean-backed CRDT carriers are `no_std + alloc`, forbid `unsafe`, and are checked by `tests/conformance.rs` against `tests/corpus.json`. `EventLog` is engineered infrastructure and is tested by Rust unit tests; it is not by itself a Lean-proven application-state convergence theorem, although its `admit_with` decision transcribes the record kernel's `step` and is tested against it. `LwwRegister`, `EnableWinsFlag`, and `LwwMap` are also engineered/tested, not Lean-proven. Evidence: [crate attributes and implementation](rust/crates/safemesh-crdt/src/lib.rs), [conformance tests](rust/crates/safemesh-crdt/tests/conformance.rs) (`cargo test -p safemesh-crdt --test conformance --locked` from `rust/`).

**TESTED** — The wire format and C ABI are also engineered infrastructure. They are covered by exact-byte, round-trip, malformed-input, FFI smoke, and header-drift tests, not by Lean theorems. **TESTED** — Evidence: [`wire`](rust/crates/safemesh-crdt/tests/wire.rs); run from `rust/`: `cargo test -p safemesh-crdt --test wire --locked`. [FFI smoke (`scripts/ffi-c-smoke.sh` from the repository root)](scripts/ffi-c-smoke.sh); [committed_header_has_not_drifted (`cargo test -p safemesh-ffi --test header --locked` from `rust/`)](rust/crates/safemesh-ffi/tests/header.rs).

The record kernel reaches Rust in two different ways. The ownership guard, token allocation and sequence counter are emitted into the `ownership`, `allocation` and `sequence` sections of `tests/corpus.json` from the same Lean definitions the theorems are about, and `tests/conformance.rs` replays them through the Rust `ownership` module. Record admission in `EventLog::admit_with` and the durable restart path in `local::DurableReplica::restart_counter` and `restart_utf8_set` are transcriptions checked by Rust unit tests (`tests/record_admission.rs` and the tests inside `src/local.rs`); they are not in the corpus. All of this is tested, not proven; only the ownership part is corpus-conformant. **TESTED** — Evidence: [`conformance`](rust/crates/safemesh-crdt/tests/conformance.rs); run from `rust/`: `cargo test -p safemesh-crdt --test conformance --locked`. **TESTED** — Evidence: [`record_1_1_live_and_replay; duplicates_and_collisions_do_not_invoke_application`](rust/crates/safemesh-crdt/tests/record_admission.rs); run from `rust/`: `cargo test -p safemesh-crdt --features laws --test record_admission --locked`. **TESTED** — Evidence: [`restart_still_works_and_fresh_allocation; restart_invalid_history`](rust/crates/safemesh-crdt/src/local.rs); run from `rust/`: `cargo test -p safemesh-crdt --features laws --lib --locked`.

API present: WASM/TypeScript and Python expose wrappers over the Rust core. Runtime tested: their Rust-side tests check core calls, canonical record/log bytes, and convergence by merging those bytes. Integration tested: the Linux package smoke flow exercises a generated WASM package in Node and an installed Python wheel. Artifact available: these flows create local packages; registry availability is not established by them. Maintainer-supported status for both bindings is unknown. These binding tests are not separate Lean proofs. Binding exposure for `LwwRegister`, `EnableWinsFlag`, and `LwwMap` inherits those types' tested-not-proven labels. **TESTED** — Evidence: [package smoke (`scripts/package-smoke.sh` from the repository root)](scripts/package-smoke.sh); [WASM tests](rust/crates/safemesh-wasm/tests/typescript.rs); [Python tests](rust/crates/safemesh-python/src/lib.rs).

The break-it demo and cold-chain kill-test are also engineered evidence. They are useful because they are visible and re-runnable, but they are not substitutes for the Lean proof or differential corpus. **TESTED** — Evidence: [`cold_chain_kill_test`](rust/crates/safemesh-crdt/examples/cold_chain_kill_test.rs); run from `rust/`: `cargo test -p safemesh-crdt --examples --locked`.

The transport coverage contract is engineered/tested. `InMemoryTransport` validates subscription, connectivity, drop, duplicate, reorder, partition, heal, and version-vector anti-entropy behavior in CI. This does not prove radio or network delivery. **TESTED** — Evidence: [`transport`](rust/crates/safemesh-crdt/tests/transport.rs); run from `rust/`: `cargo test -p safemesh-crdt --test transport --locked`.

<span id="what-remains-outside"></span>

## The honesty boundary

The Lean theorems are universal over their mathematical models. The Rust implementation is not itself proven in Lean. It is differentially tested against the Lean-generated corpus. This corpus bridge is the honesty artifact that keeps "verified" tied to the proof.

Property tests, fuzzers, and demos are useful, but they cannot by themselves justify a proven claim.

The record kernel widens the proof surface, not this boundary. Its theorems are about a modeled log, cache and guard; the Rust `EventLog`, `ownership` and `local` modules are held to that model by tests, and for ownership by the corpus, and the facts the guard decides on come from a filesystem shell that is tested rather than proven.

## Laws harness

The optional Rust `laws` module checks merge commutativity, associativity, idempotence, identity, redelivery, shuffled delivery, and split/drop-then-merge convergence over user-supplied examples. It is designed so third-party CRDT types can run the same behavioral bar in their own CI.

The harness is not a theorem prover. Passing it labels a type as laws-tested, not Lean-proven. **TESTED** — Evidence: [`merge_laws_hold_for_in_house_samples; convergence_harness_hammers_delta_delivery_shapes`](rust/crates/safemesh-crdt/tests/laws.rs); run from `rust/`: `cargo test -p safemesh-crdt --features laws --test laws --locked`.
