# SafeMesh for builders: demos

These are the v0.2 showcase demos. They are meant to be run, inspected, and used as evidence in CI, not treated as marketing mockups.

The committed images in `demos/assets/` were captured from the current app or from real command output on this branch.

| Demo | Run in 30 seconds | Shows | Claim boundary |
|---|---|---|---|
| [Interactive web convergence hero](web-hero/README.md) | `cd web && npm install && npm run dev` | A manual guided timeline plus a separate sandbox where replicas diverge under drops, duplicates, reordering, and partition, then heal into matching modeled state. | TypeScript is a demo mirror, not the verified artifact. |
| [Rust break-it](rust-break-it/README.md) | `cd rust && NO_COLOR=1 cargo run -p safemesh-crdt --example break_it` | Lean-backed Rust carriers under reversed delivery, duplicate replay, partition, and anti-entropy heal. | In-process modeled transport, not real radio delivery or storage durability. |
| [Python cold-chain data mule](python-cold-chain/README.md) | Build a local wheel with maturin, then run `examples/data_mule_demo.py`. | A vaccine custody story across clinic, offline courier, freezer blip, and lab sync. | Sensor truth, real transport, storage durability, and binding glue are not proven. |

## Quick Gate

Run the same local gate the GitHub Actions workflow runs:

```sh
./scripts/ci.sh
```

That gate keeps the Lean oracle corpus differentials, Rust examples, web build, Python wheel smoke, WASM package dry-run, and dry-run publishing checks visible together. It does not publish to any registry.

## Honest Language

Use precise verbs: "provably converges" for the Lean-backed CRDT semantics, "differentially tested" for the Rust bridge to the Lean oracle corpus, and "engineered and tested" for bindings, demos, packaging, and transport adapters.

Avoid extending the proof claim to sensors, real networks, storage durability, arbitrary user reducers, or demo UI code.
