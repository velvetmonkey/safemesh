use safemesh_crdt::{
    Admission, AppendError, Crdt, EventLog, GCounter, GCounterDelta, Record, RecordId, WireDecode,
    WireEncode,
};
fn record(sequence: u64, tally: u64) -> Record<GCounterDelta> {
    Record {
        id: RecordId {
            replica: 1,
            sequence,
        },
        delta: GCounterDelta { replica: 1, tally },
    }
}
#[test]
fn record_1_1_live_and_replay() {
    let mut log = EventLog::with_replica_count(2);
    let mut live = GCounter::new(2);
    let first = log.admit_with(record(1, 5), |d| live.apply_delta(d.clone()));
    let before = log.clone();
    let second = log.admit_with(record(1, 9), |d| live.apply_delta(d.clone()));
    let persisted =
        EventLog::<GCounterDelta>::from_wire_bytes(&log.to_wire_bytes().unwrap()).unwrap();
    let mut replay = GCounter::new(2);
    for r in persisted.records() {
        replay.apply_delta(r.delta.clone());
    }
    println!(
        "id (1,1): tally 5 -> {first:?}; tally 9 -> {second:?}; live {}; replay {}",
        live.value(),
        replay.value()
    );
    assert_eq!(live.value(), 5);
    assert_eq!(replay.value(), 5);
    assert_eq!(live, replay);
    assert_eq!(first, Admission::Accepted);
    assert_eq!(second, Admission::Collision);
    assert_eq!(log, before);
}
#[test]
fn duplicates_and_collisions_do_not_invoke_application() {
    let mut log = EventLog::new();
    assert_eq!(log.insert_record(record(1, 5)), Admission::Accepted);
    let before = log.clone();
    assert_eq!(
        log.admit_with(record(1, 5), |_| panic!("duplicate applied")),
        Admission::Duplicate
    );
    assert_eq!(
        log.admit_with(record(1, 9), |_| panic!("collision applied")),
        Admission::Collision
    );
    assert_eq!(log, before);
    let mut applied = false;
    assert_eq!(
        log.admit_with(record(2, 0), |_| applied = true),
        Admission::Accepted
    );
    assert!(applied);
}
#[test]
fn local_append_handles_gaps_and_exhaustion_without_unlogged_application() {
    let mut log = EventLog::new();
    let mut live = GCounter::new(2);
    assert_eq!(
        log.admit_with(record(2, 5), |d| live.apply_delta(d.clone())),
        Admission::Accepted
    );
    let id = log
        .append_with(1, record(1, 9).delta, |d| live.apply_delta(d.clone()))
        .unwrap();
    assert_eq!(id.sequence, 3);
    assert_eq!(log.version().get(1), 0);
    assert_eq!(
        log.admit_with(record(1, 4), |d| live.apply_delta(d.clone())),
        Admission::Accepted
    );
    assert_eq!(log.version().get(1), 3);
    let mut replay = GCounter::new(2);
    for r in log.records() {
        replay.apply_delta(r.delta.clone());
    }
    assert_eq!(live, replay);
    assert_eq!(log.insert_record(record(u64::MAX, 9)), Admission::Accepted);
    let before = log.clone();
    assert_eq!(
        log.append_with(1, record(1, 10).delta, |_| panic!(
            "exhausted append applied"
        )),
        Err(AppendError::SequenceExhausted)
    );
    assert_eq!(log, before);
}

