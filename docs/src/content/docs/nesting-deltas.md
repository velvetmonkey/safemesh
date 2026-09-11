---
title: Nesting deltas in your own record
description: A compiled Rust composition pattern using public wire APIs and named decode errors.
---

Use a length prefix around each delta's `to_wire_bytes()` output, then decode each bounded slice with that delta type's `from_wire_bytes()`. Public `WireCursor` reads and Rust's `u32::to_le_bytes` / `Vec::extend_from_slice` already provide everything required, so no new SafeMesh write helper or knowledge of inner byte layouts is needed.

This application-owned `InventoryRecord` contains an `OrSetDelta<String, u64>` followed by a `GCounterDelta` and a `PnCounterDelta`. Its layout is `[u32 little-endian item byte length][item bytes][u32 little-endian count byte length][count bytes][u32 little-endian adjustment byte length][adjustment bytes]`; lengths exclude their own four-byte fields. This adds an outer application format and changes no SafeMesh wire format. The fields have fixed, agreed types and order; length prefixes do not identify arbitrary types or versions.

The three public delta types used here are imported from `safemesh_crdt`. Construct them with these public fields (the annotations below describe field types):

| Delta type | Public construction forms |
| --- | --- |
| `OrSetDelta<String, u64>` | `OrSetDelta::Add { element: String, token: u64 }` or `OrSetDelta::Remove { tokens: Vec<u64> }` |
| `GCounterDelta` | `GCounterDelta { replica: usize, tally: u64 }` |
| `PnCounterDelta` | `PnCounterDelta::Inc { replica: usize, tally: u64 }` or `PnCounterDelta::Dec { replica: usize, tally: u64 }` |

Counter tallies are absolute values for the selected replica component, not amounts to add or subtract. `Inc` selects the positive component and `Dec` the negative component. Each of these types implements `WireEncode` and `WireDecode`; use the same framing helpers for any of them, preserving the agreed field order. For example, a two-field record can keep only `item: OrSetDelta<String, u64>` and `adjustment: PnCounterDelta`, writing and reading exactly those two fields in that order.

From the checkout's `rust/` directory, run the complete public-API example:

```sh
cargo run -p safemesh-crdt --example nested_deltas --locked
```

