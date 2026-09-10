// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Debug;

use safemesh_crdt::{
    EnableWinsFlag, EnableWinsFlagDelta, EventLog, GCounterDelta, GSet, LwwMap, LwwMapDelta,
    LwwRegister, LwwRegisterDelta, OrSet, PnCounterDelta, Record, RecordId, Rga, WireDecode,
    WireEncode, WireError,
};

fn roundtrip<T>(value: T)
where
    T: WireEncode + WireDecode + PartialEq + Debug,
{
    let bytes = value.to_wire_bytes().unwrap();
    let decoded = T::from_wire_bytes(&bytes).unwrap();
    assert_eq!(decoded, value);
    assert_eq!(decoded.to_wire_bytes().unwrap(), bytes);
}

#[test]
fn gcounter_delta_has_stable_canonical_bytes() {
    let delta = GCounterDelta {
        replica: 2,
        tally: 7,
    };
    assert_eq!(
        delta.to_wire_bytes().unwrap(),
        vec![
            0x10, // G-Counter delta tag
            0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // replica
            0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // tally
        ],
    );
    roundtrip(delta);
}

#[test]
fn record_frames_delta_with_length_prefix() {
    let record = Record {
        id: RecordId {
            replica: 9,
            sequence: 4,
        },
        delta: GCounterDelta {
            replica: 2,
            tally: 7,
        },
    };
    assert_eq!(
        record.to_wire_bytes().unwrap(),
        vec![
            0x01, // record tag
            0x09, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // record replica
            0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // sequence
            0x11, 0x00, 0x00, 0x00, // delta length
            0x10, // G-Counter delta tag
            0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // delta replica
            0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // tally
        ],
    );
    roundtrip(record);
}

#[test]
fn canonical_state_encodings_are_sorted() {
    let mut left = GSet::new();
    left.insert(3);
    left.insert(1);
    let mut right = GSet::new();
    right.insert(1);
    right.insert(3);
    assert_eq!(
        left.to_wire_bytes().unwrap(),
        right.to_wire_bytes().unwrap()
    );
    roundtrip(left);

    let mut left = OrSet::new();
    left.add(2, 20);
    left.add(1, 10);
    left.apply_remove([20]);
    let mut right = OrSet::new();
    right.apply_remove([20]);
    right.add(1, 10);
    right.add(2, 20);
    assert_eq!(
        left.to_wire_bytes().unwrap(),
        right.to_wire_bytes().unwrap()
    );
    roundtrip(left);

    let mut rga = Rga::new();
    rga.insert(30, 3);
    rga.insert(10, 1);
    rga.delete(30);
    roundtrip(rga);
}

#[test]
fn event_log_roundtrips_and_preserves_deduped_records() {
    let mut log = EventLog::with_replica_count(2);
    log.append(
        1,
        PnCounterDelta::Inc {
            replica: 1,
            tally: 5,
        },
    );
    log.append(
        1,
        PnCounterDelta::Dec {
            replica: 1,
            tally: 2,
        },
    );

    let bytes = log.to_wire_bytes().unwrap();
    let decoded = EventLog::<PnCounterDelta>::from_wire_bytes(&bytes).unwrap();
    assert_eq!(decoded.records().len(), 2);
    assert_eq!(decoded.version().get(1), 2);
    assert_eq!(decoded.to_wire_bytes().unwrap(), bytes);
}

#[test]
fn lww_register_wire_is_stable_and_roundtrips() {
    let delta = LwwRegisterDelta {
        timestamp: 9,
        replica: 2,
        value: 42,
    };
    assert_eq!(
        delta.to_wire_bytes().unwrap(),
        vec![
            0x50, // LWW register delta tag
            0x09, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // timestamp
            0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // replica
            0x2a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // value
        ],
    );
    roundtrip(delta);

    let mut register = LwwRegister::new();
    assert_eq!(register.to_wire_bytes().unwrap(), vec![0x51, 0x00]);
    register.set(9, 2, 42);
    assert_eq!(
        register.to_wire_bytes().unwrap(),
        vec![
            0x51, 0x01, // LWW register tag + present
            0x09, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // timestamp
            0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // replica
            0x2a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // value
        ],
    );
    roundtrip(register);
}

