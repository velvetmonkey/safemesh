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
