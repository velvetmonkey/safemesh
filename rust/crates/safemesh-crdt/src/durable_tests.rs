use super::*;
use alloc::string::ToString;
use alloc::vec;
use std::{
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

// IDs are scoped to a CRDT log; tokens are scoped to its set. Capture
// allocations at issuance, not received copies (which intentionally repeat).
fn unique<T: PartialEq>(values: &[T]) -> bool {
    values
        .iter()
        .enumerate()
        .all(|(i, x)| !values[..i].contains(x))
}
fn allocations<D>(records: &[Record<D>]) -> Vec<RecordId> {
    records.iter().map(|r| r.id).collect()
}
fn tokens(records: &[Record<OrSetDelta<String, u64>>]) -> Vec<u64> {
    records
        .iter()
        .filter_map(|r| match r.delta {
            OrSetDelta::Add { token, .. } => Some(token),
            OrSetDelta::Remove { .. } => None,
        })
        .collect()
}
fn check_allocations<T: PartialEq + Clone>(name: &str, before: &[T], after: &[T]) {
    assert!(!before.is_empty() && !after.is_empty());
    let mut all = before.to_vec();
    all.extend_from_slice(after);
    assert!(unique(&all), "{name}: allocation reused");
    assert!(!before.iter().any(|x| after.contains(x)));
    all.push(before[0].clone());
    assert!(!unique(&all), "{name}: planted duplicate escaped collector");
    std::println!(
        "{name} before={} after={} overlap=0 planted-duplicate=detected",
        before.len(),
        after.len()
    );
}
fn joined_exchange<C>(a: &mut DurableReplica<C>, b: &mut DurableReplica<C>, empty: impl Fn() -> C)
where
    C: Crdt + PartialEq + std::fmt::Debug,
    C::Delta: OwnedDelta + Clone + PartialEq + WireEncode + WireDecode + WireSchema,
{
    let ab: Vec<_> = a
        .log()
        .since(b.log().version())
        .iter()
        .map(|r| r.to_wire_bytes().unwrap())
        .collect();
    let ba: Vec<_> = b
        .log()
        .since(a.log().version())
        .iter()
        .map(|r| r.to_wire_bytes().unwrap())
        .collect();
    assert!(!ab.is_empty() && !ba.is_empty());
    // Same two batches, opposite delivery orders, through M1 admission.
    let mut finals = Vec::new();
    for batches in [[&ab, &ba], [&ba, &ab]] {
        let mut state = empty();
        let mut log = EventLog::for_crdt(&state);
        for batch in batches {
            for packet in batch {
                let record = Record::<C::Delta>::from_wire_bytes(packet).unwrap();
                assert_eq!(
                    log.admit_with(&mut state, record, |state, d| state.apply_delta(d.clone())),
                    Admission::Accepted
                );
            }
        }
        finals.push(state);
    }
    assert_eq!(finals[0], finals[1]);
    for (receiver, packets) in [(b, ab), (a, ba)] {
        for packet in packets {
            assert_eq!(
                receiver
                    .receive(
                        receiver.ticket(),
                        Record::<C::Delta>::from_wire_bytes(&packet).unwrap()
                    )
                    .unwrap(),
                Admission::Accepted
            );
        }
        assert_eq!(receiver.state(), &finals[0]);
    }
}
fn joined_config(writer: u64) -> WriterConfig {
    WriterConfig { writers: 2, writer }
}
fn joined_finish(root: &Path, loss: bool) {
    let counter_root = root.join("counter");
    let set_root = root.join("set");
    let before_c = EventLog::<GCounterDelta>::from_wire_bytes_for(
        &fs::read(root.join("counter-issued")).unwrap(),
        &GCounter::new(2),
    )
    .unwrap();
    let before_s = EventLog::<OrSetDelta<String, u64>>::from_wire_bytes_for(
        &fs::read(root.join("set-issued")).unwrap(),
        &OrSet::new(),
    )
    .unwrap();
    assert_eq!(before_c.records().len(), 2);
    assert_eq!(before_s.records().len(), 2);
    let mut a = DurableReplica::restart_counter(&counter_root, joined_config(0)).unwrap();
    let mut sa = DurableReplica::restart_utf8_set(&set_root, joined_config(0)).unwrap();
    if loss {
        assert_eq!(a.state().state(), &[0, 0]);
        assert!(!sa.state().contains(&"café☕".into()));
        assert!(!sa.state().contains(&"東京".into()));
        assert!(a.log().records().is_empty() && sa.log().records().is_empty());
        std::println!(
            "joined LOSS: acknowledged counter=9 and UTF-8 edits absent after process restart"
        );
        return;
    }
    assert_eq!(a.state().state(), &[9, 0]);
    for word in ["café☕", "東京"] {
        assert!(sa.state().contains(&word.into()));
    }
    let mut b = DurableReplica::restart_counter(&counter_root, joined_config(1)).unwrap();
    let mut sb = DurableReplica::restart_utf8_set(&set_root, joined_config(1)).unwrap();
    let after_c = [
        a.bump(a.ticket(), 12).unwrap(),
        b.bump(b.ticket(), 7).unwrap(),
    ];
    let after_s = [
        sa.add(sa.ticket(), "naïve".into()).unwrap(),
        sb.add(sb.ticket(), "γειά".into()).unwrap(),
    ];
    check_allocations(
        "counter IDs",
        &allocations(before_c.records()),
        &allocations(&after_c),
    );
    check_allocations(
        "set IDs",
        &allocations(before_s.records()),
        &allocations(&after_s),
    );
    check_allocations("set tokens", &tokens(before_s.records()), &tokens(&after_s));
    joined_exchange(&mut a, &mut b, || GCounter::new(2));
    joined_exchange(&mut sa, &mut sb, OrSet::new);
    assert_eq!(a.state(), b.state());
    assert_eq!(a.state().state(), &[12, 7]);
    assert_eq!(sa.state(), sb.state());
    for word in ["café☕", "東京", "naïve", "γειά"] {
        assert!(sa.state().contains(&word.into()));
    }
    for r in before_c.records() {
        assert!(a.log().records().contains(r) && b.log().records().contains(r));
    }
    for r in before_s.records() {
        assert!(sa.log().records().contains(r) && sb.log().records().contains(r));
    }
    let state_c = a.state().clone();
    let state_s = sa.state().clone();
    drop((a, b, sa, sb));
    for writer in 0..2 {
        assert_eq!(
            DurableReplica::restart_counter(&counter_root, joined_config(writer))
                .unwrap()
                .state(),
            &state_c
        );
        assert_eq!(
            DurableReplica::restart_utf8_set(&set_root, joined_config(writer))
                .unwrap()
                .state(),
            &state_s
        );
    }
    std::println!("joined HAPPY: all acknowledged records survive; both exchange orders converge; second restart survives");
}

fn joined_start(root: &Path, loss: bool) {
    for name in ["counter", "set"] {
        fs::create_dir_all(root.join(name)).unwrap();
    }
    // Make the newly created store directories durable before edits.
    fs::File::open(root).unwrap().sync_all().unwrap();
    // Both replicas exist before the offline edits; neither exchanges yet.
    let _b = DurableReplica::counter(&root.join("counter"), joined_config(1)).unwrap();
    let _sb = DurableReplica::utf8_set(&root.join("set"), joined_config(1)).unwrap();
    let mut c = DurableReplica::counter(&root.join("counter"), joined_config(0)).unwrap();
    let mut s = DurableReplica::utf8_set(&root.join("set"), joined_config(0)).unwrap();
    if loss {
        // Deliberately bypass only the commit: same owned allocation and
        // admission, falsely acknowledged by this negative-control caller.
        c.inner.bump(c.ticket(), 5).unwrap();
        c.inner.bump(c.ticket(), 9).unwrap();
        s.inner.add(s.ticket(), "café☕".into()).unwrap();
        s.inner.add(s.ticket(), "東京".into()).unwrap();
    } else {
        c.bump(c.ticket(), 5).unwrap();
        c.bump(c.ticket(), 9).unwrap();
        s.add(s.ticket(), "café☕".into()).unwrap();
        s.add(s.ticket(), "東京".into()).unwrap();
    }
    // External audit only: restart never reads these files as recovery data.
    fs::write(
        root.join("counter-issued"),
        c.log().to_wire_bytes().unwrap(),
    )
    .unwrap();
    fs::write(root.join("set-issued"), s.log().to_wire_bytes().unwrap()).unwrap();
    std::println!("joined ACK counter=5,9 set=café☕,東京 loss={loss}");
    std::io::stdout().flush().unwrap();
    // Intentionally bypass destructors: a real process ends after ACK.
    std::process::exit(77);
}
static SERIAL: AtomicU64 = AtomicU64::new(0);
fn root() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "durable-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).unwrap();
    root.canonicalize().unwrap()
}
#[test]
fn fresh_root_syncs_exact_created_parents() {
    let existing = root();
    let nested = existing.join("ancestor/store");
    let mut synced = Vec::new();
    create_durable_root_with(&nested, |parent| {
        synced.push(parent.to_path_buf());
        Ok(())
    })
    .unwrap();
    let expected: Vec<_> = nested
        .canonicalize()
        .unwrap()
        .ancestors()
        .skip(1)
        .map(Path::to_path_buf)
        .collect();
    assert_eq!(synced, expected);
    synced.clear();
    create_durable_root_with(&nested, |parent| {
        synced.push(parent.to_path_buf());
        Ok(())
    })
    .unwrap();
    assert_eq!(synced, expected);
    let failed = existing.join("failed/store");
    let error = create_durable_root_with(&failed, |_| {
        Err(io::Error::other("injected parent sync failure"))
    })
    .unwrap_err();
    assert!(
        matches!(error, LocalError::AncestorSync { path, source } if path == failed.parent().unwrap() && source.to_string() == "injected parent sync failure")
    );
    assert!(failed.is_dir());
}
#[test]
fn fresh_root_syncs_lexical_symlink_parent() {
    let base = root();
    fs::create_dir(base.join("a")).unwrap();
    fs::create_dir_all(base.join("b/real")).unwrap();
    std::os::unix::fs::symlink(base.join("b/real"), base.join("a/link")).unwrap();
    let store = base.join("a/link/store");
    let mut synced = Vec::new();
    create_durable_root_with(&store, |parent| {
        synced.push(parent.to_path_buf());
        Ok(())
    })
    .unwrap();

    // Canonical chain: b/real, b, base, and every ancestor of base.
    // Lexical chain adds a: it holds the link, but is not in the canonical chain.
    let mut expected = vec![
        base.join("b/real"),
        base.join("b"),
        base.join("a"),
        base.clone(),
    ];
    expected.extend(base.ancestors().skip(1).map(Path::to_path_buf));
    assert_eq!(synced, expected);
    fs::remove_dir_all(base).unwrap();
}
#[test]
fn fresh_root_syncs_leftover_parent_chain() {
    let existing = root();
    let leftover = existing.join("interrupted/store");
    // Simulate an earlier call dying after mkdir, without any parent fsync.
    fs::create_dir_all(&leftover).unwrap();
    let mut synced = Vec::new();
    create_durable_root_with(&leftover, |parent| {
        synced.push(parent.to_path_buf());
        Ok(())
    })
    .unwrap();
    let expected: Vec<_> = leftover
        .canonicalize()
        .unwrap()
        .ancestors()
        .skip(1)
        .map(Path::to_path_buf)
        .collect();
    assert_eq!(synced, expected);
    assert_eq!(synced[0], existing.join("interrupted"));
    assert_eq!(synced[1], existing);
    let replica = DurableReplica::counter(&leftover, config()).unwrap();
    assert!(leftover.join("writer-0.fence").is_file());
    drop(replica);
}

#[test]
fn fresh_root_syncs_cancelled_traversal_parents() {
    let existing = root();
    let cancelled = existing.join("interrupted/child");
    fs::create_dir_all(&cancelled).unwrap();
    let store = existing.join("interrupted/child/../../store");
    fs::create_dir(existing.join("store")).unwrap();
    let mut synced = Vec::new();
    create_durable_root_with(&store, |parent| {
        synced.push(parent.to_path_buf());
        Ok(())
    })
    .unwrap();
    assert!(synced.contains(&existing.join("interrupted")));
    assert!(synced.contains(&existing));
    assert_eq!(synced.last().unwrap(), Path::new("/"));
    let replica = DurableReplica::counter(&store, config()).unwrap();
    drop(replica);
}