#[test]
fn enable_wins_flag_wire_is_stable_and_roundtrips() {
    let enable = EnableWinsFlagDelta::Enable { token: 42 };
    assert_eq!(
        enable.to_wire_bytes().unwrap(),
        vec![
            0x60, // enable-wins flag enable delta tag
            0x2a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
    );
    roundtrip(enable);

    let disable = EnableWinsFlagDelta::Disable { tokens: vec![9, 2] };
    assert_eq!(
        disable.to_wire_bytes().unwrap(),
        vec![
            0x61, // enable-wins flag disable delta tag
            0x02, 0x00, 0x00, 0x00, // token count
            0x09, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // caller order
            0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // caller order
        ],
    );
    roundtrip(EnableWinsFlagDelta::Disable { tokens: vec![2, 9] });
    assert_eq!(
        EnableWinsFlagDelta::Disable {
            tokens: vec![9, 2, 2]
        }
        .to_wire_bytes()
        .unwrap(),
        vec![
            0x61, 0x03, 0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00,
        ],
    );
    roundtrip(EnableWinsFlagDelta::Disable {
        tokens: vec![9, 2, 2],
    });

    let mut flag = EnableWinsFlag::new();
    flag.enable(9);
    flag.enable(2);
    flag.disable([9]);
    assert_eq!(
        flag.to_wire_bytes().unwrap(),
        vec![
            0x62, // enable-wins flag state tag
            0x02, 0x00, 0x00, 0x00, // enable count
            0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // sorted enable
            0x09, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // sorted enable
            0x01, 0x00, 0x00, 0x00, // tombstone count
            0x09, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // sorted tombstone
        ],
    );
    roundtrip(flag);
}

#[test]
fn enable_wins_flag_record_roundtrips_unormalized_tokens() {
    let record = safemesh_crdt::Record {
        id: safemesh_crdt::RecordId {
            replica: 1,
            sequence: 1,
        },
        delta: EnableWinsFlagDelta::Disable {
            tokens: vec![9, 2, 2],
        },
    };
    let mut log = EventLog::new();
    assert_eq!(
        log.insert_record(record.clone()),
        safemesh_crdt::Admission::Accepted
    );
    let bytes = log.to_wire_bytes().unwrap();
    let reopened = EventLog::<EnableWinsFlagDelta<u64>>::from_wire_bytes(&bytes).unwrap();
    assert_eq!(reopened.records()[0].delta, record.delta);
    assert_eq!(
        log.admit_with(reopened.records()[0].clone(), |_| {}),
        safemesh_crdt::Admission::Duplicate
    );
    assert_eq!(log.records(), reopened.records());
}

#[test]
fn lww_map_wire_is_stable_and_roundtrips() {
    let set = LwwMapDelta::Set {
        key: 7_u64,
        timestamp: 9,
        replica: 2,
        value: 42_u64,
    };
    assert_eq!(
        set.to_wire_bytes().unwrap(),
        vec![
            0x70, // LWW map set delta tag
            0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // key
            0x09, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // timestamp
            0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // replica
            0x2a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // value
        ],
    );
    roundtrip(set);

    let remove = LwwMapDelta::Remove {
        key: 7_u64,
        timestamp: 10,
        replica: 2,
    };
    assert_eq!(
        remove.to_wire_bytes().unwrap(),
        vec![
            0x71, // LWW map remove delta tag
            0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // key
            0x0a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // timestamp
            0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // replica
        ],
    );
    roundtrip(remove);

    let mut map = LwwMap::new();
    assert_eq!(
        map.to_wire_bytes().unwrap(),
        vec![
            0x72, // LWW map state tag
            0x00, 0x00, 0x00, 0x00, // entry count
            0x00, 0x00, 0x00, 0x00, // removal count
        ],
    );
    map.set(9, 2, 1, 90);
    map.set(2, 1, 1, 20);
    map.remove(9, 3, 1);
    assert_eq!(
        map.to_wire_bytes().unwrap(),
        vec![
            0x72, // LWW map state tag
            0x02, 0x00, 0x00, 0x00, // entry count
            0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // sorted key
            0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // timestamp
            0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // replica
            0x14, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // value
            0x09, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // sorted key
            0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // timestamp
            0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // replica
            0x5a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // value
            0x01, 0x00, 0x00, 0x00, // removal count
            0x09, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // sorted key
            0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // timestamp
            0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // replica
        ],
    );
    roundtrip(map);
}

#[test]
fn malformed_wire_inputs_fail_closed() {
    assert_eq!(
        GCounterDelta::from_wire_bytes(&[0xff, 0, 0]),
        Err(WireError::InvalidTag),
    );
    assert_eq!(
        GCounterDelta::from_wire_bytes(&[0x10]),
        Err(WireError::UnexpectedEof)
    );

    let mut bytes = GCounterDelta {
        replica: 0,
        tally: 1,
    }
    .to_wire_bytes()
    .unwrap();
    bytes.push(0);
    assert_eq!(
        GCounterDelta::from_wire_bytes(&bytes),
        Err(WireError::TrailingBytes)
    );
}

fn orset_delta_cases() -> Vec<safemesh_crdt::OrSetDelta<u64, u64>> {
    use safemesh_crdt::OrSetDelta::{Add, Remove};
    vec![
        Add {
            element: 0,
            token: u64::MAX,
        },
        Add {
            element: u64::MAX,
            token: 0,
        },
        Remove { tokens: vec![] },
        Remove { tokens: vec![42] },
        Remove {
            tokens: vec![u64::MAX, 0, 7, 7],
        },
    ]
}

#[test]
fn orset_delta_roundtrips() {
    for delta in orset_delta_cases() {
        roundtrip(delta);
    }
}

#[test]
fn orset_delta_rejects_every_truncation() {
    for delta in orset_delta_cases() {
        let bytes = delta.to_wire_bytes().unwrap();
        for len in 0..bytes.len() {
            let result = safemesh_crdt::OrSetDelta::<u64, u64>::from_wire_bytes(&bytes[..len]);
            assert_eq!(
                result,
                Err(WireError::UnexpectedEof),
                "delta {delta:?}, prefix {len}/{}",
                bytes.len(),
            );
        }
    }
}

#[test]
fn orset_delta_rejects_unknown_tags() {
    for tag in 0..=u8::MAX {
        if tag == 0x31 || tag == 0x32 {
            continue;
        }
        let result = safemesh_crdt::OrSetDelta::<u64, u64>::from_wire_bytes(&[tag]);
        assert_eq!(result, Err(WireError::InvalidTag), "tag {tag:#04x}");
    }
}

#[test]
fn orset_delta_rejects_trailing_bytes_and_oversized_counts() {
    for delta in orset_delta_cases() {
        let mut bytes = delta.to_wire_bytes().unwrap();
        bytes.push(0);
        assert_eq!(
            safemesh_crdt::OrSetDelta::<u64, u64>::from_wire_bytes(&bytes),
            Err(WireError::TrailingBytes),
        );
    }
    assert_eq!(
        safemesh_crdt::OrSetDelta::<u64, u64>::from_wire_bytes(&[0x32, 0xff, 0xff, 0xff, 0xff]),
        Err(WireError::UnexpectedEof),
    );
}

#[test]
fn orset_delta_roundtrips_in_record_and_event_log() {
    let mut log = EventLog::new();
    for delta in orset_delta_cases() {
        let id = log.append(1, delta.clone());
        roundtrip(Record { id, delta });
    }
    let bytes = log.to_wire_bytes().unwrap();
    let decoded = EventLog::<safemesh_crdt::OrSetDelta<u64, u64>>::from_wire_bytes(&bytes).unwrap();
    assert_eq!(decoded.records(), log.records());
    assert_eq!(decoded.to_wire_bytes().unwrap(), bytes);
}

fn orset_utf8_cases() -> Vec<safemesh_crdt::OrSetDelta<String, u64>> {
    use safemesh_crdt::OrSetDelta::{Add, Remove};
    let mut cases: Vec<_> = ["", "ASCII", "é東京🦀", "a\0b", &"界".repeat(4096)]
        .into_iter()
        .enumerate()
        .map(|(i, element)| Add {
            element: element.to_owned(),
            token: if i == 0 { u64::MAX } else { i as u64 },
        })
        .collect();
    // Each Add has one token; many Adds and one Remove exercise a large token set.
    cases.extend((0..128).map(|token| Add {
        element: "shared 🦀".to_owned(),
        token,
    }));
    cases.push(Remove { tokens: vec![] });
    cases.push(Remove { tokens: vec![0] });
    cases.push(Remove {
        tokens: (0..128).rev().chain([7, 7, u64::MAX]).collect(),
    });
    cases
}

#[test]
fn orset_utf8_roundtrips_and_rejects_every_truncation() {
    use safemesh_crdt::OrSetDelta;
    let cases = orset_utf8_cases();
    let mut truncations = 0;
    for delta in &cases {
        let bytes = delta.to_wire_bytes().unwrap();
        assert_eq!(
            OrSetDelta::<String, u64>::from_wire_bytes(&bytes),
            Ok(delta.clone())
        );
        for len in 0..bytes.len() {
            let result = std::panic::catch_unwind(|| {
                OrSetDelta::<String, u64>::from_wire_bytes(&bytes[..len])
            });
            assert!(result.is_ok(), "panic at prefix {len}");
            assert_eq!(result.unwrap(), Err(WireError::UnexpectedEof));
            truncations += 1;
        }
    }
    println!("roundtrip-cases {}, roundtrip-failures 0, truncation-cases {truncations}, truncation-all-err yes, panics 0", cases.len());
}

#[test]
fn orset_utf8_rejects_invalid_utf8_without_panicking() {
    use safemesh_crdt::OrSetDelta;
    // Isolated continuation, overlong, incomplete, surrogate, out-of-range, invalid lead.
    let payloads: &[&[u8]] = &[
        &[0x80],
        &[0xc0, 0xaf],
        &[0xe2, 0x82],
        &[0xed, 0xa0, 0x80],
        &[0xf4, 0x90, 0x80, 0x80],
        &[0xff],
    ];
    for payload in payloads {
        let mut bytes = vec![0x33];
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(payload);
        bytes.extend_from_slice(&42u64.to_le_bytes());
        let result =
            std::panic::catch_unwind(|| OrSetDelta::<String, u64>::from_wire_bytes(&bytes));
        assert!(result.is_ok(), "decoder panicked for {payload:02x?}");
        let decoded = result.unwrap();
        assert_eq!(decoded, Err(WireError::InvalidUtf8));
        println!("payload {payload:02x?}: {decoded:?}, panicked false");
    }
    println!(
        "invalid-utf8-cases {}, invalid-utf8-all-err yes, panics 0",
        payloads.len()
    );
}

#[test]
fn orset_utf8_rejects_length_lies_tags_and_trailing_bytes() {
    use safemesh_crdt::OrSetDelta;
    for tag in [0x33, 0x34] {
        for length in [16u32, u32::MAX] {
            let mut bytes = vec![tag];
            bytes.extend_from_slice(&length.to_le_bytes());
            bytes.extend_from_slice(&[0; 8]);
            let result =
                std::panic::catch_unwind(|| OrSetDelta::<String, u64>::from_wire_bytes(&bytes));
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), Err(WireError::UnexpectedEof));
        }
    }
    for tag in 0..=u8::MAX {
        if tag != 0x33 && tag != 0x34 {
            assert_eq!(
                OrSetDelta::<String, u64>::from_wire_bytes(&[tag]),
                Err(WireError::InvalidTag)
            );
        }
    }
    for delta in orset_utf8_cases() {
        let mut bytes = delta.to_wire_bytes().unwrap();
        assert_eq!(
            OrSetDelta::<u64, u64>::from_wire_bytes(&bytes),
            Err(WireError::InvalidTag)
        );
        bytes.push(0);
        assert_eq!(
            OrSetDelta::<String, u64>::from_wire_bytes(&bytes),
            Err(WireError::TrailingBytes)
        );
    }
    for delta in orset_delta_cases() {
        assert_eq!(
            OrSetDelta::<String, u64>::from_wire_bytes(&delta.to_wire_bytes().unwrap()),
            Err(WireError::InvalidTag)
        );
    }
    println!("length-lie-cases 4, length-lie-all-err yes");
}

