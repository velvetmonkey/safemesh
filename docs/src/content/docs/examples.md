---
title: Examples — main (unreleased)
description: Clone the source and run Rust, Python, and browser demonstrations.
---

These examples use **main (unreleased)** source. They exercise modeled convergence;
they do not establish real transport delivery, storage durability, or maintainer support.

## Put SafeMesh in your program

Start with the [Rust and TypeScript gold paths](/safemesh/getting-started/) to add
the local library/package, make an edit, save it, restart in another process and
sync a second replica. Those complete, CI-executed consumer fixtures include
asserted output and ordinary malformed-record errors. The examples below explore
modeled delivery failures after that first integration.

## Before you start

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

Use Linux with Git, internet access, Rust and Cargo installed through rustup, and a
native C compiler/linker. For Python, install Python 3 with `venv` and pip. For the
browser example, install Node.js 24.x with npm and a WebAssembly-capable browser.
The first run downloads dependencies and compiles Rust; allow several minutes.
No Lean build is needed.

Start in an empty working directory. Clone the public repository without entering
credentials, then stay in its root for the commands below. Stop if any command fails.
Each example uses a subshell so you remain at the repository root afterwards.

```sh
git clone https://github.com/velvetmonkey/safemesh.git
cd safemesh
```

These instructions build from source rather than installing a published SafeMesh
package. Registry availability is not implied.

## Rust: partition and heal

```sh
(cd rust && NO_COLOR=1 cargo run -p safemesh-crdt --example break_it)
```

The final stdout line is:

```text
  CONVERGED=true counter=3 supplies={42} text_positions={10}
```

The preceding stages show reversed delivery, duplicate replay, and anti-entropy heal.

## Python: cold-chain data mule

Create a virtual environment, install the build tool, build a local wheel, and run
the demo with that environment's Python:

```sh
python3 -m venv .venv-examples
.venv-examples/bin/python -m pip install 'maturin>=1.7,<2'
(cd rust/crates/safemesh-python && ../../../.venv-examples/bin/maturin build --release --features extension-module --out ../../../.example-wheels)
.venv-examples/bin/python -m pip install --no-index --find-links .example-wheels safemesh-python
.venv-examples/bin/python rust/crates/safemesh-python/examples/data_mule_demo.py
```

The final stdout line is:

```text
  CONVERGED=true python_data_mule_sample=9001 holders=[300, 300, 300] audit_counts=[4, 4, 4] temperature_alerts=[True, True, True]
```

The preceding stages show modeled custody, audit counts, and temperature alerts.

## Browser: interactive convergence

Install the WASM build tool and target, then start the server. Leave this command
running while you use the browser; press Ctrl+C when finished. Port 4387 must be free.

```sh
cargo install wasm-pack --version 0.15.0 --locked
rustup target add wasm32-unknown-unknown
(cd web && unset NODE_ENV && npm ci && npm run dev -- --host 127.0.0.1 --port 4387 --strictPort)
```

Open `http://127.0.0.1:4387/`. The page shows the heading
“Same message. Five failures. Same finish.” and the prompt “CHOOSE WHAT CAN GO WRONG”.
Use the scenario buttons and Previous / Replay / Next controls to explore the demo.
The UI and modeled transport are outside the Lean proof claim.

For longer walkthroughs, see the [demo directory guide](https://github.com/velvetmonkey/safemesh/blob/main/demos/README.md).
