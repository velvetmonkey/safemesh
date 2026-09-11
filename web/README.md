# SafeMesh web convergence hero

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

Before the WASM builds, install their target and build tool:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-pack --version 0.15.0 --locked
```


Interactive browser demo for the SafeMesh "watch it converge" story.

![Captured web hero showing four replicas and convergence controls](../demos/assets/web-hero-desktop.png)

## Run

Use Node.js 24.x (`package.json` specifies `>=24 <25`; tested with v24.20.0). On Node v22.22.3, `npm install` prints `EBADENGINE`, although installation and the production build succeeded in this run. Run the commands below from `web/`, stopping each server with Ctrl-C before running the next command.

Use a browser with WebAssembly and service workers enabled; this walkthrough was rerun on 2026-09-09 with Chromium 153.0.8010.12 (Google Chrome for Testing, Playwright revision 1243, headless) on Ubuntu 24.04.4 LTS x86_64 with Node.js v24.20.0. Open the local URL printed by the server at its root path (`/`).

```sh
npm install
npm run dev
npm run build
npm run preview
npm run test
```

The documentation workflow also builds this Lab into the published documentation site under its `lab/` path with `npm --prefix docs run build:site`, passing `--base` so every asset, the service worker and the manifest resolve under that path; see the [docs README](../docs/README.md).

The app is static and has no backend. On a first online visit to the Lab root (`/`) in the production build, the service worker precaches the app shell, including JavaScript, CSS, and WASM. Once installation completes, the Lab root can reopen offline.

There is no in-page installation-complete indicator. For a local offline test:

1. Open the production preview's root URL, then run `(await navigator.serviceWorker.ready).active.state` in the browser's developer console; wait for `'activated'`.
2. Stop `npm run preview` with Ctrl-C in its terminal, disconnecting the local server.
3. Reload the same root URL: the heading `Same message. Five failures. Same finish.` still appears. Reload again or open that URL in a new tab in the same browser profile; the Lab still opens.

## What It Demonstrates

- Simulated replicas with local CRDT state.
- Five use-case buttons: normal delivery, duplicate replay, out-of-order delivery, dropped-message recovery, and partition heal.
- Scenario steps that all start from the same message and end at the same visible state: `vaccine` plus audit count `1`.
- A collapsed Advanced lab with partition/heal, latency, drop, duplicate, and reorder controls.
- Periodic anti-entropy between connected peers so dropped one-shot messages can be recovered.
- G-Counter and OR-Set divergence under partition and convergence after reconnect.
- A live convergence indicator that compares raw states and reads.
- A technical/plain-language claim toggle and reduced-motion mode.

The scenario flow is intentionally manual: choose what can go wrong, then use the Previous / Replay / Next buttons directly under the replica picture. The Advanced lab keeps the free-form controls available without making them the first thing a new viewer has to understand.

## Honesty Boundary

Lean is the SafeMesh semantic oracle. The G-Counter and OR-Set use Rust-core records, merges, and reads through WASM. The TypeScript transport simulates timing, duplication, drops, and partitions. The Lean-backed Rust core is the proof-carrying surface for modeled CRDT semantics.

- G-Counter: runs the Rust core through WASM, backed by `SafeMesh.deltaBump`, `SafeMesh.deltaGCounter_correct`, `SafeMesh.delta_dissemination_sec`, and `SafeMesh.merge_deltaState`. The Rust body is differentially tested over corpus C.
- PN-Counter: included only as a small TypeScript reference mirror of `SafeMesh.deltaBumpP`, `SafeMesh.deltaBumpN`, and `SafeMesh.deltaPNCounter_correct_P/_N`; the current UI does not center it.
- OR-Set: the Lab calls `SafeMeshStringOrSetReplica` for adds, observed removals, record delivery, log recovery, and reads. These operations delegate to the shipped Rust core; the Lab transport and UI are not claimed to be formally verified.

Do not present this PWA as the verified artifact. It is the demo skin around the proof-backed product body.

## Anti-Entropy

The simulator compares Rust-core record versions between connected peers and backfills missing G-Counter and OR-Set records with `logBytes` and `mergeLogBytes`. Guided recovery queues visible log packets before delivery; periodic recovery merges logs immediately. TypeScript schedules these exchanges; the Rust core admits records and computes state.

The Lean anchor for this backfill is `SafeMesh.merge_deltaState`: merging replicas is equivalent to receiving the union of their delta sets. `SafeMesh.delta_dissemination_sec` is the separate order/redelivery-insensitivity result once deltas are delivered. Neither theorem makes this TypeScript implementation verified.

## PWA Cache Note

The service worker uses a deployment-scoped versioned cache and network-first navigation. Production builds derive the cache version from the emitted Lab shell and precache it during installation. Old caches from this deployment are deleted on activation; legacy product-wide caches without a deployment scope are retained because their owner cannot be identified safely. Runtime tested on 2026-09-09 with Chromium 153.0.8010.12 (Google Chrome for Testing, Playwright revision 1243, headless) on Ubuntu 24.04.4 LTS x86_64 with Node.js v24.20.0: the documented walkthrough reopened the Lab root offline after one online visit to a production build (`npm run build` followed by `npm run preview`) and completed service-worker activation. Build checked: the production build precaches the JavaScript, CSS, and WASM. Browser/version/OS-wide runtime coverage remains unknown. The Vite development server registers the same worker under a scope-derived cache such as `safemesh-pwa-%2Flab%2F-dev` that precaches only the public shell: in a fresh Chromium 148.0.7778.96 profile, an offline reload after one online visit showed the “SafeMesh Lab” fallback with loading/reload guidance and a documentation link; the Lab itself reopened offline only after a second online load, with the worker controlling the page and the runtime cache holding the dev modules. Opening the standalone `README.md` first does not install the worker.
