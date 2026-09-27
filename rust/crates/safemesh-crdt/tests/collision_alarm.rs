// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

//! The record-ID collision alarm: a fork under one ID raises a typed report
//! on both replicas, an equal payload stays a silent Duplicate, and every
//! input the report decoder handles has a named outcome.

use safemesh_crdt::{
    anti_entropy, Admission, CollisionReport, CollisionVerdict, DecodeError, DecodeLimits,
    EventLog, GCounter, GCounterDelta, GSet, InMemoryTransport, LegacyFrame, Record,
    RecordCollision, RecordId, Replica, ReplicaError, TransportAdapter, VersionVector, WireDecode,
    WireEncode, WireError,
};

const ID: RecordId = RecordId {
    replica: 1,
    sequence: 1,
};

fn bump(tally: u64) -> GCounterDelta {
    GCounterDelta { replica: 1, tally }
}

/// Two replicas that each admitted `ID` for author 1: tallies 5 and 9.
fn forked() -> (Replica<GCounter>, Replica<GCounter>) {
    let mut left = Replica::new(GCounter::new(2));
    let mut right = Replica::new(GCounter::new(2));
    assert_eq!(left.append(1, bump(5)).unwrap().id, ID);
    assert_eq!(right.append(1, bump(9)).unwrap().id, ID);
    (left, right)
}

// The anti-entropy exchange: `from` offers its log, `to` merges it and
// returns its collision report, which `from` merges.
fn exchange(
    from: &mut Replica<GCounter>,
    to: &mut Replica<GCounter>,
) -> (Vec<Admission>, Option<Vec<(RecordId, CollisionVerdict)>>) {
    let admissions = to
        .merge_log_bytes(&from.log_bytes().unwrap(), DecodeLimits::default())
        .unwrap();
    let verdicts = to.collision_report_bytes().unwrap().map(|report| {
        from.merge_collision_report_bytes(&report, DecodeLimits::default())
            .unwrap()
    });
    (admissions, verdicts)
}

#[test]
fn two_replica_fork_raises_the_alarm_on_both_sides() {
    let (mut left, mut right) = forked();
    // The measured fork: equal versions, nothing to offer either way.
    assert_eq!(left.state().state(), &[0, 5]);
    assert_eq!(right.state().state(), &[0, 9]);
    assert_eq!(left.version(), right.version());
    assert!(left.since(right.version()).is_empty());
    assert!(right.since(left.version()).is_empty());
    assert!(left.collisions().is_empty() && right.collisions().is_empty());

    // Right offers its record; left refuses it and reports back.
    let (admissions, verdicts) = exchange(&mut right, &mut left);
    assert_eq!(admissions, vec![Admission::Collision]);
    assert_eq!(verdicts, Some(vec![(ID, CollisionVerdict::Recorded)]));

    assert_eq!(
        left.collisions(),
        vec![RecordCollision {
            id: ID,
            local: bump(5),
            remote: bump(9),
        }]
    );
    assert_eq!(
        right.collisions(),
        vec![RecordCollision {
            id: ID,
            local: bump(9),
            remote: bump(5),
        }]
    );
    // The alarm names the fork; it never merges either payload.
    assert_eq!(left.state().state(), &[0, 5]);
    assert_eq!(right.state().state(), &[0, 9]);
    assert_eq!(left.log().records().len(), 1);
    assert_eq!(right.log().records().len(), 1);

    // Repeating the exchange either way is idempotent.
    assert_eq!(
        exchange(&mut right, &mut left),
        (
            vec![Admission::Collision],
            Some(vec![(ID, CollisionVerdict::Known)])
        )
    );
    assert_eq!(
        exchange(&mut left, &mut right),
        (
            vec![Admission::Collision],
            Some(vec![(ID, CollisionVerdict::Known)])
        )
    );
    assert_eq!(left.collisions().len(), 1);
    assert_eq!(right.collisions().len(), 1);
}

#[test]
fn the_in_memory_anti_entropy_helper_raises_the_receivers_alarm() {
    let (left, mut right) = forked();
    let mut transport = InMemoryTransport::new();
    transport.subscribe(1);
    transport.subscribe(2);
    // A stale version for the receiver makes the helper offer the record.
    transport.send(1, 2, left.log().records().to_vec()).unwrap();
    let admissions = anti_entropy(&mut transport, 1, 2, left.log(), &mut right).unwrap();
    assert_eq!(admissions, vec![(1, ID, Admission::Collision)]);
    assert_eq!(right.collisions()[0].remote, bump(5));
    assert!(right.collision_report_bytes().unwrap().is_some());
}

