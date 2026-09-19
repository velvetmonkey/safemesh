// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0
use safemesh_crdt::{local::DurableReplica, ownership::WriterConfig, OrSet, OrSetDelta};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, BufRead, Write},
    path::Path,
};

type Replica = DurableReplica<OrSet<String, u64>>;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Inspection {
    schema_version: u32,
    checklist_version: u32,
    item_id: String,
    event_id: String,
    inspector: String,
    answer: String,
    note: String,
    observed_event_ids: Vec<String>,
}
fn canonical(text: &str) -> Result<(String, String), String> {
    let record: Inspection = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if record.schema_version != 1
        || record.checklist_version != 1
        || record.item_id != "north-yard/gate-3/latch"
        || record.event_id.is_empty()
        || record.inspector.trim().is_empty()
        || !matches!(record.answer.as_str(), "Pass" | "Fail")
        || !record.observed_event_ids.is_empty()
    {
        return Err("Unsupported inspection schema or identity".into());
    }
    let bytes = serde_json::to_string(&record).map_err(|e| e.to_string())?;
    Ok((record.event_id, bytes))
}
fn records(replica: &Replica) -> Result<BTreeMap<String, Value>, String> {
    let mut result = BTreeMap::new();
    for r in replica.log().records() {
        let OrSetDelta::Add { element, .. } = &r.delta else {
            return Err("Unexpected removal".into());
        };
        let (id, bytes) = canonical(element)?;
        if bytes != *element || r.id.replica != 0 {
            return Err("Unexpected record encoding or writer".into());
        }
        let value = json!({"record": element, "writer": r.id.replica, "sequence": r.id.sequence});
        if result.insert(id, value).is_some() {
            return Err("Application event identity collision".into());
        }
    }
    Ok(result)
}
fn emit(value: Value) {
    println!("{value}");
    io::stdout().flush().expect("client pipe");
}
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();
    let root = Path::new(args.get(1).ok_or("store path required")?);
    let config = WriterConfig {
        writers: 2,
        writer: 0,
    };
    // Existence means restart, even if the directory is empty or damaged.
    // Only create_dir success permits a fresh identity; never repair by resetting.
    let mut replica = match fs::create_dir(root) {
        Ok(()) => {
            fs::File::open(root.parent().unwrap_or(Path::new(".")))
                .and_then(|f| f.sync_all())
                .map_err(|e| e.to_string())?;
            Replica::utf8_set(root, config)
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            Replica::restart_utf8_set(root, config)
        }
        Err(e) => return Err(e.to_string()),
    }
    .map_err(|e| format!("{e:?}"))?;
    let mut known = records(&replica)?;
    emit(
        json!({"ready":true, "pid":std::process::id(), "records":known.values().collect::<Vec<_>>(),
        "checklist":serde_json::from_str::<Value>(include_str!("../checklist.json")).unwrap()}),
    );
    for line in io::stdin().lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        let (id, bytes) = match canonical(&line) {
            Ok(v) => v,
            Err(e) => {
                emit(json!({"error":e}));
                continue;
            }
        };
        if let Some(previous) = known.get(&id) {
            if previous["record"] == bytes {
                emit(json!({"saved":previous}));
            } else {
                emit(json!({"error":"Application event identity collision"}));
            }
            continue;
        }
        match replica.add(replica.ticket(), bytes.clone()) {
            Ok(record) => {
                let value = json!({"record":bytes,"writer":record.id.replica,"sequence":record.id.sequence});
                known.insert(id, value.clone());
                // Explicit external journey control: commit is complete; no reply yet.
                // Remove the barrier to continue, or SIGKILL this PID to lose the reply.
                if let Some(barrier) = args.get(2) {
                    fs::write(barrier, b"committed; reply pending\n").map_err(|e| e.to_string())?;
                    while Path::new(barrier).exists() {
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                }
                emit(json!({"saved":value}));
            }
            Err(e) => emit(json!({"storage_error":format!("{e:?}")})),
        }
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        emit(json!({"recovery_error":e}));
        std::process::exit(1);
    }
}
