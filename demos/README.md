# SafeMesh for builders: demos

**v0 scope:** G-Counter and OR-Set are **supported**, within the [language-path limits](https://velvetmonkey.github.io/safemesh/#v0-support).


These are the v0.1 showcase demos. They are meant to be run, inspected, and used as evidence in CI, not treated as marketing mockups.

## Before you run

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

Use a shell with Git and internet access. Install the tools for your chosen demo first:

- Rust: Rust and Cargo through rustup, plus a native C compiler/linker.
- Python: the Rust tools, Python 3 with `venv` and pip, and maturin 1.x (`python3 -m pip install 'maturin>=1.7,<2'` in a virtualenv).
- Web: the Rust tools, Node.js 24.x with npm (Node 22.22.3 emits `EBADENGINE`; see the execution note below), wasm-pack 0.15.0 (`cargo install wasm-pack --version 0.15.0 --locked`), and the WASM target (`rustup target add wasm32-unknown-unknown`). Use a browser with WebAssembly enabled. Unset `NODE_ENV` before installing dependencies so development tools are included.

From an empty working directory, fetch the source and enter it:

```sh
git clone https://github.com/velvetmonkey/safemesh.git && cd safemesh
```

Continue only if cloning succeeds. If Git reports a repository or network error, resolve that error before running a demo; do not enter credentials to fetch this public repository. Start each walkthrough in this checkout's root (`safemesh/`); use a fresh terminal there when switching demos. The Python walkthrough below links to the complete wheel build and run commands.

First-run time includes downloads and compilation and depends on your machine and network; allow more than 30 seconds. A timed clone, fresh wasm-pack installation, dependency install, and web server startup took 43.27 seconds on Ubuntu 24.04 x86_64 with Rust/Cargo 1.96.1 and Node 22.22.3/npm 10.9.8 already installed. This excludes installing Rust, Node, and a browser. No Lean build is needed for these demo commands.

Runtime tested on that environment: Rust break-it, the local Python wheel demo with CPython 3.12.3/maturin 1.14.1, and the five web scenarios in Chromium 153.0.8010.12 with wasm-pack 0.15.0. Node 22.22.3 emitted the engine warning above. These observations do not establish runtime coverage on other environments or maintainer support.

Captured images live in `demos/assets/`. The [Python terminal image](python-cold-chain/README.md#historical-capture) is historical; its README’s text transcript shows the current demo output.

| Demo | Run after setup | Shows | Claim boundary |
|---|---|---|---|
| [Interactive web convergence hero](web-hero/README.md) | `cd web && npm install && npm run dev` | Scenario buttons for normal delivery, duplicate replay, out-of-order delivery, dropped-message recovery, and partition heal, all ending at the same modeled state. | The G-Counter and OR-Set use the Rust core through WASM; the TypeScript UI and transport are not the verified artifact. |
| [Rust break-it](rust-break-it/README.md) | `cd rust && NO_COLOR=1 cargo run -p safemesh-crdt --example break_it` | Lean-backed Rust carriers under reversed delivery, duplicate replay, partition, and anti-entropy heal. | In-process modeled transport, not real radio delivery or storage durability. |
| [Python cold-chain data mule](python-cold-chain/README.md) | Build a local wheel with maturin, then run `examples/data_mule_demo.py`. | A vaccine custody story across clinic, offline courier, freezer blip, and lab sync. | Sensor truth, real transport, storage durability, and binding glue are not proven. |

## Quick Gate

This is a contributor check, with additional prerequisites beyond the demos: `lake` and Lean **4.28.0** (`leanprover/lean4:v4.28.0` in `lean/lean-toolchain`, installed through [elan](https://github.com/leanprover/elan#installation)), cbindgen, maturin, and the Rust, Python, and web tools above. It installs additional Rust targets, builds Lean, and runs packaging checks; allow extra downloads and build time. Read `scripts/ci.sh` and `scripts/package-smoke.sh` before running it.

Run the same local gate the GitHub Actions workflow runs:

```sh
cargo install cbindgen --version 0.28.0 --locked
./scripts/ci.sh
```

That gate keeps the Lean oracle corpus differentials, Rust examples, web build, Python wheel smoke, WASM package dry-run, and dry-run publishing checks visible together. It does not publish to any registry.

## Honest Language

Use "provably converges" for the Lean-backed CRDT semantics and "differentially tested" for the Rust bridge to the Lean oracle corpus. For bindings, demos, packaging, and transport adapters, state each applicable fact separately: API present, artifact available, build checked, runtime tested, integration tested, and maintainer-supported. Name the test and execution environment for runtime tested and integration tested claims. Maintainer-supported status for Rust, C ABI, WASM/TypeScript, and Python is unknown.

Avoid extending the proof claim to sensors, real networks, storage durability, arbitrary user reducers, or demo UI code.
