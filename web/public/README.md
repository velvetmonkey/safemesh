# SafeMesh PWA MVP

This browser demo is a TypeScript mirror, not the verified SafeMesh artifact.

Lean is the semantic oracle. G-Counter mirrors Lean semantics that also ship in Rust and are differentially tested over corpus C. OR-Set mirrors Lean-proven add-wins semantics directly; Rust OR-Set is not shipped yet.

Anti-entropy in the demo is a simulated transport/re-gossip mechanism. It backfills missing state by merging peer digests, corresponding to `SafeMesh.merge_deltaState`. `SafeMesh.delta_dissemination_sec` covers order and duplicate delivery once deltas are delivered.
