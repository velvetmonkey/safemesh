---
title: SafeMesh — main (unreleased)
description: Lean-backed CRDT convergence for small, embeddable state sync; try a runnable Rust example.
---

SafeMesh provides **Lean-backed CRDT convergence for small, embeddable state sync**.
It is for developers building state-sync systems who bring the transport and application schema.
This page follows **main (unreleased)**, rather than a release manual.

Try the [Rust break-it walkthrough](/safemesh/examples/): it partitions replicas, drops, duplicates,
and reorders messages, heals with anti-entropy, and finishes with `CONVERGED=true`.

## Orientation and source map

This orientation is a reading map for the development branch.
The linked repository documents follow the development branch and remain the source of the technical instructions.

## Choose a starting point

- [Project entrypoint](https://github.com/velvetmonkey/safemesh/blob/main/README.md): start with the repository's current status and package instructions.
- [Architecture guide](/safemesh/architecture/): locate the design overview.
- [Claims and evidence guide](/safemesh/claims/): locate the proof boundaries and evaluation account.
- [Examples guide](/safemesh/examples/): locate runnable demonstrations in their existing directories.
- [Generated API reference](/safemesh/reference/): inspect the Rust core API from this build’s source.

## Use this site

Search covers these navigation guides; it does not index the contents of the linked GitHub documents.
The Lab entry opens the Lab at the address chosen when this documentation site is built; the hosted site includes the Lab under its own `lab/` path.
