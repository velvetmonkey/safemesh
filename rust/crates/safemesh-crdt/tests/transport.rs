// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

use safemesh_crdt::{
    anti_entropy, EventLog, GCounterDelta, InMemoryTransport, TransportAdapter, TransportError,
};

fn deliver_all(
    transport: &mut InMemoryTransport<GCounterDelta>,
    to: u64,
    log: &mut EventLog<GCounterDelta>,
) {
    for envelope in transport.drain(to) {
        log.merge_records(envelope.records);
    }
}

#[test]
fn transport_requires_subscription_and_connectivity() {
    let mut transport: InMemoryTransport<GCounterDelta> = InMemoryTransport::new();
    let records = vec![];

    assert_eq!(
        transport.send(1, 2, records.clone()),
        Err(TransportError::NotSubscribed { peer: 1 })
    );

    transport.subscribe(1);
    assert_eq!(
        transport.send(1, 2, records.clone()),
        Err(TransportError::NotSubscribed { peer: 2 })
    );

    transport.subscribe(2);
    transport.set_connected(1, 2, false);
    assert_eq!(
        transport.send(1, 2, records.clone()),
        Err(TransportError::Disconnected { from: 1, to: 2 })
    );

    transport.set_connected(1, 2, true);
    assert!(transport.send(1, 2, records).is_ok());
}

#[test]
fn anti_entropy_recovers_after_drop_duplicate_and_reorder() {
    let mut left = EventLog::new();
    let mut right = EventLog::new();
    left.append(
        1,
        GCounterDelta {
            replica: 1,
            tally: 1,
        },
    );
    left.append(
        1,
        GCounterDelta {
            replica: 1,
            tally: 2,
        },
    );
    left.append(
        1,
        GCounterDelta {
            replica: 1,
            tally: 3,
        },
    );

    let mut transport = InMemoryTransport::new();
    transport.subscribe(1);
    transport.subscribe(2);

    transport.drop_next_send();
    anti_entropy(&mut transport, 1, 2, &left, right.version()).unwrap();
    assert_eq!(transport.dropped_len(), 1);
    assert_eq!(right.records().len(), 0);

    transport.duplicate_next_send();
    anti_entropy(&mut transport, 1, 2, &left, right.version()).unwrap();
    transport.reverse_pending_for(2);
    deliver_all(&mut transport, 2, &mut right);

    assert_eq!(right.records().len(), 3);
    assert_eq!(right.version().get(1), 3);
    assert!(left.since(right.version()).is_empty());
}

#[test]
fn partition_then_heal_uses_versions_to_cover_missing_records() {
    let mut left = EventLog::new();
    let mut right = EventLog::new();
    left.append(
        1,
        GCounterDelta {
            replica: 1,
            tally: 1,
        },
    );

    let mut transport = InMemoryTransport::new();
    transport.subscribe(1);
    transport.subscribe(2);
    transport.set_connected(1, 2, false);

    assert_eq!(
        anti_entropy(&mut transport, 1, 2, &left, right.version()),
        Err(TransportError::Disconnected { from: 1, to: 2 })
    );
    assert_eq!(transport.pending_len(), 0);

    transport.set_connected(1, 2, true);
    anti_entropy(&mut transport, 1, 2, &left, right.version()).unwrap();
    deliver_all(&mut transport, 2, &mut right);

    assert_eq!(right.records().len(), 1);
    assert_eq!(right.version().get(1), 1);
}

use safemesh_crdt::{Admission, Record, RecordId, VersionVector};
use std::collections::{BTreeMap, BTreeSet};

fn rebuild(entries: &BTreeMap<u64, u64>, zeros: &BTreeSet<u64>) -> VersionVector {
    let mut received = VersionVector::new();
    for (&replica, &prefix) in entries {
        for sequence in 1..=prefix {
            received.observe(RecordId { replica, sequence });
        }
    }
    for &replica in zeros {
        received.observe(RecordId {
            replica,
            sequence: 0,
        });
    }
    received
}

#[test]
fn version_exchange_public_api_round_trip() {
    let mut source = EventLog::new();
    let mut held = VersionVector::new();
    for replica in [0, 7, u64::MAX] {
        for sequence in 0..=4 {
            let record = Record {
                id: RecordId { replica, sequence },
                delta: sequence,
            };
            assert_eq!(source.insert_record(record.clone()), Admission::Accepted);
            if sequence <= 2 && (replica != 7 || sequence != 0) {
                held.observe(record.id);
            }
        }
    }
    let received = rebuild(held.entries(), held.zero_replicas());
    // First assertion makes a missing reconstruction fail on returned records.
    assert_eq!(source.since(&received), source.since(&held));
    assert_eq!(source.since(&received).len(), 7);
    assert_eq!(received, held);
    for replica in [0, 7, 8, u64::MAX] {
        assert_eq!(received.get(replica), held.get(replica));
        for sequence in [0, 1, 2, 3, 4, u64::MAX] {
            let id = RecordId { replica, sequence };
            assert_eq!(received.includes(id), held.includes(id));
        }
    }
}

#[test]
fn version_exchange_public_api_trust_boundary() {
    let mut source = EventLog::new();
    source.append(7, 1u64);
    source.append(7, 2u64);
    let empty_receiver = EventLog::<u64>::new();
    let lie = rebuild(&BTreeMap::from([(7, 2)]), &BTreeSet::new());
    assert_eq!(source.since(empty_receiver.version()).len(), 2);
    assert_eq!(source.since(&lie).len(), 0);
}

#[test]
fn version_exchange_public_api_edge_cases() {
    assert_eq!(
        rebuild(&BTreeMap::new(), &BTreeSet::new()),
        VersionVector::new()
    );
    let zeros = BTreeSet::from([0, 7, u64::MAX]);
    let only_zeros = rebuild(&BTreeMap::new(), &zeros);
    let mut source = EventLog::new();
    for &replica in &zeros {
        let id = RecordId {
            replica,
            sequence: 0,
        };
        source.insert_record(Record { id, delta: 0u64 });
        assert!(only_zeros.includes(id));
        assert_eq!(only_zeros.get(replica), 0);
    }
    assert!(source.since(&only_zeros).is_empty());
    assert_eq!(
        rebuild(only_zeros.entries(), only_zeros.zero_replicas()),
        only_zeros
    );
    let entries: BTreeMap<_, _> = (0..100_000).map(|replica| (replica, 1)).collect();
    let wide = rebuild(&entries, &BTreeSet::new());
    assert_eq!(wide.entries(), &entries);
    let mut max_id = rebuild(&BTreeMap::from([(u64::MAX, 1)]), &BTreeSet::new());
    max_id.observe(RecordId {
        replica: u64::MAX,
        sequence: u64::MAX,
    });
    assert_eq!(max_id.get(u64::MAX), 1); // A gap still cannot advance a prefix.
    assert_eq!(
        rebuild(&BTreeMap::from([(7, 0)]), &BTreeSet::new()),
        VersionVector::new()
    );
}
