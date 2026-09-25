// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0
#![cfg(all(feature = "local-writer", target_os = "linux"))]
use safemesh_crdt::{local::*, ownership::WriterConfig, *};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

fn config() -> WriterConfig {
    WriterConfig {
        writers: 2,
        writer: 0,
    }
}
fn scratch(label: &str) -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "bootfixture-{}-{}-{label}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).unwrap();
    root
}
fn record<D>(replica: u64, sequence: u64, delta: D) -> Record<D> {
    Record {
        id: RecordId { replica, sequence },
        delta,
    }
}
fn write_new(root: &Path, name: &str, bytes: &[u8]) {
    use std::io::Write;
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join(name))
        .unwrap();
    f.write_all(bytes).unwrap();
}
fn retain<C: Crdt>(out: &Path, root: &Path, name: &str, r: &DurableReplica<C>)
where
    C::Delta: ownership::OwnedDelta + Clone + PartialEq + WireEncode + WireSchema,
{
    write_new(
        out,
        &format!("{name}.transaction"),
        &fs::read(root.join("writer-0.transaction")).unwrap(),
    );
    write_new(
        out,
        &format!("{name}.fence"),
        &fs::read(root.join("writer-0.fence")).unwrap(),
    );
    write_new(
        out,
        &format!("{name}.log"),
        &r.log().to_wire_bytes().unwrap(),
    );
    println!(
        "{name}: package={} log_trait_schema={}",
        env!("CARGO_PKG_VERSION"),
        String::from_utf8(EventLog::<C::Delta>::wire_schema()).unwrap()
    );
}
#[test]
#[ignore = "explicit baseline-only generator; never overwrite retained fixtures"]
fn generate_bootstrap() {
    let out = PathBuf::from(
        std::env::var_os("BOOTFIXTURE_GENERATE").expect("BOOTFIXTURE_GENERATE required"),
    );
    fs::create_dir(&out).unwrap();
    let root = scratch("generate-counter");
    let mut c = DurableReplica::counter(&root, config()).unwrap();
    c.bump(c.ticket(), 5).unwrap();
    assert_eq!(
        c.receive(
            c.ticket(),
            record(
                1,
                3,
                GCounterDelta {
                    replica: 1,
                    tally: 7
                }
            )
        )
        .unwrap(),
        Admission::Accepted
    );
    retain(&out, &root, "counter", &c);
    let root = scratch("generate-orset");
    let mut s = DurableReplica::utf8_set(&root, config()).unwrap();
    s.add(s.ticket(), "café☕".into()).unwrap();
    s.remove(s.ticket(), &"café☕".into()).unwrap();
    assert_eq!(
        s.receive(
            s.ticket(),
            record(
                1,
                3,
                OrSetDelta::Add {
                    element: "東京".into(),
                    token: 7
                }
            )
        )
        .unwrap(),
        Admission::Accepted
    );
    retain(&out, &root, "orset", &s);
    let mut pn = EventLog::for_crdt(&PnCounter::new(2));
    assert_eq!(
        pn.insert_record(record(
            0,
            1,
            PnCounterDelta::Inc {
                replica: 0,
                tally: 9
            }
        )),
        Admission::Accepted
    );
    assert_eq!(
        pn.insert_record(record(
            1,
            3,
            PnCounterDelta::Dec {
                replica: 1,
                tally: 4
            }
        )),
        Admission::Accepted
    );
    write_new(&out, "pn.log", &pn.to_wire_bytes().unwrap());
    println!(
        "pn: package={} log_trait_schema={}",
        env!("CARGO_PKG_VERSION"),
        String::from_utf8(EventLog::<PnCounterDelta>::wire_schema()).unwrap()
    );
    let mut gs = GSet::<u64>::new();
    gs.insert(7);
    gs.insert(42);
    write_new(&out, "gset.state", &gs.to_wire_bytes().unwrap());
    let mut rga = Rga::<u64, u64>::new();
    rga.insert(1, 65);
    rga.insert(2, 233);
    rga.delete(1);
    write_new(&out, "rga.state", &rga.to_wire_bytes().unwrap());
    println!(
        "state schemas (not embedded): {} {}",
        String::from_utf8(GSet::<u64>::wire_schema()).unwrap(),
        String::from_utf8(Rga::<u64, u64>::wire_schema()).unwrap()
    );
}
