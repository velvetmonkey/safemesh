// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Debug;

use safemesh_crdt::{
    EnableWinsFlag, EnableWinsFlagDelta, EventLog, GCounterDelta, GSet, LwwMap, LwwMapDelta,
    LwwRegister, LwwRegisterDelta, OrSet, PnCounterDelta, Record, RecordId, Rga, RgaDelta,
    WireDecode, WireEncode, WireError,
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
fn remaining_collection_counts_are_bounded_before_elements() {
    use safemesh_crdt::CollectionLimits;
    fn planted<
        T: WireEncode + WireDecode + safemesh_crdt::WireSchema + Clone + PartialEq + Debug,
    >(
        value: T,
    ) {
        let bytes = value.to_wire_bytes().unwrap();
        assert_eq!(
            T::from_wire_bytes(&bytes),
            Err(WireError::CollectionElementLimitExceeded { max_elements: 4096 })
        );
        let raised = T::from_wire_bytes_with_collection_limits(
            &bytes,
            CollectionLimits {
                max_elements: Some(4097),
            },
        )
        .unwrap();
        assert_eq!(raised, value);
        assert_eq!(raised.to_wire_bytes().unwrap(), bytes);
        let record = Record {
            id: RecordId {
                replica: 0,
                sequence: 1,
            },
            delta: value,
        };
        let mut log_bytes = Vec::new();
        EventLog::<T>::encode_records(None, &[record], &mut log_bytes).unwrap();
        assert_eq!(
            EventLog::<T>::from_wire_bytes(&log_bytes),
            Err(WireError::CollectionElementLimitExceeded { max_elements: 4096 })
        );
        let log = EventLog::<T>::from_wire_bytes_with_limits(
            &log_bytes,
            safemesh_crdt::DecodeLimits {
                max_records: Some(1),
                max_collection_elements: Some(4097),
            },
        )
        .unwrap();
        assert_eq!(log.to_wire_bytes().unwrap(), log_bytes);
    }
    let tokens: Vec<u64> = (0..4097).collect();
    planted(safemesh_crdt::OrSetDelta::<u64, u64>::Remove {
        tokens: tokens.clone(),
    });
    planted(safemesh_crdt::OrSetDelta::<String, u64>::Remove {
        tokens: tokens.clone(),
    });
    planted(EnableWinsFlagDelta::<u64>::Disable {
        tokens: tokens.clone(),
    });

    let mut set = OrSet::<u64, u64>::new();
    let mut string_set = OrSet::<String, u64>::new();
    let mut flag = EnableWinsFlag::<u64>::new();
    let mut map = LwwMap::<u64, u64>::new();
    for token in 0..4097 {
        set.add(token, token);
        string_set.add(token.to_string(), token);
        flag.enable(token);
        map.set(token, token, 0, token);
    }
    planted(set);
    planted(string_set);
    planted(flag);
    planted(map);

    let mut set = OrSet::<u64, u64>::new();
    let mut string_set = OrSet::<String, u64>::new();
    let mut flag = EnableWinsFlag::<u64>::new();
    let mut map = LwwMap::<u64, u64>::new();
    for token in 0..4097 {
        set.apply_remove([token]);
        string_set.apply_remove([token]);
        flag.disable([token]);
        map.remove(token, token, 0);
    }
    planted(set);
    planted(string_set);
    planted(flag);
    planted(map);
}

#[test]
fn outer_record_budget_reaches_nested_event_logs() {
    use safemesh_crdt::{DecodeError, DecodeLimits};
    let inner_records: Vec<_> = (1..=2)
        .map(|sequence| Record {
            id: RecordId {
                replica: 0,
                sequence,
            },
            delta: GCounterDelta {
                replica: 0,
                tally: sequence,
            },
        })
        .collect();
    let mut inner_bytes = Vec::new();
    EventLog::encode_records(Some(1), &inner_records, &mut inner_bytes).unwrap();
    let inner = EventLog::<GCounterDelta>::from_wire_bytes(&inner_bytes).unwrap();
    let outer_records = [Record {
        id: RecordId {
            replica: 1,
            sequence: 1,
        },
        delta: inner,
    }];
    let mut outer_bytes = Vec::new();
    EventLog::encode_records(None, &outer_records, &mut outer_bytes).unwrap();
    assert_eq!(
        EventLog::<EventLog<GCounterDelta>>::from_wire_bytes_with_limits(
            &outer_bytes,
            DecodeLimits {
                max_records: Some(1),
                max_collection_elements: None
            }
        ),
        Err(DecodeError::Wire(WireError::NestedRecordLimitExceeded {
            max_records: 1
        }))
    );
    assert!(
        EventLog::<EventLog<GCounterDelta>>::from_wire_bytes_with_limits(
            &outer_bytes,
            DecodeLimits {
                max_records: Some(2),
                max_collection_elements: None
            }
        )
        .is_ok()
    );
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
fn gset_and_rga_collection_limits() {
    use safemesh_crdt::{CollectionLimits, WireError};
    let count = 4097u32;
    let raised = CollectionLimits {
        max_elements: Some(count as usize),
    };
    let mut bytes = vec![0x20];
    bytes.extend_from_slice(&count.to_le_bytes());
    for value in 0..u64::from(count) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    assert_eq!(
        GSet::<u64>::from_wire_bytes(&bytes),
        Err(WireError::CollectionElementLimitExceeded { max_elements: 4096 })
    );
    let decoded = GSet::<u64>::from_wire_bytes_with_limits(&bytes, raised).unwrap();
    assert_eq!(decoded.elements().len(), count as usize);
    assert_eq!(decoded.to_wire_bytes().unwrap(), bytes);
    let mut local = GSet::new();
    local.insert(9000);
    local.merge(&decoded);
    assert!(local.contains(&9000) && local.contains(&4096));

    let record = Record {
        id: RecordId {
            replica: 7,
            sequence: 1,
        },
        delta: decoded,
    };
    let record_bytes = record.to_wire_bytes().unwrap();
    assert_eq!(
        Record::<GSet<u64>>::from_wire_bytes(&record_bytes),
        Err(WireError::CollectionElementLimitExceeded { max_elements: 4096 })
    );
    let mut log_bytes = Vec::new();
    EventLog::encode_records(None, &[record], &mut log_bytes).unwrap();
    assert_eq!(
        EventLog::<GSet<u64>>::from_wire_bytes(&log_bytes),
        Err(WireError::CollectionElementLimitExceeded { max_elements: 4096 })
    );

    let mut placed = vec![0x40];
    placed.extend_from_slice(&count.to_le_bytes());
    for value in 0..u64::from(count) {
        placed.extend_from_slice(&value.to_le_bytes());
        placed.extend_from_slice(&value.to_le_bytes());
    }
    placed.extend_from_slice(&0u32.to_le_bytes());
    assert_eq!(
        Rga::<u64, u64>::from_wire_bytes(&placed),
        Err(WireError::CollectionElementLimitExceeded { max_elements: 4096 })
    );
    assert_eq!(
        Rga::<u64, u64>::from_wire_bytes_with_limits(&placed, raised)
            .unwrap()
            .to_wire_bytes()
            .unwrap(),
        placed
    );

    let mut tombstones = vec![0x40];
    tombstones.extend_from_slice(&0u32.to_le_bytes());
    tombstones.extend_from_slice(&count.to_le_bytes());
    for position in 0..u64::from(count) {
        tombstones.extend_from_slice(&position.to_le_bytes());
    }
    assert_eq!(
        Rga::<u64, u64>::from_wire_bytes(&tombstones),
        Err(WireError::CollectionElementLimitExceeded { max_elements: 4096 })
    );
    assert_eq!(
        Rga::<u64, u64>::from_wire_bytes_with_limits(&tombstones, raised)
            .unwrap()
            .to_wire_bytes()
            .unwrap(),
        tombstones
    );
}

#[test]
fn event_log_nested_collection_budget_preserves_persisted_bytes() {
    use safemesh_crdt::{Crdt, DecodeError, DecodeLimits, MergeError, Mergeable, WireError};
    struct SetCarrier(GSet<u64>);
    impl Mergeable for SetCarrier {
        fn merge(&mut self, other: &Self) -> Result<(), MergeError> {
            self.0.merge(&other.0);
            Ok(())
        }
    }
    impl Crdt for SetCarrier {
        type Delta = GSet<u64>;
        fn validate_record(&self, _: RecordId, _: &Self::Delta) -> Result<(), WireError> {
            Ok(())
        }
        fn apply_delta(&mut self, delta: Self::Delta) {
            self.0.merge(&delta);
        }
    }
    let mut state = GSet::new();
    for value in 0..5000 {
        state.insert(value);
    }
    let record = Record {
        id: RecordId {
            replica: 7,
            sequence: 1,
        },
        delta: state.clone(),
    };
    let mut persisted = Vec::new();
    EventLog::encode_records(None, &[record], &mut persisted).unwrap();
    assert_eq!(persisted.len(), 40076);
    let default_error =
        DecodeError::Wire(WireError::CollectionElementLimitExceeded { max_elements: 4096 });
    assert_eq!(
        EventLog::<GSet<u64>>::from_wire_bytes(&persisted),
        Err(WireError::CollectionElementLimitExceeded { max_elements: 4096 })
    );
    assert_eq!(
        EventLog::<GSet<u64>>::from_wire_bytes_with_limits(&persisted, DecodeLimits::default()),
        Err(default_error)
    );
    assert_eq!(
        EventLog::<GSet<u64>>::from_wire_bytes_with_limits(
            &persisted,
            DecodeLimits {
                max_records: Some(1),
                max_collection_elements: Some(4999)
            }
        ),
        Err(DecodeError::Wire(
            WireError::CollectionElementLimitExceeded { max_elements: 4999 }
        ))
    );
    let loaded = EventLog::<GSet<u64>>::from_wire_bytes_with_limits(
        &persisted,
        DecodeLimits {
            max_records: Some(1),
            max_collection_elements: Some(5000),
        },
    )
    .unwrap();
    assert_eq!(loaded.records().len(), 1);
    let destination = SetCarrier(GSet::new());
    assert_eq!(
        EventLog::<GSet<u64>>::from_wire_bytes_for(&persisted, &destination),
        Err(WireError::CollectionElementLimitExceeded { max_elements: 4096 })
    );
    assert_eq!(
        EventLog::<GSet<u64>>::from_wire_bytes_for_with_limits(
            &persisted,
            &destination,
            DecodeLimits {
                max_records: Some(1),
                max_collection_elements: Some(5000)
            },
        )
        .unwrap()
        .to_wire_bytes()
        .unwrap(),
        persisted
    );
    assert_eq!(
        EventLog::<GSet<u64>>::records_from_wire_bytes_for_with_limits(
            &persisted,
            &destination,
            DecodeLimits {
                max_records: Some(1),
                max_collection_elements: Some(5000)
            },
        )
        .unwrap()
        .len(),
        1
    );
    let mut replayed = GSet::new();
    for record in loaded.records() {
        replayed.merge(&record.delta);
    }
    assert_eq!(replayed, state);
    assert_eq!(loaded.to_wire_bytes().unwrap(), persisted);
    assert_eq!(
        EventLog::<GSet<u64>>::from_wire_bytes_with_limits(
            &persisted,
            DecodeLimits {
                max_records: Some(0),
                max_collection_elements: Some(5000)
            }
        ),
        Err(DecodeError::RecordLimitExceeded { max_records: 0 })
    );
}

#[test]
fn event_log_rga_payload_uses_collection_budget() {
    use safemesh_crdt::{DecodeError, DecodeLimits};
    let mut state = Rga::<u64, u64>::new();
    for value in 0..5000 {
        state.insert(value, value);
        state.delete(value);
    }
    let records = [Record {
        id: RecordId {
            replica: 7,
            sequence: 1,
        },
        delta: state.clone(),
    }];
    let mut bytes = Vec::new();
    EventLog::encode_records(None, &records, &mut bytes).unwrap();
    assert_eq!(
        EventLog::<Rga<u64, u64>>::from_wire_bytes(&bytes),
        Err(WireError::CollectionElementLimitExceeded { max_elements: 4096 })
    );
    let loaded = EventLog::<Rga<u64, u64>>::from_wire_bytes_with_limits(
        &bytes,
        DecodeLimits {
            max_records: Some(1),
            max_collection_elements: Some(5000),
        },
    )
    .unwrap();
    assert_eq!(loaded.records()[0].delta, state);
    assert_eq!(loaded.to_wire_bytes().unwrap(), bytes);
    assert_eq!(
        EventLog::<Rga<u64, u64>>::from_wire_bytes_with_limits(
            &bytes,
            DecodeLimits {
                max_records: Some(1),
                max_collection_elements: Some(4999)
            }
        ),
        Err(DecodeError::Wire(
            WireError::CollectionElementLimitExceeded { max_elements: 4999 }
        ))
    );
}

#[test]
fn rga_delta_variants_roundtrip_and_refuse_corruption() {
    for delta in [
        RgaDelta::Insert {
            position: 30u64,
            value: 3u64,
        },
        RgaDelta::Delete { position: 30 },
    ] {
        let bytes = delta.to_wire_bytes().unwrap();
        assert_eq!(
            RgaDelta::<u64, u64>::from_wire_bytes(&bytes),
            Ok(delta.clone())
        );
        assert_eq!(
            RgaDelta::<u64, u64>::from_wire_bytes(&bytes[..bytes.len() - 1]),
            Err(WireError::UnexpectedEof)
        );
        let mut invalid_tag = bytes;
        invalid_tag[0] = 0xff;
        assert_eq!(
            RgaDelta::<u64, u64>::from_wire_bytes(&invalid_tag),
            Err(WireError::InvalidTag)
        );
    }
}

fn assert_rga_vector<T>(name: &str, value: T, bytes: &[u8])
where
    T: WireEncode + WireDecode + PartialEq + Debug,
{
    assert_eq!(
        value.to_wire_bytes().unwrap(),
        bytes,
        "{name} encoder drift"
    );
    assert_eq!(T::from_wire_bytes(bytes), Ok(value), "{name} decode drift");
}

#[test]
fn rga_delta_u64_committed_vectors_are_canonical() {
    let insert = RgaDelta::Insert {
        position: 30u64,
        value: 3u64,
    };
    let delete = RgaDelta::<u64, u64>::Delete { position: 30 };
    let max = RgaDelta::Insert {
        position: u64::MAX,
        value: u64::MAX,
    };
    let record = Record {
        id: RecordId {
            replica: 9,
            sequence: 4,
        },
        delta: insert.clone(),
    };
    let mut log = EventLog::new();
    let mut state = Rga::new();
    log.append(&mut state, 9, insert.clone()).unwrap();
    log.append(&mut state, 9, delete.clone()).unwrap();
    assert_eq!(log.records().len(), 2);
    assert_eq!(log.records()[0].delta, insert);
    assert_eq!(log.records()[1].delta, delete);

    assert_rga_vector(
        "empty.log",
        EventLog::<RgaDelta<u64, u64>>::new(),
        include_bytes!("fixtures/rga-delta-u64/empty.log"),
    );
    assert_rga_vector(
        "insert.delta",
        insert,
        include_bytes!("fixtures/rga-delta-u64/insert.delta"),
    );
    assert_rga_vector(
        "delete.delta",
        delete,
        include_bytes!("fixtures/rga-delta-u64/delete.delta"),
    );
    assert_rga_vector(
        "max.delta",
        max,
        include_bytes!("fixtures/rga-delta-u64/max.delta"),
    );
    assert_rga_vector(
        "insert.record",
        record,
        include_bytes!("fixtures/rga-delta-u64/insert.record"),
    );
    assert_rga_vector(
        "sequence.log",
        log,
        include_bytes!("fixtures/rga-delta-u64/sequence.log"),
    );
}

#[test]
fn event_log_roundtrips_and_preserves_deduped_records() {
    let mut log = EventLog::with_replica_count(2);
    log.append(
        &mut safemesh_crdt::PnCounter::new(2),
        1,
        PnCounterDelta::Inc {
            replica: 1,
            tally: 5,
        },
    )
    .unwrap();
    log.append(
        &mut safemesh_crdt::PnCounter::new(2),
        1,
        PnCounterDelta::Dec {
            replica: 1,
            tally: 2,
        },
    )
    .unwrap();

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
        log.insert_record(&EnableWinsFlag::new(), record.clone()),
        safemesh_crdt::Admission::Accepted
    );
    let bytes = log.to_wire_bytes().unwrap();
    let reopened = EventLog::<EnableWinsFlagDelta<u64>>::from_wire_bytes(&bytes).unwrap();
    assert_eq!(reopened.records()[0].delta, record.delta);
    assert_eq!(
        log.admit_with(
            &mut EnableWinsFlag::new(),
            reopened.records()[0].clone(),
            |_, _| {}
        ),
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
        Err(WireError::CollectionElementLimitExceeded { max_elements: 4096 }),
    );
}

#[test]
fn orset_delta_roundtrips_in_record_and_event_log() {
    let mut log = EventLog::new();
    for delta in orset_delta_cases() {
        let id = log
            .append(&mut safemesh_crdt::OrSet::new(), 1, delta.clone())
            .unwrap();
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
            assert_eq!(
                result.unwrap(),
                Err(if tag == 0x34 && length > 4096 {
                    WireError::CollectionElementLimitExceeded { max_elements: 4096 }
                } else {
                    WireError::UnexpectedEof
                })
            );
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
        let id = log
            .append(&mut safemesh_crdt::OrSet::new(), 1, delta.clone())
            .unwrap();
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
        &mut safemesh_crdt::OrSet::new(),
        1,
        OrSetDelta::Add {
            element: "café☕".to_owned(),
            token: 42u64,
        },
    )
    .unwrap();
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
        &mut safemesh_crdt::GCounter::new(2),
        1,
        GCounterDelta {
            replica: 1,
            tally: 7,
        },
    )
    .unwrap();
    let mut records = log.records().to_vec();
    records.push(Record {
        id: safemesh_crdt::RecordId {
            replica: 2,
            sequence: 1,
        },
        delta: GCounterDelta {
            replica: 0,
            tally: 9,
        },
    });
    let log = wire_log(Some(2), &records);
    roundtrip(log.clone());
    let outer = wire_log(
        None,
        &[Record {
            id: safemesh_crdt::RecordId {
                replica: 1,
                sequence: 1,
            },
            delta: log.clone(),
        }],
    );
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
            source
                .append(
                    &mut GCounter::new(2),
                    0,
                    GCounterDelta {
                        replica: 0,
                        tally: 7,
                    },
                )
                .unwrap();
        }
        let bytes = source.to_wire_bytes().unwrap();
        assert_eq!(
            EventLog::<PnCounterDelta>::from_wire_bytes(&bytes),
            Err(WireError::DeltaTypeMismatch)
        );
        let records = if populated {
            vec![Record {
                id: safemesh_crdt::RecordId {
                    replica: 0,
                    sequence: 1,
                },
                delta: source,
            }]
        } else {
            vec![]
        };
        let outer = wire_log(None, &records);
        let bytes = outer.to_wire_bytes().unwrap();
        assert_eq!(
            EventLog::<EventLog<PnCounterDelta>>::from_wire_bytes(&bytes),
            Err(WireError::DeltaTypeMismatch)
        );
    }
    // Remove payloads have identical fields, but distinct element schemas.
    let mut source = EventLog::new();
    source
        .append(
            &mut safemesh_crdt::OrSet::new(),
            0,
            OrSetDelta::<u64, u64>::Remove { tokens: vec![7] },
        )
        .unwrap();
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
    // Decoding binds the wire's declared unbounded domain. The in-memory
    // binding flag must not alter equality, including when a log is a payload.
    let empty = EventLog::<EnableWinsFlagDelta<u64>>::new();
    roundtrip(empty.clone());
    roundtrip(Record {
        id: RecordId {
            replica: 0,
            sequence: 1,
        },
        delta: empty,
    });
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

