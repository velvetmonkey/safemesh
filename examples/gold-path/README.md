# First-use consumer fixtures

From the repository root, follow `docs/src/content/docs/getting-started.md`.
Rust is a standalone Cargo application consuming the local crate with `local-writer`.
TypeScript imports the complete `wasm-pack --target nodejs` output directory and
compiles against its generated declarations, without a hand-written type shim.

`python3 scripts/check-gold-paths.py` executes the nine path commands in
`commands.json`, checks exact program stdout against the `.txt` fixtures, and
checks the eight marked Markdown blocks against their sources. The existing
Documentation job runs it. It records stdout, stderr and each exit code in
`.checks/` (override with `GOLD_PATH_LOGS`). Each persist/restart pair uses separate
processes. Success removes exercise stores; a failure retains them for diagnosis.
Do not run two copies in the same checkout.

After an intentional example change, use `--refresh` to recompute expected stdout
from the asserting programs, review that diff, then use `--write-docs` to regenerate
the displayed source/commands/output. Run the normal checker again. CI never
refreshes expected output. Do not accept a new snapshot instead of investigating
an assertion failure.

These are ordinary restart fixtures. The Rust adapter performs its documented
local durable commit; the Node fixture saves application-owned log files without
fencing or a crash transaction. Neither proves a network, power-loss recovery,
cross-object atomicity, or platform support beyond the environment actually run.
