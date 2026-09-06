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
    let mut log = EventLog::new();
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
            0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // sorted token
            0x09, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // sorted token
        ],
    );
    roundtrip(EnableWinsFlagDelta::Disable { tokens: vec![2, 9] });
    assert_eq!(
        EnableWinsFlagDelta::Disable {
            tokens: vec![9, 2, 2]
        }
        .to_wire_bytes()
        .unwrap(),
        disable.to_wire_bytes().unwrap(),
    );

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
