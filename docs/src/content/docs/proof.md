---
title: The proof and its boundary — main (unreleased)
description: Named Lean theorems, their assumptions, and the finite bridge to Rust.
---

This is the proof story for **main (unreleased)**. **PROVED applies to the Lean models. Rust is TESTED against a finite oracle corpus. Neither establishes real network delivery, storage durability, sensor truth or correctness of arbitrary application code.** The [claims guide](/safemesh/claims/) remains the ceiling for product claims. [Evidence: `CLAIMS.md`](https://github.com/velvetmonkey/safemesh/blob/main/CLAIMS.md).

## What you can rely on in the model

Imagine two replicas receiving the same updates in different orders, one receiving some updates twice. `SafeMesh.delta_dissemination_sec` proves that their resulting states are equal. There is **no causal-delivery assumption** for this state-based join model. There is a crucial premise: the underlying sets of delivered deltas are equal. [Evidence: `Delta.lean`, theorem at line 70](https://github.com/velvetmonkey/safemesh/blob/main/lean/SafeMesh/Delta.lean).

The statement uses a type `S` with a join-semilattice and bottom element: a lawful merge and an empty state. `deltaApply` folds a list of deltas from that empty state. In Lean notation, the premise and conclusion are:

```lean
(h : l₁.toFinset = l₂.toFinset) :
    deltaApply l₁ = deltaApply l₂
```

`toFinset` forgets order and repetition. It does not invent a missing delta. **ASSUMED for eventual recovery:** the missing updates eventually arrive or are recovered by merge/anti-entropy. Delivery itself is outside the theorem. [Evidence: the same theorem](https://github.com/velvetmonkey/safemesh/blob/main/lean/SafeMesh/Delta.lean) and [the conditional product claim](https://github.com/velvetmonkey/safemesh/blob/main/CLAIMS.md).

## Named results, translated

The names below are real declarations, not labels for Rust tests. Names in the first five rows are in namespace `SafeMesh`; record names are in `SafeMesh.RecordKernel`.

| Source | Theorems | Plain-language reading |
| --- | --- | --- |
| [`Delta.lean`](https://github.com/velvetmonkey/safemesh/blob/main/lean/SafeMesh/Delta.lean) | `deltaApply_eq_deltaState`, `delta_dissemination_sec`, `delta_matches_state`, `merge_deltaState` | Folding delivery equals joining the delivered set; equal delivered sets give equal state; delta and full-state replicas agree when their joins agree; merging two delta states joins their delta-set union. |
| [`DeltaGCounter.lean`](https://github.com/velvetmonkey/safemesh/blob/main/lean/SafeMesh/DeltaGCounter.lean) | `deltaGCounter_correct` | Each replica coordinate contains the maximum of that coordinate's shipped tallies. |
| [`DeltaPNCounter.lean`](https://github.com/velvetmonkey/safemesh/blob/main/lean/SafeMesh/DeltaPNCounter.lean) | `deltaPNCounter_correct_P`, `deltaPNCounter_correct_N`, `deltaPNCounter_value_matches` | Increment and decrement components each accumulate per-coordinate maxima; delta and corresponding full-state counter reads agree. |
| [`DeltaORSet.lean`](https://github.com/velvetmonkey/safemesh/blob/main/lean/SafeMesh/DeltaORSet.lean) | `deltaORSet_lookup` | An element is present exactly when a shipped add token for it is outside the union of shipped removal tokens. A fresh concurrent add therefore survives a remove that did not observe it. |
| [`DeltaRGA.lean`](https://github.com/velvetmonkey/safemesh/blob/main/lean/SafeMesh/DeltaRGA.lean) | `deltaRGA_read_sec`, `deltaRGA_read_mem`, `deltaRGA_read_sorted` | Equal delivered delta sets give equal ordered position reads; a position is live when inserted and not tombstoned; reads are sorted by position. Identifier allocation is outside this model. |
| [`RecordKernel.lean`](https://github.com/velvetmonkey/safemesh/blob/main/lean/SafeMesh/RecordKernel.lean) | `admission`, `idempotence`, `collision`, `replayAgrees` | Acceptance controls log and live-state updates; replaying a record is idempotent; conflicting payloads for an existing ID are refused without mutation under the unique-ID premise; reachable cached state agrees with independent log replay. |
| [`RecordKernel.lean`](https://github.com/velvetmonkey/safemesh/blob/main/lean/SafeMesh/RecordKernel.lean) | `tokenDisjoint`, `checkedRestartSound` | With valid distinct authors in a fixed writer configuration, allocated tokens differ; a successful modeled restart yields replay-consistent state and the saved allocation mark. These are not proofs of OS locks or disk writes. |

**Record boundary:** equal *accepted* sets determine equal live states. Equal input sets containing conflicting payloads do not: which payload arrives first can determine what is accepted. Record equality compares payloads, not just hashes. The model's atomic transitions do not establish crash atomicity or storage durability. [Evidence: `RecordKernel.lean` scope and Rust obligations](https://github.com/velvetmonkey/safemesh/blob/main/lean/SafeMesh/RecordKernel.lean).

## Why these count as checked proofs

At source revision [`279926bbe05eb6cc592af54c1c4c28a9f21f84e9`](https://github.com/velvetmonkey/safemesh/tree/279926bbe05eb6cc592af54c1c4c28a9f21f84e9), all six modules above are imported by `lean/SafeMesh.lean`. The default Lake targets include both `SafeMesh` and the axiom check. `Test/Axioms.lean` guards the axiom output for the named results, and the CI script runs `lake build`. The [SafeMesh CI run for that revision](https://github.com/velvetmonkey/safemesh/actions/runs/34390575569) completed successfully. This is commit-specific evidence, not certification of every future `main` commit or a published package.

Evidence: [imports](https://github.com/velvetmonkey/safemesh/blob/279926bbe05eb6cc592af54c1c4c28a9f21f84e9/lean/SafeMesh.lean), [default targets](https://github.com/velvetmonkey/safemesh/blob/279926bbe05eb6cc592af54c1c4c28a9f21f84e9/lean/lakefile.toml), [axiom checks](https://github.com/velvetmonkey/safemesh/blob/279926bbe05eb6cc592af54c1c4c28a9f21f84e9/lean/Test/Axioms.lean), and [CI script](https://github.com/velvetmonkey/safemesh/blob/279926bbe05eb6cc592af54c1c4c28a9f21f84e9/scripts/ci.sh).

A source inventory at that revision finds **48 declarations beginning with `theorem`**: 21 in RecordKernel, seven in DeltaRGA, six each in DeltaORSet and DeltaPNCounter, five in Delta, and three in DeltaGCounter. This convention excludes private helpers and the attributed `@[simp] theorem deltaBump_apply`; 48 is not a count of all theorem declarations or 48 independent product guarantees. Evidence: the six linked Lean files above.

## How the proof reaches Rust

The proof does not compile into the Rust library. `lake exe corpus` evaluates the Lean definitions to emit `tests/corpus.json`; Rust's `tests/conformance.rs` replays that finite input/output corpus for the five carriers, including permutation, redelivery and split-merge cases. CI regenerates the corpus and compares it with the committed file before running the full gate. [Evidence: `Corpus.lean`](https://github.com/velvetmonkey/safemesh/blob/main/lean/Corpus.lean), [`conformance.rs`](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/tests/conformance.rs), and [CI workflow](https://github.com/velvetmonkey/safemesh/blob/main/.github/workflows/ci.yml).

**TESTED:** agreement over that corpus is conformance evidence, not a universal theorem about Rust, its compiler or hardware. **ASSUMED/trusted in the bridge:** Lean's compiled evaluation, emitted JSON, and its parsing in the Rust test harness. The corpus executable is deliberately outside the default proof targets. [Evidence: architecture's honesty boundary](https://github.com/velvetmonkey/safemesh/blob/main/ARCHITECTURE.md#honesty).

## What remains outside

Wire bytes, binding glue, FFI, WASM, Python, demos, storage and transport adapters are outside the Lean product proof claim. `LwwRegister`, `EnableWinsFlag`, `LwwMap` and user-defined reducers remain **TESTED, not PROVED**. Maintainer support is **UNKNOWN** in the root install matrix. Use the [limits page](/safemesh/limits/) before applying the claim to an application. Evidence: [claims](https://github.com/velvetmonkey/safemesh/blob/main/CLAIMS.md) and [install matrix](https://github.com/velvetmonkey/safemesh/blob/main/README.md#install-matrix).
