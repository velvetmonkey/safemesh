---
title: Try a merge — main (unreleased)
description: Merge two independent counter updates and replay one without counting it twice.
---

Keep local copies, make independent changes, then merge them. This first program
combines `4` and `2` into `6`; repeating the merge leaves `6` unchanged.

## Before you start

- **Build from source:** SafeMesh is main (unreleased); there is no published package to install. Run `git clone --branch main https://github.com/velvetmonkey/safemesh.git`, then `cd safemesh`.
- Use Linux x86_64, Git, internet access, Rust/Cargo through rustup and a native C compiler/linker. Select Rust **1.96.1** with `rustup default 1.96.1`; the consumer crate floor is **1.89**.
- No Lean or Node is needed here. The first source build may download dependencies and take several minutes.

## Merge, then replay

This complete program is already in `examples/gold-path/rust/examples/try_merge.rs`.
Each replica changes its own coordinate: `4` and `2` are cumulative tallies,
not increment amounts. The two replicas use the same counter size (`2`).

<!-- gold:source rust:rust/examples/try_merge.rs -->
```rust
use safemesh_crdt::GCounter;

fn main() {
    let mut left = GCounter::new(2);
    let mut right = GCounter::new(2);
    left.try_apply_bump(0, 4).unwrap();
    right.try_apply_bump(1, 2).unwrap();
    left.try_merge(&right).unwrap();
    right.try_merge(&left).unwrap();
    assert_eq!(left.value(), 6);
    assert_eq!(left.state(), right.state());
    println!("merged total={}", left.value());
    left.try_merge(&right).unwrap();
    assert_eq!(left.value(), 6);
    println!("duplicate replay total={}", left.value());
}
```
<!-- /gold -->

<!-- gold:commands try_merge -->
```sh
cargo run --quiet --locked --manifest-path examples/gold-path/rust/Cargo.toml --example try_merge
```
<!-- /gold -->

Expected output:

<!-- gold:output try_merge -->
```text
merged total=6
duplicate replay total=6
```
<!-- /gold -->

A merge takes the maximum tally at each coordinate, then the read adds those
tallies. Replaying the same information cannot increase either maximum.

Next: [Persist and restart](/safemesh/persist-and-restart/), then
[Connect replicas](/safemesh/connect-replicas/) and
[Evaluate guarantees](/safemesh/evaluate-guarantees/#proof-boundary).

## Previously bookmarked exercises

<a id="rust-gold-path"></a>
<a id="typescript-gold-path-node"></a>
The [Rust journey](/safemesh/persist-and-restart/#rust-gold-path) and
[TypeScript journey](/safemesh/persist-and-restart/#typescript-gold-path-node) now have their own page.

<a id="diagnose-a-failed-rust-exercise"></a>
[Diagnose a failed Rust exercise](/safemesh/persist-and-restart/#diagnose-a-failed-rust-exercise).

<a id="predict-then-run-a-concurrent-add-and-remove"></a>
[Concurrent add and remove](/safemesh/persist-and-restart/#predict-then-run-a-concurrent-add-and-remove).

<a id="decrement-with-supported-types"></a>
[Decrement with supported types](/safemesh/persist-and-restart/#decrement-with-supported-types).

<a id="environments-run"></a>
[Environments run](/safemesh/evaluate-guarantees/#environments-run).

<a id="next-steps"></a>
[Complete sources and integration paths](/safemesh/using-safemesh/).