#[test]
fn orset_utf8_wire_shape_and_record_roundtrips() {
    use safemesh_crdt::OrSetDelta;
    let add = OrSetDelta::Add {
        element: "é".to_owned(),
        token: 42u64,
    };
    assert_eq!(
        add.to_wire_bytes().unwrap(),
        vec![0x33, 2, 0, 0, 0, 0xc3, 0xa9, 42, 0, 0, 0, 0, 0, 0, 0]
    );
    let remove: OrSetDelta<String, u64> = OrSetDelta::Remove {
        tokens: vec![42, 42],
    };
    assert_eq!(
        remove.to_wire_bytes().unwrap(),
        vec![0x34, 2, 0, 0, 0, 42, 0, 0, 0, 0, 0, 0, 0, 42, 0, 0, 0, 0, 0, 0, 0]
    );
    let mut log = EventLog::new();
    for delta in orset_utf8_cases() {
        let id = log.append(1, delta.clone());
        roundtrip(Record { id, delta });
    }
    let bytes = log.to_wire_bytes().unwrap();
    let decoded = EventLog::<OrSetDelta<String, u64>>::from_wire_bytes(&bytes).unwrap();
    assert_eq!(decoded.records(), log.records());
    assert_eq!(decoded.to_wire_bytes().unwrap(), bytes);
}