#[cfg(all(feature = "local-writer", target_os = "linux"))]
mod owned_local {
    use super::*;
    use safemesh_crdt::{
        local::{LocalError, LocalReplica},
        ownership::{OwnedDelta, WriterConfig},
        OrSetDelta, WireSchema,
    };
    use std::{
        fmt::Debug,
        io::Write,
        path::PathBuf,
        process::{Command, Stdio},
        sync::atomic::{AtomicU64, Ordering},
    };
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    fn root(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "safemesh-{name}-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
    fn config(writer: u64) -> WriterConfig {
        WriterConfig { writers: 2, writer }
    }
    fn snapshot<C: Crdt + Debug>(r: &LocalReplica<C>) -> [Vec<u8>; 3]
    where
        C::Delta: OwnedDelta + Clone + PartialEq + WireEncode + WireSchema,
    {
        // Debug contains every state field, including OR-Set adds and tombstones,
        // in deterministic BTree order. The other two are product encodings.
        [
            format!("{:?}", r.state()).into_bytes(),
            r.log().to_wire_bytes().unwrap(),
            r.allocation_bytes(),
        ]
    }
    fn sha(bytes: &[u8]) -> String {
        let mut p = Command::new("sha256sum")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        p.stdin.take().unwrap().write_all(bytes).unwrap();
        let output = p.wait_with_output().unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .to_owned()
    }
    fn proof(name: &str, before: &[Vec<u8>; 3], after: &[Vec<u8>; 3]) {
        for (i, field) in ["state", "log", "allocation"].iter().enumerate() {
            println!(
                "{name} {field} before={} after={}",
                sha(&before[i]),
                sha(&after[i])
            );
        }
        assert_eq!(before, after, "{name}: refusal changed bytes");
    }

    #[test]
    fn owned_refusal_competing() {
        if let Some(path) = std::env::var_os("SAFEMESH_COMPETING_ROOT") {
            let mut contender = LocalReplica::counter(&PathBuf::from(path), config(0)).unwrap();
            let before = snapshot(&contender);
            let result = contender.bump(contender.ticket(), 99);
            println!("competing result={result:?}");
            let after = snapshot(&contender);
            // Print the digest pairs even when the revert control allows a write.
            proof("competing", &before, &after);
            assert!(matches!(result, Err(LocalError::Refused)));
            return;
        }
        let path = root("competing");
        let mut holder = LocalReplica::counter(&path, config(0)).unwrap();
        holder.bump(holder.ticket(), 5).unwrap();
        let before = snapshot(&holder);
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "owned_local::owned_refusal_competing",
                "--nocapture",
            ])
            .env("SAFEMESH_COMPETING_ROOT", &path)
            .output()
            .unwrap();
        print!("{}", String::from_utf8_lossy(&output.stdout));
        print!("{}", String::from_utf8_lossy(&output.stderr));
        proof("holder", &before, &snapshot(&holder));
        assert!(output.status.success(), "competing process must refuse");
    }
    #[test]
    fn owned_refusal_stale() {
        let mut r = LocalReplica::counter(&root("stale"), config(0)).unwrap();
        let stale = r.ticket();
        r.bump(stale, 5).unwrap();
        let current = r.renew(stale).unwrap();
        let before = snapshot(&r);
        let result = r.bump(stale, 99);
        println!("stale result={result:?}");
        proof("stale", &before, &snapshot(&r));
        assert!(matches!(result, Err(LocalError::Refused)));
        assert_eq!(r.bump(current, 7).unwrap().id.sequence, 2);
    }
    #[test]
    fn owned_refusal_coordinate() {
        let mut r = LocalReplica::counter(&root("coordinate"), config(0)).unwrap();
        r.bump(r.ticket(), 5).unwrap();
        let before = snapshot(&r);
        let result = r.receive(
            r.ticket(),
            Record {
                id: RecordId {
                    replica: 1,
                    sequence: 1,
                },
                delta: GCounterDelta {
                    replica: 0,
                    tally: 99,
                },
            },
        );
        println!("coordinate result={result:?}");
        proof("coordinate", &before, &snapshot(&r));
        assert!(matches!(result, Err(LocalError::Refused)));
        // A local caller cannot use the generic append to bypass ownership.
        assert!(matches!(
            r.append(
                r.ticket(),
                GCounterDelta {
                    replica: 1,
                    tally: 99
                }
            ),
            Err(LocalError::Refused)
        ));
        assert_eq!(before, snapshot(&r));
    }
    #[test]
    fn owned_refusal_token() {
        let mut r = LocalReplica::utf8_set(&root("token"), config(0)).unwrap();
        r.add(r.ticket(), "café☕".into()).unwrap();
        let before = snapshot(&r);
        let result = r.receive(
            r.ticket(),
            Record {
                id: RecordId {
                    replica: 1,
                    sequence: 1,
                },
                delta: OrSetDelta::Add {
                    element: "foreign".into(),
                    token: 2,
                },
            },
        );
        println!("token result={result:?}");
        proof("token", &before, &snapshot(&r));
        assert!(matches!(result, Err(LocalError::Refused)));
    }
    #[test]
    fn owned_convergence_counter_and_utf8_set() {
        let path = root("convergence-counter");
        let mut a = LocalReplica::counter(&path, config(0)).unwrap();
        let mut b = LocalReplica::counter(&path, config(1)).unwrap();
        let a1 = a.bump(a.ticket(), 5).unwrap();
        let a2 = a.bump(a.ticket(), 9).unwrap();
        let b1 = b.bump(b.ticket(), 7).unwrap();
        for record in [a2.clone(), a2, a1.clone(), a1] {
            b.receive(
                b.ticket(),
                Record::from_wire_bytes(&record.to_wire_bytes().unwrap()).unwrap(),
            )
            .unwrap();
        }
        assert_eq!(
            a.receive(a.ticket(), b1.clone()).unwrap(),
            Admission::Accepted
        );
        assert_eq!(a.receive(a.ticket(), b1).unwrap(), Admission::Duplicate);
        assert_eq!(a.state(), b.state());
        assert_eq!(a.state().state(), &[9, 7]);
        assert_eq!(a.log().version(), b.log().version());
        println!("counter duplicate+reordered converged={:?}", a.state());

        let path = root("convergence-set");
        let mut a = LocalReplica::utf8_set(&path, config(0)).unwrap();
        let mut b = LocalReplica::utf8_set(&path, config(1)).unwrap();
        let element = String::from("café☕");
        let a1 = a.add(a.ticket(), element.clone()).unwrap();
        let b1 = b.add(b.ticket(), "東京".into()).unwrap();
        b.receive(b.ticket(), a1.clone()).unwrap();
        let b2 = b.remove(b.ticket(), &element).unwrap(); // references A's token
        let a2 = a.add(a.ticket(), element.clone()).unwrap(); // concurrent fresh add
        for record in [b2.clone(), b2, b1.clone(), b1] {
            a.receive(
                a.ticket(),
                Record::from_wire_bytes(&record.to_wire_bytes().unwrap()).unwrap(),
            )
            .unwrap();
        }
        for record in [a2.clone(), a2, a1.clone(), a1] {
            b.receive(b.ticket(), record).unwrap();
        }
        assert_eq!(a.state(), b.state());
        assert!(a.state().contains(&element));
        assert_eq!(
            a.state().tombstones().iter().copied().collect::<Vec<_>>(),
            vec![2]
        );
        assert_eq!(a.log().version(), b.log().version());
        println!("utf8-set duplicate+reordered converged={:?}", a.state());
        let next_a = a.add(a.ticket(), "next".into()).unwrap();
        let next_b = b.add(b.ticket(), "next".into()).unwrap();
        assert_ne!(next_a.delta, next_b.delta);
        assert_eq!(next_a.id.sequence, 3);
        assert_eq!(next_b.id.sequence, 3);
    }
    #[test]
    fn owned_boundaries_and_existing_store_error() {
        let path = root("restart");
        let mut r = LocalReplica::counter(&path, config(0)).unwrap();
        let last = Record {
            id: RecordId {
                replica: 0,
                sequence: u64::MAX,
            },
            delta: GCounterDelta {
                replica: 0,
                tally: 8,
            },
        };
        r.receive(r.ticket(), last).unwrap();
        let before = snapshot(&r);
        assert!(matches!(r.bump(r.ticket(), 9), Err(LocalError::Exhausted)));
        assert_eq!(before, snapshot(&r));
        drop(r);
        let reopened = LocalReplica::counter(&path, config(0));
        println!("reopen error={:?}", reopened.as_ref().err());
        assert!(matches!(reopened, Err(LocalError::RecoveryRequired)));
        assert!(matches!(
            LocalReplica::counter(
                &path,
                WriterConfig {
                    writers: 3,
                    writer: 0
                }
            ),
            Err(LocalError::Configuration)
        ));
        assert!(matches!(
            LocalReplica::utf8_set(
                &root("zero"),
                WriterConfig {
                    writers: 0,
                    writer: 0
                }
            ),
            Err(LocalError::Configuration)
        ));
    }
}

