# EventLog collision fixtures

These four binary frames contain two records with ID `(1, 1)` and distinct
payloads. They must pass frame integrity verification and then fail admission
with `WireError::RecordCollision`. They replace the Python test's manually
assembled tag-`0x02` frames.

The product `EventLog::to_wire_bytes` encoder generates the complete frame,
including lengths and CRC. The test
`frame_tests::binding_collision_fixtures_use_product_encoder` in
`safemesh-crdt/src/lib.rs` defines the inputs and checks the exact refusal.
Regenerate from the repository root:

```sh
SAFEMESH_FRAME_FIXTURE_DIR="$PWD/rust/crates/safemesh-python/tests/fixtures" cargo test --manifest-path rust/Cargo.toml -p safemesh-crdt binding_collision_fixtures_use_product_encoder
```

- `gcounter-collision.bin`: replica 1, tallies 5 and 9.
- `flag-collision.bin`: enable tokens 5 and 9.
- `register-collision.bin`: writer 1, timestamps 1/2, values 5/9.
- `map-collision.bin`: key 1, writer 1, set value 5 at timestamp 1, remove at timestamp 2.

Do not hand-edit these bytes or checksums.
