//! An application-owned record; see the Nesting deltas guide.
use safemesh_crdt::{GCounterDelta, OrSetDelta, WireCursor, WireDecode, WireEncode, WireError};

#[derive(Debug, PartialEq, Eq)]
struct InventoryRecord {
    item: OrSetDelta<String, u64>,
    count: GCounterDelta,
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
        write_field(out, &self.count)
    }
}

impl WireDecode for InventoryRecord {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        Ok(Self {
            item: read_field(cursor)?,
            count: read_field(cursor)?,
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
    };
    let bytes = original.to_wire_bytes()?;
    let decoded = InventoryRecord::from_wire_bytes(&bytes)?;
    assert_eq!(decoded, original); // Checks element, token, replica and tally.
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
    reject("wrong inner delta type", &wrong_type, WireError::InvalidTag);

    // Inner slices also require exact consumption, independently of the outer record.
    let mut inner_trailing = Vec::new();
    let mut item = original.item.to_wire_bytes()?;
    item.push(0);
    let len = u32::try_from(item.len()).map_err(|_| WireError::LengthOverflow)?;
    inner_trailing.extend_from_slice(&len.to_le_bytes());
    inner_trailing.extend_from_slice(&item);
    write_field(&mut inner_trailing, &original.count)?;
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
        };
        assert_eq!(
            InventoryRecord::from_wire_bytes(&removed.to_wire_bytes()?)?,
            removed
        );
    }
    Ok(())
}