#[test]
fn fresh_root_syncs_concurrent_creator_parents() {
    let existing = root();
    let nested = existing.join("concurrent/store");
    let mut synced = Vec::new();
    let mut races = 0;
    create_durable_root_using(
        &nested,
        |directory| {
            fs::create_dir(directory)?;
            races += 1;
            Err(io::Error::from(io::ErrorKind::AlreadyExists))
        },
        |parent| {
            synced.push(parent.to_path_buf());
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(races, 2);
    let expected: Vec<_> = nested
        .canonicalize()
        .unwrap()
        .ancestors()
        .skip(1)
        .map(Path::to_path_buf)
        .collect();
    assert_eq!(synced, expected);
}

fn config() -> WriterConfig {
    WriterConfig {
        writers: 2,
        writer: 0,
    }
}
// Recreate the pre-protection store layout for tests of its old restart
// behavior. The new protected-store cases have separate tamper tests.
fn pre_protection<C: Crdt>(replica: &mut DurableReplica<C>) {
    let (marker, counter) = rollback::paths(&replica.root, replica.inner.config.writer).unwrap();
    fs::remove_file(marker).unwrap();
    fs::remove_file(counter).unwrap();
    replica.rollback = None;
}
// An empty store in the format main wrote before the append log: the same
// fence and a whole-history `writer-0.transaction` from the unchanged
// `commit` that main's fresh constructors called, with no journal.
fn legacy_at(kind: &str, root: &Path) {
    let path = transaction_path(root, config());
    if kind == "counter" {
        drop(DurableReplica::counter(root, config()).unwrap());
        let log = EventLog::for_crdt(&GCounter::new(2));
        DurableReplica::<GCounter>::commit(&path, config(), &log, 0).unwrap();
    } else {
        drop(DurableReplica::utf8_set(root, config()).unwrap());
        let log = EventLog::for_crdt(&OrSet::<String, u64>::new());
        DurableReplica::<OrSet<String, u64>>::commit(&path, config(), &log, 0).unwrap();
    }
    fs::remove_file(journal::path(root, 0)).unwrap();
    let (marker, counter) = rollback::paths(root, config().writer).unwrap();
    fs::remove_file(marker).unwrap();
    fs::remove_file(counter).unwrap();
}
fn legacy(kind: &str) -> PathBuf {
    let root = root();
    legacy_at(kind, &root);
    root
}
// Whichever committed history file writer 0 has.
fn store_file(root: &Path) -> PathBuf {
    let journal = journal::path(root, 0);
    if journal.exists() {
        journal
    } else {
        transaction_path(root, config())
    }
}
fn h1_record(sequence: u64) -> Record<GCounterDelta> {
    Record {
        id: RecordId {
            replica: 0,
            sequence,
        },
        delta: GCounterDelta {
            replica: 0,
            tally: 99,
        },
    }
}
#[test]
fn h1_event_log_unprotected_control() {
    let mut state = GCounter::new(2);
    let mut log = EventLog::for_crdt(&state);
    assert_eq!(
        log.insert_record(&state, h1_record(u64::MAX)),
        Admission::Accepted
    );
    assert!(matches!(
        log.append(
            &mut state,
            0,
            GCounterDelta {
                replica: 0,
                tally: 100
            }
        ),
        Err(crate::AppendError::SequenceExhausted)
    ));
}
#[test]
fn h1_local_peer_sequence_refused() {
    let mut r = LocalReplica::counter(&root(), config()).unwrap();
    let honest = r.bump(r.ticket(), 1).unwrap();
    assert_eq!(r.receive(r.ticket(), honest).unwrap(), Admission::Duplicate);
    assert_eq!(r.bump(r.ticket(), 2).unwrap().id.sequence, 2);
    let state = r.state().clone();
    let log = r.log().clone();
    let allocation = r.allocation_bytes();
    let received = r.receive(r.ticket(), h1_record(u64::MAX));
    let next = r.bump(r.ticket(), 3);
    std::println!("H1.local receive={received:?} next={next:?}");
    assert!(matches!(received, Err(LocalError::PeerWriterAhead)));
    assert_eq!(r.state(), &state);
    assert_eq!(r.log(), &log);
    assert_eq!(r.allocation_bytes(), allocation);
    assert!(matches!(
        r.bump(r.ticket(), 3),
        Err(LocalError::PeerWriterAhead)
    ));
}
#[test]
fn h1_durable_peer_sequence_refused_after_restart() {
    let root = root();
    let mut r = DurableReplica::counter(&root, config()).unwrap();
    let honest = r.bump(r.ticket(), 1).unwrap();
    let path = store_file(&root);
    let bytes = fs::read(&path).unwrap();
    assert!(matches!(
        r.receive(r.ticket(), h1_record(u64::MAX)),
        Err(LocalError::PeerWriterAhead)
    ));
    assert!(matches!(
        r.bump(r.ticket(), 2),
        Err(LocalError::PeerWriterAhead)
    ));
    assert_eq!(fs::read(&path).unwrap(), bytes);
    drop(r);
    let mut r = DurableReplica::restart_counter(&root, config()).unwrap();
    // The stop is volatile: ordinary restart still continues at high-water + 1.
    assert_eq!(r.bump(r.ticket(), 2).unwrap().id.sequence, 2);
    assert_eq!(r.receive(r.ticket(), honest).unwrap(), Admission::Duplicate);
    let before = fs::read(&path).unwrap();
    // Sync re-detects the same forged record and stops this new session.
    assert!(matches!(
        r.receive(r.ticket(), h1_record(u64::MAX)),
        Err(LocalError::PeerWriterAhead)
    ));
    assert!(matches!(
        r.bump(r.ticket(), 3),
        Err(LocalError::PeerWriterAhead)
    ));
    assert_eq!(fs::read(&path).unwrap(), before);
}
#[test]
fn h1_second_order_boundaries_and_complete_log_receive() {
    for sequence in [2, u64::MAX] {
        let root = root();
        let mut r = DurableReplica::counter(&root, config()).unwrap();
        let honest = r.bump(r.ticket(), 1).unwrap();
        assert_eq!(
            r.receive(r.ticket(), honest.clone()).unwrap(),
            Admission::Duplicate
        );
        let path = store_file(&root);
        let bytes = fs::read(&path).unwrap();
        let state = r.state().clone();
        let log = r.log().clone();
        let allocation = r.allocation_bytes();
        for _ in 0..2 {
            assert!(matches!(
                r.receive(r.ticket(), h1_record(sequence)),
                Err(LocalError::PeerWriterAhead)
            ));
            assert_eq!(r.state(), &state);
            assert_eq!(r.log(), &log);
            assert_eq!(r.allocation_bytes(), allocation);
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        let ticket = r.renew(r.ticket()).unwrap();
        assert!(matches!(
            r.append(
                ticket,
                GCounterDelta {
                    replica: 0,
                    tally: 3
                }
            ),
            Err(LocalError::PeerWriterAhead)
        ));
        assert_eq!(r.receive(ticket, honest).unwrap(), Admission::Duplicate);
        let remote = Record {
            id: RecordId {
                replica: 1,
                sequence: u64::MAX,
            },
            delta: GCounterDelta {
                replica: 1,
                tally: 7,
            },
        };
        assert_eq!(
            r.receive(ticket, remote.clone()).unwrap(),
            Admission::Accepted
        );
        assert_eq!(r.receive(ticket, remote).unwrap(), Admission::Duplicate);
        assert_eq!(r.allocation_bytes(), allocation);
        assert!(matches!(
            r.bump(ticket, 3),
            Err(LocalError::PeerWriterAhead)
        ));
    }
    let mut r = LocalReplica::counter(&root(), config()).unwrap();
    let mut incoming = EventLog::for_crdt(&GCounter::new(2));
    assert_eq!(
        incoming.insert_record(&GCounter::new(2), h1_record(u64::MAX)),
        Admission::Accepted
    );
    // Local/DurableReplica expose single-record receive, not a batch merge.
    // A complete transport log must decode then route every record to receive.
    let decoded = EventLog::<GCounterDelta>::from_wire_bytes_for(
        &incoming.to_wire_bytes().unwrap(),
        r.state(),
    )
    .unwrap();
    for record in decoded.records() {
        assert!(matches!(
            r.receive(r.ticket(), record.clone()),
            Err(LocalError::PeerWriterAhead)
        ));
    }
    assert!(matches!(
        r.bump(r.ticket(), 1),
        Err(LocalError::PeerWriterAhead)
    ));
}
#[test]
fn h1_orset_local_actions_remain_stopped() {
    let mut r = DurableReplica::utf8_set(&root(), config()).unwrap();
    let element = String::from("honest");
    r.add(r.ticket(), element.clone()).unwrap();
    let ahead = Record {
        id: RecordId {
            replica: 0,
            sequence: 2,
        },
        delta: OrSetDelta::Add {
            element: String::from("lost"),
            token: 4,
        },
    };
    assert!(matches!(
        r.receive(r.ticket(), ahead),
        Err(LocalError::PeerWriterAhead)
    ));
    assert!(matches!(
        r.add(r.ticket(), String::from("next")),
        Err(LocalError::PeerWriterAhead)
    ));
    assert!(matches!(
        r.remove(r.ticket(), &element),
        Err(LocalError::PeerWriterAhead)
    ));
    let mut r = LocalReplica::utf8_set(&root(), config()).unwrap();
    let ahead = Record {
        id: RecordId {
            replica: 0,
            sequence: 1,
        },
        delta: OrSetDelta::Add {
            element: element.clone(),
            token: 2,
        },
    };
    assert!(matches!(
        r.receive(r.ticket(), ahead),
        Err(LocalError::PeerWriterAhead)
    ));
    assert!(matches!(
        r.add(r.ticket(), element.clone()),
        Err(LocalError::PeerWriterAhead)
    ));
    assert!(matches!(
        r.remove(r.ticket(), &element),
        Err(LocalError::PeerWriterAhead)
    ));
}
#[test]
fn m3_restore_peer_return_never_reissues_id() {
    let root = root();
    let mut r = DurableReplica::counter(&root, config()).unwrap();
    pre_protection(&mut r);
    r.bump(r.ticket(), 1).unwrap();
    let path = store_file(&root);
    let backup = fs::read(&path).unwrap();
    let lost = r.bump(r.ticket(), 2).unwrap();
    drop(r);
    fs::write(&path, &backup).unwrap();
    let mut r = DurableReplica::restart_counter(&root, config()).unwrap();
    assert!(matches!(
        r.receive(r.ticket(), lost.clone()),
        Err(LocalError::PeerWriterAhead)
    ));
    let before = fs::read(&path).unwrap();
    assert!(
        matches!(r.bump(r.ticket(), 3), Err(LocalError::PeerWriterAhead)),
        "restored writer must stop before reissuing {:?}",
        lost.id
    );
    assert_eq!(before, backup);
    assert_eq!(fs::read(&path).unwrap(), backup);
}
#[test]
fn durable_orset_zero_sequence_remove_refused_on_receive_and_restart() {
    // The stored-history half writes a whole transaction, so use that format.
    let root = legacy("set");
    let mut replica = DurableReplica::restart_utf8_set(&root, config()).unwrap();
    let remove = Record {
        id: RecordId {
            replica: 1,
            sequence: 0,
        },
        delta: OrSetDelta::Remove { tokens: vec![2] },
    };
    let before = fs::read(transaction_path(&root, config())).unwrap();
    let log_before = replica.log().to_wire_bytes().unwrap();
    assert!(matches!(
        replica.receive(replica.ticket(), remove.clone()),
        Err(LocalError::InvalidRecord(WireError::ZeroSequenceRemove {
            replica: 1
        }))
    ));
    assert_eq!(replica.log().to_wire_bytes().unwrap(), log_before);
    assert!(replica.state().tombstones().is_empty());
    assert_eq!(fs::read(transaction_path(&root, config())).unwrap(), before);
    drop(replica);

    let mut bytes = Vec::new();
    EventLog::encode_records(None, &[remove], &mut bytes).unwrap();
    let inert = EventLog::<OrSetDelta<String, u64>>::from_wire_bytes(&bytes).unwrap();
    DurableReplica::<OrSet<String, u64>>::commit(
        &transaction_path(&root, config()),
        config(),
        &inert,
        0,
    )
    .unwrap();
    let stored = fs::read(transaction_path(&root, config())).unwrap();
    let error = match DurableReplica::restart_utf8_set(&root, config()) {
        Err(error) => error,
        Ok(_) => panic!("restarted from sequence-0 remove"),
    };
    assert!(matches!(
        error,
        LocalError::History(WireError::ZeroSequenceRemove { replica: 1 })
    ));
    assert!(error.to_string().contains("Recovery: "));
    assert_eq!(fs::read(transaction_path(&root, config())).unwrap(), stored);
}
#[test]
fn durable_receive_zero_sequence_add_reports_cause_without_writing() {
    // The stored-history half writes a whole transaction, so use that format.
    let root = legacy("set");
    let mut replica = DurableReplica::restart_utf8_set(&root, config()).unwrap();
    let transaction = transaction_path(&root, config());
    let store_before = fs::read(&transaction).unwrap();
    let log_before = replica.log().to_wire_bytes().unwrap();
    let error = replica
        .receive(
            replica.ticket(),
            Record {
                id: RecordId {
                    replica: 1,
                    sequence: 0,
                },
                delta: OrSetDelta::Add {
                    element: "water".into(),
                    token: 1,
                },
            },
        )
        .unwrap_err();
    assert!(matches!(
        error,
        LocalError::InvalidRecord(WireError::ZeroSequenceAdd { replica: 1 })
    ));
    assert_eq!(
        error.to_string(),
        WireError::ZeroSequenceAdd { replica: 1 }.to_string()
    );
    assert_eq!(fs::read(&transaction).unwrap(), store_before);
    assert_eq!(replica.log().to_wire_bytes().unwrap(), log_before);
    assert!(replica.state().elements().is_empty());
    drop(replica);

    let add: Record<OrSetDelta<String, u64>> = Record {
        id: RecordId {
            replica: 1,
            sequence: 0,
        },
        delta: OrSetDelta::Add {
            element: "water".into(),
            token: 1u64,
        },
    };
    let mut bytes = Vec::new();
    EventLog::encode_records(None, &[add], &mut bytes).unwrap();
    let inert = EventLog::<OrSetDelta<String, u64>>::from_wire_bytes(&bytes).unwrap();
    DurableReplica::<OrSet<String, u64>>::commit(&transaction, config(), &inert, 0).unwrap();
    let stored = fs::read(&transaction).unwrap();
    let error = match DurableReplica::restart_utf8_set(&root, config()) {
        Err(error) => error,
        Ok(_) => panic!("restarted from sequence-0 add"),
    };
    assert!(matches!(
        error,
        LocalError::History(WireError::ZeroSequenceAdd { replica: 1 })
    ));
    assert_eq!(fs::read(&transaction).unwrap(), stored);
    std::println!(
        "store={} log-bytes={}",
        transaction.display(),
        log_before.len()
    );
}
#[test]
fn receive_other_refusals_keep_ownership_text() {
    let expected = LocalError::Refused.to_string();
    let mut set = DurableReplica::utf8_set(&root(), config()).unwrap();
    let remove = set
        .receive(
            set.ticket(),
            Record {
                id: RecordId {
                    replica: 1,
                    sequence: 0,
                },
                delta: OrSetDelta::Remove {
                    tokens: alloc::vec![1],
                },
            },
        )
        .unwrap_err();
    assert!(matches!(
        remove,
        LocalError::InvalidRecord(WireError::ZeroSequenceRemove { replica: 1 })
    ));
    assert_eq!(
        remove.to_string(),
        WireError::ZeroSequenceRemove { replica: 1 }.to_string()
    );
    let outside = set
        .receive(
            set.ticket(),
            Record {
                id: RecordId {
                    replica: 2,
                    sequence: 1,
                },
                delta: OrSetDelta::Add {
                    element: "water".into(),
                    token: 4,
                },
            },
        )
        .unwrap_err();
    assert!(matches!(outside, LocalError::Refused));
    assert_eq!(outside.to_string(), expected);
    let outside_zero = set
        .receive(
            set.ticket(),
            Record {
                id: RecordId {
                    replica: 2,
                    sequence: 0,
                },
                delta: OrSetDelta::Add {
                    element: "water".into(),
                    token: 2,
                },
            },
        )
        .unwrap_err();
    assert!(matches!(outside_zero, LocalError::Refused));
    assert_eq!(outside_zero.to_string(), expected);
    let stale = set.ticket();
    set.renew(stale).unwrap();
    let stale_zero = set
        .receive(
            stale,
            Record {
                id: RecordId {
                    replica: 1,
                    sequence: 0,
                },
                delta: OrSetDelta::Add {
                    element: "water".into(),
                    token: 1,
                },
            },
        )
        .unwrap_err();
    assert!(matches!(stale_zero, LocalError::Refused));
    assert_eq!(stale_zero.to_string(), expected);

    let mut counter = DurableReplica::counter(&root(), config()).unwrap();
    let gcounter = counter
        .receive(
            counter.ticket(),
            Record {
                id: RecordId {
                    replica: 1,
                    sequence: 0,
                },
                delta: GCounterDelta {
                    replica: 1,
                    tally: 7,
                },
            },
        )
        .unwrap_err();
    assert!(matches!(gcounter, LocalError::Refused));
    assert_eq!(gcounter.to_string(), expected);

    let mut pn = DurableReplica::fresh(&root(), config(), crate::PnCounter::new(2)).unwrap();
    let pncounter = pn
        .receive(
            pn.ticket(),
            Record {
                id: RecordId {
                    replica: 1,
                    sequence: 0,
                },
                delta: crate::PnCounterDelta::Inc {
                    replica: 1,
                    tally: 7,
                },
            },
        )
        .unwrap_err();
    assert!(matches!(pncounter, LocalError::Refused));
    assert_eq!(pncounter.to_string(), expected);
}
#[test]
fn counter_width_constructor_boundaries() {
    for writers in [0, 4096, 4097] {
        let config = WriterConfig { writers, writer: 0 };
        let local = LocalReplica::counter(&root(), config);
        let durable_root = root();
        let durable = DurableReplica::counter(&durable_root, config);
        if writers == 4097 {
            assert!(matches!(
                local,
                Err(LocalError::CounterWidth(
                    crate::CoordinateError::ReplicaLimitExceeded {
                        requested: 4097,
                        maximum: 4096
                    }
                ))
            ));
            assert!(matches!(durable, Err(LocalError::CounterWidth(_))));
            assert!(matches!(
                DurableReplica::restart_counter(&durable_root, config),
                Err(LocalError::CounterWidth(_))
            ));
        } else if writers == 0 {
            assert!(matches!(local, Err(LocalError::Configuration)));
            assert!(matches!(durable, Err(LocalError::Configuration)));
        } else {
            assert_eq!(local.unwrap().state().len(), 4096);
            drop(durable.unwrap());
            assert_eq!(
                DurableReplica::restart_counter(&durable_root, config)
                    .unwrap()
                    .state()
                    .len(),
                4096
            );
            assert_eq!(
                DurableReplica::restart_counter_from_store(&durable_root, 0)
                    .unwrap()
                    .state()
                    .len(),
                4096
            );
        }
    }
}

#[test]
fn counter_width_restart_is_named_and_preserves_store() {
    for writers in [4097, usize::MAX as u64] {
        let root = root();
        let config = WriterConfig { writers, writer: 0 };
        // Match the pre-cap fresh log shape through the product encoder.
        let log = EventLog::<GCounterDelta>::with_replica_count(writers as usize);
        let mut transaction = [
            writers.to_le_bytes(),
            0u64.to_le_bytes(),
            0u64.to_le_bytes(),
        ]
        .concat();
        transaction.extend(log.to_wire_bytes().unwrap());
        let fence = [
            writers.to_le_bytes(),
            0u64.to_le_bytes(),
            1u64.to_le_bytes(),
        ]
        .concat();
        fs::write(transaction_path(&root, config), &transaction).unwrap();
        let fence_path = root.join("writer-0.fence");
        fs::write(&fence_path, &fence).unwrap();
        for _ in 0..2 {
            let error = DurableReplica::restart_counter_from_store(&root, 0)
                .err()
                .unwrap();
            assert!(
                matches!(error, LocalError::CounterWidth(crate::CoordinateError::ReplicaLimitExceeded { requested, maximum: 4096 }) if requested == writers as usize)
            );
            let message = error.to_string();
            assert!(
                message.contains(&writers.to_string()) && message.contains("4096"),
                "{message}"
            );
            assert_eq!(
                fs::read(transaction_path(&root, config)).unwrap(),
                transaction
            );
            assert_eq!(fs::read(&fence_path).unwrap(), fence);
        }
        assert!(matches!(
            DurableReplica::restart_counter(
                &root,
                WriterConfig {
                    writers: 2,
                    writer: 0
                }
            ),
            Err(LocalError::Configuration)
        ));
        assert_eq!(
            fs::read(transaction_path(&root, config)).unwrap(),
            transaction
        );
        assert_eq!(fs::read(&fence_path).unwrap(), fence);
    }
}

#[test]
fn restart_counter_from_store_replays_without_writer_count() {
    let root = root();
    let mut replica = DurableReplica::counter(&root, config()).unwrap();
    replica.bump(replica.ticket(), 5).unwrap();
    drop(replica);

    let replica = DurableReplica::restart_counter_from_store(&root, 0).unwrap();
    assert_eq!(replica.state().value(), 5);
    assert_eq!(replica.allocation_bytes()[..8], 2u64.to_le_bytes());
}
#[test]
fn restart_counter_from_store_rejects_missing_and_invalid_metadata() {
    let root = root();
    assert!(matches!(
        DurableReplica::restart_counter_from_store(&root, 0),
        Err(LocalError::Io(_))
    ));
    let replica = DurableReplica::counter(&root, config()).unwrap();
    drop(replica);
    // The append-log header: magic, then writer count and writer.
    let path = journal::path(&root, 0);
    let original = fs::read(&path).unwrap();
    fs::write(&path, &original[..journal::HEADER - 1]).unwrap();
    assert!(matches!(
        DurableReplica::restart_counter_from_store(&root, 0),
        Err(LocalError::RecoveryRequired)
    ));
    let mut invalid = original.clone();
    invalid[8..16].copy_from_slice(&1u64.to_le_bytes());
    invalid[16..24].copy_from_slice(&1u64.to_le_bytes());
    fs::write(&path, invalid).unwrap();
    assert!(matches!(
        DurableReplica::restart_counter_from_store(&root, 0),
        Err(LocalError::Configuration)
    ));
    fs::write(&path, &original).unwrap();
    assert!(DurableReplica::restart_counter_from_store(&root, 0).is_ok());

    let root = legacy("counter");
    let path = transaction_path(&root, config());
    let original = fs::read(&path).unwrap();

    fs::remove_file(&path).unwrap();
    assert!(matches!(
        DurableReplica::restart_counter_from_store(&root, 0),
        Err(LocalError::Io(_))
    ));
    fs::write(&path, &original[..8]).unwrap();
    assert!(matches!(
        DurableReplica::restart_counter_from_store(&root, 0),
        Err(LocalError::RecoveryRequired)
    ));

    let mut invalid = original.clone();
    invalid[..8].copy_from_slice(&1u64.to_le_bytes());
    invalid[8..16].copy_from_slice(&1u64.to_le_bytes());
    fs::write(&path, invalid).unwrap();
    assert!(matches!(
        DurableReplica::restart_counter_from_store(&root, 0),
        Err(LocalError::Configuration)
    ));
    fs::write(&path, original).unwrap();
    assert!(DurableReplica::restart_counter_from_store(&root, 0).is_ok());
}
#[test]
fn restart_counter_from_store_checks_writer_and_live_lease() {
    let root = root();
    let replica = DurableReplica::counter(&root, config()).unwrap();
    assert!(matches!(
        DurableReplica::restart_counter_from_store(&root, 0),
        Err(LocalError::Refused)
    ));
    drop(replica);

    fs::copy(root.join("writer-0.fence"), root.join("writer-1.fence")).unwrap();
    fs::copy(root.join("writer-0.journal"), root.join("writer-1.journal")).unwrap();
    assert!(matches!(
        DurableReplica::restart_counter_from_store(&root, 1),
        Err(LocalError::Configuration)
    ));
    assert!(DurableReplica::restart_counter_from_store(&root, 0).is_ok());
}
#[test]
fn restart_counter_from_archived_store() {
    // These committed files were generated by the archived 3a9179b library.
    let root = root();
    fs::write(
        root.join("writer-0.fence"),
        include_bytes!("../tests/fixtures/bootstrap/counter.fence"),
    )
    .unwrap();
    fs::write(
        root.join("writer-0.transaction"),
        include_bytes!("../tests/fixtures/bootstrap/counter.transaction"),
    )
    .unwrap();
    let replica = DurableReplica::restart_counter_from_store(&root, 0).unwrap();
    assert_eq!(replica.state().state(), &[5, 7]);
}
#[test]
fn fresh_root_second_order() {
    let parent = root();
    for name in ["empty", "stray", "symlink"] {
        let target = parent.join(format!("target-{name}"));
        fs::create_dir(&target).unwrap();
        if name == "stray" {
            fs::write(target.join("stray"), b"keep").unwrap();
        }
        let store = if name == "symlink" {
            let link = parent.join("link");
            std::os::unix::fs::symlink(&target, &link).unwrap();
            link
        } else {
            target.clone()
        };
        std::eprintln!("ROOT-PROBE fresh {}", store.display());
        let replica = DurableReplica::counter(&store, config()).unwrap();
        drop(replica);
        std::eprintln!("ROOT-PROBE reopen {}", store.display());
        let mut replica = DurableReplica::restart_counter(&store, config()).unwrap();
        let ticket = replica.ticket();
        replica.bump(ticket, 1).unwrap();
        drop(replica);
        std::eprintln!("ROOT-PROBE restart {}", store.display());
        let replica = DurableReplica::restart_counter(&store, config()).unwrap();
        assert_eq!(replica.state().value(), 1);
        drop(replica);
        if name == "stray" {
            assert_eq!(fs::read(target.join("stray")).unwrap(), b"keep");
        }
    }
}

#[test]
fn fresh_durable_counter_creates_missing_root() {
    let store = root().join("fresh-counter");
    assert!(!store.exists());
    let replica = DurableReplica::counter(&store, config()).unwrap();
    assert!(store.is_dir());
    assert_eq!(replica.state().value(), 0);
}
#[test]
fn fresh_durable_set_creates_missing_root() {
    let store = root().join("fresh-set");
    assert!(!store.exists());
    let replica = DurableReplica::utf8_set(&store, config()).unwrap();
    assert!(store.is_dir());
    assert!(replica.state().elements().is_empty());
}
#[test]
fn fresh_durable_root_inputs() {
    let parent = root();
    let nested = parent.join("missing-parent/store");
    assert!(DurableReplica::counter(&nested, config()).is_ok());
    assert!(nested.is_dir());

    let file = parent.join("regular-file");
    fs::write(&file, b"file").unwrap();
    assert!(matches!(
        DurableReplica::counter(&file, config()),
        Err(LocalError::Io(_))
    ));

    let dangling = parent.join("dangling-link");
    std::os::unix::fs::symlink(parent.join("missing-target"), &dangling).unwrap();
    assert!(matches!(
        DurableReplica::counter(&dangling, config()),
        Err(LocalError::Io(_))
    ));
    assert!(!parent.join("missing-target").exists());

    let missing = parent.join("restart-missing");
    assert!(matches!(
        DurableReplica::restart_counter(&missing, config()),
        Err(LocalError::Io(_))
    ));
    assert!(matches!(
        DurableReplica::restart_utf8_set(&missing, config()),
        Err(LocalError::Io(_))
    ));
    assert!(!missing.exists());
}
#[test]
fn fresh_durable_race_and_live_writer() {
    use std::sync::{Arc, Barrier};

    let store = root().join("raced-store");
    let start = Arc::new(Barrier::new(2));
    let finish = Arc::new(Barrier::new(2));
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let store = store.clone();
            let start = start.clone();
            let finish = finish.clone();
            std::thread::spawn(move || {
                start.wait();
                let result = DurableReplica::counter(&store, config());
                let outcome = match &result {
                    Ok(_) => "ok",
                    Err(LocalError::StoreExists) => "store exists",
                    Err(LocalError::RecoveryRequired) => "recovery",
                    Err(LocalError::Refused) => "refused",
                    Err(_) => "unexpected",
                };
                finish.wait();
                outcome
            })
        })
        .collect();
    let outcomes: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(
        outcomes.iter().filter(|&&outcome| outcome == "ok").count(),
        1
    );
    assert!(outcomes.iter().all(|outcome| *outcome != "unexpected"));

    let live_root = root().join("live");
    let _live = DurableReplica::counter(&live_root, config()).unwrap();
    assert!(matches!(
        DurableReplica::counter(&live_root, config()),
        Err(LocalError::StoreExists)
    ));
}
#[test]
fn second_create_is_store_exists() {
    let root = root();
    let first = DurableReplica::counter(&root, config()).unwrap();
    drop(first);
    assert!(matches!(
        DurableReplica::counter(&root, config()),
        Err(LocalError::StoreExists)
    ));
}
#[test]
fn truncated_fence_restart_is_recovery_required() {
    let root = root();
    let first = DurableReplica::counter(&root, config()).unwrap();
    drop(first);
    let fence = root.join("writer-0.fence");
    let original = fs::read(&fence).unwrap();
    fs::write(&fence, &original[..23]).unwrap();
    let before = store_files(&root);
    assert!(matches!(
        DurableReplica::restart_counter(&root, config()),
        Err(LocalError::RecoveryRequired)
    ));
    assert_eq!(store_files(&root), before, "refusal changed store files");
}
#[test]
fn store_exists_and_recovery_texts_differ() {
    let exists = LocalError::StoreExists.to_string();
    let recovery = LocalError::RecoveryRequired.to_string();
    assert_ne!(exists, recovery);
    assert!(exists.contains("already exists"));
    assert!(recovery.contains("requires recovery"));
}
#[test]
fn fresh_ancestor_sync_refusal_and_retry() {
    use std::os::unix::fs::PermissionsExt;
    if std::process::Command::new("id")
        .arg("-u")
        .output()
        .unwrap()
        .stdout
        == b"0\n"
    {
        std::eprintln!("SKIP: root bypasses execute-only ancestor permissions");
        return;
    }
    let ancestor = root().join("execute-only");
    let writable = ancestor.join("writable");
    fs::create_dir_all(&writable).unwrap();
    let ancestor = ancestor.canonicalize().unwrap();
    let store = ancestor.join("writable/store");
    fs::set_permissions(&ancestor, fs::Permissions::from_mode(0o111)).unwrap();
    std::eprintln!("ANCESTOR-PROBE refusal {}", ancestor.display());
    let result = DurableReplica::counter(&store, config());
    fs::set_permissions(&ancestor, fs::Permissions::from_mode(0o755)).unwrap();
    let error = result
        .err()
        .expect("execute-only ancestor must refuse creation");
    assert!(error.to_string().contains(&ancestor.display().to_string()));
    assert!(error.to_string().contains("Permission denied"));
    assert!(matches!(error, LocalError::AncestorSync { path, source }
            if path == ancestor && source.kind() == io::ErrorKind::PermissionDenied));
    assert!(store.is_dir());
    assert!(!store.join("writer-0.fence").exists());
    std::eprintln!("ANCESTOR-PROBE retry {}", store.display());
    let replica = DurableReplica::counter(&store, config()).unwrap();
    assert!(store.join("writer-0.fence").exists());
    drop(replica);
    std::eprintln!("ANCESTOR-PROBE reopen {}", store.display());
    drop(DurableReplica::restart_counter(&store, config()).unwrap());
}
#[test]
fn fresh_durable_read_only_parent() {
    use std::os::unix::fs::PermissionsExt;

    let parent = root();
    let original = fs::metadata(&parent).unwrap().permissions();
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o555)).unwrap();
    let result = DurableReplica::counter(&parent.join("store"), config());
    fs::set_permissions(&parent, original).unwrap();
    assert!(
        matches!(result, Err(LocalError::Io(ref error)) if error.kind() == io::ErrorKind::PermissionDenied)
    );
    assert!(!parent.join("store").exists());
}
// Seed a committed store of `records` records in one transaction: allocate
// and admit in memory through the owned writer, then commit once. Growing
// it by durable appends would rewrite the whole history per record.
fn seeded<C: Crdt>(
    root: &Path,
    mut replica: DurableReplica<C>,
    records: usize,
    mut edit: impl FnMut(&mut LocalReplica<C>, u64),
) -> DurableReplica<C>
where
    C::Delta: OwnedDelta + Clone + PartialEq + WireEncode + WireSchema,
{
    pre_protection(&mut replica);
    for index in 0..records as u64 {
        edit(&mut replica.inner, index);
    }
    let DurableReplica { inner, store, .. } = &mut replica;
    match store {
        Store::Transaction(path) => {
            DurableReplica::<C>::commit(path, inner.config, &inner.log, inner.last_sequence)
                .unwrap()
        }
        // The whole history as the journal's base frame, as migration writes it.
        Store::Journal { file, len } => {
            let path = journal::path(root, inner.config.writer);
            let log = inner.log.to_wire_bytes().unwrap();
            let header = journal::header(inner.config, inner.last_sequence, &log).unwrap();
            persistence::replace(&path, &header).unwrap();
            *file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
                .unwrap();
            *len = header.len() as u64;
        }
    }
    replica
}
fn store_files(root: &Path) -> Vec<(PathBuf, Vec<u8>, std::time::SystemTime)> {
    let mut files: Vec<_> = fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let modified = fs::metadata(&path).unwrap().modified().unwrap();
            (path.clone(), fs::read(&path).unwrap(), modified)
        })
        .collect();
    files.sort();
    files
}
fn budget(max_records: usize) -> crate::DecodeLimits {
    crate::DecodeLimits {
        max_records: Some(max_records),
        max_collection_elements: None,
    }
}
fn assert_refused_by_name<C: Crdt>(
    result: Result<DurableReplica<C>, LocalError>,
    max_records: usize,
) {
    match result {
        Err(
            error @ LocalError::RecordLimitExceeded {
                max_records: named,
                default_limit,
            },
        ) => {
            assert_eq!(named, max_records);
            assert_eq!(
                    error.to_string(),
                    format!("local history exceeds restart record budget: RecordLimitExceeded: {max_records}{}",
                        if default_limit { "; pass DecodeLimits::UNLIMITED to reopen" } else { "" })
                );
        }
        Err(other) => panic!("expected RecordLimitExceeded, got {other:?}"),
        Ok(replica) => panic!(
            "store of {} records opened under a {max_records}-record budget",
            replica.inner.log.records().len()
        ),
    }
}
#[test]
fn restart_budget_opens_supported_size_and_refuses_one_more() {
    const N: usize = SUPPORTED_MAX_RECORDS;
    // G-Counter: a store of N opens; one more durable bump makes N + 1.
    let root = self::root();
    let fresh = DurableReplica::counter(&root, config()).unwrap();
    drop(seeded(&root, fresh, N, |inner, index| {
        inner.bump(inner.ticket(), index + 1).unwrap();
    }));
    let mut counter =
        DurableReplica::restart_counter_with_limits(&root, config(), budget(N)).unwrap();
    assert_eq!(counter.log().records().len(), N);
    counter.bump(counter.ticket(), N as u64 + 1).unwrap();
    drop(counter);
    let before = store_files(&root);
    assert_refused_by_name(
        DurableReplica::restart_counter_with_limits(&root, config(), budget(N)),
        N,
    );
    let explicit = DurableReplica::restart_counter_with_limits(&root, config(), budget(N))
        .err()
        .unwrap();
    assert_eq!(
        explicit.to_string(),
        format!("local history exceeds restart record budget: RecordLimitExceeded: {N}")
    );
    assert_refused_by_name(
        DurableReplica::restart_counter_from_store_with_limits(&root, 0, budget(N)),
        N,
    );
    assert_eq!(
        store_files(&root),
        before,
        "refusal leaves the store untouched"
    );
    assert_refused_by_name(DurableReplica::restart_counter(&root, config()), N);
    let default = DurableReplica::restart_counter(&root, config())
        .err()
        .unwrap();
    assert_eq!(default.to_string(), format!("local history exceeds restart record budget: RecordLimitExceeded: {N}; pass DecodeLimits::UNLIMITED to reopen"));
    assert_refused_by_name(DurableReplica::restart_counter_from_store(&root, 0), N);
    // Explicit unlimited preserves access to older, oversized histories.
    let reopened = DurableReplica::restart_counter_with_limits(
        &root,
        config(),
        crate::DecodeLimits::UNLIMITED,
    )
    .unwrap();
    assert_eq!(reopened.log().records().len(), N + 1);
    assert_eq!(reopened.state().value(), N as u128 + 1);
    drop(reopened);
    let reopened = DurableReplica::restart_counter_from_store_with_limits(
        &root,
        0,
        crate::DecodeLimits::UNLIMITED,
    )
    .unwrap();
    assert_eq!(reopened.log().records().len(), N + 1);

    // UTF-8 OR-Set: the same boundary through one more durable add.
    let root = self::root();
    let fresh = DurableReplica::utf8_set(&root, config()).unwrap();
    drop(seeded(&root, fresh, N, |inner, index| {
        inner.add(inner.ticket(), format!("m{index}")).unwrap();
    }));
    let mut set = DurableReplica::restart_utf8_set_with_limits(&root, config(), budget(N)).unwrap();
    assert_eq!(set.log().records().len(), N);
    set.add(set.ticket(), "one more".into()).unwrap();
    drop(set);
    let before = store_files(&root);
    assert_refused_by_name(
        DurableReplica::restart_utf8_set_with_limits(&root, config(), budget(N)),
        N,
    );
    assert_eq!(
        store_files(&root),
        before,
        "refusal leaves the store untouched"
    );
    assert_refused_by_name(DurableReplica::restart_utf8_set(&root, config()), N);
    // Explicit unlimited preserves access to this older, oversized set.
    let reopened = DurableReplica::restart_utf8_set_with_limits(
        &root,
        config(),
        crate::DecodeLimits::UNLIMITED,
    )
    .unwrap();
    assert_eq!(reopened.log().records().len(), N + 1);
}
#[test]
fn persisted_150000_record_history_requires_explicit_unlimited_on_restart() {
    const N: usize = 150_000;
    let root = self::root();
    let fresh = DurableReplica::counter(&root, config()).unwrap();
    drop(seeded(&root, fresh, N, |inner, index| {
        inner.bump(inner.ticket(), index + 1).unwrap();
    }));
    let before = store_files(&root);
    assert_refused_by_name(
        DurableReplica::restart_counter(&root, config()),
        crate::DecodeLimits::DEFAULT_MAX_RECORDS,
    );
    assert_eq!(store_files(&root), before);
    let reopened = DurableReplica::restart_counter_with_limits(
        &root,
        config(),
        crate::DecodeLimits::UNLIMITED,
    )
    .unwrap();
    assert_eq!(reopened.log().records().len(), N);
    assert_eq!(reopened.state().value(), N as u128);
}
#[test]
fn restart_budget_refuses_before_reading_records() {
    // A transaction-format store declares its count in the frame header.
    let root = legacy("counter");
    let mut counter = DurableReplica::restart_counter(&root, config()).unwrap();
    for tally in 1..=3 {
        counter.bump(counter.ticket(), tally).unwrap();
    }
    drop(counter);
    // Keep the header through the declared count (3); drop every record and
    // the CRC. Only a refusal from the declared count can name the budget.
    let path = transaction_path(&root, config());
    let bytes = fs::read(&path).unwrap();
    let schema = <GCounterDelta as WireSchema>::wire_schema().len();
    let header = 24 + 1 + 8 + 4 + 4 + schema + 1 + 8 + 4;
    assert_eq!(bytes[header - 4..header], 3u32.to_le_bytes());
    fs::write(&path, &bytes[..header]).unwrap();
    let before = store_files(&root);
    assert_refused_by_name(
        DurableReplica::restart_counter_with_limits(&root, config(), budget(2)),
        2,
    );
    assert_eq!(store_files(&root), before);
    // Within budget, the full checked read reaches the missing records.
    assert!(matches!(
        DurableReplica::restart_counter_with_limits(&root, config(), budget(3)),
        Err(LocalError::History(WireError::UnexpectedEof))
    ));
    fs::write(&path, &bytes).unwrap();
    assert_eq!(
        DurableReplica::restart_counter_with_limits(&root, config(), budget(3))
            .unwrap()
            .state()
            .value(),
        3
    );
}
fn counter_journal(tallies: &[u64]) -> PathBuf {
    let root = root();
    let mut r = DurableReplica::counter(&root, config()).unwrap();
    pre_protection(&mut r);
    for &tally in tallies {
        r.bump(r.ticket(), tally).unwrap();
    }
    root
}
fn store_copy(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let bytes = fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect()
}
fn restore_copy(copy: &[(PathBuf, Vec<u8>)]) {
    for (path, bytes) in copy {
        fs::write(path, bytes).unwrap();
    }
}
fn protected_counter_path(root: &Path, writer: u64) -> PathBuf {
    rollback::paths(root, writer).unwrap().1
}
#[test]
fn refused_create_leaves_no_store_files() {
    let store_root = root();
    let first = DurableReplica::counter(&store_root, config()).unwrap();
    drop(first);
    let counter = protected_counter_path(&store_root, 0);
    let original = fs::read(&counter).unwrap();
    fs::remove_dir_all(&store_root).unwrap();
    assert!(matches!(
        DurableReplica::counter(&store_root, config()),
        Err(LocalError::StoreExists)
    ));
    let entries: Vec<_> = fs::read_dir(&store_root).unwrap().collect();
    assert!(
        entries.is_empty(),
        "refused create left {} store entries",
        entries.len()
    );
    assert_eq!(fs::read(&counter).unwrap(), original);
    let other = WriterConfig {
        writers: 2,
        writer: 1,
    };
    assert!(DurableReplica::counter(&store_root, other).is_ok());

    let set_root = root();
    drop(DurableReplica::utf8_set(&set_root, config()).unwrap());
    let set_counter = protected_counter_path(&set_root, 0);
    let set_original = fs::read(&set_counter).unwrap();
    fs::remove_dir_all(&set_root).unwrap();
    assert!(matches!(
        DurableReplica::utf8_set(&set_root, config()),
        Err(LocalError::StoreExists)
    ));
    assert_eq!(fs::read_dir(&set_root).unwrap().count(), 0);
    assert_eq!(fs::read(&set_counter).unwrap(), set_original);
}
#[test]
fn existing_store_refusal_preserves_files() {
    let root = root();
    drop(LocalReplica::counter(&root, config()).unwrap());
    let fence = root.join("writer-0.fence");
    let before = fs::read(&fence).unwrap();
    assert!(matches!(
        DurableReplica::counter(&root, config()),
        Err(LocalError::StoreExists)
    ));
    assert_eq!(fs::read(&fence).unwrap(), before);
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
}
#[test]
fn rollback_single_advance_refuses_skipped_sequence() {
    let root = root();
    let mut r = DurableReplica::counter(&root, config()).unwrap();
    r.bump(r.ticket(), 1).unwrap();
    let counter = protected_counter_path(&root, 0);
    let before = fs::read(&counter).unwrap();

    let result = r.rollback.as_mut().unwrap().advance(3);
    assert!(
        matches!(&result, Err(LocalError::RecoveryRequired)),
        "skipped advance returned {:?}",
        result
    );
    assert_eq!(fs::read(&counter).unwrap(), before);
}
#[test]
fn rollback_r1_store_only_restore_refuses() {
    let root = root();
    let mut r = DurableReplica::counter(&root, config()).unwrap();
    r.bump(r.ticket(), 1).unwrap();
    let copy = store_copy(&root);
    r.bump(r.ticket(), 2).unwrap();
    drop(r);
    restore_copy(&copy);
    assert!(matches!(
        DurableReplica::restart_counter(&root, config()),
        Err(LocalError::RecoveryRequired)
    ));
}
#[test]
fn rollback_r2_new_writer_after_refusal() {
    let root = root();
    let mut old = DurableReplica::counter(&root, config()).unwrap();
    old.bump(old.ticket(), 1).unwrap();
    let copy = store_copy(&root);
    old.bump(old.ticket(), 2).unwrap();
    drop(old);
    restore_copy(&copy);
    assert!(matches!(
        DurableReplica::restart_counter(&root, config()),
        Err(LocalError::RecoveryRequired)
    ));
    let new_config = WriterConfig {
        writers: 2,
        writer: 1,
    };
    let mut new = DurableReplica::counter(&root, new_config).unwrap();
    let record = new.bump(new.ticket(), 1).unwrap();
    assert_eq!(record.id.replica, 1);
}
#[test]
fn rollback_r3_honest_restart_100_times() {
    let root = root();
    let mut r = DurableReplica::counter(&root, config()).unwrap();
    r.bump(r.ticket(), 1).unwrap();
    drop(r);
    for _ in 0..100 {
        let r = DurableReplica::restart_counter(&root, config()).unwrap();
        assert_eq!(r.state().value(), 1);
    }
}
#[test]
fn rollback_crash_child() {
    let Ok(root) = std::env::var("SAFEMESH_ROLLBACK_CRASH_ROOT") else {
        return;
    };
    let boundary = std::env::var("SAFEMESH_ROLLBACK_CRASH_BOUNDARY")
        .unwrap()
        .parse()
        .unwrap();
    let mut r = DurableReplica::restart_counter(Path::new(&root), config()).unwrap();
    fault(boundary, true);
    r.bump(r.ticket(), 2).unwrap();
    panic!("crash checkpoint did not exit");
}
#[test]
fn rollback_batch_crash_child() {
    let Ok(root) = std::env::var("SAFEMESH_ROLLBACK_BATCH_ROOT") else {
        return;
    };
    let boundary = std::env::var("SAFEMESH_ROLLBACK_BATCH_BOUNDARY")
        .unwrap()
        .parse()
        .unwrap();
    let mut r = DurableReplica::restart_counter(Path::new(&root), config()).unwrap();
    fault(boundary, true);
    r.bump_batch(r.ticket(), [2, 3, 4]).unwrap();
    panic!("batch crash checkpoint did not exit");
}
#[test]
fn rollback_batch_crash_boundaries() {
    for boundary in [19, 20, 21, 13, 14] {
        let root = root();
        let mut r = DurableReplica::counter(&root, config()).unwrap();
        r.bump(r.ticket(), 1).unwrap();
        drop(r);
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "local::durable_tests::rollback_batch_crash_child",
            ])
            .env("SAFEMESH_ROLLBACK_BATCH_ROOT", &root)
            .env("SAFEMESH_ROLLBACK_BATCH_BOUNDARY", boundary.to_string())
            .output()
            .unwrap();
        let status = root.join("rollback-batch-child.exit");
        fs::write(&status, output.status.code().unwrap().to_string()).unwrap();
        assert_eq!(fs::read_to_string(status).unwrap(), "77", "{output:?}");
        match boundary {
            19 => assert_eq!(
                DurableReplica::restart_counter(&root, config())
                    .unwrap()
                    .state()
                    .value(),
                1
            ),
            20 | 21 => {
                assert!(
                    matches!(
                        DurableReplica::restart_counter(&root, config()),
                        Err(LocalError::RecoveryRequired)
                    ),
                    "boundary {boundary} must refuse"
                );
                let other = WriterConfig {
                    writers: 2,
                    writer: 1,
                };
                assert!(DurableReplica::counter(&root, other).is_ok());
            }
            13 => match DurableReplica::restart_counter(&root, config()) {
                Ok(restarted) => {
                    std::println!("batch boundary 13: complete journal was visible");
                    assert_eq!(restarted.state().value(), 4);
                }
                Err(LocalError::RecoveryRequired) => {
                    std::println!("batch boundary 13: restart refused");
                }
                Err(error) => panic!("boundary 13: {error}"),
            },
            14 => assert_eq!(
                DurableReplica::restart_counter(&root, config())
                    .unwrap()
                    .state()
                    .value(),
                4
            ),
            _ => unreachable!(),
        }
    }
}
#[test]
fn rollback_batch_empty_and_refused_write_nothing() {
    let root = root();
    let mut r = DurableReplica::counter(&root, config()).unwrap();
    let path = store_file(&root);
    let counter = protected_counter_path(&root, 0);
    let journal_bytes = fs::read(&path).unwrap();
    let counter_bytes = fs::read(&counter).unwrap();
    assert!(r.bump_batch(r.ticket(), []).unwrap().is_empty());
    assert_eq!(fs::read(&path).unwrap(), journal_bytes);
    assert_eq!(fs::read(&counter).unwrap(), counter_bytes);
    assert!(matches!(
        r.receive(r.ticket(), h1_record(u64::MAX)),
        Err(LocalError::PeerWriterAhead)
    ));
    assert!(matches!(
        r.bump_batch(r.ticket(), [1, 2]),
        Err(LocalError::PeerWriterAhead)
    ));
    assert_eq!(fs::read(&path).unwrap(), journal_bytes);
    assert_eq!(fs::read(&counter).unwrap(), counter_bytes);
}
#[test]
fn rollback_r4_crash_write_boundaries() {
    for (boundary, refused, value) in [(19, false, 1), (20, true, 1), (14, false, 2)] {
        let root = root();
        let mut r = DurableReplica::counter(&root, config()).unwrap();
        r.bump(r.ticket(), 1).unwrap();
        drop(r);
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "local::durable_tests::rollback_crash_child"])
            .env("SAFEMESH_ROLLBACK_CRASH_ROOT", &root)
            .env("SAFEMESH_ROLLBACK_CRASH_BOUNDARY", boundary.to_string())
            .output()
            .unwrap();
        let status = root.join("rollback-child.exit");
        fs::write(&status, output.status.code().unwrap().to_string()).unwrap();
        assert_eq!(fs::read_to_string(status).unwrap(), "77", "{output:?}");
        if refused {
            assert!(matches!(
                DurableReplica::restart_counter(&root, config()),
                Err(LocalError::RecoveryRequired)
            ));
            let new_config = WriterConfig {
                writers: 2,
                writer: 1,
            };
            assert!(DurableReplica::counter(&root, new_config).is_ok());
        } else {
            let r = DurableReplica::restart_counter(&root, config()).unwrap();
            assert_eq!(r.state().value(), value);
        }
    }
}
#[test]
fn rollback_r5_deleted_counter_refuses() {
    let root = root();
    let mut r = DurableReplica::counter(&root, config()).unwrap();
    r.bump(r.ticket(), 1).unwrap();
    drop(r);
    fs::remove_file(protected_counter_path(&root, 0)).unwrap();
    assert!(matches!(
        DurableReplica::restart_counter(&root, config()),
        Err(LocalError::RecoveryRequired)
    ));
}
#[test]
fn rollback_r6_counter_damage_refuses() {
    let root = root();
    let mut r = DurableReplica::counter(&root, config()).unwrap();
    r.bump(r.ticket(), 1).unwrap();
    drop(r);
    let path = protected_counter_path(&root, 0);
    let original = fs::read(&path).unwrap();
    let mut flipped = original.clone();
    flipped[40] ^= 1;
    let mut trailing = original.clone();
    trailing.push(0);
    let mut impossible = original.clone();
    impossible[40..48].copy_from_slice(&u64::MAX.to_le_bytes());
    let checksum = crate::codec::frame_crc32(&impossible[..48]);
    impossible[48..52].copy_from_slice(&checksum.to_le_bytes());
    for bytes in [Vec::new(), flipped, trailing, impossible] {
        fs::write(&path, bytes).unwrap();
        assert!(matches!(
            DurableReplica::restart_counter(&root, config()),
            Err(LocalError::RecoveryRequired)
        ));
    }
    fs::write(&path, original).unwrap();
    assert!(DurableReplica::restart_counter(&root, config()).is_ok());
}
#[test]
fn rollback_counter_symlink_refuses_without_reading_target() {
    let root = root();
    drop(DurableReplica::counter(&root, config()).unwrap());
    let path = protected_counter_path(&root, 0);
    let victim = root.join("victim");
    fs::write(&victim, b"keep").unwrap();
    fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(&victim, &path).unwrap();
    assert!(matches!(
        DurableReplica::restart_counter(&root, config()),
        Err(LocalError::RecoveryRequired)
    ));
    assert_eq!(fs::read(victim).unwrap(), b"keep");
}
#[test]
fn rollback_r7_limit_whole_machine_copy_passes() {
    let root = root();
    let mut r = DurableReplica::counter(&root, config()).unwrap();
    r.bump(r.ticket(), 1).unwrap();
    let copy = store_copy(&root);
    let counter_path = protected_counter_path(&root, 0);
    let counter = fs::read(&counter_path).unwrap();
    r.bump(r.ticket(), 2).unwrap();
    drop(r);
    restore_copy(&copy);
    fs::write(counter_path, counter).unwrap();
    let r = DurableReplica::restart_counter(&root, config()).unwrap();
    assert_eq!(r.state().value(), 1);
}
#[test]
fn rollback_r8_two_writers_two_counters() {
    let root = root();
    let other = WriterConfig {
        writers: 2,
        writer: 1,
    };
    let mut first = DurableReplica::counter(&root, config()).unwrap();
    let mut second = DurableReplica::counter(&root, other).unwrap();
    assert_ne!(
        protected_counter_path(&root, 0),
        protected_counter_path(&root, 1)
    );
    first.bump(first.ticket(), 1).unwrap();
    second.bump(second.ticket(), 1).unwrap();
    drop(first);
    drop(second);
    assert!(DurableReplica::restart_counter(&root, config()).is_ok());
    assert!(DurableReplica::restart_counter(&root, other).is_ok());
}
#[test]
fn rollback_limit_other_writer_only_restore_passes() {
    let root = root();
    let other = WriterConfig {
        writers: 2,
        writer: 1,
    };
    let mut receiver = DurableReplica::counter(&root, config()).unwrap();
    let mut sender = DurableReplica::counter(&root, other).unwrap();
    let first = sender.bump(sender.ticket(), 1).unwrap();
    receiver.receive(receiver.ticket(), first).unwrap();
    let copy = store_copy(&root);
    let second = sender.bump(sender.ticket(), 2).unwrap();
    receiver.receive(receiver.ticket(), second).unwrap();
    drop(receiver);
    drop(sender);
    restore_copy(&copy);
    let receiver = DurableReplica::restart_counter(&root, config()).unwrap();
    assert_eq!(receiver.state().state()[1], 1);
}
#[test]
fn rollback_check_precedes_saved_collision_alarm() {
    let root = root();
    let mut r = DurableReplica::counter(&root, config()).unwrap();
    r.bump(r.ticket(), 1).unwrap();
    let conflict = Record {
        id: RecordId {
            replica: 0,
            sequence: 1,
        },
        delta: GCounterDelta {
            replica: 0,
            tally: 9,
        },
    };
    assert_eq!(
        r.receive(r.ticket(), conflict).unwrap(),
        Admission::Collision
    );
    assert!(root.join("writer-0.alarms").exists());
    let copy = store_copy(&root);
    r.bump(r.ticket(), 2).unwrap();
    drop(r);
    restore_copy(&copy);
    assert!(matches!(
        DurableReplica::restart_counter(&root, config()),
        Err(LocalError::RecoveryRequired)
    ));
}
fn restart_state(root: &Path) -> Result<(Vec<u64>, Option<TornTail>), LocalError> {
    let r = DurableReplica::restart_counter(root, config())?;
    Ok((r.state().state().to_vec(), r.torn_tail()))
}
#[test]
fn append_log_writes_one_entry_per_record() {
    let root = counter_journal(&[1]);
    let path = journal::path(&root, 0);
    let mut r = DurableReplica::restart_counter(&root, config()).unwrap();
    for tally in 2..=50 {
        let before = fs::read(&path).unwrap();
        let record = r.bump(r.ticket(), tally).unwrap();
        let after = fs::read(&path).unwrap();
        // The earlier bytes are untouched; exactly one entry follows them.
        assert_eq!(after[..before.len()], before[..]);
        let entry = journal::entry(tally, &record.to_wire_bytes().unwrap()).unwrap();
        assert_eq!(after[before.len()..], entry[..]);
    }
    assert!(!transaction_path(&root, config()).exists());
}
#[test]
fn append_log_torn_tail_is_truncated_and_reported() {
    let full = fs::read(journal::path(&counter_journal(&[5, 9, 12]), 0)).unwrap();
    let two = fs::read(journal::path(&counter_journal(&[5, 9]), 0)).unwrap();
    let last = full.len() - two.len();
    let mut flipped = full.clone();
    *flipped.last_mut().unwrap() ^= 1;
    let mut zero_fill = full.clone();
    zero_fill.extend([0u8; 64]);
    let mut short_pair = full.clone();
    short_pair.extend([7u8; 3]);
    for (case, bytes, state, kept) in [
        (
            "final entry cut short",
            full[..full.len() - 7].to_vec(),
            [9, 0],
            two.len(),
        ),
        ("final entry checksum", flipped, [9, 0], two.len()),
        ("zero fill", zero_fill, [12, 0], full.len()),
        ("length pair cut short", short_pair, [12, 0], full.len()),
    ] {
        let root = counter_journal(&[]);
        let path = journal::path(&root, 0);
        fs::write(&path, &bytes).unwrap();
        let (restored, torn) = restart_state(&root).unwrap();
        std::println!("torn tail case={case} state={restored:?} torn_tail={torn:?}");
        assert_eq!(restored, state, "{case}");
        assert_eq!(
            torn,
            Some(TornTail {
                offset: kept as u64,
                discarded_bytes: (bytes.len() - kept) as u64
            }),
            "{case}"
        );
        assert_eq!(fs::read(&path).unwrap(), bytes[..kept], "{case}: truncated");
        let mut r = DurableReplica::restart_counter(&root, config()).unwrap();
        assert_eq!(r.torn_tail(), None, "{case}: nothing left to discard");
        r.bump(r.ticket(), 30).unwrap();
        drop(r);
        assert_eq!(restart_state(&root).unwrap(), (vec![30, 0], None), "{case}");
    }
    assert!(last > 7);
}
#[test]
fn append_log_damage_before_the_tail_is_refused() {
    let root = counter_journal(&[5, 9, 12]);
    let path = journal::path(&root, 0);
    let original = fs::read(&path).unwrap();
    let first = journal::HEADER + u32::from_le_bytes(original[32..36].try_into().unwrap()) as usize;
    let mut payload = original.clone();
    payload[first + 8 + 8 + 4] ^= 1;
    let mut pair = original.clone();
    pair[first] ^= 1;
    let mut base = original.clone();
    base[first - 1] ^= 1;
    let mut writers = original.clone();
    writers[8..16].copy_from_slice(&3u64.to_le_bytes());
    let mut magic = original.clone();
    magic[0] ^= 1;
    for (case, bytes, expected) in [
        (
            "record before others",
            payload,
            "History(IntegrityMismatch)",
        ),
        (
            "length pair before others",
            pair,
            "History(IntegrityMismatch)",
        ),
        ("base frame", base, "History(IntegrityMismatch)"),
        ("writer count", writers, "Configuration"),
        ("magic", magic, "RecoveryRequired"),
        (
            "header cut short",
            original[..journal::HEADER - 1].to_vec(),
            "RecoveryRequired",
        ),
    ] {
        fs::write(&path, &bytes).unwrap();
        let error = restart_state(&root).unwrap_err();
        std::println!("damaged journal case={case} restart={error:?}");
        assert_eq!(format!("{error:?}"), expected, "{case}");
        assert_eq!(fs::read(&path).unwrap(), bytes, "{case}: journal unchanged");
    }
    fs::write(&path, &original).unwrap();
    assert_eq!(restart_state(&root).unwrap(), (vec![12, 0], None));
}
#[test]
fn restart_budget_counts_append_log_without_decoding() {
    let root = counter_journal(&[1, 2, 3]);
    let path = journal::path(&root, 0);
    let original = fs::read(&path).unwrap();
    // Damage every record payload but keep each length pair: only a count
    // taken from the pairs can name the budget.
    let mut bytes = original.clone();
    let mut at =
        journal::HEADER + u32::from_le_bytes(original[32..36].try_into().unwrap()) as usize;
    let mut entries = 0;
    while at < bytes.len() {
        let len = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        bytes[at + 8 + 8] ^= 0xff;
        at += 8 + len + 4;
        entries += 1;
    }
    assert_eq!(entries, 3);
    fs::write(&path, &bytes).unwrap();
    let before = store_files(&root);
    assert_refused_by_name(
        DurableReplica::restart_counter_with_limits(&root, config(), budget(2)),
        2,
    );
    assert_refused_by_name(
        DurableReplica::restart_counter_from_store_with_limits(&root, 0, budget(2)),
        2,
    );
    assert_eq!(store_files(&root), before);
    assert!(matches!(
        DurableReplica::restart_counter_with_limits(&root, config(), budget(3)),
        Err(LocalError::History(WireError::IntegrityMismatch))
    ));
    fs::write(&path, &original).unwrap();
    assert_eq!(
        DurableReplica::restart_counter_with_limits(&root, config(), budget(3))
            .unwrap()
            .state()
            .value(),
        3
    );
}
#[test]
fn transaction_store_opens_appends_in_place_and_migrates_explicitly() {
    let root = legacy("set");
    let mut r = DurableReplica::restart_utf8_set(&root, config()).unwrap();
    for index in 0..5_000 {
        r.receive(
            r.ticket(),
            Record {
                id: RecordId {
                    replica: 1,
                    sequence: index + 1,
                },
                delta: OrSetDelta::Add {
                    element: format!("kept-{index:04}"),
                    token: allocate_token(2, 1, index + 1).unwrap(),
                },
            },
        )
        .unwrap();
    }
    r.add(r.ticket(), "local".into()).unwrap();
    drop(r);
    // Restart opens the old format and never migrates it.
    let transaction = transaction_path(&root, config());
    let r = DurableReplica::restart_utf8_set(&root, config()).unwrap();
    assert_eq!(r.log().records().len(), 5_001);
    let state = r.state().clone();
    drop(r);
    let committed = read(&root);
    assert!(transaction.exists() && !journal::path(&root, 0).exists());
    let mut r = DurableReplica::migrate_utf8_set_to_append_log(&root, config()).unwrap();
    assert!(!transaction.exists() && journal::path(&root, 0).exists());
    assert_eq!(r.state(), &state);
    assert_eq!(read(&root), committed, "every record kept, in order");
    let before = fs::metadata(journal::path(&root, 0)).unwrap().len();
    r.add(r.ticket(), "appended".into()).unwrap();
    let after = fs::metadata(journal::path(&root, 0)).unwrap().len();
    assert!(after - before < 100, "one entry, not the history");
    drop(r);
    let r = DurableReplica::restart_utf8_set(&root, config()).unwrap();
    assert_eq!(r.log().records().len(), 5_002);
    assert!(r.state().contains(&"kept-4999".into()) && r.state().contains(&"appended".into()));
    drop(r);
    // Migrating an append-log store restarts it unchanged.
    let r = DurableReplica::migrate_utf8_set_to_append_log(&root, config()).unwrap();
    assert_eq!(r.log().records().len(), 5_002);
    drop(r);

    // The store the archived 3a9179b library wrote migrates the same way.
    let root = self::root();
    fs::write(
        root.join("writer-0.fence"),
        include_bytes!("../tests/fixtures/bootstrap/counter.fence"),
    )
    .unwrap();
    fs::write(
        root.join("writer-0.transaction"),
        include_bytes!("../tests/fixtures/bootstrap/counter.transaction"),
    )
    .unwrap();
    let committed = read(&root);
    let mut r = DurableReplica::migrate_counter_to_append_log(&root, config()).unwrap();
    assert_eq!(r.state().state(), &[5, 7]);
    assert_eq!(read(&root), committed);
    r.bump(r.ticket(), 8).unwrap();
    drop(r);
    assert_eq!(restart_state(&root).unwrap(), (vec![8, 7], None));
}
#[test]
fn interrupted_migration_is_refused_then_completed() {
    let root = initialized_legacy("counter");
    let committed = read(&root);
    // Fail after the journal is in place, before the transaction is removed.
    fault(15, false);
    let result = DurableReplica::migrate_counter_to_append_log(&root, config());
    fault(0, false);
    assert!(matches!(result, Err(LocalError::Io(_))));
    assert!(transaction_path(&root, config()).exists() && journal::path(&root, 0).exists());
    let both = store_files(&root);
    assert!(matches!(
        DurableReplica::restart_counter(&root, config()),
        Err(LocalError::RecoveryRequired)
    ));
    assert!(matches!(
        DurableReplica::restart_counter_from_store(&root, 0),
        Err(LocalError::RecoveryRequired)
    ));
    assert!(matches!(
        DurableReplica::counter(&root, config()),
        Err(LocalError::StoreExists)
    ));
    assert_eq!(store_files(&root), both, "refusals change nothing");
    let r = DurableReplica::migrate_counter_to_append_log(&root, config()).unwrap();
    assert_eq!(r.state().state(), &[5, 0]);
    drop(r);
    assert!(!transaction_path(&root, config()).exists());
    assert_eq!(read(&root), committed);

    // A journal that has moved on from the transaction is not completed.
    let root = initialized_legacy("counter");
    let transaction = fs::read(transaction_path(&root, config())).unwrap();
    drop(DurableReplica::migrate_counter_to_append_log(&root, config()).unwrap());
    let mut r = DurableReplica::restart_counter(&root, config()).unwrap();
    r.bump(r.ticket(), 6).unwrap();
    drop(r);
    fs::write(transaction_path(&root, config()), &transaction).unwrap();
    assert!(matches!(
        DurableReplica::migrate_counter_to_append_log(&root, config()),
        Err(LocalError::RecoveryRequired)
    ));
}
#[test]
fn oversized_stored_orset_restarts_ordinary() {
    let root = self::root();
    let mut replica = DurableReplica::utf8_set(&root, config()).unwrap();
    let element = "bulk".to_string();
    for _ in 0..4097 {
        replica.add(replica.ticket(), element.clone()).unwrap();
    }
    assert_eq!(replica.state().observed_tokens(&element).len(), 4097);
    replica.remove(replica.ticket(), &element).unwrap();
    drop(replica);
    let mut restored = DurableReplica::restart_utf8_set(&root, config()).unwrap();
    assert_eq!(restored.state().observed_tokens(&element).len(), 0);
    assert_eq!(restored.state().tombstones().len(), 4097);
    assert_eq!(
        restored
            .receive(
                restored.ticket(),
                Record {
                    id: RecordId {
                        replica: 1,
                        sequence: 1,
                    },
                    delta: OrSetDelta::Add {
                        element: "peer".into(),
                        token: 3,
                    },
                },
            )
            .unwrap(),
        Admission::Accepted
    );
    drop(restored);
    let restored = DurableReplica::restart_utf8_set(&root, config()).unwrap();
    assert_eq!(restored.state().tombstones().len(), 4097);
    assert!(restored.state().contains(&"peer".to_string()));
}
#[test]
fn duplicate_and_replay_do_not_clone_history() {
    use std::{cell::Cell, rc::Rc};

    struct Delta(u64, Rc<Cell<usize>>);
    impl Clone for Delta {
        fn clone(&self) -> Self {
            self.1.set(self.1.get() + 1);
            Self(self.0, self.1.clone())
        }
    }
    impl PartialEq for Delta {
        fn eq(&self, other: &Self) -> bool {
            self.0 == other.0
        }
    }
    impl OwnedDelta for Delta {
        fn owned_payload(&self) -> OwnedPayload {
            OwnedPayload::Remove
        }
    }
    struct State(u64);
    impl crate::Mergeable for State {
        fn merge(&mut self, other: &Self) -> Result<(), crate::MergeError> {
            self.0 = self.0.max(other.0);
            Ok(())
        }
    }
    impl Crdt for State {
        type Delta = Delta;
        fn validate_record(&self, _: RecordId, _: &Delta) -> Result<(), WireError> {
            Ok(())
        }
        fn apply_delta(&mut self, delta: Delta) {
            self.0 = self.0.max(delta.0);
        }
    }

    let clones = Rc::new(Cell::new(0));
    let record = |sequence, value| Record {
        id: RecordId {
            replica: 0,
            sequence,
        },
        delta: Delta(value, clones.clone()),
    };
    let mut r = LocalReplica::fresh(&root(), config(), State(0)).unwrap();
    for sequence in 1..=128 {
        r.replay_validated(record(sequence, sequence)).unwrap();
    }
    // Only the current delta is copied for application, never the prefix.
    assert_eq!(clones.get(), 128);
    assert_eq!(r.last_sequence, 128);
    assert_eq!(r.state().0, 128);
    clones.set(0);
    for (value, expected) in [(128, Admission::Duplicate), (999, Admission::Collision)] {
        assert_eq!(
            r.admit_committed(
                r.ticket(),
                record(128, value),
                false,
                |_, _| { panic!("non-admission must not commit") },
                |_| Ok(())
            )
            .unwrap(),
            expected
        );
        assert!(matches!(
            r.replay_validated(record(128, value)),
            Err(LocalError::InvalidHistory)
        ));
    }
    assert_eq!(clones.get(), 0);
    assert_eq!(r.log().records().len(), 128);
    assert_eq!(r.last_sequence, 128);
    assert_eq!(r.state().0, 128);
    assert_eq!(
        r.admit_committed(
            r.ticket(),
            record(129, 129),
            true,
            |log, sequence| {
                assert_eq!(log.records().len(), 129);
                assert_eq!(sequence, 129);
                Ok(())
            },
            |_| Ok(())
        )
        .unwrap(),
        Admission::Accepted
    );
    assert_eq!(clones.get(), 1, "accepted writes must not clone history");
    let before = r.log.version().clone();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = r.admit_committed(
            r.ticket(),
            record(130, 130),
            true,
            |_, _| panic!("commit unwind"),
            |_| Ok(()),
        );
    }));
    assert!(result.is_err());
    assert_eq!(r.log.records().len(), 129);
    assert_eq!(r.log.version(), &before);
    assert_eq!(r.state().0, 129);
    assert_eq!(r.last_sequence, 129);
    assert!(!r.held);
    assert_eq!(
        r.log.admission(&r.state, &record(130, 130)),
        Admission::Accepted
    );
}

