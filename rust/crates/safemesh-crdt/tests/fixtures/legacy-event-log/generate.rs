// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0
//! Pinned historical generator for the legacy EventLog fixtures.
//!
//! Not compiled in this checkout. The regeneration check copies it into an
//! archived library tree as `tests/legacy_generate.rs` and runs it there, so it
//! uses only the API shared by 3fb38cb (tag 0x02) and 4a12ae6 (unshaped 0x03).
//! It runs the archived public encoder and never decodes its own output.
use safemesh_crdt::{
    Admission, EnableWinsFlagDelta, EventLog, GCounterDelta, LwwMapDelta, LwwRegisterDelta,
    OrSetDelta, PnCounterDelta, Record, RecordId, WireEncode,
};
use std::{fs, path::Path};

fn rec<D>(replica: u64, sequence: u64, delta: D) -> Record<D> {
    Record {
        id: RecordId { replica, sequence },
        delta,
    }
}

fn write<D: PartialEq>(dir: &Path, name: &str, records: Vec<Record<D>>)
where
    EventLog<D>: WireEncode,
{
    let mut log = EventLog::new();
    for record in records {
        assert!(log.insert_record(record) == Admission::Accepted);
    }
    fs::write(dir.join(name), log.to_wire_bytes().unwrap()).unwrap();
}

#[test]
#[ignore]
fn generate_legacy_event_logs() {
    let dir = std::env::var_os("LEGACY_EVENT_LOG_GENERATE")
        .expect("set LEGACY_EVENT_LOG_GENERATE to a new output directory");
    let dir = Path::new(&dir);
    fs::create_dir(dir).expect("output directory must not already exist");

    write::<GCounterDelta>(dir, "gcounter-empty.log", vec![]);
    write(
        dir,
        "gcounter.log",
        vec![
            rec(0, 1, GCounterDelta { replica: 0, tally: 10 }),
            rec(1, 1, GCounterDelta { replica: 1, tally: 20 }),
            rec(0, 2, GCounterDelta { replica: 0, tally: 15 }),
        ],
    );
    write(
        dir,
        "pncounter.log",
        vec![
            rec(0, 1, PnCounterDelta::Inc { replica: 0, tally: 5 }),
            rec(1, 1, PnCounterDelta::Dec { replica: 1, tally: 3 }),
            rec(1, 2, PnCounterDelta::Inc { replica: 1, tally: 9 }),
        ],
    );
    write(
        dir,
        "orset-u64.log",
        vec![
            rec(0, 1, OrSetDelta::<u64, u64>::Add { element: 7, token: 100 }),
            rec(1, 1, OrSetDelta::Add { element: 8, token: 101 }),
            rec(0, 2, OrSetDelta::Remove { tokens: vec![100] }),
        ],
    );
    write(
        dir,
        "orset-utf8.log",
        vec![
            rec(0, 1, OrSetDelta::<String, u64>::Add { element: "milk".into(), token: 100 }),
            rec(1, 1, OrSetDelta::Add { element: "eggs".into(), token: 101 }),
            rec(1, 2, OrSetDelta::Remove { tokens: vec![100] }),
        ],
    );
    write(
        dir,
        "lww-register-u64.log",
        vec![
            rec(0, 1, LwwRegisterDelta { timestamp: 1, replica: 0, value: 42u64 }),
            rec(1, 1, LwwRegisterDelta { timestamp: 2, replica: 1, value: 43u64 }),
        ],
    );
    write(
        dir,
        "enable-wins-flag-u64.log",
        vec![
            rec(0, 1, EnableWinsFlagDelta::<u64>::Enable { token: 100 }),
            rec(1, 1, EnableWinsFlagDelta::Disable { tokens: vec![100] }),
        ],
    );
    write(
        dir,
        "lww-map-u64.log",
        vec![
            rec(0, 1, LwwMapDelta::<u64, u64>::Set { key: 1, timestamp: 1, replica: 0, value: 10 }),
            rec(1, 1, LwwMapDelta::Remove { key: 1, timestamp: 2, replica: 1 }),
        ],
    );
}