The source is [`nested_deltas.rs`](https://github.com/velvetmonkey/safemesh/blob/main/rust/crates/safemesh-crdt/examples/nested_deltas.rs). It checks all decoded values, both OR-Set and both PN-Counter variants, every truncated prefix, and the failures below. The complete copyable program is:

```rust
//! An application-owned record; see the Nesting deltas guide.
use safemesh_crdt::{
    GCounterDelta, OrSetDelta, PnCounterDelta, WireCursor, WireDecode, WireEncode, WireError,
};

#[derive(Debug, PartialEq, Eq)]
struct InventoryRecord {
    item: OrSetDelta<String, u64>,
    count: GCounterDelta,
    adjustment: PnCounterDelta,
}

// These are application functions, not additions to SafeMesh's public API.
fn write_field(out: &mut Vec<u8>, value: &impl WireEncode) -> Result<(), WireError> {
    let bytes = value.to_wire_bytes()?;
    let len = u32::try_from(bytes.len()).map_err(|_| WireError::LengthOverflow)?;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&bytes);
    Ok(())
}

fn read_field<T: WireDecode>(cursor: &mut WireCursor<'_>) -> Result<T, WireError> {
    let len = cursor.read_len()?;
    T::from_wire_bytes(cursor.read_exact(len)?)
}

impl WireEncode for InventoryRecord {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        write_field(out, &self.item)?;
        write_field(out, &self.count)?;
        write_field(out, &self.adjustment)
    }
}

impl WireDecode for InventoryRecord {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        Ok(Self {
            item: read_field(cursor)?,
            count: read_field(cursor)?,
            adjustment: read_field(cursor)?,
        })
    }
}

fn reject(label: &str, bytes: &[u8], expected: WireError) {
    let result = InventoryRecord::from_wire_bytes(bytes);
    assert_eq!(result, Err(expected));
    println!("{label}: {expected:?}");
}

fn main() -> Result<(), WireError> {
    let original = InventoryRecord {
        item: OrSetDelta::Add {
            element: "water 💧".into(),
            token: 42,
        },
        count: GCounterDelta {
            replica: 0,
            tally: 7,
        },
        adjustment: PnCounterDelta::Inc {
            replica: 0,
            tally: 3,
        },
    };
    let bytes = original.to_wire_bytes()?;
    let decoded = InventoryRecord::from_wire_bytes(&bytes)?;
    assert_eq!(decoded, original); // Checks every field, including the PN-Counter variant.
    println!("round trip: {decoded:?}");

    reject(
        "truncated record",
        &bytes[..bytes.len() - 1],
        WireError::UnexpectedEof,
    );
    // Every proper prefix is incomplete, including a partial length field.
    for end in 0..bytes.len() {
        assert_eq!(
            InventoryRecord::from_wire_bytes(&bytes[..end]),
            Err(WireError::UnexpectedEof)
        );
    }

    let mut oversized = bytes.clone();
    let too_long = u32::try_from(bytes.len()).map_err(|_| WireError::LengthOverflow)?;
    oversized[..4].copy_from_slice(&too_long.to_le_bytes());
    reject(
        "length exceeds remaining input",
        &oversized,
        WireError::UnexpectedEof,
    );

    let mut trailing = bytes.clone();
    trailing.push(0);
    reject("trailing bytes", &trailing, WireError::TrailingBytes);

    // A complete, valid G-Counter delta in the OR-Set field, using its encoder.
    let mut wrong_type = Vec::new();
    write_field(&mut wrong_type, &original.count)?;
    write_field(&mut wrong_type, &original.count)?;
    write_field(&mut wrong_type, &original.adjustment)?;
    reject("wrong inner delta type", &wrong_type, WireError::InvalidTag);

    // Inner slices also require exact consumption, independently of the outer record.
    let mut inner_trailing = Vec::new();
    let mut item = original.item.to_wire_bytes()?;
    item.push(0);
    let len = u32::try_from(item.len()).map_err(|_| WireError::LengthOverflow)?;
    inner_trailing.extend_from_slice(&len.to_le_bytes());
    inner_trailing.extend_from_slice(&item);
    write_field(&mut inner_trailing, &original.count)?;
    write_field(&mut inner_trailing, &original.adjustment)?;
    reject(
        "trailing bytes inside field",
        &inner_trailing,
        WireError::TrailingBytes,
    );

    // The other OR-Set variant, including an empty removal, composes identically.
    for tokens in [vec![], vec![42, 99]] {
        let removed = InventoryRecord {
            item: OrSetDelta::Remove { tokens },
            count: GCounterDelta {
                replica: 1,
                tally: 9,
            },
            adjustment: PnCounterDelta::Dec {
                replica: 1,
                tally: 2,
            },
        };
        assert_eq!(
            InventoryRecord::from_wire_bytes(&removed.to_wire_bytes()?)?,
            removed
        );
    }
    Ok(())
}
```

## Failure meanings

| Input | Returned error | Meaning |
| --- | --- | --- |
| Record truncated by one byte (also every proper prefix) | `WireError::UnexpectedEof` | A required length field or its declared payload is incomplete. |
| First length larger than the remaining input | `WireError::UnexpectedEof` | The borrowed field slice cannot be read; the decoder does not allocate based on the outer length. |
| Valid record followed by an extra byte | `WireError::TrailingBytes` | Outer `from_wire_bytes` requires exact consumption. The same error is returned for extra bytes inside an otherwise valid field. |
| Valid G-Counter delta substituted for the OR-Set field | `WireError::InvalidTag` | The OR-Set decoder rejects the G-Counter tag. This raw delta check does not return the persisted-log schema error `DeltaTypeMismatch`. |

The example asserts these errors directly; a panic or an incorrect value fails the run. It propagates other inner codec errors, including `InvalidUtf8`, and checked length/conversion failures return `LengthOverflow`. On encoding, a field exceeding `u32::MAX` bytes returns `LengthOverflow`; `to_wire_bytes` discards its temporary output on failure. Direct `encode_wire` callers must discard partially written output on an error.

This is structural decoding, not application admission: it does not validate replica ownership, counter arity, token allocation or the relationship between these three edits. Corruption that is still a valid value cannot be detected by lengths or tags alone. Applications must define those checks and transport integrity themselves. If persisting a custom payload in `EventLog`, implement `WireSchema` with an application-owned, globally unique versioned identity and change it when the interpretation changes; this example is a standalone record, not a new proven CRDT or durable writer.