#[test]
fn equal_payload_stays_duplicate_with_no_alarm() {
    let mut left = Replica::new(GCounter::new(2));
    let mut right = Replica::new(GCounter::new(2));
    left.append(1, bump(5)).unwrap();
    right.append(1, bump(5)).unwrap();
    for _ in 0..2 {
        assert_eq!(
            exchange(&mut right, &mut left),
            (vec![Admission::Duplicate], None)
        );
        assert_eq!(
            exchange(&mut left, &mut right),
            (vec![Admission::Duplicate], None)
        );
    }
    assert!(left.collisions().is_empty() && right.collisions().is_empty());
    assert_eq!(left.collision_report_bytes().unwrap(), None);
    assert_eq!(left.state(), right.state());
    assert_eq!(left.state().state(), &[0, 5]);
    assert_eq!(left.log(), right.log());
}

#[test]
fn every_admission_path_raises_the_same_alarm() {
    let offered = Record {
        id: ID,
        delta: bump(9),
    };
    let mut replica = forked().0;
    assert_eq!(
        replica.merge_record_bytes(
            &offered.to_wire_bytes().unwrap(),
            safemesh_crdt::CollectionLimits::WIRE_DEFAULT
        ),
        Ok(Admission::Collision)
    );
    assert_eq!(replica.collisions().len(), 1);

    let state = GCounter::new(2);
    let mut log = EventLog::for_crdt(&state);
    assert_eq!(
        log.insert_record(
            &state,
            Record {
                id: ID,
                delta: bump(5)
            }
        ),
        Admission::Accepted
    );
    assert_eq!(
        log.merge_records(&state, [offered.clone()]),
        vec![Admission::Collision]
    );
    let mut state = GCounter::new(2);
    let mut other = EventLog::for_crdt(&state);
    other
        .append(&mut state, 1, bump(5))
        .expect("fresh ID is accepted");
    assert_eq!(
        other.admit_with(&mut state, offered.clone(), |_, _| unreachable!()),
        Admission::Collision
    );
    for log in [&log, &other] {
        assert_eq!(
            log.collisions(),
            vec![RecordCollision {
                id: ID,
                local: bump(5),
                remote: bump(9),
            }]
        );
        // The register is an alarm, not log data.
        assert_eq!(log.records().len(), 1);
    }
    assert_eq!(log, other);
    assert_eq!(
        log,
        EventLog::from_wire_bytes(&log.to_wire_bytes().unwrap()).unwrap()
    );
    assert!(
        EventLog::<GCounterDelta>::from_wire_bytes(&log.to_wire_bytes().unwrap())
            .unwrap()
            .collisions()
            .is_empty()
    );
    // The first differing payload stays the witness.
    let third = Record {
        id: ID,
        delta: bump(7),
    };
    assert_eq!(
        log.insert_record(&GCounter::new(2), third),
        Admission::Collision
    );
    assert_eq!(log.collisions()[0].remote, bump(9));
}

fn report(entries: &[(RecordId, u64, u64)]) -> Vec<u8> {
    CollisionReport {
        collisions: entries
            .iter()
            .map(|&(id, local, remote)| RecordCollision {
                id,
                local: bump(local),
                remote: bump(remote),
            })
            .collect(),
    }
    .to_wire_bytes()
    .unwrap()
}

#[test]
fn every_report_entry_has_a_named_verdict() {
    let (mut left, _) = forked();
    let other = RecordId {
        replica: 1,
        sequence: 2,
    };
    let verdicts = left
        .merge_collision_report_bytes(
            &report(&[
                (other, 5, 9), // not held here
                (ID, 5, 5),    // names only the payload held here
                (ID, 5, 7),    // reporter holds ours: witness is the offered payload
                (ID, 9, 5),    // alarm already raised
            ]),
            DecodeLimits::default(),
        )
        .unwrap();
    assert_eq!(
        verdicts,
        vec![
            (other, CollisionVerdict::Unheld),
            (ID, CollisionVerdict::Agrees),
            (ID, CollisionVerdict::Recorded),
            (ID, CollisionVerdict::Known),
        ]
    );
    assert_eq!(left.collisions()[0].remote, bump(7));
    let (mut fresh, _) = forked();
    assert_eq!(
        fresh.merge_collision_report_bytes(&report(&[]), DecodeLimits::default()),
        Ok(vec![])
    );
    assert!(fresh.collisions().is_empty());
    assert_eq!(fresh.state().state(), &[0, 5]);
}

