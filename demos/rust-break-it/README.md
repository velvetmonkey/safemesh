# SafeMesh for builders: Rust break-it

**v0 scope:** G-Counter and OR-Set are **supported**, within the [language-path limits](https://velvetmonkey.github.io/safemesh/#v0-support). G-Set, PN-Counter, RGA/Text, LWW Register (`LwwRegister`), Enable-wins Flag (`EnableWinsFlag`) and LWW Map (`LwwMap`) are **experimental**, including their deltas and wrappers. Existing proof/test evidence is unchanged by release status.


SafeMesh's Rust crate floor for consumers is **Rust 1.89**. For the source builds,
demos and locked wasm-pack 0.15.0 installation on this page, use **Rust 1.96.1**,
the full-gate CI version. Install rustup first (Linux/Bash, with curl and a native
C compiler/linker), then select that toolchain:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.96.1
. "$HOME/.cargo/env"
rustup default 1.96.1
```

The default applies to your user account; the repository's `rust-toolchain.toml`
also selects 1.96.1 inside this checkout. An outside application's toolchain remains
its own choice; consuming the crate requires at least 1.89.


This is the terminal proof sketch: four Rust replicas append local CRDT deltas, split into two sides of a mesh, accept reversed delivery and a duplicate, then heal through anti-entropy until every replica shows the same state.

It is intentionally small enough to audit and dramatic enough to run in front of someone.

![Captured Rust break-it terminal run](../assets/rust-break-it-terminal.png)

## 30-second run

First complete [Before you run](../README.md#before-you-run), including the checkout and tools for
this demo. The timing below excludes that setup and is an observation on the named machine, not a
guarantee on other machines or networks.

Runtime tested on Ubuntu 24.04.4 x86_64 (AMD EPYC-Genoa, Rust/Cargo 1.96.1): the commands below
completed in 2.50 seconds with an empty Cargo target directory and an already populated Cargo
registry, then 0.03 seconds reusing that build. Both runs reached `CONVERGED=true`.

From the repository root:

```sh
cd rust
NO_COLOR=1 cargo run -p safemesh-crdt --example break_it
```

Drop `NO_COLOR=1` for colored stage headers in a normal terminal.

## What You Should See

Continuous stdout excerpt, from duplicate replay through healing and convergence:

```text
[3/5] Replay a duplicate
  duplicate=A -> B event=gcounter.bump(0,1) effect=idempotent
  converged_during_partition=false
  partition_state
    A counter=3 supplies={} text_positions={10}
    B counter=3 supplies={} text_positions={10}
    C counter=0 supplies={42} text_positions={30}
    D counter=0 supplies={42} text_positions={30}

[4/5] Heal with anti-entropy
  anti_entropy=all_to_all_merge coverage=same_modeled_record_set_after_heal

[5/5] Shared state
  final_state
    A counter=3 supplies={42} text_positions={10}
    B counter=3 supplies={42} text_positions={10}
    C counter=3 supplies={42} text_positions={10}
    D counter=3 supplies={42} text_positions={10}
  CONVERGED=true counter=3 supplies={42} text_positions={10}
```

The actual run prints the partition and final state for replicas A, B, C, and D.

## What It Demonstrates

- G-Counter, OR-Set, and RGA/Text deltas can be applied in different orders.
- Duplicate delivery has one effect because merge/application is idempotent for the modeled deltas.
- A partition can leave replicas with different local views.
- A heal step that gives replicas the same modeled record set makes the final Rust state converge.

## Honest Claim Boundary

This demo exercises the Rust body of the Lean-backed CRDT carriers. The Lean proofs cover the CRDT convergence semantics; the Rust implementation is checked against the Lean-generated oracle corpus in CI.

This demo does not prove real radio delivery, durable storage, sensor truth, or arbitrary application reducers. It uses an in-process modeled fault campaign so the merge behavior is visible and re-runnable.

## Why This Matters

SafeMesh is for builder infrastructure where the shared state layer is load-bearing. The demo is not a benchmark and not a network simulator. It is a compact witness that the same Rust core can be broken at the delivery layer and still converge once the modeled records are exchanged.
