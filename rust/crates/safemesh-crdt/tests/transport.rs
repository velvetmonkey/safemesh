// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: AGPL-3.0-or-later

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