#[test]
fn event_log_refuses_every_single_byte_change() {
    use safemesh_crdt::OrSetDelta;
    let mut log = EventLog::new();
    log.append(
        1,
        OrSetDelta::Add {
            element: "café☕".to_owned(),
            token: 42u64,
        },
    );
    let bytes = log.to_wire_bytes().unwrap();
    roundtrip(log.clone());
    let mut refused = 0;
    let mut different = 0;
    let mut equal = 0;
    for offset in 0..bytes.len() {
        let mut bad = bytes.clone();
        bad[offset] ^= 1;
        match EventLog::<OrSetDelta<String, u64>>::from_wire_bytes(&bad) {
            Err(_) => refused += 1,
            Ok(decoded) if decoded == log => equal += 1,
            Ok(_) => {
                different += 1;
                println!("surviving hole offset={offset}");
            }
        }
        // Also exhaust all 255 possible nonzero byte XOR masks at each offset.
        for mask in 1..=255u8 {
            bad[offset] = bytes[offset] ^ mask;
            let expected = if offset == 0 {
                WireError::InvalidTag
            } else {
                WireError::IntegrityMismatch
            };
            assert_eq!(
                EventLog::<OrSetDelta<String, u64>>::from_wire_bytes(&bad),
                Err(expected),
                "offset={offset} mask={mask}"
            );
        }
    }
    println!(
        "sweep total={} refused={refused} decoded-different={different} decoded-equal={equal}",
        bytes.len()
    );
    assert_eq!((refused, different, equal), (bytes.len(), 0, 0));
}

#[test]
fn event_log_empty_and_embedded_frames_roundtrip_and_reject_truncation() {
    use safemesh_crdt::WireCursor;
    let empty = EventLog::<GCounterDelta>::with_replica_count(2);
    roundtrip(empty.clone());
    let mut log = EventLog::with_replica_count(2);
    log.append(
        1,
        GCounterDelta {
            replica: 1,
            tally: 7,
        },
    );
    log.append(
        2,
        GCounterDelta {
            replica: 0,
            tally: 9,
        },
    );
    roundtrip(log.clone());
    let mut outer = EventLog::new();
    outer.append(1, log.clone());
    roundtrip(outer);
    let mut stream = vec![99];
    empty.encode_wire(&mut stream).unwrap();
    log.encode_wire(&mut stream).unwrap();
    let mut cursor = WireCursor::new(&stream[1..]);
    assert_eq!(
        EventLog::<GCounterDelta>::decode_wire(&mut cursor).unwrap(),
        empty
    );
    assert_eq!(
        EventLog::<GCounterDelta>::decode_wire(&mut cursor).unwrap(),
        log
    );
    assert!(cursor.is_empty());
    for value in [empty, log] {
        let bytes = value.to_wire_bytes().unwrap();
        for end in 0..bytes.len() {
            assert!(EventLog::<GCounterDelta>::from_wire_bytes(&bytes[..end]).is_err());
        }
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert_eq!(
            EventLog::<GCounterDelta>::from_wire_bytes(&trailing),
            Err(WireError::TrailingBytes)
        );
        let mut legacy_tag = bytes;
        legacy_tag[0] = 0x02;
        assert_eq!(
            EventLog::<GCounterDelta>::from_wire_bytes(&legacy_tag),
            Err(WireError::InvalidTag)
        );
    }
}

