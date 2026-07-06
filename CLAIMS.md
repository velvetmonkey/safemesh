# SafeMesh claims

SafeMesh's verified claim is deliberately narrow:

> In-house CRDT types backed by the Lean suite provably converge under drop, duplication, reordering, and repeated merge. The Rust product body earns that claim only when it continues to pass the Lean-generated differential oracle corpus.

## Proven

- The Lean delta suite in `lean/SafeMesh/` proves Strong Eventual Consistency for delta dissemination over the supported CRDT carriers.
- G-Set, G-Counter, PN-Counter, OR-Set, and RGA/Text have Rust implementations that are differentially tested against `tests/corpus.json`, emitted from the Lean definitions by `lake exe corpus`.
- The Rust `EventLog` is a deduplicating append/merge/since/version infrastructure layer. It supports the product surface, but the proven convergence claim lives in the CRDT carriers and their Lean-backed corpus bridge.
- `LwwRegister` is tested-not-proven. It is a flat max-register over `(timestamp, replica, value)` and is covered by Rust laws/wire tests, but it is not in the current Lean oracle corpus.

## Tested, not proven

- Rust code is checked against a finite Lean-generated corpus. Passing the corpus is strong conformance evidence, not a universal theorem about the Rust compiler or hardware.
- Property tests and the `laws` feature harness test algebraic behavior over generated scenarios. They supplement the Lean-oracle differential test; they do not replace it.
- Canonical wire encoding, binding glue, FFI, WASM, Python, demos, and storage or transport adapters are engineered and tested. Transport adapters can be tested against the coverage contract, but SafeMesh does not prove network delivery. Bindings and adapters must call the Rust core rather than reimplementing merge semantics.
- User-defined types are tested by their authors against the merge laws. SafeMesh does not prove arbitrary user reducers.

## Not covered in v0

- References between objects.
- Trees and ordered move semantics.
- Leader election or consensus.
- Guaranteed network delivery or radio-transport correctness.
- Hardware puck behavior.
- Flag or Map as proven SafeMesh product types, unless matching Lean proofs and corpus-backed Rust conformance are added. `LwwRegister` exists, but remains tested-not-proven until that proof bridge lands.

## Wording rule

Use: "provably converges", "no lost / duplicated events", "machine-checked", and "re-runnable in your CI".

Do not use: "can't corrupt", "unbreakable", "guaranteed delivery", or "a faster Yjs".