// Frame a report body exactly as the encoder does, to reach body-level checks.
fn frame(body: &[u8]) -> Vec<u8> {
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = u32::MAX;
        for &byte in bytes {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
            }
        }
        !crc
    }
    let len = u32::try_from(body.len()).unwrap();
    let mut out = vec![0x05];
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&(!len).to_le_bytes());
    out.extend_from_slice(body);
    let crc = crc32(&out[1..]);
    out.extend_from_slice(&crc.to_le_bytes());
    out
}

fn body(entries: u32, tail: &[u8]) -> Vec<u8> {
    let schema = b"safemesh/collision-report/v1/safemesh/gcounter-delta/v1";
    let mut body = Vec::new();
    body.extend_from_slice(&u32::try_from(schema.len()).unwrap().to_le_bytes());
    body.extend_from_slice(schema);
    body.extend_from_slice(&entries.to_le_bytes());
    body.extend_from_slice(tail);
    body
}

fn entry(local: &[u8], remote: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&1u64.to_le_bytes());
    out.extend_from_slice(&1u64.to_le_bytes());
    for payload in [local, remote] {
        out.extend_from_slice(&u32::try_from(payload.len()).unwrap().to_le_bytes());
        out.extend_from_slice(payload);
    }
    out
}

#[test]
fn every_refused_report_is_named_and_changes_nothing() {
    let valid = report(&[(ID, 9, 5)]);
    assert_eq!(
        frame(&body(
            1,
            &entry(
                &bump(9).to_wire_bytes().unwrap(),
                &bump(5).to_wire_bytes().unwrap()
            )
        )),
        valid
    );
    let delta = bump(5).to_wire_bytes().unwrap();
    let mut padded = delta.clone();
    padded.push(0);
    let mut crc_flipped = valid.clone();
    *crc_flipped.last_mut().unwrap() ^= 1;
    let mut complement_flipped = valid.clone();
    complement_flipped[5] ^= 1;
    let mut trailing = valid.clone();
    trailing.push(0);
    let mut gset_body = Vec::new();
    let gset_schema = b"safemesh/collision-report/v1/safemesh/gset-u64/v1";
    gset_body.extend_from_slice(&u32::try_from(gset_schema.len()).unwrap().to_le_bytes());
    gset_body.extend_from_slice(gset_schema);
    gset_body.extend_from_slice(&0u32.to_le_bytes());
    let wrong_schema = frame(&gset_body);
    let log_frame = Replica::new(GCounter::new(2)).log_bytes().unwrap();
    let record = Record {
        id: ID,
        delta: bump(9),
    }
    .to_wire_bytes()
    .unwrap();

    let cases: Vec<(&str, Vec<u8>, DecodeLimits, DecodeError)> = vec![
        (
            "empty",
            vec![],
            DecodeLimits::default(),
            WireError::UnexpectedEof.into(),
        ),
        (
            "record tag",
            record,
            DecodeLimits::default(),
            WireError::InvalidTag.into(),
        ),
        (
            "log tag",
            log_frame,
            DecodeLimits::default(),
            WireError::InvalidTag.into(),
        ),
        (
            "truncated",
            valid[..valid.len() - 1].to_vec(),
            DecodeLimits::default(),
            WireError::UnexpectedEof.into(),
        ),
        (
            "length complement",
            complement_flipped,
            DecodeLimits::default(),
            WireError::IntegrityMismatch.into(),
        ),
        (
            "crc",
            crc_flipped,
            DecodeLimits::default(),
            WireError::IntegrityMismatch.into(),
        ),
        (
            "trailing frame bytes",
            trailing,
            DecodeLimits::default(),
            WireError::TrailingBytes.into(),
        ),
        (
            "schema",
            wrong_schema,
            DecodeLimits::default(),
            WireError::DeltaTypeMismatch.into(),
        ),
        (
            "count past body",
            frame(&body(2, &entry(&delta, &delta))),
            DecodeLimits::default(),
            WireError::UnexpectedEof.into(),
        ),
        (
            "body past count",
            frame(&body(0, &entry(&delta, &delta))),
            DecodeLimits::default(),
            WireError::TrailingBytes.into(),
        ),
        (
            "payload trailing bytes",
            frame(&body(1, &entry(&padded, &delta))),
            DecodeLimits::default(),
            WireError::TrailingBytes.into(),
        ),
        (
            "payload tag",
            frame(&body(1, &entry(&[0x20, 0, 0, 0, 0], &delta))),
            DecodeLimits::default(),
            WireError::InvalidTag.into(),
        ),
        (
            "entry budget",
            report(&[(ID, 9, 5), (ID, 9, 7)]),
            DecodeLimits {
                max_records: Some(1),
                ..DecodeLimits::default()
            },
            DecodeError::RecordLimitExceeded { max_records: 1 },
        ),
    ];
    for (name, bytes, limits, expected) in cases {
        let (mut left, _) = forked();
        let before = left.log().clone();
        assert_eq!(
            left.merge_collision_report_bytes(&bytes, limits),
            Err(ReplicaError::ReportDecode(expected)),
            "{name}"
        );
        assert!(left.collisions().is_empty(), "{name}");
        assert_eq!(left.log(), &before, "{name}");
        assert_eq!(left.state().state(), &[0, 5], "{name}");
    }
    // Budgets within the limit decode.
    let (mut left, _) = forked();
    assert_eq!(
        left.merge_collision_report_bytes(
            &report(&[(ID, 9, 5)]),
            DecodeLimits {
                max_records: Some(1),
                ..DecodeLimits::default()
            }
        ),
        Ok(vec![(ID, CollisionVerdict::Recorded)])
    );
}

