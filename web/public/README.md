# SafeMesh PWA MVP

**v0 scope:** G-Counter and OR-Set are **supported**, within the [language-path limits](https://velvetmonkey.github.io/safemesh/#v0-support). G-Set, PN-Counter, RGA/Text, LWW Register (`LwwRegister`), Enable-wins Flag (`EnableWinsFlag`) and LWW Map (`LwwMap`) are **experimental**, including their deltas and wrappers. Existing proof/test evidence is unchanged by release status.


Lean is the SafeMesh semantic oracle. The G-Counter and OR-Set use Rust-core records, merges, and reads through WASM. The TypeScript transport simulates timing, duplication, drops, and partitions. The Lean-backed Rust core is the proof-carrying surface for modeled CRDT semantics.

- G-Counter: runs the Rust core through WASM, backed by `SafeMesh.deltaBump`, `SafeMesh.deltaGCounter_correct`, `SafeMesh.delta_dissemination_sec`, and `SafeMesh.merge_deltaState`. The Rust body is differentially tested over corpus C.
- PN-Counter: included only as a small TypeScript reference mirror of `SafeMesh.deltaBumpP`, `SafeMesh.deltaBumpN`, and `SafeMesh.deltaPNCounter_correct_P/_N`; the current UI does not center it.
- OR-Set: the Lab calls `SafeMeshStringOrSetReplica` for adds, observed removals, record delivery, log recovery, and reads. These operations delegate to the shipped Rust core; the Lab transport and UI are not claimed to be formally verified.

Do not present this PWA as the verified artifact. It is the demo skin around the proof-backed product body.

## Anti-Entropy

The simulator compares Rust-core record versions between connected peers and backfills missing G-Counter and OR-Set records with `logBytes` and `mergeLogBytes`. Guided recovery queues visible log packets before delivery; periodic recovery merges logs immediately. TypeScript schedules these exchanges; the Rust core admits records and computes state.

The Lean anchor for this backfill is `SafeMesh.merge_deltaState`: merging replicas is equivalent to receiving the union of their delta sets. `SafeMesh.delta_dissemination_sec` is the separate order/redelivery-insensitivity result once deltas are delivered. Neither theorem makes this TypeScript implementation verified.
