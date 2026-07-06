# SafeMesh claims

SafeMesh's verified claim is deliberately narrow:

> In-house CRDT types backed by the Lean suite provably converge under drop, duplication, reordering, and repeated merge. The Rust product body earns that claim only when it continues to pass the Lean-generated differential oracle corpus.

## Proven

- The Lean delta suite in `lean/SafeMesh/` proves Strong Eventual Consistency for delta dissemination over the supported CRDT carriers.
- G-Counter and PN-Counter have Rust implementations that are differentially tested against `tests/corpus.json`, emitted from the Lean definitions by `lake exe corpus`.
- OR-Set and RGA/Text have Lean proofs; they are not Rust-proven product surface until their Rust bodies and Lean-oracle corpus cases land.

## Tested, not proven

- Rust code is checked against a finite Lean-generated corpus. Passing the corpus is strong conformance evidence, not a universal theorem about the Rust compiler or hardware.
- Property tests and the laws harness test algebraic behavior. They supplement the Lean-oracle differential test; they do not replace it.
- Binding glue, FFI, WASM, Python, demos, and storage or transport adapters are engineered and tested.
- User-defined types are tested by their authors against the merge laws. SafeMesh does not prove arbitrary user reducers.

## Not covered in v0

- References between objects.
- Trees and ordered move semantics.
- Leader election or consensus.
- Guaranteed network delivery or radio-transport correctness.
- Hardware puck behavior.
- Register, Flag, or Map as proven SafeMesh product types, unless matching Lean proofs and corpus-backed Rust conformance are added.

## Wording rule

Use: "provably converges", "no lost / duplicated events", "machine-checked", and "re-runnable in your CI".

Do not use: "can't corrupt", "unbreakable", "guaranteed delivery", or "a faster Yjs".