#[test]
fn report_payload_collection_budget_applies() {
    let mut set = GSet::new();
    for value in 0..3 {
        set.insert(value);
    }
    let bytes = CollisionReport {
        collisions: vec![RecordCollision {
            id: ID,
            local: set.clone(),
            remote: GSet::new(),
        }],
    }
    .to_wire_bytes()
    .unwrap();
    let limits = DecodeLimits {
        max_collection_elements: Some(2),
        ..DecodeLimits::default()
    };
    let mut log = EventLog::<GSet<u64>>::new();
    assert_eq!(
        log.merge_collision_report_bytes(&bytes, limits),
        Err(WireError::CollectionElementLimitExceeded { max_elements: 2 }.into())
    );
    assert_eq!(
        CollisionReport::<GSet<u64>>::from_wire_bytes(&bytes)
            .unwrap()
            .collisions[0]
            .local,
        set
    );
}

// Every decoder that predates the report refuses its tag before reading
// further, so an old peer handed a report raises InvalidTag and mutates nothing.
#[test]
fn earlier_decoders_refuse_a_report_without_mutation() {
    let bytes = report(&[(ID, 9, 5)]);
    let state = GCounter::new(2);
    assert_eq!(
        Record::<GCounterDelta>::from_wire_bytes(&bytes),
        Err(WireError::InvalidTag)
    );
    assert_eq!(
        EventLog::<GCounterDelta>::from_wire_bytes(&bytes),
        Err(WireError::InvalidTag)
    );
    assert_eq!(
        EventLog::<GCounterDelta>::records_from_wire_bytes_for(&bytes, &state),
        Err(WireError::InvalidTag)
    );
    assert_eq!(
        EventLog::<GCounterDelta>::migrate_legacy_wire_bytes_for(&bytes, &state),
        Err(WireError::InvalidTag)
    );
    assert_eq!(
        VersionVector::from_wire_bytes(&bytes),
        Err(WireError::InvalidTag)
    );
    assert_ne!(
        EventLog::<GCounterDelta>::from_wire_bytes(&bytes),
        Err(WireError::LegacyEventLogFrame {
            found: LegacyFrame::Tag02
        })
    );
    let (mut left, _) = forked();
    let before = left.log().clone();
    assert_eq!(
        left.merge_record_bytes(&bytes, safemesh_crdt::CollectionLimits::WIRE_DEFAULT),
        Err(ReplicaError::RecordDecode(WireError::InvalidTag))
    );
    assert_eq!(
        left.merge_log_bytes(&bytes, DecodeLimits::default()),
        Err(ReplicaError::LogDecode(WireError::InvalidTag.into()))
    );
    assert_eq!(left.log(), &before);
    assert_eq!(left.state().state(), &[0, 5]);
    assert!(left.collisions().is_empty());
}
