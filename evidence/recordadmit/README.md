# Rust record admission

Base: `origin/main` = `437cb52dc233ff144e987b10543a82eb2864021a`.
Worktree branch: `agent/recordadmit`.
Specification read: `agent/record-kernel-r2:lean/SafeMesh/RecordKernel.lean`, especially `outcome`, `step`, and `RUST OBLIGATION`. This is Rust regression evidence, not a Lean proof of Rust or a claim that the specification's other divergences have been discharged. No Lean build was run.

## Complete Rust path inventory

Each row names every method that applied record payloads in that replica. There are 30 application sites: 14 per binding and two in the cold-chain example.

| File | Type | Local methods | Incoming methods |
| --- | --- | --- | --- |
| `rust/crates/safemesh-wasm/src/lib.rs` | `SafeMeshGCounterReplica` | `append_bump` | `merge_record_bytes`, `merge_log_bytes` |
| same | `SafeMeshEnableWinsFlagReplica` | `append_enable`, `append_disable_observed` | `merge_record_bytes`, `merge_log_bytes` |
| same | `SafeMeshLwwMapReplica` | `append_set`, `append_remove` | `merge_record_bytes`, `merge_log_bytes` |
| same | `SafeMeshLwwRegisterReplica` | `append_set` | `merge_record_bytes`, `merge_log_bytes` |
| `rust/crates/safemesh-python/src/lib.rs` | `PyGCounterReplica` | `append_bump` | `merge_record_bytes`, `merge_log_bytes` |
| same | `PyEnableWinsFlagReplica` | `append_enable`, `append_disable_observed` | `merge_record_bytes`, `merge_log_bytes` |
| same | `PyLwwMapReplica` | `append_set`, `append_remove` | `merge_record_bytes`, `merge_log_bytes` |
| same | `PyLwwRegisterReplica` | `append_set` | `merge_record_bytes`, `merge_log_bytes` |
| `rust/crates/safemesh-crdt/examples/cold_chain_kill_test.rs` | `Replica` | `emit` (also reached from `note_domain_event`) | `receive`, then `ColdChainState::apply` |

Additional admission paths in `safemesh-crdt/src/lib.rs`: `EventLog::append`, `merge_records`, `insert_record`, and `EventLog<D>::decode_wire`. The decoder formerly deduplicated by ID before a wrapper could inspect a conflicting payload. It now errors with `WireError::RecordCollision`. Identical decoded records still deduplicate. `Record<D>::decode_wire` itself only constructs a record. Transport send/drain/anti-entropy only move records; the receiver performs admission. The cold-chain receive path no longer filters by version-vector inclusion, which could hide collisions.

Raw CRDT application surfaces also examined (these have no record IDs or companion log):

- `Crdt::apply_delta` implementations for `GCounter`, `PnCounter`, `GSet`, `OrSet`, `Rga`, `EnableWinsFlag`, `LwwRegister`, and `LwwMap`. They delegate to `apply_bump`, `apply_inc`/`apply_dec`, `insert`, `add`/`apply_remove`, `insert`/`delete`, `enable`/`disable`, `set`, and `set`/`remove`, respectively. Full-state `merge` methods join these carriers. State wire decoders construct fresh carriers, not live record replicas.
- WASM/Python bare carrier wrappers: GCounter `apply_bump`, LwwRegister `set`, LwwMap `set`/`remove`, EnableWinsFlag `enable`/`disable_observed`.
- FFI `safemesh_gcounter_apply_bump` operates on a bare counter.
- `examples/break_it.rs`: `Replica::apply`, invoked for local script events, packet delivery, and redelivery; `Replica::merge` joins full states. These events carry no `RecordId` and use no `EventLog`.
- Core API, wire, conformance, transport tests and the `laws` harness exercise raw carriers or logs without a live cache. Replay in the new regression independently folds stored records through the raw carrier.

## Admission contract

`EventLog::admit_with` compares the full decoded payload at an indexed ID and returns `Accepted`, `Duplicate`, or `Collision`. It invokes application exactly once for Accepted and never for the other outcomes. The index points into the accepted log; lookup remains logarithmic. Every record-bearing live application above uses this gate, including local append through `append_with`.

Local IDs are allocated above the highest stored sequence for that replica; allocation is checked for exhaustion before either mutation. Existing `append` retains its ID return type and explicitly panics on exhaustion; bindings use fallible `append_with`. Counter replica bindings reject out-of-range coordinates before admission. Binding conflicts return errors containing `record ID collision`, including conflicts detected inside the decoder.

Generic callers must validate domain-specific payloads first and provide an infallible application callback with the same interpretation as replay. Accepted redundant deltas need not change the numeric value. This is a synchronous in-memory transition; panic recovery, allocation failure and durable crash atomicity are outside this change. Batch merge is per-record: a prefix accepted before a conflict against the destination remains accepted and applied. An internally conflicting wire log fails decoding before any destination mutation.

API changes: admission methods require payload `PartialEq`; `merge_records` returns outcomes in input order; decoding EventLog requires `WireDecode + PartialEq`; `WireError` has a new collision variant. Wire bytes are unchanged. Bare CRDT APIs remain usable independently of logs.

## Controls and results

Command (from repository root):

```sh
cargo test --manifest-path rust/Cargo.toml --workspace --features safemesh-crdt/laws -- --nocapture --test-threads=1 > evidence/recordadmit/cargo-test.log 2>&1
printf '%s\n' "$?" > evidence/recordadmit/cargo-test.exit
cat evidence/recordadmit/cargo-test.exit
```

File contents: `0`. Final restored-source run: **63 tests passed, zero failed**.

Verbatim named-case output:

```text
id (1,1): tally 5 -> Accepted; tally 9 -> Collision; live 5; replay 5
```

`python3 evidence/recordadmit/tamper.py` inserts an application of the rejected payload in the duplicate/collision branch, runs the SAME named test, writes and reads `tamper.exit`, and restores the source in `finally`. Exit file: `101`. Verbatim excerpt from `tamper.log`:

```text
id (1,1): tally 5 -> Accepted; tally 9 -> Collision; live 9; replay 5

assertion `left == right` failed
  left: 9
 right: 5
```

Existing oracle conformance: 7 tests passed, covering **9 G-Counter, 9 PN-Counter, 6 OR-Set, 6 G-Set, 6 RGA cases**, plus 9 G-Counter split/merge replays. Existing laws: 2 tests passed; convergence reports **8 scenarios each** for G-Counter, PN-Counter, OR-Set, RGA, LWW register, enable-wins flag and LWW map (56 total). Original merge-law samples also pass.

Four new core regression tests cover the named case, callbacks on duplicate/collision/redundant acceptance, decoder conflicts in both payload orders versus identical duplicates, and local gaps/exhaustion/replay. Two new Python binding tests exercise all four replica types for record collisions, destination-log collisions, internal wire-log collisions, duplicates, and counter-coordinate validation. The workspace also runs the existing 12 WASM tests on the host; no browser/WASM runtime test is claimed.

Cold-chain example: `cargo run --manifest-path rust/Cargo.toml -p safemesh-crdt --example cold_chain_kill_test`; file-recorded exit `0`, `KILL_TEST_PASS=true questions=10`. See `cold-chain.log`.
