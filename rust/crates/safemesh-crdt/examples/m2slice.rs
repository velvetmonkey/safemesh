// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0
//! Public API walk: m2slice <scratch-directory> [--require-integrity].
//! Use --features laws and a fresh scratch directory for the packet D process
//! journey (Linux local filesystems). Keep the directory for subsequent recovery.
use safemesh_crdt::{
    Admission, Crdt, EventLog, GCounter, GCounterDelta, OrSet, OrSetDelta, Record, WireDecode,
    WireEncode, WireError, WireSchema,
};
use std::{fmt::Debug, fs, path::Path};

#[path = "../src/persistence.rs"]
mod persistence;

// Caller orchestration; no library-private or test-only APIs.
struct Replica<C: Crdt> {
    state: C,
    log: EventLog<C::Delta>,
}
impl<C: Crdt> Replica<C>
where
    C::Delta: Clone + PartialEq + WireEncode + WireDecode + WireSchema,
{
    fn new(state: C) -> Self {
        Self {
            log: EventLog::for_crdt(&state),
            state,
        }
    }
    fn local(&mut self, id: u64, delta: C::Delta) {
        self.log
            .append_with(id, delta, |d| self.state.apply_delta(d.clone()))
            .unwrap();
    }
    fn persist(&self, path: &Path) {
        persistence::replace(path, &self.log.to_wire_bytes().unwrap()).unwrap();
    }
    fn restart(path: &Path, mut state: C) -> Result<Self, WireError> {
        let log = EventLog::<C::Delta>::from_wire_bytes_for(&fs::read(path).unwrap(), &state)?;
        for r in log.records() {
            state.apply_delta(r.delta.clone());
        }
        Ok(Self { state, log })
    }
}
fn exchange<C: Crdt>(a: &mut Replica<C>, b: &mut Replica<C>)
where
    C::Delta: Clone + PartialEq + WireEncode + WireDecode + WireSchema,
{
    // Both directions captured before delivery; only real wire bytes cross.
    let ab: Vec<_> = a
        .log
        .since(b.log.version())
        .iter()
        .map(|r| r.to_wire_bytes().unwrap())
        .collect();
    let ba: Vec<_> = b
        .log
        .since(a.log.version())
        .iter()
        .map(|r| r.to_wire_bytes().unwrap())
        .collect();
    for (receiver, packets) in [(b, ab), (a, ba)] {
        for packet in packets {
            let record = Record::<C::Delta>::from_wire_bytes(&packet).unwrap();
            assert_eq!(
                receiver
                    .log
                    .admit_with(record, |d| receiver.state.apply_delta(d.clone())),
                Admission::Accepted
            );
        }
    }
}
fn journey<C: Crdt + Debug + PartialEq>(
    root: &Path,
    name: &str,
    empty: impl Fn() -> C,
    initial: [C::Delta; 2],
    partition: impl Fn(&C) -> [C::Delta; 2],
) where
    C::Delta: Clone + PartialEq + WireEncode + WireDecode + WireSchema,
{
    let paths = [
        root.join(format!("{name}-a.log")),
        root.join(format!("{name}-b.log")),
    ];
    let mut a = Replica::new(empty());
    let mut b = Replica::new(empty());
    println!("{name} step=1 WORKS");
    let [da, db] = initial;
    a.local(0, da);
    b.local(1, db);
    println!("{name} step=2 WORKS");
    exchange(&mut a, &mut b);
    assert_eq!(a.state, b.state);
    println!("{name} step=3 WORKS");
    a.persist(&paths[0]);
    b.persist(&paths[1]);
    println!("{name} step=4 WORKS");
    drop(a);
    drop(b);
    let mut a = Replica::restart(&paths[0], empty()).unwrap();
    let mut b = Replica::restart(&paths[1], empty()).unwrap();
    assert_eq!(a.state, b.state);
    assert_eq!(a.log.records().len(), 2);
    println!("{name} step=5 WORKS clean-bytes=true");
    let [da, db] = partition(&a.state);
    // Partition: no exchange until after the inequality assertion.
    a.local(0, da);
    b.local(1, db);
    assert_ne!(a.state, b.state, "partition must diverge");
    a.persist(&paths[0]);
    b.persist(&paths[1]);
    println!("{name} step=6 WORKS diverged=true");
    exchange(&mut a, &mut b);
    println!("{name} step=7 WORKS");
    assert_eq!(a.state, b.state);
    assert_eq!(a.log.records().len(), 4);
    assert_eq!(b.log.records().len(), 4);
    assert_eq!(a.log.version(), b.log.version());
    for (replica, path) in [(&a, &paths[0]), (&b, &paths[1])] {
        replica.persist(path);
        let replay = Replica::restart(path, empty()).unwrap();
        assert_eq!(replay.state, replica.state); // includes tombstones
        assert!(replay.log == replica.log);
    }
    println!(
        "{name} step=8 WORKS records-per-replica=4 bytes-a={} bytes-b={} state={:?}",
        fs::metadata(&paths[0]).unwrap().len(),
        fs::metadata(&paths[1]).unwrap().len(),
        a.state
    );
}
fn corruption<C: Crdt + Debug + PartialEq>(
    root: &Path,
    name: &str,
    empty: impl Fn() -> C,
    needle: &[u8],
) -> bool
where
    C::Delta: Clone + PartialEq + WireEncode + WireDecode + WireSchema,
{
    let path = root.join(format!("{name}-a.log"));
    let bytes = fs::read(&path).unwrap();
    let original = Replica::restart(&path, empty()).unwrap();
    let bad_path = root.join(format!("{name}-corrupt.log"));
    let mut bad = bytes.clone();
    bad[0] ^= 0xff;
    fs::write(&bad_path, &bad).unwrap();
    assert!(matches!(
        Replica::restart(&bad_path, empty()),
        Err(WireError::InvalidTag)
    ));
    println!("{name} corrupt-tag-detected=true error=InvalidTag");
    let mut bad = bytes;
    let offset = bad
        .windows(needle.len())
        .rposition(|w| w == needle)
        .unwrap();
    bad[offset] ^= 1;
    fs::write(&bad_path, &bad).unwrap();
    let detected = match Replica::restart(&bad_path, empty()) {
        Err(error) => {
            println!("{name} corrupt-payload-offset={offset} restart=Err({error:?})");
            true
        }
        Ok(restarted) => {
            assert_ne!(
                restarted.state, original.state,
                "control must change full state"
            );
            println!(
                "{name} corrupt-payload-offset={offset} restart=Ok wrong-state={:?}",
                restarted.state
            );
            false
        }
    };
    println!("{name} corrupt-byte-detected={detected}");
    detected
}
fn main() {
    #[cfg(feature = "local-writer")]
    if let Some(root) = std::env::var_os("SAFEMESH_M2_JOINED_CHILD") {
        joined::child(Path::new(&root));
    }
    let mut args = std::env::args().skip(1);
    let root = args
        .next()
        .expect("usage: m2slice <scratch-directory> [--require-integrity]");
    let require_integrity = args.next().as_deref() == Some("--require-integrity");
    let root = Path::new(&root);
    fs::create_dir_all(root).unwrap();
    #[cfg(feature = "local-writer")]
    joined::run(root);
    journey(
        root,
        "counter",
        || GCounter::new(2),
        [
            GCounterDelta {
                replica: 0,
                tally: 10,
            },
            GCounterDelta {
                replica: 1,
                tally: 20,
            },
        ],
        |s| {
            [
                GCounterDelta {
                    replica: 0,
                    tally: s.state()[0] + 1,
                },
                GCounterDelta {
                    replica: 1,
                    tally: s.state()[1] + 2,
                },
            ]
        },
    );
    journey(
        root,
        "utf8-orset",
        OrSet::<String, u64>::new,
        [
            OrSetDelta::Add {
                element: "café☕".into(),
                token: 100,
            },
            OrSetDelta::Add {
                element: "東京".into(),
                token: 200,
            },
        ],
        |s| {
            [
                OrSetDelta::Remove {
                    tokens: s.observed_tokens(&"café☕".into()).into_iter().collect(),
                },
                OrSetDelta::Add {
                    element: "café☕".into(),
                    token: 201,
                },
            ]
        },
    );
    let counter = corruption(root, "counter", || GCounter::new(2), &11u64.to_le_bytes());
    let orset = corruption(
        root,
        "utf8-orset",
        OrSet::<String, u64>::new,
        "café☕".as_bytes(),
    );
    if require_integrity {
        assert!(
            counter && orset,
            "step 5 integrity wall: persisted payload corruption silently loads a different state"
        );
    }
}

