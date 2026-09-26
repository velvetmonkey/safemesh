// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0
mod exchange;
use safemesh_crdt::{
    local::{DurableReplica, LocalError},
    ownership::WriterConfig,
    OrSet, OrSetDelta,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, BufRead, Write},
    path::Path,
    sync::{Arc, Mutex},
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
        || record
            .observed_event_ids
            .windows(2)
            .any(|ids| ids[0] >= ids[1])
        || record
            .observed_event_ids
            .iter()
            .any(|id| id == &record.event_id)
    {
        return Err("Unsupported inspection schema or identity".into());
    }
    let bytes = serde_json::to_string(&record).map_err(|e| e.to_string())?;
    Ok((record.event_id, bytes))
}
fn validate_submission(bytes: &str, known: &BTreeMap<String, Value>) -> Result<(), String> {
    let record: Inspection = serde_json::from_str(bytes).map_err(|e| e.to_string())?;
    if record.observed_event_ids.is_empty() {
        return Ok(());
    }
    let mut differs = false;
    for id in &record.observed_event_ids {
        let prior = known.get(id).ok_or("Review cites an unknown event")?;
        let prior: Inspection =
            serde_json::from_str(prior["record"].as_str().ok_or("Invalid record")?)
                .map_err(|e| e.to_string())?;
        if prior.item_id != record.item_id {
            return Err("Review cites another item".into());
        }
        differs |= prior.answer != record.answer;
    }
    if !differs {
        return Err("Review must explain a differing answer".into());
    }
    Ok(())
}
fn review_projection(
    known: &BTreeMap<String, Value>,
) -> Result<(Vec<String>, Vec<String>), String> {
    let mut by_item: BTreeMap<String, Vec<Inspection>> = BTreeMap::new();
    for value in known.values() {
        let record: Inspection =
            serde_json::from_str(value["record"].as_str().ok_or("Invalid record")?)
                .map_err(|e| e.to_string())?;
        by_item
            .entry(record.item_id.clone())
            .or_default()
            .push(record);
    }
    let (mut needs_review, mut resolved) = (Vec::new(), Vec::new());
    for (item, inspections) in by_item {
        let answers: BTreeSet<_> = inspections.iter().map(|r| r.answer.as_str()).collect();
        if answers.len() < 2 {
            continue;
        }
        // A reviewer must name every other observation in the current set.
        // A later unreferenced observation, including another review, reopens it.
        let complete = inspections.iter().any(|review| {
            let cited: BTreeSet<_> = review.observed_event_ids.iter().collect();
            !cited.is_empty()
                && inspections.iter().all(|other| {
                    other.event_id == review.event_id || cited.contains(&other.event_id)
                })
                && inspections
                    .iter()
                    .any(|other| cited.contains(&other.event_id) && other.answer != review.answer)
        });
        if complete {
            resolved.push(item);
        } else {
            needs_review.push(item);
        }
    }
    Ok((needs_review, resolved))
}
fn records(replica: &Replica) -> Result<BTreeMap<String, Value>, String> {
    let mut result = BTreeMap::new();
    for r in replica.log().records() {
        let OrSetDelta::Add { element, .. } = &r.delta else {
            return Err("Unexpected removal".into());
        };
        let (id, bytes) = canonical(element)?;
        if bytes != *element || r.id.replica > 1 {
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
enum RunError {
    Recovery { kind: &'static str, reason: String },
    Other(String),
}
impl From<String> for RunError {
    fn from(reason: String) -> Self {
        Self::Other(reason)
    }
}
fn run() -> Result<(), RunError> {
    let args: Vec<_> = std::env::args().collect();
    let root = Path::new(args.get(1).ok_or("store path required".to_string())?);
    let option = |name: &str| {
        args.iter()
            .position(|s| s == name)
            .and_then(|i| args.get(i + 1))
    };
    let writer = option("--writer")
        .map_or(Ok(0), |s| s.parse::<u64>())
        .map_err(|e| e.to_string())?;
    if writer > 1 || (option("--listen").is_some() && option("--connect").is_some()) {
        return Err(RunError::Other(
            "expected writer 0 or 1 and at most one network role".into(),
        ));
    }
    let config = WriterConfig { writers: 2, writer };
    // Existence means restart, even if the directory is empty or damaged.
    // Only create_dir success permits a fresh identity; never repair by resetting.
    let replica = match fs::create_dir(root) {
        Ok(()) => {
            fs::File::open(root.parent().unwrap_or(Path::new(".")))
                .and_then(|f| f.sync_all())
                .map_err(|e| RunError::Recovery {
                    kind: "storage",
                    reason: e.to_string(),
                })?;
            Replica::utf8_set(root, config)
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            Replica::restart_utf8_set(root, config)
        }
        Err(e) => {
            return Err(RunError::Recovery {
                kind: if e.kind() == io::ErrorKind::NotFound {
                    "missing_parent"
                } else {
                    "storage"
                },
                reason: e.to_string(),
            })
        }
    }
    .map_err(|e| {
        let kind = match e {
            LocalError::Refused => "owned",
            LocalError::Io(_) => "storage",
            LocalError::Configuration => "configuration",
            LocalError::Exhausted => "storage",
            LocalError::RecoveryRequired
            | LocalError::InvalidRecord(_)
            | LocalError::History(_)
            | LocalError::InvalidHistory => {
                "replay"
            }
        };
        RunError::Recovery {
            kind,
            reason: e.to_string(),
        }
    })?;
    let service = Arc::new(Mutex::new(
        exchange::Service::open(replica, writer, root).map_err(|reason| RunError::Recovery {
            kind: "startup",
            reason,
        })?,
    ));
    exchange::start(
        service.clone(),
        option("--listen").map(String::as_str),
        option("--connect").map(String::as_str),
    )?;
    let known = records(&service.lock().map_err(|e| e.to_string())?.replica).map_err(|reason| {
        RunError::Recovery {
            kind: "replay",
            reason,
        }
    })?;
    let (needs_review, resolved) =
        review_projection(&known).map_err(|reason| RunError::Recovery {
            kind: "replay",
            reason,
        })?;
    emit(
        json!({"ready":true, "pid":std::process::id(), "records":known.values().collect::<Vec<_>>(),
        "needs_review":needs_review,"resolved":resolved,
        "checklist":serde_json::from_str::<Value>(include_str!("../checklist.json")).unwrap()}),
    );
    for line in io::stdin().lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        let mut state = service.lock().map_err(|e| e.to_string())?;
        if line == "{\"command\":\"status\"}" {
            emit(state.status()?);
            continue;
        }
        if !state.writable() {
            emit(json!({"storage_error":"restart required"}));
            continue;
        }
        let known = records(&state.replica)?;
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
        if let Err(e) = validate_submission(&bytes, &known) {
            emit(json!({"error":e}));
            continue;
        }
        let ticket = state.replica.ticket();
        match state.replica.add(ticket, bytes.clone()) {
            Ok(record) => {
                let value = json!({"record":bytes,"writer":record.id.replica,"sequence":record.id.sequence});
                // Explicit external journey control: commit is complete; no reply yet.
                // Remove the barrier to continue, or SIGKILL this PID to lose the reply.
                if let Some(barrier) = option("--reply-barrier")
                    .or_else(|| args.get(2).filter(|s| !s.starts_with("--")))
                {
                    fs::write(barrier, b"committed; reply pending\n").map_err(|e| e.to_string())?;
                    while Path::new(barrier).exists() {
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                }
                emit(json!({"saved":value}));
            }
            Err(e) => emit(json!({"storage_error":e.to_string()})),
        }
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        match e {
            RunError::Recovery { kind, reason } => {
                emit(json!({"recovery_error":reason, "recovery_kind":kind}))
            }
            RunError::Other(reason) => emit(json!({"error":reason})),
        }
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, answer: &str, refs: &[&str]) -> String {
        serde_json::to_string(&Inspection {
            schema_version: 1,
            checklist_version: 1,
            item_id: "north-yard/gate-3/latch".into(),
            event_id: id.into(),
            inspector: "A".into(),
            answer: answer.into(),
            note: id.into(),
            observed_event_ids: refs.iter().map(|s| (*s).into()).collect(),
        })
        .unwrap()
    }

    #[test]
    fn multiple_inspectors_and_conflicting_reviews_remain_visible() {
        let mut known = BTreeMap::new();
        for (id, answer) in [("a", "Pass"), ("b", "Fail"), ("c", "Fail")] {
            known.insert(id.into(), json!({"record":record(id, answer, &[])}));
        }
        assert_eq!(review_projection(&known).unwrap().0.len(), 1);
        let first = record("first-review", "Pass", &["a", "b", "c"]);
        validate_submission(&first, &known).unwrap();
        known.insert("first-review".into(), json!({"record":first}));
        assert_eq!(review_projection(&known).unwrap().1.len(), 1);
        let second = record("second-review", "Fail", &["a", "b", "c"]);
        validate_submission(&second, &known).unwrap();
        known.insert("second-review".into(), json!({"record":second}));
        assert_eq!(review_projection(&known).unwrap().0.len(), 1);
        assert_eq!(known.len(), 5);
    }
}
