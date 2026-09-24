---
title: Evaluate guarantees — main (unreleased)
description: Separate model proofs, implementation tests, platform evidence and release limitations.
---

## Release status

**v0 scope:** G-Counter and OR-Set are **supported**, within the [language-path limits](/safemesh/#v0-support). G-Set, PN-Counter, RGA/Text, LWW Register (`LwwRegister`), Enable-wins Flag (`EnableWinsFlag`) and LWW Map (`LwwMap`) are **experimental**, including their deltas and wrappers. Existing proof/test evidence is unchanged by release status.

These guides use **main (unreleased)** source and a locally built WASM package.
There is no published SafeMesh package to install; a successful local package build
is not registry availability or a maintainer support pledge.

## Proof boundary

Convergence answers whether replicas agree. Your application still decides whether
the agreed result makes sense for its users and business rules.

Lean proves convergence over mathematical models: a join-semilattice with bottom
and equal delivered delta sets gives equal state, independent of order and duplicates.
The model does not deliver missing updates; eventual recovery is a delivery assumption.
OR-Set removal concerns observed tokens; RGA positions are caller supplied, with
identifier allocation outside that model. Conditional liveness also requires its
explicit fairness and quiescence assumptions.

The Rust implementation is **not itself proven in Lean**. Finite differential tests
compare the Lean-backed carriers and ownership/allocation behavior against the
Lean-generated corpus. The record kernel proves modeled admission, replay and
successful checked restart; Rust admission and durable restart are transcriptions
checked by separate Rust tests, not by that corpus. Equal accepted logs determine
state; equal inputs with conflicting payloads need not yield equal accepted logs.
Atomic model transitions do not prove crash atomicity, and modeled lock/generation
inputs do not establish OS exclusion or correct generation reads.

Frame decoding, lock reacquisition, `fsync`, rename, power-loss recovery, version
vectors, cursors, envelopes, authentication and peer collision reporting are outside
those theorems. Wire formats, C ABI, WASM/TypeScript and Python bindings, transport,
`LwwRegister`, `EnableWinsFlag` and `LwwMap` have engineering tests, not separate Lean
proofs. Property tests, fuzzers, law checks and demos do not turn those into proven claims.
A compiled target does not establish runtime or hardware behavior.

Read the [full named results, assumptions and evidence](/safemesh/proof/) and
[claim-by-claim evidence](/safemesh/claims/).

## Environments run

Install the prescribed Rust 1.96.1 toolchain in [Persist and restart](/safemesh/persist-and-restart/#before-you-start) for builds now. The measurements
below are execution evidence, not a support pledge.

The table records the historical measurement from 11 September 2026; its Rust
1.98.1 entries describe the toolchain used for that run, not the toolchain to
install now. The original measurement was reported at source revision
[`2b0813ffbaf69148e4f5822daedb1e221ee01ed1`](https://github.com/velvetmonkey/safemesh/commit/2b0813ffbaf69148e4f5822daedb1e221ee01ed1)
([first-use journeys PR #87](https://github.com/velvetmonkey/safemesh/pull/87));
no CI run identifier was recorded for that local measurement.

The allocated TypeScript journey was rerun on 17 September 2026 with Rust 1.96.1,
wasm-pack 0.15.0, TypeScript 6.0.3, Node 22.22.3 and npm 10.9.8 on Linux x86_64.
This later rerun is separate from the historical measurement in the table.

The checkout’s `scripts/check-gold-paths.py` compares values, output and the
source blocks on these pages in the
[documentation workflow](https://github.com/velvetmonkey/safemesh/blob/main/.github/workflows/docs.yml).

| Support field | Rust path | TypeScript path |
| --- | --- | --- |
| API present | Counter, UTF-8 OR-Set, durable local writer | Counter and UTF-8 OR-Set record/log binding |
| Artifact available | Source path dependency, no registry release | Locally built Node WASM directory, no registry release |
| Build checked (historical toolchain) | Linux x86_64, Rust 1.98.1 | Linux x86_64, Rust 1.98.1, wasm-pack 0.15.0, TypeScript 6.0.3 |
| Runtime tested | Ubuntu 24.04.4, native executable | Ubuntu 24.04.4, Node 22.22.3, npm 10.9.8 |
| Integration tested | Edit, disk commit, process restart, new edit, byte exchange, malformed record | Edit, file save, process reload, new edit, byte exchange, malformed record |
| Maintainer-supported | Unknown | Unknown |

Not run for these paths: Windows/PowerShell, macOS, ARM64, musl, embedded hardware,
browsers (including Safari), Deno, Bun, or other Rust/Node versions. Python and C
are separate routes, not equivalent persistence APIs. A WASM build is not browser
runtime evidence. These fixtures hand bytes between objects after restart; they
do not test a physical network, sudden power loss or storage hardware guarantees.


## Limitations and fit

SafeMesh is a small replicated-state library for modest structured state. It is
not an automatically networked database, radio stack, authenticated peer discovery
service, distributed lock or leader-election system, or a complete collaborative
text editor. Merge convergence does not enforce strict business invariants such as
nonnegative stock. History remains append-only; there is no public snapshot or
compaction API, and bounded storage needs an application retention protocol.

Read [When not to use it](/safemesh/limits/), the
[application responsibilities](/safemesh/connect-replicas/#what-the-application-owns),
and the [full storage and language contracts](/safemesh/using-safemesh/).
Return to [Try a merge](/safemesh/getting-started/) or
[Persist and restart](/safemesh/persist-and-restart/).
