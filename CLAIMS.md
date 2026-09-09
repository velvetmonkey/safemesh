# SafeMesh claims

SafeMesh's verified claim is deliberately narrow:

> In-house CRDT types backed by the Lean suite are provably insensitive to duplicate and reordered delivery, and convergent after missing deltas are eventually recovered by merge/anti-entropy. The Rust product body earns that claim only when it continues to pass the Lean-generated differential oracle corpus.

## Proven

- The Lean delta suite in `lean/SafeMesh/` proves Strong Eventual Consistency for delta dissemination over the supported CRDT carriers.
- G-Set, G-Counter, PN-Counter, OR-Set, and RGA/Text have Rust implementations that are differentially tested against `tests/corpus.json`, emitted from the Lean definitions by `lake exe corpus`.
- The Rust `EventLog` is a deduplicating append/merge/since/version infrastructure layer. It supports the product surface, but the proven convergence claim lives in the CRDT carriers and their Lean-backed corpus bridge.
- `LwwRegister` is tested-not-proven. It is a flat max-register over `(timestamp, replica, value)` and is covered by Rust laws/wire tests, but it is not in the current Lean oracle corpus.
- `EnableWinsFlag` is tested-not-proven. It is a flat observed-token boolean flag and is covered by Rust laws/wire tests, but it is not in the current Lean oracle corpus.
- `LwwMap` is tested-not-proven. It is a flat per-key LWW map with remove tombstones and is covered by Rust laws/wire tests, but it is not in the current Lean oracle corpus.

## Tested, not proven

- Rust code is checked against a finite Lean-generated corpus. Passing the corpus is strong conformance evidence, not a universal theorem about the Rust compiler or hardware.
- Property tests and the `laws` feature harness test algebraic behavior over generated scenarios. They supplement the Lean-oracle differential test; they do not replace it.
- Canonical wire encoding, binding glue, FFI, WASM, Python, demos, and storage or transport adapters are outside the Lean proof claim. For binding support facts, use the separate API present, artifact available, build checked, runtime tested, integration tested, and maintainer-supported statements in the root README; name the test environment rather than combining them into a single "tested" label. Transport adapters can be tested against the coverage contract, but SafeMesh does not prove network delivery. Bindings and adapters must call the Rust core rather than reimplementing merge semantics.
- Integrity vertical examples, including the cold-chain kill-test, are engineered evaluation artifacts. They test modeled software convergence; they do not prove sensor truth, legal custody, storage durability, or real transport delivery.
- User-defined types are tested by their authors against the merge laws. SafeMesh does not prove arbitrary user reducers.

## Not covered in v0

- References between objects.
- Trees and ordered move semantics.
- Leader election or consensus.
- Guaranteed network delivery or radio-transport correctness.
- Hardware puck behavior.
- Flag or Map as proven SafeMesh product types, unless matching Lean proofs and corpus-backed Rust conformance are added. `LwwRegister`, `EnableWinsFlag`, and `LwwMap` exist, but remain tested-not-proven until that proof bridge lands.

## Wording rule

Use: "provably converges (once deltas are delivered)", "duplicate delivery does not duplicate modeled state", "machine-checked", and "re-runnable in your CI".

Do not use: "can't corrupt", "unbreakable", "guaranteed delivery", or "a faster Yjs".
