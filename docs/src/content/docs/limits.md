---
title: When not to use SafeMesh — main (unreleased)
description: Decide whether the semantics and assurance boundary fit your application.
---

This guide follows **main (unreleased)**. Use it with the [claims ceiling](/safemesh/claims/), not as a release support policy.

## You need guaranteed delivery

SafeMesh's convergence claim is conditional on missing deltas eventually being recovered. It does not prove network or radio delivery. A partition-and-heal example exercises modeled delivery faults; it cannot establish that your real transport will repair every gap. [Evidence: `CLAIMS.md`](https://github.com/velvetmonkey/safemesh/blob/main/CLAIMS.md) and [example scope](/safemesh/examples/).

## You need consensus or leader election

These are explicitly outside v0's coverage. Equal state after joining the same updates does not provide a leader-election protocol. [Evidence: not covered in v0](https://github.com/velvetmonkey/safemesh/blob/main/CLAIMS.md#not-covered-in-v0).

## Your data model needs trees, moves or object references

References between objects, trees and ordered move semantics are outside the stated v0 scope. The shipped RGA model reads live positioned elements; it does not supply identifier allocation machinery. [Evidence: scope exclusions](https://github.com/velvetmonkey/safemesh/blob/main/CLAIMS.md#not-covered-in-v0) and [RGA scope](https://github.com/velvetmonkey/safemesh/blob/main/lean/SafeMesh/DeltaRGA.lean).

## You need a proof of all your application code

The universal results are about Lean definitions. Rust is checked against a finite corpus. Bindings, canonical wire encoding and adapters are outside the Lean claim, and arbitrary user reducers are not proved by SafeMesh. The laws harness supplies tests rather than a theorem prover. [Evidence: tested-not-proven boundary](https://github.com/velvetmonkey/safemesh/blob/main/CLAIMS.md#tested-not-proven).

## Your requirement is a proved flag, map or register

`EnableWinsFlag`, `LwwMap` and `LwwRegister` exist, but are **TESTED, not PROVED**. They have Rust laws/wire evidence and are not in the current Lean CRDT oracle corpus. Do not transfer the five carriers' proof label to these types. [Evidence: claims](https://github.com/velvetmonkey/safemesh/blob/main/CLAIMS.md).

## You need physical or legal truth

Cold-chain examples model software state. They do not prove sensor truth, legal custody, hardware puck behavior, or storage durability. A converged temperature alert does not prove that the sensor reading was true. [Evidence: evaluation limits](https://github.com/velvetmonkey/safemesh/blob/main/CLAIMS.md) and [Python example scope](/safemesh/examples/).

## You need unconditional crash or storage guarantees

Record-kernel proofs cover modeled atomic transitions and replay. They do not prove disk durability or crash atomicity. The Rust durable journey requires Linux and a local filesystem supporting locks and file/directory sync; its checks are not a general power-loss guarantee. [Evidence: record obligations](https://github.com/velvetmonkey/safemesh/blob/main/lean/SafeMesh/RecordKernel.lean) and [durable walkthrough](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/README.md).

## You need established distribution or maintainer support

The source-building examples do not establish registry availability. The root install matrix labels maintainer support **UNKNOWN** for all four surfaces. A generated WASM package running in Node does not establish a browser/OS matrix; a Linux Python wheel smoke test does not establish every interpreter/platform combination. [Evidence: install and status matrix](https://github.com/velvetmonkey/safemesh/blob/main/README.md#status).

If these limits fit your requirements, [try the local Rust example](/safemesh/getting-started/) and then [choose your integration](/safemesh/using-safemesh/).
