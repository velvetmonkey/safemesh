// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0
#![cfg(feature = "local-writer")]
use safemesh_crdt::{local::*, ownership::WriterConfig, *};
use serde_json::{json, Value};
use std::{
    fmt::Debug,
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
fn corpus() -> PathBuf {
    std::env::var_os("BOOTFIXTURE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bootstrap"))
}
fn expectations() -> Value {
    let text = fs::read_to_string(corpus().join("README.md")).unwrap();
    serde_json::from_str(
        text.split("```json\n")
            .nth(1)
            .unwrap()
            .split("```")
            .next()
            .unwrap(),
    )
    .unwrap()
}
fn records<D: Debug>(log: &EventLog<D>) -> Value {
    json!(log
        .records()
        .iter()
        .map(|r| json!([r.id.replica, r.id.sequence, format!("{:?}", r.delta)]))
        .collect::<Vec<_>>())
}
fn versions<D>(log: &EventLog<D>) -> Value {
    json!([log.version().get(0), log.version().get(1)])
}
fn words(bytes: &[u8]) -> Value {
    json!(bytes
        .chunks_exact(8)
        .map(|x| u64::from_le_bytes(x.try_into().unwrap()))
        .collect::<Vec<_>>())
}
fn set_state(s: &OrSet<String, u64>) -> Value {
    json!({"adds":s.adds(),"tombstones":s.tombstones(),"elements":s.elements()})
}
fn check_log<D: Debug>(name: &str, log: &EventLog<D>, e: &Value) {
    assert_eq!(
        records(log),
        e["records"],
        "{name}: accepted records disagreed"
    );
    assert_eq!(
        versions(log),
        e["versions"],
        "{name}: contiguous versions disagreed"
    );
    assert_eq!(
        log.version().entries().len(),
        1,
        "{name}: unexpected version coordinate"
    );
    assert!(
        log.version().zero_replicas().is_empty(),
        "{name}: unexpected sequence-zero observation"
    );
}
fn replay<C>(name: &str, mut state: C, view: impl Fn(&C) -> Value) -> (EventLog<C::Delta>, C)
where
    C: Crdt,
    C::Delta: Clone + PartialEq + Debug + WireDecode + WireSchema + WireEncode,
{
    let e = &expectations()[name];
    let bytes = fs::read(corpus().join(format!("{name}.log"))).unwrap();
    let decoded = EventLog::<C::Delta>::from_wire_bytes_for(&bytes, &state)
        .unwrap_or_else(|e| panic!("{name}: fixture log decode disagreed: {e:?}"));
    let mut log = EventLog::for_crdt(&state);
    for r in decoded.records() {
        assert_eq!(
            log.admit_with(r.clone(), |d| state.apply_delta(d.clone())),
            Admission::Accepted,
            "{name}: replay admission disagreed"
        );
    }
    check_log(name, &log, e);
    assert_eq!(view(&state), e["state"], "{name}: replay state disagreed");
    for r in decoded.records() {
        let before = (
            view(&state),
            log.to_wire_bytes().unwrap(),
            log.version().clone(),
        );
        assert_eq!(
            log.admit_with(r.clone(), |_| panic!("{name}: duplicate reapplied")),
            Admission::Duplicate
        );
        assert_eq!(
            (
                view(&state),
                log.to_wire_bytes().unwrap(),
                log.version().clone()
            ),
            before,
            "{name}: duplicate changed state/log/version"
        );
    }
    (log, state)
}
fn store(name: &str) -> PathBuf {
    let root = scratch(name);
    for suffix in ["transaction", "fence"] {
        fs::copy(
            corpus().join(format!("{name}.{suffix}")),
            root.join(format!("writer-0.{suffix}")),
        )
        .unwrap();
    }
    root
}
fn durable_checks<C>(name: &str, root: &Path, r: &mut DurableReplica<C>, view: impl Fn(&C) -> Value)
where
    C: Crdt,
    C::Delta: ownership::OwnedDelta + Clone + PartialEq + Debug + WireEncode + WireSchema,
{
    let e = &expectations()[name];
    check_log(name, r.log(), e);
    assert_eq!(
        view(r.state()),
        e["state"],
        "{name}: durable state disagreed"
    );
    assert_eq!(
        words(&r.allocation_bytes()),
        e["allocation"],
        "{name}: allocation disagreed"
    );
    let committed = CommittedTransaction::read(root, config()).unwrap();
    assert_eq!(committed.config, config());
    assert_eq!(
        committed.last_sequence,
        e["allocation"][2].as_u64().unwrap()
    );
    assert_eq!(
        committed.log_bytes,
        fs::read(corpus().join(format!("{name}.log"))).unwrap(),
        "{name}: wrapper suffix disagreed"
    );
    let before = (
        view(r.state()),
        r.log().to_wire_bytes().unwrap(),
        r.log().version().clone(),
        r.allocation_bytes(),
        fs::read(root.join("writer-0.transaction")).unwrap(),
    );
    for record in r.log().records().to_vec() {
        assert_eq!(
            r.receive(r.ticket(), record).unwrap(),
            Admission::Duplicate,
            "{name}: durable redelivery disagreed"
        );
        assert_eq!(
            (
                view(r.state()),
                r.log().to_wire_bytes().unwrap(),
                r.log().version().clone(),
                r.allocation_bytes(),
                fs::read(root.join("writer-0.transaction")).unwrap()
            ),
            before,
            "{name}: durable duplicate changed state/log/version/allocation/store"
        );
    }
}
#[test]
fn replay_bootstrap() {
    replay("counter", GCounter::new(2), |c| json!(c.state()));
    replay("orset", OrSet::<String, u64>::new(), set_state);
    replay(
        "pn",
        PnCounter::new(2),
        |c| json!({"p":c.p_state(),"n":c.n_state(),"value":c.value()}),
    );
    let e = expectations();
    for name in ["counter", "orset"] {
        assert_eq!(
            words(&fs::read(corpus().join(format!("{name}.fence"))).unwrap()),
            json!([2, 0, 1]),
            "{name}: fence disagreed"
        );
    }
    let root = store("counter");
    let mut c = DurableReplica::restart_counter(&root, config())
        .unwrap_or_else(|e| panic!("counter: durable restart disagreed: {e:?}"));
    durable_checks("counter", &root, &mut c, |c| json!(c.state()));
    let next = c.bump(c.ticket(), 9).unwrap();
    assert_eq!(
        next.id,
        RecordId {
            replica: 0,
            sequence: e["counter"]["next_sequence"].as_u64().unwrap()
        },
        "counter: recovered sequence disagreed"
    );
    drop(c);
    assert_eq!(
        DurableReplica::restart_counter(&root, config())
            .unwrap()
            .state()
            .state(),
        &[9, 7]
    );
    let root = store("orset");
    let mut s = DurableReplica::restart_utf8_set(&root, config())
        .unwrap_or_else(|e| panic!("orset: durable restart disagreed: {e:?}"));
    durable_checks("orset", &root, &mut s, set_state);
    let next = s.add(s.ticket(), "café☕".into()).unwrap();
    assert_eq!(
        next.id,
        RecordId {
            replica: 0,
            sequence: e["orset"]["next_sequence"].as_u64().unwrap()
        },
        "orset: recovered sequence disagreed"
    );
    assert_eq!(
        next.delta,
        OrSetDelta::Add {
            element: "café☕".into(),
            token: 6
        },
        "orset: recovered token disagreed"
    );
    drop(s);
    let s = DurableReplica::restart_utf8_set(&root, config()).unwrap();
    assert!(s.state().contains(&"café☕".into()));
    assert!(s.state().tombstones().contains(&2));
    let bytes = fs::read(corpus().join("gset.state")).unwrap();
    let decoded = GSet::<u64>::from_wire_bytes(&bytes).expect("gset: state decode disagreed");
    let mut gs = GSet::new();
    gs.merge(&decoded);
    gs.merge(&decoded);
    assert_eq!(
        json!(gs.elements()),
        e["gset"]["state"],
        "gset: merged state disagreed"
    );
    let bytes = fs::read(corpus().join("rga.state")).unwrap();
    let decoded = Rga::<u64, u64>::from_wire_bytes(&bytes).expect("rga: state decode disagreed");
    let mut rga = Rga::new();
    rga.merge(&decoded);
    rga.merge(&decoded);
    assert_eq!(
        json!({"placed":rga.placed(),"tombstones":rga.tombstones(),"live":rga.live_entries()}),
        e["rga"]["state"],
        "rga: merged state disagreed"
    );
}
#[test]
fn load_failures() {
    for name in ["counter", "orset"] {
        for (case, expected) in [
            ("truncated", "RecoveryRequired"),
            ("integrity", "History(IntegrityMismatch)"),
            ("retired", "History(InvalidTag)"),
            ("unknown", "History(InvalidTag)"),
        ] {
            let root = store(name);
            let path = root.join("writer-0.transaction");
            let mut bytes = fs::read(&path).unwrap();
            match case {
                "truncated" => bytes.truncate(23),
                "integrity" => *bytes.last_mut().unwrap() ^= 1,
                "retired" => bytes[24] = 0x02,
                "unknown" => bytes[24] = 0xff,
                _ => unreachable!(),
            }
            fs::write(&path, &bytes).unwrap();
            let result = if name == "counter" {
                DurableReplica::restart_counter(&root, config()).map(|_| ())
            } else {
                DurableReplica::restart_utf8_set(&root, config()).map(|_| ())
            };
            println!(
                "{name} {case}: {result:?}; empty replica returned={}",
                result.is_ok()
            );
            let err = result.expect_err("PRODUCT DEFECT: damaged history returned a replica");
            assert_eq!(
                format!("{err:?}"),
                expected,
                "{name}/{case}: load diagnosis changed; investigate version dispatch"
            );
            assert_eq!(
                fs::read(&path).unwrap(),
                bytes,
                "load failure changed transaction"
            );
            assert_eq!(
                fs::read(root.join("writer-0.fence")).unwrap(),
                fs::read(corpus().join(format!("{name}.fence"))).unwrap(),
                "load failure changed fence"
            );
        }
    }
}
