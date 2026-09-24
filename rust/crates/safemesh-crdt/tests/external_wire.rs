use safemesh_crdt::{
    write_bytes, write_len, write_u32, write_u64, write_u8, GCounterDelta, WireCursor, WireDecode,
    WireEncode, WireError,
};
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

#[test]
fn external_writers_roundtrip_with_public_readers() {
    let mut bytes = Vec::new();
    write_u8(&mut bytes, 0xa5);
    write_u32(&mut bytes, 0x0102_0304);
    write_u64(&mut bytes, 0x0102_0304_0506_0708);
    write_len(&mut bytes, 3).unwrap();
    write_bytes(&mut bytes, &[0, 0xff, 0x80]).unwrap();

    let mut cursor = WireCursor::new(&bytes);
    assert_eq!(cursor.read_u8().unwrap(), 0xa5);
    assert_eq!(cursor.read_u32().unwrap(), 0x0102_0304);
    assert_eq!(cursor.read_u64().unwrap(), 0x0102_0304_0506_0708);
    assert_eq!(cursor.read_len().unwrap(), 3);
    let len = cursor.read_len().unwrap();
    assert_eq!(cursor.read_exact(len).unwrap(), &[0, 0xff, 0x80]);
    assert!(cursor.is_empty());

    let mut set_frame = Vec::new();
    write_u8(&mut set_frame, 0x20);
    write_len(&mut set_frame, 2).unwrap();
    write_u64(&mut set_frame, 2);
    write_u64(&mut set_frame, 9);
    let mut expected = GSet::new();
    expected.insert(2u64);
    expected.insert(9u64);
    assert_eq!(GSet::<u64>::from_wire_bytes(&set_frame).unwrap(), expected);
}

#[test]
fn external_write_len_rejects_unrepresentable_prefix_without_mutation() {
    if let Ok(too_large) = usize::try_from(u64::from(u32::MAX) + 1) {
        let mut bytes = vec![0xa5];
        assert_eq!(
            write_len(&mut bytes, too_large),
            Err(WireError::LengthOverflow)
        );
        assert_eq!(bytes, [0xa5]);
    }
}

use safemesh_crdt::{EventLog, GSet, WireSchema};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Custom {
    version: u8,
    count: u32,
    value: u64,
    name: Vec<u8>,
}
impl WireEncode for Custom {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        out.push(self.version);
        out.extend_from_slice(&self.count.to_le_bytes());
        out.extend_from_slice(&self.value.to_le_bytes());
        let len = u32::try_from(self.name.len()).map_err(|_| WireError::LengthOverflow)?;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&self.name);
        Ok(())
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
    log.append(&mut GSet::new(), 2, value).unwrap();
    let bytes = log.to_wire_bytes().unwrap();
    assert_eq!(EventLog::<Custom>::from_wire_bytes(&bytes).unwrap(), log);
}

#[test]
fn external_encoder_matches_library_set_framing() {
    struct ExternalSet(Vec<u64>);
    impl WireEncode for ExternalSet {
        fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
            out.push(0x20);
            let len = u32::try_from(self.0.len()).map_err(|_| WireError::LengthOverflow)?;
            out.extend_from_slice(&len.to_le_bytes());
            for value in &self.0 {
                out.extend_from_slice(&value.to_le_bytes());
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