// Inert codec fixtures can contain data which no live CRDT may admit.
fn wire_log<D: WireEncode + WireDecode + safemesh_crdt::WireSchema + PartialEq>(
    shape: Option<usize>,
    records: &[Record<D>],
) -> EventLog<D> {
    let mut bytes = Vec::new();
    EventLog::encode_records(shape, records, &mut bytes).unwrap();
    EventLog::from_wire_bytes(&bytes).unwrap()
}

std::thread_local! {
    static BUDGET_PAYLOAD_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BudgetPayload(u64);

impl safemesh_crdt::WireSchema for BudgetPayload {
    fn wire_schema() -> std::borrow::Cow<'static, [u8]> {
        std::borrow::Cow::Borrowed(b"test/decode-budget/v1")
    }
}

impl WireEncode for BudgetPayload {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        out.extend_from_slice(&self.0.to_le_bytes());
        Ok(())
    }
}

impl WireDecode for BudgetPayload {
    fn decode_wire(cursor: &mut safemesh_crdt::WireCursor<'_>) -> Result<Self, WireError> {
        BUDGET_PAYLOAD_READS.with(|reads| reads.set(reads.get() + 1));
        Ok(Self(cursor.read_u64()?))
    }
}

fn budget_frame(count: usize, duplicate: bool) -> (Vec<u8>, Vec<Record<BudgetPayload>>) {
    let records: Vec<_> = (0..count)
        .map(|i| Record {
            id: RecordId {
                replica: 0,
                sequence: if duplicate { 1 } else { i as u64 + 1 },
            },
            delta: BudgetPayload(if duplicate { 0 } else { i as u64 }),
        })
        .collect();
    let mut bytes = Vec::new();
    EventLog::encode_records(None, &records, &mut bytes).unwrap();
    (bytes, records)
}

