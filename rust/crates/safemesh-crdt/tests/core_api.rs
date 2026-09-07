// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

use safemesh_crdt::{
    Crdt, EnableWinsFlag, EnableWinsFlagDelta, EventLog, GCounter, GCounterDelta, LwwMap,
    LwwMapDelta, LwwRegister, LwwRegisterDelta, OrSet, OrSetDelta, PnCounter, PnCounterDelta, Rga,
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
fn enable_wins_flag_keeps_concurrent_enable_live() {
    let mut flag = EnableWinsFlag::new();
    flag.apply_delta(EnableWinsFlagDelta::Enable { token: 10_u64 });
    assert!(flag.value());

    let observed = flag.observed_tokens().into_iter().collect::<Vec<_>>();
    flag.apply_delta(EnableWinsFlagDelta::Disable { tokens: observed });
    assert!(!flag.value());

    let mut concurrent = EnableWinsFlag::new();
    concurrent.apply_delta(EnableWinsFlagDelta::Enable { token: 11 });
    flag.merge(&concurrent);
    assert!(flag.value());
    assert_eq!(
        flag.enables().iter().copied().collect::<Vec<_>>(),
        vec![10, 11]
    );
    assert_eq!(
        flag.tombstones().iter().copied().collect::<Vec<_>>(),
        vec![10]
    );
}

#[test]
fn lww_map_tracks_value_and_remove_dots_per_key() {
    let mut map = LwwMap::new();
    map.apply_delta(LwwMapDelta::Set {
        key: 7_u64,
        timestamp: 10,
        replica: 1,
        value: 100_u64,
    });
    assert_eq!(map.get(&7), Some(&100));

    map.apply_delta(LwwMapDelta::Remove {
        key: 7,
        timestamp: 11,
        replica: 1,
    });
    assert_eq!(map.get(&7), None);

    let mut stale_writer = LwwMap::new();
    stale_writer.set(7, 10, 2, 200);
    map.merge(&stale_writer);
    assert_eq!(map.get(&7), None);

    let mut later_writer = LwwMap::new();
    later_writer.set(7, 12, 1, 300);
    later_writer.set(2, 1, 1, 20);
    map.merge(&later_writer);
    assert_eq!(map.get(&7), Some(&300));
    assert_eq!(
        map.value().into_iter().collect::<Vec<_>>(),
        vec![(2, 20), (7, 300)]
    );
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

#[test]
fn event_log_version_does_not_hide_out_of_order_gaps() {
    let mut source = EventLog::new();
    let first = source.append(
        1,
        GCounterDelta {
            replica: 1,
            tally: 1,
        },
    );
    let second = source.append(
        1,
        GCounterDelta {
            replica: 1,
            tally: 2,
        },
    );
    let third = source.append(
        1,
        GCounterDelta {
            replica: 1,
            tally: 3,
        },
    );

    let mut reordered = EventLog::new();
    reordered.merge_records([source.records()[2].clone()]);
    assert_eq!(reordered.version().get(1), 0);
    assert_eq!(
        source
            .since(reordered.version())
            .into_iter()
            .map(|record| record.id)
            .collect::<Vec<_>>(),
        vec![first, second, third]
    );

    reordered.merge_records([source.records()[1].clone()]);
    assert_eq!(reordered.version().get(1), 0);

    reordered.merge_records([source.records()[0].clone()]);
    assert_eq!(reordered.version().get(1), 3);
    assert!(source.since(reordered.version()).is_empty());
}

#[test]
fn lww_register_uses_total_ordered_winner() {
    let mut register = LwwRegister::new();
    register.apply_delta(LwwRegisterDelta {
        timestamp: 10,
        replica: 1,
        value: "alpha",
    });
    register.apply_delta(LwwRegisterDelta {
        timestamp: 9,
        replica: 99,
        value: "stale",
    });
    register.apply_delta(LwwRegisterDelta {
        timestamp: 10,
        replica: 2,
        value: "beta",
    });

    assert_eq!(register.value(), Some(&"beta"));

    let mut other = LwwRegister::new();
    other.apply_delta(LwwRegisterDelta {
        timestamp: 10,
        replica: 2,
        value: "alpha",
    });
    register.merge(&other);
    assert_eq!(register.value(), Some(&"beta"));
}

// Encode every coordinate in fixed-width bytes; equality checks also cover lengths.
fn g_bytes(counter: &GCounter) -> Vec<u8> {
    counter
        .state()
        .iter()
        .flat_map(|tally| tally.to_le_bytes())
        .collect()
}

fn pn_bytes(counter: &PnCounter) -> Vec<u8> {
    counter
        .p_state()
        .iter()
        .chain(counter.n_state())
        .flat_map(|tally| tally.to_le_bytes())
        .collect()
}

#[test]
fn checked_counter_coordinates_reject_without_changing_any_state_bytes() {
    use safemesh_crdt::CoordinateError;

    for count in [0, 3] {
        let mut g = GCounter::new(count);
        let mut pn = PnCounter::new(count);
        for replica in 0..count {
            g.apply_bump(replica, 10 + replica as u64);
            pn.apply_inc(replica, 20 + replica as u64);
            pn.apply_dec(replica, 30 + replica as u64);
        }
        let g_before = g.clone();
        let pn_before = pn.clone();
        let g_before_bytes = g_bytes(&g);
        let pn_before_bytes = pn_bytes(&pn);
        for replica in [count, 7, usize::MAX] {
            let error = Err(CoordinateError::ReplicaOutOfRange {
                replica,
                replica_count: count,
            });
            assert_eq!(g.try_apply_bump(replica, u64::MAX), error);
            assert_eq!(g, g_before);
            assert_eq!(g_bytes(&g), g_before_bytes);
            assert_eq!(pn.try_apply_inc(replica, u64::MAX), error);
            assert_eq!(pn, pn_before);
            assert_eq!(pn_bytes(&pn), pn_before_bytes);
            assert_eq!(pn.try_apply_dec(replica, u64::MAX), error);
            assert_eq!(pn, pn_before);
            assert_eq!(pn_bytes(&pn), pn_before_bytes);

            // The compatible infallible paths still silently ignore bad indices.
            g.apply_bump(replica, u64::MAX);
            pn.apply_inc(replica, u64::MAX);
            pn.apply_dec(replica, u64::MAX);
            assert_eq!(g_bytes(&g), g_before_bytes);
            assert_eq!(pn_bytes(&pn), pn_before_bytes);
        }
    }
}

#[test]
fn checked_counter_coordinates_match_infallible_valid_results() {
    let mut checked_g = GCounter::new(3);
    let mut legacy_g = checked_g.clone();
    let mut checked_pn = PnCounter::new(3);
    let mut legacy_pn = checked_pn.clone();
    for replica in 0..3 {
        for tally in [0, 9, 9, 4, u64::MAX] {
            assert_eq!(checked_g.try_apply_bump(replica, tally), Ok(()));
            legacy_g.apply_bump(replica, tally);
            assert_eq!(checked_g, legacy_g);
            assert_eq!(g_bytes(&checked_g), g_bytes(&legacy_g));
            assert_eq!(checked_pn.try_apply_inc(replica, tally), Ok(()));
            legacy_pn.apply_inc(replica, tally);
            assert_eq!(checked_pn, legacy_pn);
            assert_eq!(pn_bytes(&checked_pn), pn_bytes(&legacy_pn));
            assert_eq!(checked_pn.try_apply_dec(replica, tally), Ok(()));
            legacy_pn.apply_dec(replica, tally);
            assert_eq!(checked_pn, legacy_pn);
            assert_eq!(pn_bytes(&checked_pn), pn_bytes(&legacy_pn));
        }
    }
}