#[test]
fn existing_counter_loader_refuses_foreign_coordinate_history() {
    let state = GCounter::new(2);
    let mut log = EventLog::with_replica_count(2);
    let bad = Record {
        id: RecordId {
            replica: 1,
            sequence: 1,
        },
        delta: GCounterDelta {
            replica: 0,
            tally: 99,
        },
    };
    assert_eq!(log.insert_record(bad), Admission::Accepted); // raw log is a carrier
    let bytes = log.to_wire_bytes().unwrap();
    assert_eq!(
        EventLog::<GCounterDelta>::from_wire_bytes_for(&bytes, &state),
        Err(safemesh_crdt::WireError::OwnershipViolation)
    );
    assert_eq!(state, GCounter::new(2));
    assert_eq!(log.to_wire_bytes().unwrap(), bytes);
}

#[test]
fn seqzero_admission_since_population() {
    use safemesh_crdt::VersionVector;
    let mut cases = 0;
    let mut records_tested = 0;
    let mut missing = 0;
    let mut refused_returned = 0;
    let mut wrong_prefix = 0;
    // Author identity is independent of the prefix comparison. Cover both word
    // boundaries, positive contiguous histories, and an unacknowledged gap.
    for author in [0, 1, u64::MAX] {
        for sequence in [0, 1, 2, 3, u64::MAX - 1, u64::MAX] {
            for history in [vec![], vec![1], vec![1, 2], vec![2]] {
                let make = |sequence, tally| Record {
                    id: RecordId {
                        replica: author,
                        sequence,
                    },
                    delta: GCounterDelta { replica: 1, tally },
                };
                let mut log = EventLog::new();
                let mut accepted = Vec::new();
                for seq in history {
                    let r = make(seq, 5);
                    assert_eq!(log.insert_record(r.clone()), Admission::Accepted);
                    accepted.push(r);
                    records_tested += 1;
                }
                let r = make(sequence, 5);
                let first = log.insert_record(r.clone());
                if first == Admission::Accepted {
                    accepted.push(r.clone());
                } else {
                    assert_eq!(first, Admission::Duplicate);
                }
                assert_eq!(
                    log.admit_with(r.clone(), |_| panic!("duplicate applied")),
                    Admission::Duplicate
                );
                let refused = make(sequence, 99);
                assert_eq!(
                    log.admit_with(refused.clone(), |_| panic!("collision applied")),
                    Admission::Collision
                );
                let all = log.since(&VersionVector::new());
                missing += accepted.iter().filter(|r| !all.contains(r)).count();
                refused_returned += all.iter().filter(|r| !accepted.contains(r)).count();
                // Observe builds only legal prefixes. Another author's prefix
                // stays nonempty even when this author's entry is absent.
                for prefix in [0, 1, 2, 3] {
                    let mut version = VersionVector::new();
                    for seq in 1..=prefix {
                        version.observe(RecordId {
                            replica: author,
                            sequence: seq,
                        });
                    }
                    version.observe(RecordId {
                        replica: author.wrapping_add(1),
                        sequence: 1,
                    });
                    let expected: Vec<_> = accepted
                        .iter()
                        .filter(|r| r.id.sequence == 0 || r.id.sequence > prefix)
                        .cloned()
                        .collect();
                    let actual = log.since(&version);
                    wrong_prefix += usize::from(actual != expected);
                    refused_returned += actual.iter().filter(|r| !accepted.contains(r)).count();
                }
                records_tested += 3;
                cases += 1;
            }
        }
    }
    println!("RECORDS TESTED {records_tested} BOUNDARY CASES {cases} MISSING {missing} RETURNED BUT REFUSED {refused_returned} WRONG PREFIX {wrong_prefix}");
    assert_eq!(
        refused_returned, 0,
        "since returned a record outside accepted history"
    );
    assert_eq!(
        missing, 0,
        "accepted records missing from empty-version pull"
    );
    assert_eq!(wrong_prefix, 0, "positive-prefix selection changed");
}

