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
    let mut log = EventLog::new();
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
