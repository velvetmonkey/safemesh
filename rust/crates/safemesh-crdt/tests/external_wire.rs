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
    fn wire_schema() -> std::borrow::Cow<'static, [u8]> {
        std::borrow::Cow::Borrowed(b"example.org/wirecursor-test/v1")
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

mod external_replica_codec {
    use safemesh_crdt::{
        Admission, AppendError, Crdt, DecodeLimits, EventLog, GSet, MergeError, Mergeable, Record,
        RecordId, Replica, ReplicaError, WireCursor, WireDecode, WireEncode, WireError, WireSchema,
    };
    use std::borrow::Cow;

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct MaxReg(u64);

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct MaxDelta(u64);

    impl Mergeable for MaxReg {
        fn merge(&mut self, other: &Self) -> Result<(), MergeError> {
            self.0 = self.0.max(other.0);
            Ok(())
        }
    }

    impl Crdt for MaxReg {
        type Delta = MaxDelta;

        fn validate_record(&self, _id: RecordId, delta: &MaxDelta) -> Result<(), WireError> {
            if delta.0 == u64::MAX {
                Err(WireError::OwnershipViolation)
            } else {
                Ok(())
            }
        }

        fn apply_delta(&mut self, delta: MaxDelta) {
            self.0 = self.0.max(delta.0);
        }
    }

    impl WireEncode for MaxReg {
        fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
            out.push(0xd0);
            out.extend_from_slice(&self.0.to_le_bytes());
            Ok(())
        }
    }

    impl WireDecode for MaxReg {
        fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
            if cursor.read_u8()? != 0xd0 {
                return Err(WireError::InvalidTag);
            }
            Ok(Self(cursor.read_u64()?))
        }
    }

    impl WireSchema for MaxReg {
        fn wire_schema() -> Cow<'static, [u8]> {
            Cow::Borrowed(b"example.org/max-reg/v1")
        }
    }

    impl WireEncode for MaxDelta {
        fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
            out.push(0xd1);
            out.extend_from_slice(&self.0.to_le_bytes());
            Ok(())
        }
    }

    impl WireDecode for MaxDelta {
        fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
            if cursor.read_u8()? != 0xd1 {
                return Err(WireError::InvalidTag);
            }
            Ok(Self(cursor.read_u64()?))
        }
    }

    impl WireSchema for MaxDelta {
        fn wire_schema() -> Cow<'static, [u8]> {
            Cow::Borrowed(b"example.org/max-delta/v1")
        }
    }

    fn peers() -> (Replica<MaxReg>, Replica<MaxReg>) {
        let mut a = Replica::new(MaxReg::default());
        let mut b = Replica::new(MaxReg::default());
        assert_eq!(a.append(1, MaxDelta(10)).unwrap().id.sequence, 1);
        assert_eq!(a.append(1, MaxDelta(30)).unwrap().id.sequence, 2);
        assert_eq!(b.append(2, MaxDelta(20)).unwrap().id.sequence, 1);
        assert_eq!(b.append(2, MaxDelta(40)).unwrap().id.sequence, 2);
        (a, b)
    }

    fn exchange(a: &mut Replica<MaxReg>, b: &mut Replica<MaxReg>) {
        let a_bytes = a.log_bytes().unwrap();
        let b_bytes = b.log_bytes().unwrap();
        assert_eq!(
            b.merge_log_bytes(&a_bytes, DecodeLimits::default())
                .unwrap(),
            vec![Admission::Accepted; 2]
        );
        assert_eq!(
            a.merge_log_bytes(&b_bytes, DecodeLimits::default())
                .unwrap(),
            vec![Admission::Accepted; 2]
        );
    }

    #[test]
    fn a1_exchange() {
        let (mut a, mut b) = peers();
        exchange(&mut a, &mut b);
        assert_eq!(a.state(), b.state());
        assert_eq!(*a.state(), MaxReg(40));
        assert_eq!(a.version(), b.version());
    }

    #[test]
    fn a2_redelivery() {
        let (mut a, mut b) = peers();
        exchange(&mut a, &mut b);
        let a_state = *a.state();
        let b_state = *b.state();
        let a_len = a.log().records().len();
        let b_len = b.log().records().len();
        let a_version = a.version().clone();
        let b_version = b.version().clone();
        let a_bytes = a.log_bytes().unwrap();
        let b_bytes = b.log_bytes().unwrap();
        assert_eq!(
            b.merge_log_bytes(&a_bytes, DecodeLimits::default())
                .unwrap(),
            vec![Admission::Duplicate; 4]
        );
        assert_eq!(
            a.merge_log_bytes(&b_bytes, DecodeLimits::default())
                .unwrap(),
            vec![Admission::Duplicate; 4]
        );
        assert_eq!(*a.state(), a_state);
        assert_eq!(*b.state(), b_state);
        assert_eq!(a.log().records().len(), a_len);
        assert_eq!(b.log().records().len(), b_len);
        assert_eq!(a.version(), &a_version);
        assert_eq!(b.version(), &b_version);
    }

    #[test]
    fn a3_peer_vectors() {
        let (mut a, mut b) = peers();
        assert_eq!(a.since(b.version()), a.log().records());
        exchange(&mut a, &mut b);
        assert!(a.since(b.version()).is_empty());
        assert_eq!(b.version().get(1), 2);
        assert_eq!(b.version().get(2), 2);
        assert_eq!(b.version().entries().len(), 2);
    }

    #[test]
    fn a4_restore() {
        let (mut a, mut b) = peers();
        exchange(&mut a, &mut b);
        let restored = Replica::<MaxReg>::restore(
            MaxReg::default(),
            &a.log_bytes().unwrap(),
            DecodeLimits::default(),
        )
        .unwrap();
        assert_eq!(restored.state(), a.state());
        assert_eq!(restored.log().records(), a.log().records());
        assert_eq!(restored.version(), a.version());
    }

    #[test]
    fn a5_refused_record() {
        let (mut a, _) = peers();
        let state = *a.state();
        let len = a.log().records().len();
        let version = a.version().clone();
        let refused = MaxDelta(u64::MAX);
        assert_eq!(
            a.admit(Record {
                id: RecordId {
                    replica: 3,
                    sequence: 1
                },
                delta: refused,
            }),
            Admission::Invalid(WireError::OwnershipViolation)
        );
        assert_eq!(
            a.append(1, refused),
            Err(ReplicaError::Append(AppendError::InvalidRecord(
                WireError::OwnershipViolation
            )))
        );
        assert_eq!(*a.state(), state);
        assert_eq!(a.log().records().len(), len);
        assert_eq!(a.version(), &version);
    }

    #[test]
    fn a6_wrong_schema() {
        let (mut a, _) = peers();
        let state = *a.state();
        let len = a.log().records().len();
        let version = a.version().clone();
        let value = super::Custom {
            version: 7,
            count: 0x01020304,
            value: u64::MAX,
            name: b"reading".to_vec(),
        };
        let mut log = EventLog::new();
        log.append(&mut GSet::new(), 2, value).unwrap();
        assert!(matches!(
            a.merge_log_bytes(&log.to_wire_bytes().unwrap(), DecodeLimits::default()),
            Err(ReplicaError::LogDecode(_))
        ));
        assert_eq!(*a.state(), state);
        assert_eq!(a.log().records().len(), len);
        assert_eq!(a.version(), &version);
    }
}