fn read(root: &Path) -> CommittedTransaction {
    CommittedTransaction::read(root, config()).unwrap()
}
fn fault(boundary: u8, crash: bool) {
    persistence::FAULT.with(|f| f.set((boundary, crash)));
}
fn exercise(
    kind: &str,
    legacy: bool,
    root: &Path,
    boundary: u8,
) -> (CommittedTransaction, CommittedTransaction) {
    let old;
    if legacy {
        legacy_at(kind, root);
    }
    if kind == "counter" {
        let mut r = if legacy {
            DurableReplica::restart_counter(root, config()).unwrap()
        } else {
            DurableReplica::counter(root, config()).unwrap()
        };
        if !legacy {
            pre_protection(&mut r);
        }
        r.bump(r.ticket(), 5).unwrap();
        old = read(root);
        fault(boundary, true);
        assert_eq!(r.bump(r.ticket(), 9).unwrap().id.sequence, 2);
        assert_eq!(r.state().state(), &[9, 0]);
    } else {
        let mut r = if legacy {
            DurableReplica::restart_utf8_set(root, config()).unwrap()
        } else {
            DurableReplica::utf8_set(root, config()).unwrap()
        };
        if !legacy {
            pre_protection(&mut r);
        }
        r.add(r.ticket(), "café☕".into()).unwrap();
        old = read(root);
        fault(boundary, true);
        assert_eq!(
            r.remove(r.ticket(), &"café☕".into()).unwrap().id.sequence,
            2
        );
        assert!(!r.state().contains(&"café☕".into()));
        assert_eq!(r.state().tombstones().len(), 1);
    }
    // The public call has returned success to its caller.
    std::println!("ACK {kind}");
    std::io::stdout().flush().unwrap();
    persistence::checkpoint(8).unwrap();
    (old, read(root))
}
fn replay(kind: &str, transaction: &CommittedTransaction) {
    assert_eq!(transaction.config, config());
    assert_eq!(transaction.last_sequence, 2);
    if kind == "counter" {
        let mut state = GCounter::new(2);
        let log =
            EventLog::<GCounterDelta>::from_wire_bytes_for(&transaction.log_bytes, &state).unwrap();
        assert_eq!(log.records().len(), 2);
        assert_eq!(log.to_wire_bytes().unwrap(), transaction.log_bytes);
        for record in log.records() {
            state.apply_delta(record.delta.clone());
        }
        assert_eq!(state.state(), &[9, 0]);
    } else {
        let mut state = OrSet::<String, u64>::new();
        let log = EventLog::<OrSetDelta<String, u64>>::from_wire_bytes_for(
            &transaction.log_bytes,
            &state,
        )
        .unwrap();
        assert_eq!(log.records().len(), 2);
        assert_eq!(log.to_wire_bytes().unwrap(), transaction.log_bytes);
        for record in log.records() {
            state.apply_delta(record.delta.clone());
        }
        assert!(!state.contains(&"café☕".into()));
        assert_eq!(
            state.tombstones().iter().copied().collect::<Vec<_>>(),
            alloc::vec![2]
        );
    }
}
#[test]
fn restart_still_works_and_fresh_allocation() {
    let root = root();
    let mut r = DurableReplica::counter(&root, config()).unwrap();
    let before = r.bump(r.ticket(), 5).unwrap();
    r.receive(
        r.ticket(),
        Record {
            id: RecordId {
                replica: 1,
                sequence: 40,
            },
            delta: GCounterDelta {
                replica: 1,
                tally: 7,
            },
        },
    )
    .unwrap();
    let old = r.ticket();
    let state = r.state().state().to_vec();
    let log = r.log().to_wire_bytes().unwrap();
    let allocation = r.allocation_bytes();
    drop(r);
    let mut r = DurableReplica::restart_counter(&root, config()).unwrap();
    assert_eq!(r.state().state(), state);
    assert_eq!(r.log().to_wire_bytes().unwrap(), log);
    assert_eq!(r.allocation_bytes(), allocation);
    assert!(matches!(r.bump(old, 9), Err(LocalError::Refused)));
    let after = r.bump(r.ticket(), 9).unwrap();
    assert_eq!(
        before.id,
        RecordId {
            replica: 0,
            sequence: 1
        }
    );
    assert_eq!(
        after.id,
        RecordId {
            replica: 0,
            sequence: 2
        }
    );
    assert_eq!(r.state().state(), &[9, 7]);
    std::println!(
        "controls=7,2 counter replay={state:?} writable=true IDs {:?} -> {:?}",
        before.id,
        after.id
    );
    drop(r);
    assert_eq!(
        DurableReplica::restart_counter(&root, config())
            .unwrap()
            .state()
            .state(),
        &[9, 7]
    );

    let root = self::root();
    let mut r = DurableReplica::utf8_set(&root, config()).unwrap();
    let before = r.add(r.ticket(), "café☕".into()).unwrap();
    let removed = r.remove(r.ticket(), &"café☕".into()).unwrap();
    r.receive(
        r.ticket(),
        Record {
            id: RecordId {
                replica: 1,
                sequence: 40,
            },
            delta: OrSetDelta::Add {
                element: "東京".into(),
                token: 81,
            },
        },
    )
    .unwrap();
    let state = r.state().clone();
    let log = r.log().to_wire_bytes().unwrap();
    let allocation = r.allocation_bytes();
    let old = r.ticket();
    drop(r);
    let mut r = DurableReplica::restart_utf8_set(&root, config()).unwrap();
    assert_eq!(r.state(), &state);
    assert_eq!(r.log().to_wire_bytes().unwrap(), log);
    assert_eq!(r.allocation_bytes(), allocation);
    assert!(matches!(
        r.add(old, "stale".into()),
        Err(LocalError::Refused)
    ));
    let after = r.add(r.ticket(), "café☕".into()).unwrap();
    assert_eq!(
        before.id,
        RecordId {
            replica: 0,
            sequence: 1
        }
    );
    assert_eq!(
        removed.id,
        RecordId {
            replica: 0,
            sequence: 2
        }
    );
    assert_eq!(
        after.id,
        RecordId {
            replica: 0,
            sequence: 3
        }
    );
    assert!(matches!(before.delta, OrSetDelta::Add { token: 2, .. }));
    assert!(matches!(after.delta, OrSetDelta::Add { token: 6, .. }));
    assert!(r.state().contains(&"café☕".into()));
    assert!(r.state().contains(&"東京".into()));
    assert!(r.state().tombstones().contains(&2));
    std::println!("controls=7,2 set replay=equal including tombstone 2 and remote token 81; writable=true IDs {:?}, {:?} -> {:?}; tokens 2 -> 6", before.id, removed.id, after.id);
    drop(r);
    let r = DurableReplica::restart_utf8_set(&root, config()).unwrap();
    assert!(r.state().contains(&"café☕".into()));
    assert!(r.state().tombstones().contains(&2));
}
fn initialized(kind: &str) -> PathBuf {
    let root = root();
    if kind == "counter" {
        let mut r = DurableReplica::counter(&root, config()).unwrap();
        r.bump(r.ticket(), 5).unwrap();
    } else {
        let mut r = DurableReplica::utf8_set(&root, config()).unwrap();
        r.add(r.ticket(), "café☕".into()).unwrap();
    }
    root
}
// The same one-record history in the pre-append-log transaction format.
fn initialized_legacy(kind: &str) -> PathBuf {
    let root = legacy(kind);
    if kind == "counter" {
        let mut r = DurableReplica::restart_counter(&root, config()).unwrap();
        r.bump(r.ticket(), 5).unwrap();
    } else {
        let mut r = DurableReplica::restart_utf8_set(&root, config()).unwrap();
        r.add(r.ticket(), "café☕".into()).unwrap();
    }
    assert!(!journal::path(&root, 0).exists());
    root
}
// Success proves the returned replica can commit a fresh edit.
fn restart_and_write(kind: &str, root: &Path, config: WriterConfig) -> Result<(), LocalError> {
    if kind == "counter" {
        let mut r = DurableReplica::restart_counter(root, config)?;
        r.bump(r.ticket(), 9)?;
    } else {
        let mut r = DurableReplica::restart_utf8_set(root, config)?;
        r.add(r.ticket(), "new".into())?;
    }
    Ok(())
}
#[test]
fn restart_corrupt_history() {
    for kind in ["counter", "set"] {
        let root = initialized_legacy(kind);
        let path = transaction_path(&root, config());
        let mut bytes = fs::read(&path).unwrap();
        // Damage the checksum; the well-formed payload permits revert control 8.
        *bytes.last_mut().unwrap() ^= 1;
        fs::write(&path, &bytes).unwrap();
        let result = restart_and_write(kind, &root, config());
        std::println!(
            "control=3 {kind} result={result:?} writable={}",
            result.is_ok()
        );
        assert!(matches!(
            result,
            Err(LocalError::History(WireError::IntegrityMismatch))
        ));
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}
#[test]
fn restart_mismatched_history() {
    for kind in ["counter", "set"] {
        let root = initialized_legacy(kind);
        let path = transaction_path(&root, config());
        let original = fs::read(&path).unwrap();
        let mut bytes = original.clone();
        bytes[..8].copy_from_slice(&3u64.to_le_bytes());
        fs::write(&path, &bytes).unwrap();
        let result = restart_and_write(kind, &root, config());
        std::println!(
            "control=4 {kind} result={result:?} writable={}",
            result.is_ok()
        );
        assert!(matches!(result, Err(LocalError::Configuration)));
        assert_eq!(fs::read(&path).unwrap(), bytes);
        fs::write(&path, original).unwrap();
        let result = restart_and_write(
            kind,
            &root,
            WriterConfig {
                writers: 3,
                writer: 0,
            },
        );
        std::println!("control=4 {kind} mismatched writer result={result:?}");
        assert!(
            matches!(result, Err(LocalError::Configuration)),
            "{kind}: mismatched writer restart returned {result:?}"
        );
    }
    for root in [initialized("counter"), initialized_legacy("counter")] {
        assert!(matches!(
            DurableReplica::restart_utf8_set(&root, config()),
            Err(LocalError::History(WireError::DeltaTypeMismatch))
        ));
    }
}
#[test]
fn restart_invalid_history() {
    for kind in ["counter", "set"] {
        let root = initialized_legacy(kind);
        let path = transaction_path(&root, config());
        // Serialize with the product: valid framing, invalid ownership.
        if kind == "counter" {
            let record = Record {
                id: RecordId {
                    replica: 0,
                    sequence: 1,
                },
                delta: GCounterDelta {
                    replica: 1,
                    tally: 5,
                },
            };
            let mut log = EventLog::for_crdt(&GCounter::new(2));
            assert_eq!(
                log.insert_record(&GCounter::new(2), record.clone()),
                Admission::Invalid(WireError::OwnershipViolation)
            );
            let mut bytes = Vec::new();
            EventLog::encode_records(Some(2), &[record], &mut bytes).unwrap();
            let log = EventLog::from_wire_bytes(&bytes).unwrap();
            DurableReplica::<GCounter>::commit(&path, config(), &log, 1).unwrap();
        } else {
            let mut log = EventLog::for_crdt(&OrSet::<String, u64>::new());
            assert_eq!(
                log.insert_record(
                    &OrSet::<String, u64>::new(),
                    Record {
                        id: RecordId {
                            replica: 0,
                            sequence: 1
                        },
                        delta: OrSetDelta::Add {
                            element: "foreign".into(),
                            token: 3
                        },
                    }
                ),
                Admission::Accepted
            );
            DurableReplica::<OrSet<String, u64>>::commit(&path, config(), &log, 1).unwrap();
        }
        let bytes = fs::read(&path).unwrap();
        let result = restart_and_write(kind, &root, config());
        std::println!(
            "control=5 {kind} result={result:?} writable={}",
            result.is_ok()
        );
        assert!(matches!(
            result,
            Err(LocalError::Refused) | Err(LocalError::History(_))
        ));
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let root = initialized_legacy(kind);
        let path = transaction_path(&root, config());
        let original = fs::read(&path).unwrap();
        for last in [0u64, 2, u64::MAX] {
            let mut bytes = original.clone();
            bytes[16..24].copy_from_slice(&last.to_le_bytes());
            fs::write(&path, &bytes).unwrap();
            assert!(matches!(
                restart_and_write(kind, &root, config()),
                Err(LocalError::InvalidHistory)
            ));
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
}
#[test]
fn restart_unavailable_ownership() {
    let root = root();
    let r = DurableReplica::counter(&root, config()).unwrap();
    assert!(matches!(
        DurableReplica::restart_counter(&root, config()),
        Err(LocalError::Refused)
    ));
    drop(r);
    restart_and_write("counter", &root, config()).unwrap();
    let root = self::root();
    let r = DurableReplica::utf8_set(&root, config()).unwrap();
    assert!(matches!(
        DurableReplica::restart_utf8_set(&root, config()),
        Err(LocalError::Refused)
    ));
    drop(r);
    restart_and_write("set", &root, config()).unwrap();
    std::println!("control=6 both primitives: held fence Refused, no replica; released fence and empty histories: writable");
}

#[test]
fn failed_restart_unlocks_with_duplicated_fence_handle() {
    let root = initialized("counter");
    let transaction = store_file(&root);
    let withheld = root.join("withheld-transaction");
    fs::rename(&transaction, &withheld).unwrap();

    RETAIN_RESTART_FENCE.with(|slot| *slot.borrow_mut() = Some(None));
    let first = DurableReplica::restart_counter(&root, config());
    let duplicate = RETAIN_RESTART_FENCE.with(|slot| slot.borrow_mut().take().unwrap().unwrap());
    assert!(matches!(first, Err(LocalError::Io(_))));
    assert!(!transaction.exists());

    let second = DurableReplica::restart_counter(&root, config());
    assert!(
        matches!(second, Err(LocalError::Io(_))),
        "next restart was Refused"
    );
    drop(duplicate);
    fs::rename(&withheld, &transaction).unwrap();
    restart_and_write("counter", &root, config()).unwrap();
}

#[test]
fn durable_child() {
    if let Ok(root) = std::env::var("SAFEMESH_JOINED_ROOT") {
        joined_start(
            Path::new(&root),
            std::env::var_os("SAFEMESH_JOINED_LOSS").is_some(),
        );
    }
    if let Ok(root) = std::env::var("SAFEMESH_DURABLE_ROOT") {
        let kind = std::env::var("SAFEMESH_DURABLE_KIND").unwrap();
        if std::env::var_os("SAFEMESH_DURABLE_READ").is_some() {
            replay(&kind, &read(Path::new(&root)));
            return;
        }
        let boundary = std::env::var("SAFEMESH_DURABLE_BOUNDARY")
            .unwrap()
            .parse()
            .unwrap();
        let legacy = std::env::var_os("SAFEMESH_DURABLE_LEGACY").is_some();
        exercise(&kind, legacy, Path::new(&root), boundary);
    }
}
fn crash(kind: &str, legacy: bool, boundary: u8, root: &Path) -> std::process::Output {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "local::durable_tests::durable_child",
            "--nocapture",
        ])
        .env("SAFEMESH_DURABLE_ROOT", root)
        .env("SAFEMESH_DURABLE_KIND", kind)
        .env("SAFEMESH_DURABLE_BOUNDARY", boundary.to_string());
    if legacy {
        command.env("SAFEMESH_DURABLE_LEGACY", "1");
    }
    let output = command.output().unwrap();
    // Save and read child exit codes, too; never derive one through a pipe.
    let status = root.join("child.exit");
    fs::write(&status, output.status.code().unwrap().to_string()).unwrap();
    assert_eq!(fs::read_to_string(status).unwrap(), "77", "{:?}", output);
    output
}
#[test]
fn durable_still_works() {
    for loss in [false, true] {
        let root = root();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "local::durable_tests::durable_child",
                "--nocapture",
            ])
            .env("SAFEMESH_JOINED_ROOT", &root);
        if loss {
            command.env("SAFEMESH_JOINED_LOSS", "1");
        }
        let output = command.output().unwrap();
        fs::write(
            root.join("joined.exit"),
            output.status.code().unwrap().to_string(),
        )
        .unwrap();
        assert_eq!(fs::read_to_string(root.join("joined.exit")).unwrap(), "77");
        assert!(String::from_utf8_lossy(&output.stdout).contains("joined ACK"));
        joined_finish(&root, loss);
    }

    for (kind, legacy) in [("counter", false), ("set", false), ("counter", true)] {
        let root = root();
        let (_, expected) = exercise(kind, legacy, &root, 0);
        // All writer handles are dropped. Reopen the committed transaction
        // and replay fresh state; enabling writes on restart is packet C.
        assert_eq!(read(&root), expected);
        replay(kind, &read(&root));
        let restarted = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "local::durable_tests::durable_child",
                "--nocapture",
            ])
            .env("SAFEMESH_DURABLE_ROOT", &root)
            .env("SAFEMESH_DURABLE_KIND", kind)
            .env("SAFEMESH_DURABLE_READ", "1")
            .output()
            .unwrap();
        let status = root.join("restart.exit");
        fs::write(&status, restarted.status.code().unwrap().to_string()).unwrap();
        assert_eq!(fs::read_to_string(status).unwrap(), "0", "{restarted:?}");
        std::println!("control=6 {kind} ordinary-write/read/restart=PASS");
    }
}
// Transaction format (whole-file replacement), boundaries 1-6: before the
// temporary, mid-write, written, synced, renamed, directory synced.
// Append log, boundaries 11-14: before the write, half an entry written,
// entry written but not synced, entry synced. 7: commit returned; 8: ACK.
#[test]
fn durable_crash_boundaries() {
    for kind in ["counter", "set"] {
        for legacy in [true, false] {
            let (old, new) = exercise(kind, legacy, &root(), 0);
            let boundaries: &[u8] = if legacy {
                &[1, 2, 3, 4, 5, 6, 7, 8]
            } else {
                &[11, 12, 13, 14, 7, 8]
            };
            for &boundary in boundaries {
                let root = root();
                let output = crash(kind, legacy, boundary, &root);
                let recovered = read(&root);
                // A process exit keeps page-cache writes, so an entry written
                // before its sync (13) reads back whole: the new state. A power
                // cut could instead leave the old state or a torn tail.
                let complete = if legacy {
                    boundary >= 5
                } else {
                    !matches!(boundary, 11 | 12)
                };
                assert_eq!(&recovered, if complete { &new } else { &old });
                assert_eq!(
                    String::from_utf8_lossy(&output.stdout).contains("ACK"),
                    boundary == 8
                );
                let restart = if legacy {
                    "transaction format".into()
                } else {
                    let torn = restart_after_crash(kind, &root, &recovered);
                    assert_eq!(torn.is_some(), boundary == 12);
                    format!("append log, torn_tail={torn:?}")
                };
                std::println!(
                    "{kind} boundary={boundary} state={} ({restart})",
                    if complete { "new" } else { "old" },
                );
            }
        }
    }
}
// Restart must replay exactly the recovered history, truncate a torn tail
// to its reported offset, then accept and keep one more durable edit.
fn restart_after_crash(
    kind: &str,
    root: &Path,
    recovered: &CommittedTransaction,
) -> Option<TornTail> {
    let path = journal::path(root, 0);
    let before = fs::metadata(&path).unwrap().len();
    let (torn, log, records) = if kind == "counter" {
        let mut r = DurableReplica::restart_counter(root, config()).unwrap();
        let log = r.log().to_wire_bytes().unwrap();
        let torn = r.torn_tail();
        r.bump(r.ticket(), 20).unwrap();
        (torn, log, r.log().records().len())
    } else {
        let mut r = DurableReplica::restart_utf8_set(root, config()).unwrap();
        let log = r.log().to_wire_bytes().unwrap();
        let torn = r.torn_tail();
        r.add(r.ticket(), "after".into()).unwrap();
        (torn, log, r.log().records().len())
    };
    assert_eq!(
        log, recovered.log_bytes,
        "restart replays exactly one state"
    );
    if let Some(tail) = torn {
        assert_eq!(tail.offset + tail.discarded_bytes, before);
    }
    let again = read(root);
    assert_eq!(again.last_sequence, recovered.last_sequence + 1);
    let replayed = if kind == "counter" {
        let r = DurableReplica::restart_counter(root, config()).unwrap();
        assert_eq!(r.torn_tail(), None);
        r.log().records().len()
    } else {
        let r = DurableReplica::restart_utf8_set(root, config()).unwrap();
        assert_eq!(r.torn_tail(), None);
        r.log().records().len()
    };
    assert_eq!(replayed, records);
    torn
}
fn acknowledged_survives(kind: &str) {
    for legacy in [true, false] {
        let root = root();
        let output = crash(kind, legacy, 8, &root);
        assert!(String::from_utf8_lossy(&output.stdout).contains("ACK"));
        replay(kind, &read(&root));
    }
    std::println!("control=3 {kind} acknowledged-crash-recovery=PASS");
}
#[test]
fn durable_acknowledged_survives_counter() {
    acknowledged_survives("counter");
}
#[test]
fn durable_acknowledged_survives_set() {
    acknowledged_survives("set");
}
fn failure<C: Crdt + std::fmt::Debug>(
    mut r: DurableReplica<C>,
    delta: C::Delta,
    boundary: u8,
    prior: Option<C::Delta>,
) where
    C::Delta: OwnedDelta + Clone + PartialEq + WireEncode + WireSchema,
{
    if let Some(prior) = prior {
        r.append(r.ticket(), prior).unwrap();
    }
    let version = r.log().version().clone();
    let state = format!("{:?}", r.state());
    let log = r.log().to_wire_bytes().unwrap();
    let allocation = r.allocation_bytes();
    let ticket = r.ticket();
    fault(boundary, false);
    assert!(matches!(
        r.append(ticket, delta.clone()),
        Err(LocalError::Io(_))
    ));
    fault(0, false);
    assert_eq!(format!("{:?}", r.state()), state);
    assert_eq!(r.log().to_wire_bytes().unwrap(), log);
    assert_eq!(r.log().version(), &version);
    assert_eq!(r.allocation_bytes(), allocation);
    assert!(matches!(
        r.append(ticket, delta.clone()),
        Err(LocalError::Refused)
    ));
    assert!(matches!(
        r.receive(
            ticket,
            Record {
                id: RecordId {
                    replica: 0,
                    sequence: 1
                },
                delta
            }
        ),
        Err(LocalError::Refused)
    ));
    assert!(matches!(r.renew(ticket), Err(LocalError::Refused)));
}
#[test]
fn durable_io_failure_disables_writes() {
    let counter = |legacy: bool| {
        let root = root();
        if legacy {
            legacy_at("counter", &root);
            DurableReplica::restart_counter(&root, config()).unwrap()
        } else {
            DurableReplica::counter(&root, config()).unwrap()
        }
    };
    let set = |legacy: bool| {
        let root = root();
        if legacy {
            legacy_at("set", &root);
            DurableReplica::restart_utf8_set(&root, config()).unwrap()
        } else {
            DurableReplica::utf8_set(&root, config()).unwrap()
        }
    };
    let boundaries = (2..=6)
        .map(|b| (b, true))
        .chain((11..=14).map(|b| (b, false)));
    for ((boundary, legacy), populated) in
        boundaries.flat_map(|boundary| [false, true].map(|populated| (boundary, populated)))
    {
        failure(
            counter(legacy),
            GCounterDelta {
                replica: 0,
                tally: 9,
            },
            boundary,
            populated.then_some(GCounterDelta {
                replica: 0,
                tally: 1,
            }),
        );
        failure(
            set(legacy),
            OrSetDelta::Add {
                element: "東京".into(),
                token: if populated { 4 } else { 2 },
            },
            boundary,
            populated.then_some(OrSetDelta::Add {
                element: "prior".into(),
                token: 2,
            }),
        );
        std::println!("controls=4,5 write/sync/uncertain-boundary={boundary} error+disabled=PASS");
    }
    // Closing a remote gap can advance over several existing records.
    // Failure must restore that prefix as well as the identity index.
    for prior in [&[][..], &[2, 3][..]] {
        let mut r = LocalReplica::counter(&root(), config()).unwrap();
        let remote = |sequence| Record {
            id: RecordId {
                replica: 1,
                sequence,
            },
            delta: GCounterDelta {
                replica: 1,
                tally: sequence,
            },
        };
        for &sequence in prior {
            assert_eq!(
                r.receive(r.ticket(), remote(sequence)).unwrap(),
                Admission::Accepted
            );
        }
        let before = r.log().clone();
        let state = r.state().clone();
        assert!(matches!(
            r.admit_committed(
                r.ticket(),
                remote(1),
                false,
                |log, sequence| {
                    assert_eq!(log.version().get(1), prior.last().copied().unwrap_or(1));
                    assert_eq!(sequence, 0);
                    Err(LocalError::Io(io::Error::other("commit failed")))
                },
                |_| Ok(())
            ),
            Err(LocalError::Io(_))
        ));
        assert_eq!(r.log(), &before);
        assert_eq!(r.state(), &state);
        assert_eq!(r.last_sequence, 0);
        assert!(!r.held);
    }
}
#[test]
fn durable_receive_and_refusal() {
    let root = root();
    let mut r = DurableReplica::counter(&root, config()).unwrap();
    let record = Record {
        id: RecordId {
            replica: 1,
            sequence: 1,
        },
        delta: GCounterDelta {
            replica: 1,
            tally: 7,
        },
    };
    assert_eq!(
        r.receive(r.ticket(), record.clone()).unwrap(),
        Admission::Accepted
    );
    let transaction = read(&root);
    assert_eq!(transaction.last_sequence, 0);
    assert_eq!(transaction.log_bytes, r.log().to_wire_bytes().unwrap());
    assert_eq!(
        r.receive(r.ticket(), record.clone()).unwrap(),
        Admission::Duplicate
    );
    let mut collision = record;
    collision.delta.tally = 8;
    assert_eq!(
        r.receive(r.ticket(), collision).unwrap(),
        Admission::Collision
    );
    assert!(matches!(
        r.append(
            r.ticket(),
            GCounterDelta {
                replica: 1,
                tally: 10
            }
        ),
        Err(LocalError::Refused)
    ));
    assert_eq!(read(&root), transaction);
    let old = r.ticket();
    let ticket = r.renew(old).unwrap();
    assert!(matches!(r.bump(old, 9), Err(LocalError::Refused)));
    r.bump(ticket, 9).unwrap();
    assert_eq!(read(&root).last_sequence, 1);
    drop(r);
    assert!(matches!(
        DurableReplica::counter(&root, config()),
        Err(LocalError::StoreExists)
    ));
}