#[test]
fn seqzero_two_replica_exchange_and_persisted_replay() {
    use safemesh_crdt::{anti_entropy, InMemoryTransport, TransportAdapter, VersionVector};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    let mut source = EventLog::with_replica_count(2);
    let mut source_state = GCounter::new(2);
    // A zero record has an effect that later positive records do not subsume.
    for r in [record(0, 7), record(2, 5), record(1, 3)] {
        assert_eq!(
            source.admit_with(r, |d| source_state.apply_delta(d.clone())),
            Admission::Accepted
        );
    }
    let bytes = source.to_wire_bytes().unwrap();
    let restored = EventLog::<GCounterDelta>::from_wire_bytes(&bytes).unwrap();
    // The raw record kernel admits zero. The separate owned-counter loader
    // has always required positive sequences; this version repair does not
    // broaden that loader's admission contract.
    assert_eq!(
        EventLog::<GCounterDelta>::from_wire_bytes_for(&bytes, &GCounter::new(2)),
        Err(safemesh_crdt::WireError::OwnershipViolation)
    );
    assert_eq!(restored, source);
    assert_eq!(restored.since(&VersionVector::new()), source.records());
    {
        let bytes = include_bytes!("fixtures/old-zero.bin");
        let old = EventLog::<GCounterDelta>::from_wire_bytes(bytes).unwrap();
        let mut replay = GCounter::new(2);
        for r in old.records() {
            replay.apply_delta(r.delta.clone());
        }
        assert_eq!(replay.value(), 7);
        assert_eq!(old.since(&VersionVector::new()), vec![record(0, 7)]);
        println!("OLDER BUILD persisted zero: read=7 returned=1");
    }
    let mut peer = EventLog::with_replica_count(2);
    let mut peer_state = GCounter::new(2);
    let mut transport = InMemoryTransport::new();
    transport.subscribe(0);
    transport.subscribe(1);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut sender = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (mut receiver, _) = listener.accept().unwrap();
    for round in 0..2 {
        anti_entropy(&mut transport, 0, 1, &restored, peer.version()).unwrap();
        let envelopes = transport.drain(1);
        assert_eq!(envelopes.len(), if round == 0 { 1 } else { 0 });
        assert_eq!(
            envelopes.iter().map(|e| e.records.len()).sum::<usize>(),
            if round == 0 { 3 } else { 0 }
        );
        for envelope in envelopes {
            for r in envelope.records {
                let bytes = r.to_wire_bytes().unwrap();
                sender.write_all(&bytes).unwrap();
                let mut received = vec![0; bytes.len()];
                receiver.read_exact(&mut received).unwrap();
                let r = Record::<GCounterDelta>::from_wire_bytes(&received).unwrap();
                assert_eq!(
                    peer.admit_with(r, |d| peer_state.apply_delta(d.clone())),
                    if round == 0 {
                        Admission::Accepted
                    } else {
                        Admission::Duplicate
                    }
                );
            }
        }
        assert_eq!(peer_state, source_state);
        assert_eq!(peer.records(), source.records());
        assert_eq!(peer.version(), source.version());
    }
    println!(
        "TWO REPLICA EXCHANGE CONVERGES true tcp_rounds=2 value={}",
        peer_state.value()
    );
}

