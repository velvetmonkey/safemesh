# SafeMesh

Formally verified coordination primitives for ad-hoc mesh networks (LoRa, Wi-Fi mesh, BLE, off-grid / disaster / community).

SafeMesh ships small, embeddable building blocks whose correctness is machine-checked in Lean 4, not just tested. Each primitive is a dual artifact: a Lean proof of its key property, and a thin `no_std`-friendly Rust crate that is differential-tested against that proof.

## Status

Early. First primitive: state-based CRDTs (Strong Eventual Consistency), building on the verified core in [crdt-lean](https://github.com/velvetmonkey/crdt-lean).

## Architecture

The verified core lives in the public, MIT-licensed [crdt-lean](https://github.com/velvetmonkey/crdt-lean): machine-checked state-based CvRDT convergence (SEC, conditional liveness, G-Set / G-Counter / PN-Counter / OR-Set / Sequence), zero `sorry`, standard axioms only. SafeMesh depends on that core and does not modify it.

SafeMesh adds:

- `lean/` — a delta-state CRDT (δ-CRDT) extension. Full-state gossip is too heavy for constrained links (LoRa duty cycles); delta-CRDTs ship only the change. This proves delta dissemination converges to the same state as full-state gossip.
- `rust/` — thin `no_std` Rust implementations of the CRDTs, each differential-tested against the Lean-proven semantics (the verification-guided-development loop: the proof is the oracle).

See `ARCHITECTURE.md`.

## License

AGPL-3.0-or-later. Copyright (c) 2026 Ben Cassie. See `LICENSE` and `NOTICE`.

Commercial licenses, to use SafeMesh without the AGPL network-copyleft obligations, are available. Contact the copyright holder.
