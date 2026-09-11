# SafeMesh kill-test: field-science cold chain

**v0 scope:** G-Counter and OR-Set are **supported**, within the [language-path limits](https://velvetmonkey.github.io/safemesh/#v0-support). G-Set, PN-Counter, RGA/Text, LWW Register (`LwwRegister`), Enable-wins Flag (`EnableWinsFlag`) and LWW Map (`LwwMap`) are **experimental**, including their deltas and wrappers. Existing proof/test evidence is unchanged by release status.


Reproduction commands use **Rust 1.96.1**, the full-gate CI and demo toolchain; the consumer crate floor is **Rust 1.89**. Install rustup and select the toolchain using the [build toolchain setup](README.md#build-toolchain) before running them.


This is the software-only integrity vertical for the v0 evaluation. It is not a claim about sensors, law, radio delivery, or custody procedure. It asks whether SafeMesh's current library surface is enough for a builder to model a load-bearing convergent state workflow and rerun the failure campaign in CI.

Run:

```sh
cd rust
cargo run -p safemesh-crdt --example cold_chain_kill_test
```

The scenario models a field sample moving from clinic to courier to lab while a lab-side freezer alert is recorded during a partition. The example uses:

- `EventLog` for append/merge/since/version record exchange.
- `InMemoryTransport` plus `anti_entropy` for drop, duplicate, reorder, partition, and heal.
- `GSet` for known samples.
- `OrSet` for active custody holders and active alerts.
- `Rga` for ordered audit entries.
- `GCounter` for per-replica domain-event counts.

The final projection must converge to one sample, lab custody, one active temperature alert, four audit entries, and four counted domain events. The program exits nonzero if replicas do not converge after heal.

## Ten questions

| Question | Cold-chain answer | Evidence |
|---|---|---|
| 1. Is the domain integrity-critical enough for verification to matter? | Yes. Silent loss, duplicate handling mistakes, or divergent custody state can invalidate sample handling decisions. | The scenario tracks sample identity, custody, alert, audit, and event count as state that must match after sync. |
| 2. Can the v0 flat-first model express the workflow without references, trees, or moves? | Yes for this slice. The sample id is a value inside flat sets and sequences; there are no object references or tree moves. | `cold_chain_kill_test` uses only `GSet`, `OrSet`, `Rga`, and `GCounter`. |
| 3. Does every replica converge after drop, duplication, reordering, partition, and heal? | Yes for the modeled state. | The example prints `CONVERGED=true` and has an example test under `cargo test -p safemesh-crdt --examples`. |
| 4. Is packet delivery being claimed as proven? | No. The transport is engineered test infrastructure. | The example uses `InMemoryTransport` and `anti_entropy`; `CLAIMS.md` keeps delivery and radio correctness out of scope. |
| 5. Does the vertical depend on custom reducer proof? | No for this slice. Domain events route into in-house flat CRDT carriers. | The state projection is composed from existing SafeMesh types; no arbitrary reducer earns a proven label. |
| 6. Are domain conflicts hidden? | No. Concurrent or unresolved facts remain visible in the converged state. | Alerts are OR-Set entries; custody is explicit add/remove-token state. Application policy decides what an alert means. |
| 7. Can a builder rerun the evidence locally? | Yes. | `cargo run -p safemesh-crdt --example cold_chain_kill_test` and `cargo test -p safemesh-crdt --examples`. |
| 8. Does the artifact exercise the event-log API, not just direct state merge? | Yes. | Replicas exchange `EventLog::since(remote_version)` record batches through the transport adapter. |
| 9. Is the proof boundary visible to a stranger? | Yes. | README, `CLAIMS.md`, and `WHAT-IS-PROVEN.md` label examples, transport, and app-domain semantics as engineered/tested evidence. |
| 10. Does this identify a plausible buyer wedge? | Yes, narrowly. Field-science/cold-chain users care about auditable convergence under intermittent connectivity, but API present: SafeMesh v0 exposes Rust durable storage and language bindings. A finished cold-chain application still needs domain integration and evaluation. | This file is an evaluation artifact, not a product claim. |

## Honest residuals

- Sensor truth is outside SafeMesh. Bad temperature readings can still produce converged bad data.
- Legal chain-of-custody is outside SafeMesh. The library preserves modeled state; it does not certify procedure.
- Real network delivery is outside SafeMesh. Adapters can be tested against the coverage contract, but not proven by this repo.
- This in-memory example does not exercise the shipped Rust durable store; compaction remains product work.
- `LwwRegister` is not used in this vertical; the scenario stays on the current Lean-backed carrier set.