#[test]
fn event_log_shape_rejects_wrong_type_even_when_empty_or_nested() {
    use safemesh_crdt::{GCounter, OrSetDelta};
    for populated in [false, true] {
        let mut source = EventLog::for_crdt(&GCounter::new(2));
        if populated {
            source.append(
                0,
                GCounterDelta {
                    replica: 0,
                    tally: 7,
                },
            );
        }
        let bytes = source.to_wire_bytes().unwrap();
        assert_eq!(
            EventLog::<PnCounterDelta>::from_wire_bytes(&bytes),
            Err(WireError::DeltaTypeMismatch)
        );
        let mut outer = EventLog::new();
        if populated {
            outer.append(0, source);
        }
        let bytes = outer.to_wire_bytes().unwrap();
        assert_eq!(
            EventLog::<EventLog<PnCounterDelta>>::from_wire_bytes(&bytes),
            Err(WireError::DeltaTypeMismatch)
        );
    }
    // Remove payloads have identical fields, but distinct element schemas.
    let mut source = EventLog::new();
    source.append(0, OrSetDelta::<u64, u64>::Remove { tokens: vec![7] });
    assert_eq!(
        EventLog::<OrSetDelta<String, u64>>::from_wire_bytes(&source.to_wire_bytes().unwrap()),
        Err(WireError::DeltaTypeMismatch)
    );
}

#[test]
fn event_log_shape_checks_full_domain_including_empty_and_zero() {
    use safemesh_crdt::{GCounter, PnCounter};
    for count in [0, 2, 3] {
        let state = GCounter::new(count);
        let log = EventLog::for_crdt(&state);
        let bytes = log.to_wire_bytes().unwrap();
        assert_eq!(
            EventLog::<GCounterDelta>::from_wire_bytes_for(&bytes, &state),
            Ok(log)
        );
        for other in [0, 2, 3] {
            if count != other {
                assert_eq!(
                    EventLog::<GCounterDelta>::from_wire_bytes_for(&bytes, &GCounter::new(other)),
                    Err(WireError::ReplicaCountMismatch {
                        expected: other,
                        actual: count
                    })
                );
            }
        }
        let pn = PnCounter::new(count);
        let bytes = EventLog::for_crdt(&pn).to_wire_bytes().unwrap();
        assert_eq!(
            EventLog::<PnCounterDelta>::from_wire_bytes_for(&bytes, &PnCounter::new(count + 1)),
            Err(WireError::ReplicaCountMismatch {
                expected: count + 1,
                actual: count
            })
        );
    }
    assert_eq!(
        EventLog::<GCounterDelta>::new().to_wire_bytes(),
        Err(WireError::MissingShape)
    );
    assert_eq!(
        EventLog::<PnCounterDelta>::new().to_wire_bytes(),
        Err(WireError::MissingShape)
    );
}

#[test]
fn event_log_shape_distinguishes_unbounded_from_fixed_zero() {
    let state = EnableWinsFlag::<u64>::new();
    let bytes = EventLog::<EnableWinsFlagDelta<u64>>::with_replica_count(0)
        .to_wire_bytes()
        .unwrap();
    assert_eq!(
        EventLog::<EnableWinsFlagDelta<u64>>::from_wire_bytes_for(&bytes, &state),
        Err(WireError::ArityKindMismatch)
    );
    let log = EventLog::for_crdt(&state);
    let bytes = log.to_wire_bytes().unwrap();
    assert_eq!(
        EventLog::<EnableWinsFlagDelta<u64>>::from_wire_bytes_for(&bytes, &state),
        Ok(log)
    );
}

mod resource_limits {
    use safemesh_crdt::{
        Admission, AppendError, ExportCursor, GSet, LimitedEventLog, OrSetDelta, Record, RecordId,
        ResourceDimension as Dim, ResourceLimit, ResourceLimits, WireDecode, WireEncode, WireError,
    };

