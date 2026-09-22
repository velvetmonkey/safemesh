use safemesh_crdt::{
    Admission, Crdt, EventLog, GCounter, GCounterDelta, Record, VersionVector, VersionVectorLimits,
    WireDecode, WireEncode,
};

fn main() {
    let mut left = GCounter::new(2);
    let mut right = GCounter::new(2);
    let mut sent = EventLog::for_crdt(&left);
    let mut received = EventLog::for_crdt(&right);
    for tally in [4, 6] {
        sent.append_with(
            &mut left,
            0,
            GCounterDelta { replica: 0, tally },
            |state, delta| state.apply_delta(delta.clone()),
        )
        .unwrap();
    }
    // Bytes leave the library here. An application would send these frames.
    let frames: Vec<_> = sent
        .records()
        .iter()
        .map(|record| record.to_wire_bytes().unwrap())
        .collect();
    let receive = |bytes: &[u8], state: &mut GCounter, log: &mut EventLog<GCounterDelta>| {
        assert!(bytes.len() <= 1024); // Check before decoding each record.
        let record = Record::<GCounterDelta>::from_wire_bytes(bytes).unwrap();
        log.admit_with(state, record, |state, delta| {
            state.apply_delta(delta.clone())
        })
    };
    // The first frame is lost; receiving sequence 2 must not acknowledge the gap.
    assert_eq!(
        receive(&frames[1], &mut right, &mut received),
        Admission::Accepted
    );
    assert_eq!(received.version().get(0), 0);
    // The app carries BOTH version collections; no built-in version wire codec.
    // Enforce byte/entry budgets in that decoder before allocating these collections.
    let peer = VersionVector::from_peer_prefixes_with_limits(
        received.version().entries(),
        received.version().zero_replicas(),
        VersionVectorLimits {
            max_authors: Some(2),
            max_zero_replicas: Some(2),
        },
    )
    .unwrap();
    let missing = sent.since(&peer);
    assert_eq!(missing.len(), 2); // Contiguous prefix conservatively resends sequence 2.
    let repair: Vec<_> = missing
        .iter()
        .map(|record| record.to_wire_bytes().unwrap())
        .collect();
    assert_eq!(
        receive(&repair[0], &mut right, &mut received),
        Admission::Accepted
    );
    assert_eq!(
        receive(&repair[1], &mut right, &mut received),
        Admission::Duplicate
    );
    assert_eq!(received.version().get(0), 2);
    assert_eq!(received.records().len(), 2);
    assert_eq!(right.value(), 6);
    assert_eq!(left.state(), right.state());
    assert!(sent.since(received.version()).is_empty());
    println!("recovered records=2 total=6 duplicate=unchanged");
}
