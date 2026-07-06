// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: AGPL-3.0-or-later

use safemesh_crdt::{
    Crdt, EventLog, GCounter, GCounterDelta, OrSet, OrSetDelta, PnCounter, PnCounterDelta, Rga,
    RgaDelta,
};

#[test]
fn counter_traits_preserve_existing_behavior() {
    let mut g = GCounter::new(3);
    g.apply_delta(GCounterDelta {
        replica: 1,
        tally: 4,
    });
    g.apply_delta(GCounterDelta {
        replica: 1,
        tally: 2,
    });
    assert_eq!(g.state(), &[0, 4, 0]);
    assert_eq!(g.value(), 4);

    let mut pn = PnCounter::new(2);
    pn.apply_delta(PnCounterDelta::Inc {
        replica: 0,
        tally: 7,
    });
    pn.apply_delta(PnCounterDelta::Dec {
        replica: 1,
        tally: 3,
    });
    assert_eq!(pn.value(), 4);
}

#[test]
fn orset_delta_is_add_wins() {
    let mut set = OrSet::new();
    set.apply_delta(OrSetDelta::Add {
        element: "medkit",
        token: 10_u64,
    });
    set.apply_delta(OrSetDelta::Remove { tokens: vec![10] });
    set.apply_delta(OrSetDelta::Add {
        element: "medkit",
        token: 11,
    });

    assert!(set.contains(&"medkit"));
    assert_eq!(
        set.observed_tokens(&"medkit")
            .into_iter()
            .collect::<Vec<_>>(),
        vec![10, 11]
    );
}

#[test]
fn rga_read_is_sorted_by_position_not_delivery() {
    let mut rga = Rga::new();
    rga.apply_delta(RgaDelta::Insert {
        position: 30_u64,
        value: 'c',
    });
    rga.apply_delta(RgaDelta::Insert {
        position: 10,
        value: 'a',
    });
    rga.apply_delta(RgaDelta::Insert {
        position: 20,
        value: 'b',
    });
    rga.apply_delta(RgaDelta::Delete { position: 20 });

    assert_eq!(rga.read_positions(), vec![10, 30]);
}

#[test]
fn event_log_deduplicates_and_serves_since_version() {
    let mut left = EventLog::new();
    let first = left.append(
        1,
        GCounterDelta {
            replica: 1,
            tally: 1,
        },
    );
    let second = left.append(
        1,
        GCounterDelta {
            replica: 1,
            tally: 2,
        },
    );

    assert_eq!(first.sequence, 1);
    assert_eq!(second.sequence, 2);
    assert_eq!(left.version().get(1), 2);

    let mut right = EventLog::new();
    right.merge_records(left.records().iter().cloned());
    right.merge_records(left.records().iter().cloned());

    assert_eq!(right.records().len(), 2);
    assert_eq!(right.version().get(1), 2);
    assert!(left.since(right.version()).is_empty());

    let third = left.append(
        2,
        GCounterDelta {
            replica: 2,
            tally: 1,
        },
    );
    let missing = left.since(right.version());
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].id, third);
}
