use safemesh_crdt::{GCounterDelta, WireCursor, WireDecode, WireEncode, WireError};
#[derive(Debug, PartialEq)]
struct Stranger {
    replica: u64,
    tally: u64,
}
impl WireDecode for Stranger {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        if cursor.read_u8()? != 0x10 {
            return Err(WireError::InvalidTag);
        }
        Ok(Self {
            replica: cursor.read_u64()?,
            tally: cursor.read_u64()?,
        })
    }
}
#[test]
fn external_decoder_reads_library_value() {
    let value = GCounterDelta {
        replica: 2,
        tally: 0x0102030405060708,
    };
    let bytes = value.to_wire_bytes().unwrap();
    assert_eq!(
        Stranger::from_wire_bytes(&bytes).unwrap(),
        Stranger {
            replica: value.replica as u64,
            tally: value.tally,
        }
    );
}

use safemesh_crdt::{
    read_tag, write_bytes, write_len, write_u32, write_u64, write_u8, EventLog, GSet, WireSchema,
};

#[derive(Clone, Debug, PartialEq)]
struct Custom {
    version: u8,
    count: u32,
    value: u64,
    name: Vec<u8>,
}
impl WireEncode for Custom {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        write_u8(out, self.version);
        write_u32(out, self.count);
        write_u64(out, self.value);
        write_bytes(out, &self.name)
    }
}
impl WireDecode for Custom {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        let version = cursor.read_u8()?;
        let count = cursor.read_u32()?;
        let value = cursor.read_u64()?;
        let len = cursor.read_len()?;
        let name = cursor.read_exact(len)?.to_vec();
        Ok(Self {
            version,
            count,
            value,
            name,
        })
    }
}
impl WireSchema for Custom {
    fn wire_schema() -> Vec<u8> {
        b"example.org/wirecursor-test/v1".to_vec()
    }
}

#[test]
fn external_custom_payload_roundtrips_in_library_log() {
    let value = Custom {
        version: 7,
        count: 0x01020304,
        value: u64::MAX,
        name: b"reading".to_vec(),
    };
    let mut log = EventLog::new();
    log.append(2, value);
    let bytes = log.to_wire_bytes().unwrap();
    assert_eq!(EventLog::<Custom>::from_wire_bytes(&bytes).unwrap(), log);
}

#[test]
fn external_encoder_matches_library_set_framing() {
    struct ExternalSet(Vec<u64>);
    impl WireEncode for ExternalSet {
        fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
            write_u8(out, 0x20);
            write_len(out, self.0.len())?;
            for value in &self.0 {
                write_u64(out, *value);
            }
            Ok(())
        }
    }
    let mut value = GSet::new();
    value.insert(9u64);
    value.insert(2u64);
    let bytes = ExternalSet(vec![2, 9]).to_wire_bytes().unwrap();
    assert_eq!(bytes, value.to_wire_bytes().unwrap());
    assert_eq!(GSet::<u64>::from_wire_bytes(&bytes).unwrap(), value);
}

#[test]
fn external_reader_bounds_and_overflow_preserve_position() {
    let bytes = [7, 8];
    let mut cursor = WireCursor::new(&bytes);
    assert_eq!(cursor.read_exact(usize::MAX), Err(WireError::UnexpectedEof));
    assert_eq!(cursor.read_exact(0).unwrap(), &[]);
    assert_eq!(cursor.read_u8().unwrap(), 7);
    assert_eq!(
        cursor.read_exact(usize::MAX),
        Err(WireError::LengthOverflow)
    );
    assert_eq!(cursor.read_u32(), Err(WireError::UnexpectedEof));
    assert_eq!(cursor.read_u64(), Err(WireError::UnexpectedEof));
    assert_eq!(cursor.read_len(), Err(WireError::UnexpectedEof));
    assert_eq!(cursor.read_exact(1).unwrap(), &[8]);
    assert!(cursor.is_empty());
    assert_eq!(cursor.read_u8(), Err(WireError::UnexpectedEof));
    assert_eq!(cursor.read_exact(1), Err(WireError::UnexpectedEof));
    assert_eq!(cursor.read_exact(0).unwrap(), &[]);
    assert!(cursor.is_empty());
}

#[test]
fn external_reader_rejects_every_truncated_integer() {
    for len in 0..8 {
        let bytes = [0xff; 8];
        let mut cursor = WireCursor::new(&bytes[..len]);
        assert_eq!(cursor.read_u64(), Err(WireError::UnexpectedEof));
        assert_eq!(cursor.read_exact(len).unwrap(), &bytes[..len]);
        if len < 4 {
            let mut cursor = WireCursor::new(&bytes[..len]);
            assert_eq!(cursor.read_u32(), Err(WireError::UnexpectedEof));
            assert_eq!(cursor.read_len(), Err(WireError::UnexpectedEof));
            assert_eq!(cursor.read_exact(len).unwrap(), &bytes[..len]);
        }
    }
}

#[test]
fn external_length_extremes_are_checked() {
    for len in [0, 1, u32::MAX] {
        let mut bytes = Vec::new();
        write_u32(&mut bytes, len);
        let mut cursor = WireCursor::new(&bytes);
        let expected = usize::try_from(len).map_err(|_| WireError::LengthOverflow);
        assert_eq!(cursor.read_len(), expected);
        assert!(cursor.is_empty());
    }
    if let Ok(too_large) = usize::try_from(u64::from(u32::MAX) + 1) {
        let mut bytes = vec![99];
        assert_eq!(
            write_len(&mut bytes, too_large),
            Err(WireError::LengthOverflow)
        );
        assert_eq!(bytes, vec![99]);
    }
}

#[test]
fn external_tag_and_trailing_bytes_are_checked() {
    let mut cursor = WireCursor::new(&[3, 4]);
    assert_eq!(read_tag(&mut cursor, 2), Err(WireError::InvalidTag));
    assert_eq!(read_tag(&mut cursor, 4), Ok(()));
    assert_eq!(read_tag(&mut cursor, 4), Err(WireError::UnexpectedEof));
    assert!(cursor.is_empty());
    let mut bytes = GCounterDelta {
        replica: 2,
        tally: 7,
    }
    .to_wire_bytes()
    .unwrap();
    bytes.push(0);
    assert_eq!(
        Stranger::from_wire_bytes(&bytes),
        Err(WireError::TrailingBytes)
    );
}

#[test]
fn external_byte_strings_roundtrip_empty_and_binary_data() {
    for name in [vec![], vec![0, 255, 128, 1]] {
        let value = Custom {
            version: 0,
            count: 0,
            value: 0,
            name,
        };
        assert_eq!(
            Custom::from_wire_bytes(&value.to_wire_bytes().unwrap()).unwrap(),
            value
        );
    }
}
