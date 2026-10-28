#![cfg(all(feature = "local-writer", target_os = "linux"))]

use safemesh_crdt::{
    local::DurableReplica, ownership::WriterConfig, Admission, DecodeLimits, EventLog, GCounter,
    GCounterDelta, Replica,
};
use std::{collections::BTreeSet, fs};

#[test]
fn a_permanent_gap_reoffers_duplicates_without_growing_the_journal() {
    const N: u64 = 1_000;
    const EXCHANGES: usize = 3;

    let root = std::env::temp_dir().join(format!("smp43dupzero-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let mut sender = Replica::new(GCounter::new(2));
    let records: Vec<_> = (1..=N)
        .map(|tally| {
            sender
                .append(1, GCounterDelta { replica: 1, tally })
                .unwrap()
        })
        .collect();
    let mut receiver = DurableReplica::counter(
        &root,
        WriterConfig {
            writers: 2,
            writer: 0,
        },
    )
    .unwrap();
    let ticket = receiver.ticket();
    for record in records.iter().filter(|record| record.id.sequence != 2) {
        assert_eq!(
            receiver.receive(ticket, record.clone()).unwrap(),
            Admission::Accepted
        );
    }

    let journal = root.join("writer-0.journal");
    let initial_bytes = fs::read(&journal).unwrap();
    let initial_count = receiver.log().records().len();
    let initial_state = receiver.state().clone();
    assert_eq!(initial_count, (N - 1) as usize);
    assert_eq!(receiver.log().version().get(1), 1);

    for exchange in 0..EXCHANGES {
        let offered = sender.since(receiver.log().version());
        assert_eq!(
            offered.len(),
            (N - 1) as usize,
            "exchange {exchange}: offer count"
        );
        let offered_sequences: BTreeSet<_> =
            offered.iter().map(|record| record.id.sequence).collect();
        assert_eq!(
            offered_sequences,
            (2..=N).collect(),
            "exchange {exchange}: offered IDs"
        );
        // The simulated transport keeps losing sequence 2 while delivering later records.
        let delivered: Vec<_> = offered
            .into_iter()
            .filter(|record| record.id.sequence != 2)
            .collect();
        assert_eq!(
            (N - 1) as usize - delivered.len(),
            1,
            "exchange {exchange}: dropped gap"
        );
        assert_eq!(
            delivered.len(),
            (N - 2) as usize,
            "exchange {exchange}: duplicates delivered"
        );
        let mut frame = Vec::new();
        EventLog::encode_records(Some(2), &delivered, &mut frame).unwrap();
        // This N-1 limit could decode the full offer, including the missing record.
        let decoded = EventLog::<GCounterDelta>::records_from_wire_bytes_for_with_limits(
            &frame,
            receiver.state(),
            DecodeLimits {
                max_records: Some((N - 1) as usize),
                ..DecodeLimits::default()
            },
        )
        .unwrap();
        assert_eq!(decoded.len(), (N - 2) as usize);
        let before_len = fs::metadata(&journal).unwrap().len();
        for record in decoded {
            assert_eq!(
                receiver.receive(ticket, record).unwrap(),
                Admission::Duplicate
            );
        }
        assert_eq!(
            fs::metadata(&journal).unwrap().len(),
            before_len,
            "exchange {exchange}: journal bytes"
        );
        assert_eq!(
            receiver.log().records().len(),
            initial_count,
            "exchange {exchange}: record count"
        );
        assert_eq!(
            receiver.state(),
            &initial_state,
            "exchange {exchange}: state"
        );
        assert_eq!(
            receiver.log().version().get(1),
            1,
            "exchange {exchange}: prefix"
        );
    }
    assert_eq!(
        fs::read(&journal).unwrap(),
        initial_bytes,
        "journal content after duplicates"
    );

    let before_repair = fs::metadata(&journal).unwrap().len();
    assert_eq!(
        receiver.receive(ticket, records[1].clone()).unwrap(),
        Admission::Accepted
    );
    assert!(fs::metadata(&journal).unwrap().len() > before_repair);
    assert_eq!(receiver.log().version().get(1), N);
    assert_eq!(receiver.log().records().len(), N as usize);
    assert!(sender.since(receiver.log().version()).is_empty());
    println!(
        "N={N} exchanges={EXCHANGES} journal_bytes={}",
        fs::metadata(&journal).unwrap().len()
    );
}
