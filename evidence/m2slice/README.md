# M2 public API journey

## Current reproduction (2026-09-09)

Re-ran `rust/crates/safemesh-crdt/examples/m2slice.rs` on main `88b9691`.
Clean persisted logs restart correctly, and the single payload-byte mutations
are rejected as `IntegrityMismatch` for both GCounter and
`OrSetDelta<String, u64>`. Both normal and `--require-integrity` runs exit 0:
8 steps WORKS, 0 BLOCKED for each type, with both payload corruptions detected.

Run from the repository root with your own scratch directory:

```sh
cargo run --manifest-path rust/Cargo.toml -p safemesh-crdt --example m2slice -- /home/monkey/scratch/falsefour/m2-normal
cargo run --manifest-path rust/Cargo.toml -p safemesh-crdt --example m2slice -- /home/monkey/scratch/falsefour/m2-strict --require-integrity
```

Current corruption output (the example locates each payload in the current
frame, flips one bit, writes it to disk, and uses the same restart function):

```text
counter corrupt-tag-detected=true error=InvalidTag
counter corrupt-payload-offset=174 restart=Err(IntegrityMismatch)
counter corrupt-byte-detected=true
utf8-orset corrupt-tag-detected=true error=InvalidTag
utf8-orset corrupt-payload-offset=212 restart=Err(IntegrityMismatch)
utf8-orset corrupt-byte-detected=true
```

Each final log still holds 4 records: counter logs are now 228 bytes each and
OR-Set logs 232 bytes each. The final counter state `[11, 22]` and OR-Set adds
and tombstones match the historical walk below. See the
[core README's persistence frame](../../rust/crates/safemesh-crdt/README.md#eventlog-persistence-frame)
for the current format and its integrity boundary.

## Historical walk at a77f1b4 (superseded)

Everything below records the original walk of main
`a77f1b47e2b2fb229aa2afa3ea34deaa9300cca3` using
`rust/crates/safemesh-crdt/examples/m2slice.rs`, including its old byte offsets,
validation counts and missing-integrity finding. These are historical results,
not the behavior of the current revision. No product implementation changes
were made for that walk.

**First wall at a77f1b4: step 5's corruption control.** Clean persisted logs restarted correctly,
but a single valid payload-byte change was accepted and replayed to a different state.
The eight clean-log steps complete for both GCounter and `OrSetDelta<String, u64>`.
This does **not** satisfy the corruption-aware release control.

Run from the repository root, supplying your own scratch directory:

```sh
cargo run --manifest-path rust/Cargo.toml -p safemesh-crdt --example m2slice -- /home/monkey/scratch/m2slice/logs
cargo run --manifest-path rust/Cargo.toml -p safemesh-crdt --example m2slice -- /home/monkey/scratch/m2slice/logs --require-integrity
```

The first command completes the clean walk and reports corruption observations.
The second also requires corruption detection and deliberately reproduces this
failure at a77f1b4 (exit 101):

```text
step 5 integrity wall: persisted payload corruption silently loads a different state
```

| Step | Counter | UTF-8 OR-Set | Evidence |
| --- | --- | --- | --- |
| 1 Create | WORKS | WORKS | Public state constructors and `EventLog::new`. |
| 2 Local operations | WORKS | WORKS | `append_with` and `Crdt::apply_delta`; independent replica IDs. |
| 3 Exchange | WORKS | WORKS | `since`, `Record::to_wire_bytes`, `Record::from_wire_bytes`, `admit_with`; both directions captured before delivery. |
| 4 Persist | WORKS | WORKS | `EventLog::to_wire_bytes`, Rust file write and `sync_all` for each log. |
| 5 Restart | BLOCKED | BLOCKED | Clean-byte restart WORKS after dropping both replicas: decode log, replay public deltas into empty state. Required payload-corruption detection fails for both. |
| 6 Partition | WORKS | WORKS | No exchange during independent updates; `assert_ne!` on full states before reconciliation; persist each divergent log. |
| 7 Reconcile | WORKS | WORKS | Exchange missing records through wire bytes again, accepting and applying each. |
| 8 Agree | WORKS | WORKS | Full state equality, equal version vectors, and fresh disk replay matching each state and its complete log. |

Counts use the stricter control-inclusive interpretation: 8 steps attempted, 7
WORKS, 1 BLOCKED, 0 requiring library-private helpers. All 8 clean-log steps work.
The example's local functions only compose public APIs; they do not expose or
replace a library-private capability. Counter arity (2), type schema and replica
identities are application configuration; no pre-restart state or tally survives.

### Results

Each replica finishes with 4 records. Counter logs are 173 bytes each, full state
`[11, 22]`, value 33. OR-Set logs are 179 bytes each, adds
`{("café☕", 100), ("café☕", 201), ("東京", 200)}`, tombstones `{100}`.
The partition removes the observed café token on A while B concurrently re-adds
café with a fresh token, exercising add-wins and tombstone replay.

Corrupting byte 0 is rejected as `InvalidTag` for both logs. This is only syntax
validation. Independently, flipping one bit in counter log byte 123 changes tally
11 to 10 and silently loads `[10, 22]`. Flipping one bit in OR-Set log byte 163
changes the live add `café☕` to `bafé☕`, and restart also succeeds. Each mutation
starts from original persisted bytes, changes exactly one byte, writes to disk,
and passes that file through the same restart function. The harness detects a
state difference using an external original-state oracle; the restart API does
not detect the corruption. `corrupt-byte-detected no`.

### Missing versus awkward

Missing at a77f1b4: persisted-byte integrity validation. That revision's wire decoder checks
structure, UTF-8 validity and record collisions; a different valid payload is
indistinguishable from an intentional value. No checksum or integrity envelope
is supplied by `EventLog`'s wire format. Building one is a second-round decision.

Awkward but usable: caller owns file I/O, empty-state configuration, replay loop,
record-to-wire delivery, counter tally progression, and unique OR-Set tokens.
The caller can do all of these with public Rust APIs today. There is no need to
build UTF-8 delta encoding, event-log encoding or record admission for this slice.

### Validation

- `cargo test --workspace`: 70 passed, 0 failed.
- `cargo test -p safemesh-crdt --no-default-features`: 39 passed, 0 failed.
- `cargo test -p safemesh-crdt --features laws`: 41 passed, 0 failed.
- `cargo test -p safemesh-crdt --examples`: 2 passed, 0 failed.
- `cargo fmt --all --check`: passed.
- Example normal mode: exit 0; strict integrity probe: expected failure, exit 101.

These are overlapping suite runs, not 152 distinct tests. The strict probe is
one separate expected failing assertion, not a Rust-suite failure. The example
is exercised with `cargo run`; its assertions are not counted as test functions.

### Unverified

No process kill, torn-write/power-loss recovery, filesystem atomic replacement,
real network adapter, or application configuration recovery was tested. Files
are whole-log rewrites, not a transactional append journal. No Lean build,
browser, distribution or cross-target build was run. CI is left to the draft PR
gate; local results do not establish that gate's required-check verdict.

### What I tried that did not work

Requiring the same public restart path to reject a one-byte valid payload
mutation fails for both types. Rejecting a damaged outer tag alone gave an
insufficient corruption control; the payload probe demonstrates the missing
integrity guarantee without implementing a checksum or a private workaround.
