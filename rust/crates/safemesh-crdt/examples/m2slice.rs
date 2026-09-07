// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0
//! Public API walk: m2slice <scratch-directory> [--require-integrity].
use safemesh_crdt::{
    Admission, Crdt, EventLog, GCounter, GCounterDelta, OrSet, OrSetDelta, Record, WireDecode,
    WireEncode, WireError,
};
use std::{fmt::Debug, fs, io::Write, path::Path};

// Caller orchestration; no library-private or test-only APIs.
struct Replica<C: Crdt> {
    state: C,
    log: EventLog<C::Delta>,
}
impl<C: Crdt> Replica<C>
where
    C::Delta: Clone + PartialEq + WireEncode + WireDecode,
{
    fn new(state: C) -> Self {
        Self {
            state,
            log: EventLog::new(),
        }
    }
    fn local(&mut self, id: u64, delta: C::Delta) {
        self.log
            .append_with(id, delta, |d| self.state.apply_delta(d.clone()))
            .unwrap();
    }
    fn persist(&self, path: &Path) {
        let temporary = path.with_extension("tmp");
        let mut file = fs::File::create(&temporary).unwrap();
        file.write_all(&self.log.to_wire_bytes().unwrap()).unwrap();
        file.sync_all().unwrap();
        fs::rename(&temporary, path).unwrap();
    }
    fn restart(path: &Path, mut state: C) -> Result<Self, WireError> {
        let log = EventLog::<C::Delta>::from_wire_bytes(&fs::read(path).unwrap())?;
        for r in log.records() {
            state.apply_delta(r.delta.clone());
        }
        Ok(Self { state, log })
    }
}
fn exchange<C: Crdt>(a: &mut Replica<C>, b: &mut Replica<C>)
where
    C::Delta: Clone + PartialEq + WireEncode + WireDecode,
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
    C::Delta: Clone + PartialEq + WireEncode + WireDecode,
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
    C::Delta: Clone + PartialEq + WireEncode + WireDecode,
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
        Err(_) => true,
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
    let mut args = std::env::args().skip(1);
    let root = args
        .next()
        .expect("usage: m2slice <scratch-directory> [--require-integrity]");
    let require_integrity = args.next().as_deref() == Some("--require-integrity");
    let root = Path::new(&root);
    fs::create_dir_all(root).unwrap();
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