#[cfg(test)]
mod logshape_tests {
    use super::*;

    #[test]
    fn physical_tamper_two_replica_log_into_three() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/logshape-tamper");
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("counter.log");
        let path = path.as_path();
        let mut source = Replica::new(GCounter::new(2));
        source.local(
            0,
            GCounterDelta {
                replica: 0,
                tally: 10,
            },
        );
        source.local(
            1,
            GCounterDelta {
                replica: 1,
                tally: 20,
            },
        );
        source.persist(path);
        let bytes = fs::read(path).unwrap();
        let result = Replica::restart(path, GCounter::new(3));
        assert_eq!(fs::read(path).unwrap(), bytes);
        let outcome = match result {
            Ok(replica) => format!("Ok wrong-state={:?}", replica.state.state()),
            Err(error) => format!("Err({error:?})"),
        };
        println!(
            "two-replica persisted bytes={} three-replica restart={outcome}",
            bytes.len()
        );
        assert_eq!(
            outcome,
            "Err(ReplicaCountMismatch { expected: 3, actual: 2 })"
        );
    }
}

// Packet D composes the public owned/durable/restart APIs. The earlier walk
// remains available without local-writer; use --features laws for this journey.
#[cfg(feature = "local-writer")]
mod joined {
    use super::*;
    use safemesh_crdt::{
        local::DurableReplica,
        ownership::{OwnedDelta, WriterConfig},
        RecordId,
    };
    use std::io::Write;

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
    fn joined_exchange<C>(
        a: &mut DurableReplica<C>,
        b: &mut DurableReplica<C>,
        empty: impl Fn() -> C,
    ) where
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
                        log.admit_with(record, |d| state.apply_delta(d.clone())),
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
        std::println!(
            "joined reconciled counter={:?} set={:?}",
            a.state().state(),
            sa.state()
        );
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
        assert!(!loss);
        c.bump(c.ticket(), 5).unwrap();
        c.bump(c.ticket(), 9).unwrap();
        s.add(s.ticket(), "café☕".into()).unwrap();
        s.add(s.ticket(), "東京".into()).unwrap();
        // External audit only: restart never reads these files as recovery data.
        fs::write(
            root.join("counter-issued"),
            c.log().to_wire_bytes().unwrap(),
        )
        .unwrap();
        fs::write(root.join("set-issued"), s.log().to_wire_bytes().unwrap()).unwrap();
        std::println!("joined ACK counter-successive-tallies=5,9 set=café☕,東京 loss={loss}");
        std::io::stdout().flush().unwrap();
        // Intentionally bypass destructors: a real process ends after ACK.
        std::process::exit(77);
    }

    pub fn child(root: &Path) {
        joined_start(root, false);
    }
    pub fn run(root: &Path) {
        let root = root.join("joined");
        fs::create_dir_all(&root).unwrap();
        fs::File::open(root.parent().unwrap())
            .unwrap()
            .sync_all()
            .unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .env("SAFEMESH_M2_JOINED_CHILD", &root)
            .output()
            .unwrap();
        fs::write(
            root.join("child.exit"),
            output.status.code().unwrap().to_string(),
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("child.exit")).unwrap(),
            "77",
            "{output:?}"
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("joined ACK"));
        println!("{}", String::from_utf8_lossy(&output.stdout));
        joined_finish(&root, false);
    }
}