#[test]
fn seqzero_converged_replicas_quiesce_at_scale() {
    use safemesh_crdt::{anti_entropy, InMemoryTransport, TransportAdapter, VersionVector};
    for (authors, positives) in [(1, true), (1, false), (10, false), (100, false)] {
        let mut left = EventLog::with_replica_count(1);
        let mut right = EventLog::with_replica_count(1);
        for replica in 0..authors {
            // Admit zero AFTER positives: a positive prefix must not imply zero.
            let sequences = if positives { vec![1, 2, 0] } else { vec![0] };
            for sequence in sequences {
                let r = Record {
                    id: RecordId { replica, sequence },
                    delta: GCounterDelta {
                        replica: 0,
                        tally: 7,
                    },
                };
                assert_eq!(left.insert_record(r), Admission::Accepted);
            }
        }
        let mut transport = InMemoryTransport::new();
        transport.subscribe(0);
        transport.subscribe(1);
        // First repair from an empty peer must recover every admitted record.
        anti_entropy(&mut transport, 0, 1, &left, right.version()).unwrap();
        let mut recovered = 0;
        for envelope in transport.drain(1) {
            for r in envelope.records {
                let bytes = r.to_wire_bytes().unwrap();
                let decoded = Record::<GCounterDelta>::from_wire_bytes(&bytes).unwrap();
                assert_eq!(right.insert_record(decoded), Admission::Accepted);
                recovered += 1;
            }
        }
        assert_eq!(recovered, left.records().len());
        assert_eq!(left, right);
        let mut series = Vec::new();
        for _ in 0..10 {
            anti_entropy(&mut transport, 0, 1, &left, right.version()).unwrap();
            anti_entropy(&mut transport, 1, 0, &right, left.version()).unwrap();
            let mut sent = 0;
            for (peer, log) in [(0, &mut left), (1, &mut right)] {
                for envelope in transport.drain(peer) {
                    sent += envelope.records.len();
                    for r in envelope.records {
                        assert_eq!(
                            log.admit_with(r, |_| panic!("converged duplicate applied")),
                            Admission::Duplicate
                        );
                    }
                }
            }
            series.push(sent);
        }
        println!(
            "QUIESCENCE N={authors} POSITIVES={positives} RECOVERED={recovered} SERIES {series:?}"
        );
        assert_eq!(series, vec![0; 10], "converged replicas must stay quiet");
        // Rebuild versions through the legacy log format, not a cloned cache.
        let restored =
            EventLog::<GCounterDelta>::from_wire_bytes(&left.to_wire_bytes().unwrap()).unwrap();
        assert_eq!(restored.version(), left.version());
        assert!(left.since(restored.version()).is_empty());
        assert_eq!(left.since(&VersionVector::new()).len(), recovered);
    }
}