    fn record(value: u64, sequence: u64) -> Vec<u8> {
        let mut delta = GSet::new();
        delta.insert(value);
        Record {
            id: RecordId {
                replica: 7,
                sequence,
            },
            delta,
        }
        .to_wire_bytes()
        .unwrap()
    }
    fn refusal(dimension: Dim, limit: usize, requested: usize) -> WireError {
        WireError::ResourceLimit(ResourceLimit {
            dimension,
            limit,
            requested,
        })
    }
    fn set(limits: &mut ResourceLimits, dimension: Dim, n: usize) {
        match dimension {
            Dim::HistoryRecordCount => limits.history_record_count = n,
            Dim::HistoryEncodedBytes => limits.history_encoded_bytes = n,
            Dim::WriterReplicaCount => limits.writer_replica_count = n,
            Dim::PerRecordPayloadBytes => limits.per_record_payload_bytes = n,
            Dim::RecordsPerSyncBatch => limits.records_per_sync_batch = n,
            Dim::BytesPerSyncBatch => limits.bytes_per_sync_batch = n,
            Dim::LiveCarrierEntries => limits.live_carrier_entries = n,
            Dim::RetainedTombstoneCount => limits.retained_tombstone_count = n,
        }
    }
    fn admission_boundary(dimension: Dim, requested: usize, batch: bool) {
        let bytes = record(42, 1);
        for limit in [requested - 1, requested, requested + 1] {
            let mut limits = ResourceLimits::default();
            set(&mut limits, dimension, limit);
            let mut log = LimitedEventLog::<GSet<u64>>::new(None, limits).unwrap();
            let result = if batch {
                log.import_batch(&[&bytes]).map(|r| r[0])
            } else {
                log.admit_wire(&bytes)
            };
            if limit < requested {
                assert_eq!(result, Err(refusal(dimension, limit, requested)));
                assert!(log.log().records().is_empty());
            } else {
                assert_eq!(result, Ok(Admission::Accepted));
                assert_eq!(log.log().records().len(), 1);
            }
        }
    }
    #[test]
    fn history_record_boundary_and_inversion() {
        admission_boundary(Dim::HistoryRecordCount, 1, false);
    }
    #[test]
    fn history_bytes_boundary_and_inversion() {
        let empty = LimitedEventLog::<GSet<u64>>::new(None, ResourceLimits::default()).unwrap();
        admission_boundary(
            Dim::HistoryEncodedBytes,
            empty.history_encoded_bytes() + 4 + record(42, 1).len(),
            false,
        );
    }
    #[test]
    fn writers_boundary_and_inversion() {
        admission_boundary(Dim::WriterReplicaCount, 1, false);
    }
    #[test]
    fn payload_boundary_and_inversion() {
        admission_boundary(Dim::PerRecordPayloadBytes, 13, false);
    }
    #[test]
    fn batch_records_boundary_and_inversion() {
        admission_boundary(Dim::RecordsPerSyncBatch, 1, true);
    }
    #[test]
    fn batch_bytes_boundary_and_inversion() {
        admission_boundary(Dim::BytesPerSyncBatch, record(42, 1).len(), true);
    }
    #[test]
    fn live_entries_boundary_and_inversion() {
        let mut delta = GSet::new();
        delta.insert(42u64);
        let bytes = delta.to_wire_bytes().unwrap();
        for limit in [0, 1, 2] {
            let limits = ResourceLimits {
                live_carrier_entries: limit,
                ..ResourceLimits::default()
            };
            let result = GSet::<u64>::from_wire_bytes_with_limits(&bytes, limits);
            if limit == 0 {
                assert_eq!(result, Err(refusal(Dim::LiveCarrierEntries, 0, 1)));
            } else {
                assert_eq!(result, Ok(delta.clone()));
            }
        }
    }
    #[test]
    fn tombstones_boundary_and_inversion() {
        let delta = OrSetDelta::<u64, u64>::Remove { tokens: vec![42] };
        let bytes = delta.to_wire_bytes().unwrap();
        for limit in [0, 1, 2] {
            let limits = ResourceLimits {
                retained_tombstone_count: limit,
                ..ResourceLimits::default()
            };
            let result = OrSetDelta::<u64, u64>::from_wire_bytes_with_limits(&bytes, limits);
            if limit == 0 {
                assert_eq!(result, Err(refusal(Dim::RetainedTombstoneCount, 0, 1)));
            } else {
                assert_eq!(result, Ok(delta.clone()));
            }
        }
    }
    fn full_log() -> LimitedEventLog<GSet<u64>> {
        let mut log = LimitedEventLog::new(
            None,
            ResourceLimits {
                history_record_count: 1,
                ..ResourceLimits::default()
            },
        )
        .unwrap();
        assert_eq!(log.admit_wire(&record(42, 1)), Ok(Admission::Accepted));
        log
    }
    #[test]
    fn duplicate_fits_after_growth_refusal() {
        let mut log = full_log();
        assert_eq!(
            log.admit_wire(&record(43, 2)),
            Err(refusal(Dim::HistoryRecordCount, 1, 2))
        );
        assert_eq!(
            log.import_batch(&[&record(42, 1)]),
            Ok(vec![Admission::Duplicate])
        );
        assert_eq!(log.log().records().len(), 1);
    }
    #[test]
    fn clone_budget_refuses_before_duplicate_classification() {
        let mut log = full_log();
        let bytes = log.history_encoded_bytes();
        log.set_limits(ResourceLimits {
            history_encoded_bytes: bytes - 1,
            ..log.limits()
        });
        assert_eq!(
            log.import_batch(&[&record(42, 1)]),
            Err(refusal(Dim::HistoryEncodedBytes, bytes - 1, bytes))
        );
        assert_eq!(log.log().records().len(), 1);
    }
    #[test]
    fn hidden_collision_and_false_conflict_are_unclassified() {
        let mut log = full_log();
        log.set_limits(ResourceLimits {
            per_record_payload_bytes: 12,
            ..log.limits()
        });
        for value in [42, 43] {
            assert_eq!(
                log.admit_wire(&record(value, 1)),
                Err(refusal(Dim::PerRecordPayloadBytes, 12, 13))
            );
            assert_eq!(log.log().records().len(), 1);
        }
        log.set_limits(ResourceLimits {
            per_record_payload_bytes: 13,
            ..log.limits()
        });
        assert_eq!(log.admit_wire(&record(42, 1)), Ok(Admission::Duplicate));
        assert_eq!(log.admit_wire(&record(43, 1)), Ok(Admission::Collision));
        assert_eq!(
            log.log().records()[0].delta,
            Record::<GSet<u64>>::from_wire_bytes(&record(42, 1))
                .unwrap()
                .delta
        );
    }
    #[test]
    fn batch_refusal_is_atomic_and_next_append_and_export_work() {
        let mut log = LimitedEventLog::<GSet<u64>>::new(
            None,
            ResourceLimits {
                history_record_count: 1,
                ..ResourceLimits::default()
            },
        )
        .unwrap();
        assert_eq!(
            log.import_batch(&[&record(42, 1), &record(43, 2)]),
            Err(refusal(Dim::HistoryRecordCount, 1, 2))
        );
        assert!(log.log().records().is_empty());
        assert_eq!(log.admit_wire(&record(42, 1)), Ok(Admission::Accepted));
        log.set_limits(ResourceLimits {
            history_encoded_bytes: 0,
            per_record_payload_bytes: 0,
            history_record_count: 0,
            bytes_per_sync_batch: 3,
            ..log.limits()
        });
        let mut cursor = ExportCursor::default();
        let mut recovered = Vec::new();
        while let Some(chunk) = log.export_chunk(cursor).unwrap() {
            assert!(chunk.bytes.len() <= 3);
            assert_ne!(chunk.next, cursor);
            recovered.extend_from_slice(chunk.bytes);
            cursor = chunk.next;
        }
        assert_eq!(recovered, record(42, 1));
        assert_eq!(
            Record::<GSet<u64>>::from_wire_bytes(&recovered).unwrap(),
            log.log().records()[0]
        );
        assert!(log.log().to_wire_bytes().is_ok());
    }
    #[test]
    fn append_typed_refusal_does_not_consume_id() {
        let mut log = LimitedEventLog::<GSet<u64>>::new(
            None,
            ResourceLimits {
                history_record_count: 0,
                ..ResourceLimits::default()
            },
        )
        .unwrap();
        let bytes = record(42, 1);
        let payload = &bytes[21..];
        assert_eq!(
            log.append_wire(7, payload),
            Err(AppendError::ResourceLimit(ResourceLimit {
                dimension: Dim::HistoryRecordCount,
                limit: 0,
                requested: 1
            }))
        );
        log.set_limits(ResourceLimits::default());
        assert_eq!(
            log.append_wire(7, payload),
            Ok(RecordId {
                replica: 7,
                sequence: 1
            })
        );
    }
    #[test]
    fn nested_decoder_inherits_limits_and_frame_is_unchanged() {
        let log = full_log();
        let bytes = log.log().to_wire_bytes().unwrap();
        assert_eq!(bytes.len(), log.history_encoded_bytes());
        let limits = ResourceLimits {
            live_carrier_entries: 0,
            ..ResourceLimits::default()
        };
        assert_eq!(
            safemesh_crdt::EventLog::<GSet<u64>>::from_wire_bytes_with_limits(&bytes, limits),
            Err(refusal(Dim::LiveCarrierEntries, 0, 1))
        );
        let restored = safemesh_crdt::EventLog::<GSet<u64>>::from_wire_bytes_with_limits(
            &bytes,
            ResourceLimits::default(),
        )
        .unwrap();
        assert_eq!(restored.to_wire_bytes().unwrap(), bytes);
    }
    #[test]
    fn advertised_counts_refuse_before_reading_missing_elements() {
        let mut bytes = GSet::<u64>::new().to_wire_bytes().unwrap();
        bytes[1..5].copy_from_slice(&100u32.to_le_bytes());
        let limits = ResourceLimits {
            live_carrier_entries: 1,
            ..ResourceLimits::default()
        };
        assert_eq!(
            GSet::<u64>::from_wire_bytes_with_limits(&bytes, limits),
            Err(refusal(Dim::LiveCarrierEntries, 1, 100))
        );
    }
    #[test]
    fn export_zero_budget_can_resume_after_policy_change() {
        let mut log = full_log();
        log.set_limits(ResourceLimits {
            bytes_per_sync_batch: 0,
            ..log.limits()
        });
        assert_eq!(
            log.export_chunk(ExportCursor::default()),
            Err(refusal(Dim::BytesPerSyncBatch, 0, 1))
        );
        log.set_limits(ResourceLimits {
            bytes_per_sync_batch: 1,
            ..log.limits()
        });
        assert_eq!(
            log.export_chunk(ExportCursor::default())
                .unwrap()
                .unwrap()
                .bytes
                .len(),
            1
        );
        assert_eq!(
            log.export_chunk(ExportCursor {
                record: 9,
                offset: 0
            }),
            Err(WireError::UnexpectedEof)
        );
    }
    #[test]
    fn malformed_or_colliding_batch_preserves_history() {
        let mut log = full_log();
        let saved = log.log().clone();
        assert_eq!(
            log.import_batch(&[&record(42, 1), &record(43, 1)]),
            Err(WireError::RecordCollision)
        );
        assert_eq!(log.log(), &saved);
        assert!(log.import_batch(&[&record(42, 1), &[0]]).is_err());
        assert_eq!(log.log(), &saved);
        assert_eq!(
            log.import_batch(&[&record(42, 1)]),
            Ok(vec![Admission::Duplicate])
        );
    }
    #[test]
    fn retained_operand_is_checked_before_equality() {
        let mut repeated = record(42, 1);
        repeated[17..21].copy_from_slice(&21u32.to_le_bytes());
        repeated[22..26].copy_from_slice(&2u32.to_le_bytes());
        repeated.extend_from_slice(&42u64.to_le_bytes());
        let mut log = LimitedEventLog::<GSet<u64>>::new(None, ResourceLimits::default()).unwrap();
        assert_eq!(log.admit_wire(&repeated), Ok(Admission::Accepted));
        log.set_limits(ResourceLimits {
            per_record_payload_bytes: 13,
            ..log.limits()
        });
        assert_eq!(
            log.admit_wire(&record(42, 1)),
            Err(refusal(Dim::PerRecordPayloadBytes, 13, 21))
        );
        log.set_limits(ResourceLimits::default());
        assert_eq!(log.admit_wire(&record(42, 1)), Ok(Admission::Duplicate));
    }
    #[test]
    fn configured_decode_enforces_history_and_writer_boundaries() {
        let log = full_log();
        let bytes = log.log().to_wire_bytes().unwrap();
        for (dimension, requested) in [
            (Dim::HistoryRecordCount, 1),
            (Dim::HistoryEncodedBytes, bytes.len()),
            (Dim::WriterReplicaCount, 1),
        ] {
            for limit in [requested - 1, requested, requested + 1] {
                let mut limits = ResourceLimits::default();
                set(&mut limits, dimension, limit);
                let result = safemesh_crdt::EventLog::<GSet<u64>>::from_wire_bytes_with_limits(
                    &bytes, limits,
                );
                if limit < requested {
                    assert_eq!(result, Err(refusal(dimension, limit, requested)));
                } else {
                    assert_eq!(result.unwrap(), *log.log());
                }
            }
        }
    }
    #[test]
    fn duplicate_uses_work_budgets_after_collection_growth_ceiling() {
        let mut log = full_log();
        log.set_limits(ResourceLimits {
            live_carrier_entries: 0,
            ..log.limits()
        });
        assert_eq!(log.admit_wire(&record(42, 1)), Ok(Admission::Duplicate));
        assert_eq!(log.admit_wire(&record(43, 1)), Ok(Admission::Collision));
    }
}

