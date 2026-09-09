---
title: Getting started — main (unreleased)
description: Run the partition-and-heal example, recognize the result, then choose an integration.
---

This path uses **main (unreleased)** source, not a published package or a version-matched release manual. The examples demonstrate modeled convergence, not real transport delivery or storage durability. [Evidence: examples and their scope](/safemesh/examples/).

## 1. Prepare a local experiment

Start with the [examples prerequisites and clone commands](/safemesh/examples/#before-you-start): Linux, Git, internet access, Rust/Cargo through rustup, and a native C compiler/linker. Choose an empty working directory. The Rust example needs neither Lean nor a Python or JavaScript environment; its first build downloads dependencies and may take several minutes.

## 2. Get the first result

Run [Rust: partition and heal](/safemesh/examples/#rust-partition-and-heal). That section contains the command to copy from the repository root. It runs four replicas through lost, reordered and repeated messages, then repairs missing updates with anti-entropy.

Look for the final `CONVERGED=true` line: counter `3`, supplies `{42}`, and text positions `{10}`. Those are the three modeled states agreeing after the heal, not evidence that a physical network delivered anything. The executable asserts convergence and exits unsuccessfully if its checks fail. [Evidence: `break_it.rs`](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/examples/break_it.rs).

## 3. Understand the result before embedding it

Read [convergence and deltas](/safemesh/concepts/#convergence), then the [proof and its boundary](/safemesh/proof/). In particular, missing deltas must eventually be recovered; duplicate tolerance cannot recover an update that never arrives. [Evidence: `CLAIMS.md`](https://github.com/velvetmonkey/safemesh/blob/main/CLAIMS.md).

## 4. Choose your next step

| Your next question | Follow this path |
| --- | --- |
| What changes when delivery fails? | Open **Lab** in the global navigation; use the scenario and replay controls described in the [browser walkthrough](/safemesh/examples/#browser-interactive-convergence). |
| Can I use this from Python? | Run the [local wheel and data-mule example](/safemesh/examples/#python-cold-chain-data-mule), then read [Python integration](/safemesh/using-safemesh/#python). |
| Where does this fit in my application? | Read the [layer map](/safemesh/architecture/) and [transport responsibilities](/safemesh/using-safemesh/#you-bring-the-transport). |
| Can I persist and restore it? | Start with the [Rust persistence path](/safemesh/using-safemesh/#rust), including its Linux and filesystem requirements. |
| Is this the wrong abstraction? | Read [when not to use SafeMesh](/safemesh/limits/). |

The commands on this path build locally from public source; they do not publish packages or require a Lean build. Registry availability and maintainer support are not established by running them. [Evidence: examples](/safemesh/examples/) and [the install matrix](https://github.com/velvetmonkey/safemesh/blob/main/README.md#install-matrix).