#[test]
fn seqzero_acknowledgment_is_independent_of_positive_prefix() {
    use safemesh_crdt::VersionVector;
    let zero = RecordId {
        replica: 42,
        sequence: 0,
    };
    let mut version = VersionVector::new();
    assert!(!version.includes(zero));
    for sequence in 1..=3 {
        version.observe(RecordId {
            replica: 42,
            sequence,
        });
    }
    assert!(!version.includes(zero));
    assert_eq!(version.get(42), 3);
    let entries = version.entries().clone();
    version.observe(zero);
    assert!(version.includes(zero));
    assert_eq!(version.entries(), &entries);
    assert_eq!(
        version.zero_replicas().iter().copied().collect::<Vec<_>>(),
        vec![42]
    );
    assert!(!version.includes(RecordId {
        replica: 43,
        sequence: 0
    }));
    let snapshot = version.clone();
    version.observe(zero);
    assert_eq!(version, snapshot);
    let mut log = EventLog::new();
    for replica in [42, 43] {
        for sequence in 0..=4 {
            assert_eq!(
                log.insert_record(Record {
                    id: RecordId { replica, sequence },
                    delta: 7u64
                }),
                Admission::Accepted
            );
        }
    }
    let ids: Vec<_> = log.since(&version).iter().map(|r| r.id).collect();
    let expected: Vec<_> = log
        .records()
        .iter()
        .filter(|r| r.id.replica == 43 || r.id.sequence == 4)
        .map(|r| r.id)
        .collect();
    assert_eq!(ids, expected);
}