#[test]
fn decode_budget_compatibility_control() {
    let (bytes, records) = budget_frame(1024, false);
    BUDGET_PAYLOAD_READS.with(|reads| reads.set(0));
    let decoded = EventLog::<BudgetPayload>::from_wire_bytes(&bytes).unwrap();
    assert_eq!(decoded.records(), records);
    assert_eq!(decoded.to_wire_bytes().unwrap(), bytes);
    assert_eq!(BUDGET_PAYLOAD_READS.with(|reads| reads.get()), 1024);
}

#[test]
fn decode_budget_stops_planted_frame() {
    let (bytes, _) = budget_frame(1024, false);
    BUDGET_PAYLOAD_READS.with(|reads| reads.set(0));
    let result = EventLog::<BudgetPayload>::from_wire_bytes_with_limits(
        &bytes,
        safemesh_crdt::DecodeLimits {
            max_records: Some(8),
            ..Default::default()
        },
    );
    assert_eq!(
        result,
        Err(safemesh_crdt::DecodeError::RecordLimitExceeded { max_records: 8 })
    );
    assert_eq!(BUDGET_PAYLOAD_READS.with(|reads| reads.get()), 8);
}

#[test]
fn decode_budget_boundaries_duplicates_and_default() {
    use safemesh_crdt::{DecodeError, DecodeLimits};
    for (count, duplicate) in [(0, false), (1, false), (1024, false), (1024, true)] {
        let (bytes, _) = budget_frame(count, duplicate);
        let old = EventLog::<BudgetPayload>::from_wire_bytes(&bytes).unwrap();
        for limit in [None, Some(count), Some(count + 1)] {
            BUDGET_PAYLOAD_READS.with(|reads| reads.set(0));
            let decoded = EventLog::<BudgetPayload>::from_wire_bytes_with_limits(
                &bytes,
                DecodeLimits {
                    max_records: limit,
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(decoded, old);
            assert_eq!(decoded.to_wire_bytes(), old.to_wire_bytes());
            assert_eq!(BUDGET_PAYLOAD_READS.with(|reads| reads.get()), count);
        }
        if count > 0 {
            for max_records in [0, count - 1] {
                BUDGET_PAYLOAD_READS.with(|reads| reads.set(0));
                assert_eq!(
                    EventLog::<BudgetPayload>::from_wire_bytes_with_limits(
                        &bytes,
                        DecodeLimits {
                            max_records: Some(max_records),
                            ..Default::default()
                        }
                    ),
                    Err(DecodeError::RecordLimitExceeded { max_records })
                );
                assert_eq!(BUDGET_PAYLOAD_READS.with(|reads| reads.get()), max_records);
            }
        }
        let mut trailing = bytes.clone();
        trailing.push(0);
        let mut corrupt = bytes.clone();
        *corrupt.last_mut().unwrap() ^= 1;
        for input in [&bytes[..bytes.len() - 1], &trailing, &corrupt, &[0]] {
            assert_eq!(
                EventLog::<BudgetPayload>::from_wire_bytes_with_limits(
                    input,
                    DecodeLimits::default()
                ),
                EventLog::<BudgetPayload>::from_wire_bytes(input).map_err(DecodeError::Wire)
            );
        }
        BUDGET_PAYLOAD_READS.with(|reads| reads.set(0));
        assert_eq!(
            EventLog::<BudgetPayload>::from_wire_bytes_with_limits(
                &corrupt,
                DecodeLimits {
                    max_records: Some(0),
                    ..Default::default()
                }
            ),
            Err(DecodeError::Wire(WireError::IntegrityMismatch))
        );
        assert_eq!(BUDGET_PAYLOAD_READS.with(|reads| reads.get()), 0);
    }
}

#[test]
fn orset_utf8_state_roundtrips_canonically_with_tombstones() {
    roundtrip(OrSet::<String, u64>::new());
    let entries = ["", "ASCII", "é", "東京", "🦀", "e\u{301}", "a\0b"];
    let mut state = OrSet::new();
    for (token, element) in entries.iter().enumerate() {
        state.add(element.to_string(), token as u64);
    }
    state.add("ASCII".to_string(), u64::MAX);
    state.apply_remove([1, 99]);
    let mut reversed = OrSet::new();
    reversed.apply_remove([99, 1]);
    reversed.add("ASCII".to_string(), u64::MAX);
    for (token, element) in entries.iter().enumerate().rev() {
        reversed.add(element.to_string(), token as u64);
    }
    assert_eq!(state.to_wire_bytes(), reversed.to_wire_bytes());
    roundtrip(state.clone());
    roundtrip(Record {
        id: RecordId {
            replica: 1,
            sequence: 1,
        },
        delta: state.clone(),
    });
    let log = wire_log(
        None,
        &[Record {
            id: RecordId {
                replica: 1,
                sequence: 1,
            },
            delta: state,
        }],
    );
    roundtrip(log.clone());
    assert_eq!(
        EventLog::<OrSet<u64, u64>>::from_wire_bytes(&log.to_wire_bytes().unwrap()),
        Err(WireError::DeltaTypeMismatch)
    );
}

#[test]
fn orset_utf8_state_rejects_corruption_and_other_tags() {
    let mut state = OrSet::new();
    state.add("é".to_string(), 1);
    state.add("🦀".to_string(), 2);
    let original = state.to_wire_bytes().unwrap();
    // Tag + u32 add count precede the first string's u32 byte length.
    let mut scratch = original.clone();
    assert_eq!(scratch[5], 2);
    scratch[5] = 1;
    assert_eq!(
        OrSet::<String, u64>::from_wire_bytes(&scratch),
        Err(WireError::InvalidUtf8)
    );
    scratch[5] = original[5];
    assert_eq!(OrSet::<String, u64>::from_wire_bytes(&scratch), Ok(state));
    scratch[5] = 255;
    assert_eq!(
        OrSet::<String, u64>::from_wire_bytes(&scratch),
        Err(WireError::UnexpectedEof)
    );
    for end in 0..original.len() {
        assert!(OrSet::<String, u64>::from_wire_bytes(&original[..end]).is_err());
    }
    let mut trailing = original.clone();
    trailing.push(0);
    assert_eq!(
        OrSet::<String, u64>::from_wire_bytes(&trailing),
        Err(WireError::TrailingBytes)
    );
    for tag in 0..=u8::MAX {
        if tag != original[0] {
            let mut bad = original.clone();
            bad[0] = tag;
            assert_eq!(
                OrSet::<String, u64>::from_wire_bytes(&bad),
                Err(WireError::InvalidTag)
            );
        }
    }
    assert_eq!(
        OrSet::<u64, u64>::from_wire_bytes(&original),
        Err(WireError::InvalidTag)
    );
    assert_eq!(
        safemesh_crdt::OrSetDelta::<String, u64>::from_wire_bytes(&original),
        Err(WireError::InvalidTag)
    );
}

#[test]
fn orset_utf8_state_rejects_duplicate_pairs_without_changing_receiver() {
    fn frame(entries: &[(&str, u64)]) -> Vec<u8> {
        let mut bytes = vec![0x35];
        bytes.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        for (element, token) in entries {
            bytes.extend_from_slice(&(element.len() as u32).to_le_bytes());
            bytes.extend_from_slice(element.as_bytes());
            bytes.extend_from_slice(&token.to_le_bytes());
        }
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes
    }

    let mut receiver = OrSet::<String, u64>::new();
    receiver.add("preserved".into(), 41);
    let before = receiver.clone();
    for entries in [
        vec![("repeat", 7), ("repeat", 7)],
        vec![("repeat", 7), ("other", 9), ("repeat", 7)],
        vec![("🦀", u64::MAX), ("🦀", u64::MAX)],
    ] {
        let decoded = OrSet::<String, u64>::from_wire_bytes(&frame(&entries));
        if let Ok(state) = &decoded {
            receiver.merge(state);
        }
        assert_eq!(decoded, Err(WireError::DuplicateEntry), "{entries:?}");
        assert_eq!(receiver, before);
    }

    for entries in [
        vec![("same", 1), ("same", 2)],
        vec![("first", 1), ("second", 1)],
    ] {
        let decoded = OrSet::<String, u64>::from_wire_bytes(&frame(&entries)).unwrap();
        assert_eq!(decoded.adds().len(), 2);
        assert_eq!(decoded.to_wire_bytes().unwrap(), frame(&entries));
    }
}

proptest::proptest! {
    #[test]
    fn orset_utf8_state_preserves_arbitrary_strings(
        entries in proptest::collection::vec((proptest::prelude::any::<String>(), proptest::prelude::any::<u64>()), 0..32),
        tombstones in proptest::collection::vec(proptest::prelude::any::<u64>(), 0..32),
        left in proptest::prelude::any::<String>(),
        right in proptest::prelude::any::<String>(),
    ) {
        let mut state = OrSet::new();
        for (element, token) in entries { state.add(element, token); }
        state.apply_remove(tombstones);
        roundtrip(state);
        let mut a = OrSet::new();
        a.add(left.clone(), 7);
        let mut b = OrSet::new();
        b.add(right.clone(), 7);
        proptest::prop_assert_eq!(a.to_wire_bytes().unwrap() == b.to_wire_bytes().unwrap(), left == right);
    }
}
