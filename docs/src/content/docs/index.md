---
title: SafeMesh — main (unreleased)
description: Lean-backed CRDT convergence for small, embeddable state sync; try a runnable Rust example.
---

SafeMesh provides **Lean-backed CRDT convergence for small, embeddable state sync**.
Lean proves mathematical models; Rust is checked against a finite generated corpus.
Bindings, storage and transport are outside that proof. See the [claims ceiling](/safemesh/claims/).
It is for developers building state-sync systems who bring the transport and application schema.
This page follows **main (unreleased)**, rather than a release manual.

## Primary path

1. **Fault exploration — start here:** build and run the [Rust break-it walkthrough](/safemesh/examples/)
   from **main (unreleased) source** with Rust/Cargo and a native C compiler/linker
   (no Lean build needed) to partition replicas, drop, duplicate and reorder messages,
   then heal with anti-entropy and finish with `CONVERGED=true`.
2. **Integration:** follow [Get started](/safemesh/getting-started/) to save an edit,
   restart and sync a second replica in a complete Rust or TypeScript program.
3. **Assurance review:** [Understand the proof](/safemesh/proof/) before deciding
   whether its guarantees and assumptions fit your application.


## v0 support

**G-Counter and OR-Set** are the two types selected for v0; **Supported** in the
table means this type selection, within the language-path limits below. All other
carriers remain **experimental**. This selection records Ben's OR-1 option A
ruling (11 September 2026).

Main is **unreleased**. Release artifacts and measured platforms are separate
from this selection; the [install matrix](https://github.com/velvetmonkey/safemesh/blob/main/README.md#install-matrix)
records their availability and evidence. Maintainer support is **UNKNOWN** for
all four language surfaces: this selection establishes no maintenance commitment.
Proof breadth and release support are separate: the five existing carrier proofs remain valid.

| Type | v0 status | Evidence |
| --- | --- | --- |
| G-Counter (`GCounter`) | **Supported** | [Lean model](https://github.com/velvetmonkey/safemesh/blob/main/lean/SafeMesh/DeltaGCounter.lean), [Rust conformance](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/tests/conformance.rs) |
| OR-Set (`OrSet`) | **Supported** | [Lean model](https://github.com/velvetmonkey/safemesh/blob/main/lean/SafeMesh/DeltaORSet.lean), [Rust conformance](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/tests/conformance.rs) |
| G-Set (`GSet`) | **Experimental** | [Proof and finite-test boundary](/safemesh/claims/#tested-not-proven) |
| PN-Counter (`PnCounter`) | **Experimental** | [Proof and finite-test boundary](/safemesh/claims/#tested-not-proven) |
| RGA/Text (`Rga`) | **Experimental** | [Proof and finite-test boundary](/safemesh/claims/#tested-not-proven) |
| LWW Register (`LwwRegister`) | **Experimental** | [Tested, not proven](/safemesh/claims/#tested-not-proven) |
| Enable-wins Flag (`EnableWinsFlag`) | **Experimental** | [Tested, not proven](/safemesh/claims/#tested-not-proven) |
| LWW Map (`LwwMap`) | **Experimental** | [Tested, not proven](/safemesh/claims/#tested-not-proven) |

The supported language paths have different limits:

- Rust: G-Counter and numeric/UTF-8 OR-Set mutation, record exchange and checked
  restore; only Linux Rust has the durable writer.
- TypeScript/WASM `SafeMeshOrSet`: numeric `add`, `applyRemove`, `contains`,
  `elements`, `merge`, `observedTokens`, and `tombstones`.
- TypeScript/WASM `SafeMeshStringOrSetReplica`: UTF-8 `createAllocated`,
  `appendAllocatedAdd`, `exportIdentity`, `importIdentity`, `appendAdd`,
  `appendRemoveObserved`, `mergeRecordBytes`, `mergeLogBytes`, `logBytes`,
  `versionFor`, `elements`, `observedTokens`, `tombstones`, `addEntries`, and
  `inspectRecordBytes`.
- Python: G-Counter mutation/exchange with caller-owned storage; numeric OR-Set
  mutation/remove/whole-state merge, without a UTF-8 replica or recovery promise.
- C: G-Counter carrier mutation/read and delta encoding, and numeric OR-Set
  mutation/remove/whole-state merge; no log or recovery promise.

Bindings are tested glue, not Lean proofs. The [install matrix](https://github.com/velvetmonkey/safemesh/blob/main/README.md#install-matrix)
records measured environments and artifact availability separately.
For decrementing stock, use [paired supported G-Counters](/safemesh/getting-started/#decrement-with-supported-types).
Application schemas, custom durable replicas and delivery/retry/storage remain caller-owned.

## Start with a result

1. **Integration:** [Get started](/safemesh/getting-started/): prepare a local experiment and recognize its first successful result.
2. **Assurance review:** [Learn the concepts](/safemesh/concepts/): replicas, deltas, convergence and the five Lean-backed carriers.
3. **Assurance review:** [Understand the proof](/safemesh/proof/): what Lean establishes and what remains tested or assumed.
4. **Integration:** [Use SafeMesh in your code](/safemesh/using-safemesh/): Rust, C ABI, WASM/TypeScript and Python, with caller responsibilities.
5. **Assurance review:** [Check the limits](/safemesh/limits/): decide whether the semantics and assurance fit your application.

## Orientation and source map

This orientation is a reading map for the development branch.
The linked repository documents follow the development branch and remain the source of the technical instructions.

## Choose a starting point

- **Integration:** [Project entrypoint](https://github.com/velvetmonkey/safemesh/blob/main/README.md): start with the repository's current status and package instructions.
- **Integration:** [Architecture guide](/safemesh/architecture/): locate the design overview.
- **Assurance review:** [Claims and evidence guide](/safemesh/claims/): locate the proof boundaries and evaluation account.
- **Fault exploration:** [Examples guide](/safemesh/examples/): locate runnable demonstrations in their existing directories.
- **Integration:** [Generated API reference](/safemesh/reference/): inspect the Rust core API from this build’s source.

## Use this site

Site search covers all guides and the generated Rust reference, including source pages; it does not index the contents of linked GitHub documents.
Rustdoc provides a separate item search for Rust items.
The Lab entry opens the Lab at the address chosen when this documentation site is built; the hosted site includes the Lab under its own `lab/` path.
