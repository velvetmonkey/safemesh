# Packaged Rust consumer

This separate Cargo project checks mutation, wire exchange, malformed input,
and durable restart through the public API of the unpacked `safemesh-crdt`
release crate. The package smoke script unpacks the crate beside a copy of this
project and checks that Cargo resolves the package from that location.

Run from the repository root: `scripts/package-smoke.sh`
