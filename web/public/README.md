# SafeMesh PWA MVP

This browser demo runs the G-Counter through the Rust core via WASM and the OR-Set through a TypeScript demo mirror. The Lean-backed Rust core is the proof-carrying surface for modeled CRDT semantics; the browser UI is not itself the verified SafeMesh artifact.

Lean is the semantic oracle. The G-Counter uses the Rust implementation of Lean semantics, differentially tested over corpus C, through WASM. The Lab’s TypeScript OR-Set mirrors Lean-proven add-wins semantics; Rust OR-Set ships in `safemesh-crdt` and is differentially tested over corpus C, but the Lab does not use it.

Anti-entropy in the demo is a simulated transport/re-gossip mechanism. It backfills missing state by merging peer digests, corresponding to `SafeMesh.merge_deltaState`. `SafeMesh.delta_dissemination_sec` covers order and duplicate delivery once deltas are delivered.