#[test]
fn persisted_loading_policy_boundary_and_frame_bytes() {
    use safemesh_crdt::{GCounter, ResourceDimension, ResourceLimit, ResourceLimits};
    let state = GCounter::new(1);
    let mut log = EventLog::for_crdt(&state);
    log.append(
        0,
        GCounterDelta {
            replica: 0,
            tally: 7,
        },
    );
    let bytes = log.to_wire_bytes().unwrap();
    assert_eq!(bytes[0], 0x03);
    let mut limits = ResourceLimits::default();
    limits.history_record_count = 0;
    assert_eq!(
        EventLog::<GCounterDelta>::from_wire_bytes_for_with_limits(&bytes, &state, limits),
        Err(WireError::ResourceLimit(ResourceLimit {
            dimension: ResourceDimension::HistoryRecordCount,
            limit: 0,
            requested: 1
        }))
    );
    limits.history_record_count = 1;
    let loaded =
        EventLog::<GCounterDelta>::from_wire_bytes_for_with_limits(&bytes, &state, limits).unwrap();
    assert_eq!(loaded.to_wire_bytes().unwrap(), bytes);
    assert_eq!(
        EventLog::<GCounterDelta>::from_wire_bytes_for(&bytes, &state)
            .unwrap()
            .to_wire_bytes()
            .unwrap(),
        bytes
    );
    let default_writer_limit = ResourceLimits::default().writer_replica_count;
    let wide_state = GCounter::new(default_writer_limit + 1);
    let wide_bytes = EventLog::for_crdt(&wide_state).to_wire_bytes().unwrap();
    assert_eq!(
        EventLog::<GCounterDelta>::from_wire_bytes_for(&wide_bytes, &wide_state),
        Err(WireError::ResourceLimit(ResourceLimit {
            dimension: ResourceDimension::WriterReplicaCount,
            limit: default_writer_limit,
            requested: default_writer_limit + 1
        }))
    );
    let raised = ResourceLimits {
        writer_replica_count: default_writer_limit + 1,
        ..ResourceLimits::default()
    };
    assert!(EventLog::<GCounterDelta>::from_wire_bytes_for_with_limits(
        &wide_bytes,
        &wide_state,
        raised
    )
    .is_ok());
    limits.history_encoded_bytes = bytes.len() - 1;
    assert!(matches!(
        EventLog::<GCounterDelta>::from_wire_bytes_for_with_limits(&bytes, &state, limits),
        Err(WireError::ResourceLimit(ResourceLimit {
            dimension: ResourceDimension::HistoryEncodedBytes,
            ..
        }))
    ));
}
