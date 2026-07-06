# SafeMesh CRDT

`safemesh-crdt` is the Rust core of SafeMesh: a `no_std + alloc` delta-state CRDT library with an engineered event log for append, merge, `since`, and version-vector sync.

## Claim boundary

The current Lean-backed surface is G-Set, G-Counter, PN-Counter, OR-Set, and RGA/Text. The Rust implementation earns the verified claim only while `tests/conformance.rs` passes against the Lean-generated oracle corpus from `lake exe corpus`.

`EventLog`, canonical wire encoding, LWW Register, Enable-wins Flag, and LWW Map are engineered and tested Rust product surfaces. They are not Lean-proven in v0.1.

See the repository `CLAIMS.md` and `WHAT-IS-PROVEN.md` for the full wording rule.

## Install

```toml
[dependencies]
safemesh-crdt = "0.1.0"
```

## Quickstart

```rust
use safemesh_crdt::{Crdt, GCounter, GCounterDelta, Mergeable};

let mut left = GCounter::new(2);
let mut right = GCounter::new(2);

let delta = GCounterDelta {
    replica: 0,
    tally: 3,
};
left.apply_delta(delta.clone());
right.apply_delta(delta);
left.merge(&right);

assert_eq!(left.value(), right.value());
```

## Verify locally

```sh
cd rust
cargo publish --dry-run -p safemesh-crdt --allow-dirty
cargo test -p safemesh-crdt --features laws
```
