# Legacy EventLog frames

Every EventLog frame layout an earlier `safemesh-crdt` could have written before
the current shaped `0x03` frame. No version of the crate was ever tagged or
released (`safemesh-crdt 0.1.0` is unpublished), so each set is generated from the
**last main-branch commit that wrote that frame**, not from a release tag.

Tag 0x02 source: `3fb38cbac87846e690a7565dec2325e79d899c90`.

Unshaped tag 0x03 source: `4a12ae6df26df7947d5e95c9179e5be0fdfa719f`.

| Directory | Frame | Written by main from | Last writer | Replaced by |
| --- | --- | --- | --- | --- |
| `tag02/` | `0x02`, u32 record count, length-prefixed records; no length pair, CRC or shape | `ec5b0c5` (canonical wire format) | `3fb38cb` (#15) | #16, `773293d` |
| `tag03-unshaped/` | `0x03`, u32 length, its complement, body (u32 record count, records), CRC-32; no shape header | `773293d` (#16) | `4a12ae6` (#17) | #18, `eaaaef2` |

The Record and EventLog encoders are byte-identical across each era: the 0x02
encoder at `ec5b0c5` and `3fb38cb`, and the unshaped 0x03 encoder (with its CRC)
at `773293d` and `4a12ae6`. Record and delta bytes inside a frame are unchanged
from both eras to the current encoder; the tests below check this byte for byte.

Each directory holds the same logs, one per built-in delta that had a wire
codec in both eras (G-Set deltas were raw `T` and RGA deltas had no codec, so
neither could be persisted in an EventLog then):

| File | Delta | Records `(author, sequence)` | Arity needed to migrate |
| --- | --- | --- | --- |
| `gcounter-empty.log` | `GCounterDelta` | none | fixed, 2 |
| `gcounter.log` | `GCounterDelta` | (0,1) (1,1) (0,2) | fixed, 2 |
| `pncounter.log` | `PnCounterDelta` | (0,1) (1,1) (1,2) | fixed, 2 |
| `orset-u64.log` | `OrSetDelta<u64, u64>` | (0,1) (1,1) (0,2) | unbounded |
| `orset-utf8.log` | `OrSetDelta<String, u64>` | (0,1) (1,1) (1,2) | unbounded |
| `lww-register-u64.log` | `LwwRegisterDelta<u64>` | (0,1) (1,1) | unbounded |
| `enable-wins-flag-u64.log` | `EnableWinsFlagDelta<u64>` | (0,1) (1,1) | unbounded |
| `lww-map-u64.log` | `LwwMapDelta<u64, u64>` | (0,1) (1,1) | unbounded |

The exact records are listed in `generate.rs` and asserted in
`tests/legacy_event_log.rs`. That test decodes every retained file and asserts
its exact outcome. The current decoder must return
`WireError::LegacyEventLogFrame` naming the found frame. Migration must give
the listed records with unchanged record bytes, then round-trip through the
current destination-aware decoder. It also pins each file's SHA-256, so a
one-byte change to any fixture fails the test.

## Reproduction

`generate.rs` is the pinned generator. It is not compiled in this checkout.
From the repository root:

```sh
./scripts/check-legacy-event-log-fixtures.sh
```

For each claimed source above, the script checks the commit is an ancestor of
HEAD and runs `git archive <sha>` into a scratch tree. It copies `generate.rs`
there as `rust/crates/safemesh-crdt/tests/legacy_generate.rs` and runs:

```sh
LEGACY_EVENT_LOG_GENERATE=<new-dir> cargo test \
  --manifest-path <scratch>/rust/Cargo.toml -p safemesh-crdt \
  --test legacy_generate generate_legacy_event_logs -- --ignored --exact
```

It then requires the generated file set and every file's bytes to equal this
directory. The old commits pin no toolchain; CI builds them with this checkout's
`rust-toolchain.toml` (1.96.1). The generator uses only the archived public
encoder (`EventLog::new`, `insert_record`, `to_wire_bytes`) and never decodes
its output. `LEGACY_FIXTURE_DIR` points the comparison at a separate copy for
negative controls. CI runs this script in the full gate.

Never regenerate these files from a newer source. They record what old code
actually wrote; if the current decoder stops accepting them for migration, that
is the regression this corpus exists to catch.
