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
