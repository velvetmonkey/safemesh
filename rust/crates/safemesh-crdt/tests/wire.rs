// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fmt::Debug;

use safemesh_crdt::{
    EventLog, GCounterDelta, GSet, LwwRegister, LwwRegisterDelta, OrSet, PnCounterDelta, Record,
    RecordId, Rga, WireDecode, WireEncode, WireError,
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
