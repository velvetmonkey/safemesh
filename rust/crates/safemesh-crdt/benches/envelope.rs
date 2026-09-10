// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0
// Opt-in measurement instrument; no product ceilings are defined here.
use safemesh_crdt::{local::DurableReplica, ownership::WriterConfig, *};
use serde_json::{json, Value};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    fs,
    hint::black_box,
    path::Path,
    sync::atomic::{AtomicUsize, Ordering::Relaxed},
    time::Instant,
};
struct Meter;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static ALLOC: AtomicUsize = AtomicUsize::new(0);
static CALLS: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Meter {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = System.alloc(l);
        if !p.is_null() {
            let v = LIVE.fetch_add(l.size(), Relaxed) + l.size();
            PEAK.fetch_max(v, Relaxed);
            ALLOC.fetch_add(l.size(), Relaxed);
            CALLS.fetch_add(1, Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size(), Relaxed);
        System.dealloc(p, l);
    }
}
#[global_allocator]
static METER: Meter = Meter;
fn proc_value(file: &str, key: &str) -> u64 {
    fs::read_to_string(file)
        .unwrap()
        .lines()
        .find_map(|l| {
            l.strip_prefix(key)
                .and_then(|v| v.split_whitespace().next())
                .and_then(|v| v.parse().ok())
        })
        .unwrap_or(0)
}
fn measure<T>(op: &str, f: impl FnOnce() -> T) -> (T, Value) {
    let writes = proc_value("/proc/self/io", "write_bytes:");
    let base = LIVE.load(Relaxed);
    PEAK.store(base, Relaxed);
    let alloc = ALLOC.load(Relaxed);
    let calls = CALLS.load(Relaxed);
    let start = Instant::now();
    let value = f();
    let ns = start.elapsed().as_nanos() as u64;
    let peak = PEAK.load(Relaxed);
    let bytes = ALLOC.load(Relaxed) - alloc;
    let count = CALLS.load(Relaxed) - calls;
    let physical = proc_value("/proc/self/io", "write_bytes:") - writes;
    let rss = proc_value("/proc/self/status", "VmHWM:") * 1024;
    (
        value,
        json!({"op":op,"ns":ns,"allocator_peak_bytes":peak,"allocator_extra_peak_bytes":peak.saturating_sub(base),"allocation_bytes":bytes,"allocation_calls":count,"process_peak_rss_bytes":rss,"kernel_write_bytes":physical}),
    )
}
fn emit(mut row: Value, extra: Value) {
    row.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    println!("{row}");
}
fn payload(seed: u64, i: usize, size: usize) -> String {
    // Deterministic xorshift64; retain a UTF-8 prefix and unique fixed-width ID.
    let mut x = seed ^ (i as u64).wrapping_mul(0x9e3779b97f4a7c15);
    let mut s = format!("é{i:016x}");
    while s.len() < size {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.push((b'a' + (x % 26) as u8) as char);
    }
    s
}
type Set = OrSet<String, u64>;
type Delta = OrSetDelta<String, u64>;
fn rec(i: usize, size: usize, seed: u64, writers: usize) -> Record<Delta> {
    let writer = i % writers;
    let seq = i / writers + 1;
    Record {
        id: RecordId {
            replica: writer as u64,
            sequence: seq as u64,
        },
        delta: Delta::Add {
            element: payload(seed, i, size),
            token: (seq * writers + writer) as u64,
        },
    }
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.get(1).map(String::as_str) == Some("self-test") {
        assert_eq!(payload(42, 1, 64), payload(42, 1, 64));
        assert_ne!(payload(42, 1, 64), payload(43, 1, 64));
        let (v, m) = measure("allocation-control", || vec![7u8; 1048576]);
        black_box(&v);
        assert!(m["allocation_bytes"].as_u64().unwrap() >= 1048576);
        println!("self-test held");
        return;
    }
    assert_eq!(
        a.len(),
        10,
        "family n payload writers batch byte_cap repetitions seed root"
    );
    let family = &a[1];
    let n: usize = a[2].parse().unwrap();
    let size: usize = a[3].parse().unwrap();
    let writers: usize = a[4].parse().unwrap();
    let batch: usize = a[5].parse().unwrap();
    let cap: usize = a[6].parse().unwrap();
    let reps: usize = a[7].parse().unwrap();
    let seed: u64 = a[8].parse().unwrap();
    let root = Path::new(&a[9]);
    fs::create_dir_all(root).unwrap();
    println!(
        "{}",
        json!({"family":family,"n":n,"payload":size,"writers":writers,"batch":batch,"byte_cap":cap,"repetitions":reps,"seed":seed,"crate_version":env!("CARGO_PKG_VERSION")})
    );
    let cfg = WriterConfig {
        writers: writers as u64,
        writer: 0,
    };
    if ["B1", "B2", "B3"].contains(&family.as_str()) {
        let mut r = DurableReplica::utf8_set(root, cfg).unwrap();
        for i in 0..n {
            assert_eq!(
                r.receive(r.ticket(), rec(i, size, seed, writers)).unwrap(),
                Admission::Accepted
            );
        }
        if family == "B1" {
            for k in 0..reps {
                let d = payload(seed, n + k, size);
                let accepted = d.len();
                let history = r.log().to_wire_bytes().unwrap().len();
                let (_, m) = measure("append", || r.add(r.ticket(), d).unwrap());
                let tx = fs::metadata(root.join("writer-0.transaction"))
                    .unwrap()
                    .len();
                emit(
                    m,
                    json!({"rep":k,"condition":"warm","history_records":n+k,"history_encoded_bytes":history,"payload_bytes":accepted,"transaction_bytes":tx,"serialized_write_amplification":tx as f64/accepted as f64,"assertion":"accepted"}),
                );
            }
        } else if family == "B2" {
            for ratio in [100usize, 50, 0] {
                for k in 0..reps {
                    let state = r.state().clone();
                    let log = r.log().to_wire_bytes().unwrap();
                    let version = r.log().version().clone();
                    let allocation = r.allocation_bytes();
                    let inputs: Vec<_> = (0..batch)
                        .map(|j| {
                            if j * 100 / batch < ratio {
                                rec(j % n, size, seed, writers)
                            } else {
                                rec(n + 1 + (k * batch + j) * writers, size, seed, writers)
                            }
                        })
                        .collect();
                    let (out, m) = measure(
                        if ratio == 100 {
                            "duplicate_batch"
                        } else {
                            "mixed_batch"
                        },
                        || {
                            inputs
                                .into_iter()
                                .map(|record| r.receive(r.ticket(), record).unwrap())
                                .collect::<Vec<_>>()
                        },
                    );
                    if ratio == 100 {
                        assert_eq!(r.state(), &state);
                        assert_eq!(r.log().to_wire_bytes().unwrap(), log);
                        assert_eq!(r.log().version(), &version);
                        assert_eq!(r.allocation_bytes(), allocation);
                    }
                    emit(
                        m,
                        json!({"rep":k,"condition":"warm","duplicate_percent_requested":ratio,"accepted":out.iter().filter(|x|**x==Admission::Accepted).count(),"duplicates":out.iter().filter(|x|**x==Admission::Duplicate).count(),"batch_records":batch,"history_encoded_bytes":log.len(),"assertion":"pure duplicates preserve state log version allocation"}),
                    );
                }
            }
        } else {
            let state = r.state().clone();
            let log = r.log().to_wire_bytes().unwrap();
            let allocation = r.allocation_bytes();
            drop(r);
            for condition in ["warm", "file-cache-eviction-requested"] {
                for k in 0..reps {
                    if condition != "warm" {
                        evict(&root.join("writer-0.transaction"));
                    }
                    let (next, m) = measure("restart", || {
                        DurableReplica::restart_utf8_set(root, cfg).unwrap()
                    });
                    assert_eq!(next.state(), &state);
                    assert_eq!(next.log().to_wire_bytes().unwrap(), log);
                    assert_eq!(next.allocation_bytes(), allocation);
                    emit(
                        m,
                        json!({"rep":k,"condition":condition,"history_records":n,"history_encoded_bytes":log.len(),"recovered_sequence":u64::from_le_bytes(allocation[16..24].try_into().unwrap()),"assertion":"exact state log recovered sequence"}),
                    );
                    drop(next);
                }
            }
        }
    } else if family == "B4" {
        let mut src = EventLog::for_crdt(&Set::new());
        // Odds arrive before evens, putting missing low IDs across batch boundaries.
        for parity in [1, 0] {
            for i in (0..n).filter(|i| i % 2 == parity) {
                src.insert_record(rec(i, size, seed, writers));
            }
        }
        for prefix in [0, n / 4] {
            for k in 0..reps {
                let mut dst = EventLog::for_crdt(&Set::new());
                for i in 0..prefix {
                    dst.insert_record(rec(i, size, seed, writers));
                }
                let mut transferred = 0;
                let mut rounds = 0;
                while dst.records().len() < n {
                    let (missing, m) = measure("export", || src.since(dst.version()));
                    emit(
                        m,
                        json!({"rep":k,"prefix":prefix,"round":rounds,"returned_records":missing.len(),"condition":"warm"}),
                    );
                    // Caller batching over the existing unbounded export API. Skip
                    // already held IDs without advertising a prefix over a gap.
                    let mut chosen = Vec::new();
                    let mut bytes = 0;
                    for record in missing {
                        if dst.records().iter().any(|r| r.id == record.id) {
                            continue;
                        }
                        let len = record.to_wire_bytes().unwrap().len();
                        if chosen.len() == batch || bytes + len > cap {
                            break;
                        }
                        bytes += len;
                        chosen.push(record);
                    }
                    assert!(!chosen.is_empty(), "byte cap must fit one record");
                    let count = chosen.len();
                    let (_, m) = measure("import", || {
                        for record in chosen {
                            assert_eq!(dst.insert_record(record), Admission::Accepted);
                        }
                    });
                    transferred += bytes;
                    rounds += 1;
                    emit(
                        m,
                        json!({"rep":k,"prefix":prefix,"round":rounds,"wire_record_bytes":bytes,"batch_records":count,"progress_records":dst.records().len(),"condition":"warm"}),
                    );
                    assert!(rounds <= n);
                }
                assert_eq!(dst.version(), src.version());
                for record in src.records() {
                    assert!(dst.records().contains(record));
                }
                println!(
                    "{}",
                    json!({"assertion":"no skipped gap at caller batch boundary","rep":k,"prefix":prefix,"wire_record_bytes_total":transferred,"rounds":rounds})
                );
            }
        }
    } else if family == "B5" {
        for k in 0..reps {
            let mut state = Set::new();
            let mut log = EventLog::for_crdt(&state);
            for i in 0..n {
                let record = rec(i, size, seed, 1);
                state.apply_delta(record.delta.clone());
                log.insert_record(record);
            }
            for remove_count in [n / 2, n] {
                let start = if remove_count == n { n / 2 } else { 0 };
                let (_, m) = measure("remove", || {
                    for i in start..remove_count {
                        let tokens = state
                            .observed_tokens(&payload(seed, i, size))
                            .into_iter()
                            .collect();
                        let d = Delta::Remove { tokens };
                        state.apply_delta(d.clone());
                        log.append(0, d);
                    }
                });
                let live = state.elements().len();
                let tomb = state.tombstones().len();
                let bytes = log.to_wire_bytes().unwrap().len();
                let (copy, sm) = measure("state_clone_memory", || state.clone());
                black_box(&copy);
                drop(copy);
                let (copy, tm) = measure("tombstone_clone_memory", || state.tombstones().clone());
                black_box(&copy);
                drop(copy);
                let (copy, lm) = measure("log_clone_memory", || log.clone());
                black_box(&copy);
                drop(copy);
                let extra = json!({"rep":k,"condition":"warm","live_entries":live,"retained_adds":state.adds().len(),"tombstones":tomb,"history_records":log.records().len(),"history_encoded_bytes":bytes,"operations":remove_count-start,"state_clone_allocation_bytes":sm["allocation_bytes"],"tombstone_clone_allocation_bytes":tm["allocation_bytes"],"log_clone_allocation_bytes":lm["allocation_bytes"],"encoded_bytes_per_operation":bytes as f64/log.records().len() as f64});
                emit(m, extra.clone());
                let (_, m) = measure("query", || {
                    for i in 0..n {
                        assert_eq!(
                            black_box(state.contains(&payload(seed, i, size))),
                            i >= remove_count
                        );
                    }
                });
                emit(m, extra);
                assert_eq!(tomb, remove_count);
                assert_eq!(live, n - remove_count);
            }
        }
    } else if family == "B6" {
        let mut log = EventLog::for_crdt(&Set::new());
        for i in 0..n {
            log.insert_record(rec(i, size, seed, 1));
        }
        let original = log.to_wire_bytes().unwrap();
        let version = log.version().clone();
        for kind in ["large-valid-frame", "truncated", "malformed", "trailing"] {
            let mut input = original.clone();
            match kind {
                "truncated" => {
                    input.truncate(input.len() / 2);
                }
                "malformed" => {
                    input[0] ^= 255;
                }
                "trailing" => input.push(0),
                _ => {}
            }
            for k in 0..reps {
                let (result, m) = measure("decode", || {
                    EventLog::<Delta>::from_wire_bytes_for(&input, &Set::new())
                });
                if kind == "large-valid-frame" {
                    assert!(result.is_ok());
                } else {
                    assert!(result.is_err());
                }
                assert_eq!(log.to_wire_bytes().unwrap(), original);
                assert_eq!(log.version(), &version);
                emit(
                    m,
                    json!({"rep":k,"condition":"warm","kind":kind,"input_bytes":input.len(),"error":result.err().map(|e|format!("{e:?}")),"assertion":"accepted history unchanged; large valid input has no size refusal"}),
                );
            }
        }
    } else if family == "B7" {
        let mut r = DurableReplica::counter(root, cfg).unwrap();
        for i in 0..n {
            r.receive(
                r.ticket(),
                Record {
                    id: RecordId {
                        replica: 0,
                        sequence: (i + 1) as u64,
                    },
                    delta: GCounterDelta {
                        replica: 0,
                        tally: (i + 1) as u64,
                    },
                },
            )
            .unwrap();
        }
        for k in 0..reps {
            let tally = (n + k + 1) as u64;
            let history = r.log().to_wire_bytes().unwrap().len();
            let (_, m) = measure("durable_counter_bump", || {
                r.bump(r.ticket(), tally).unwrap()
            });
            assert!(
                m["ns"].as_u64().unwrap() < 5_000_000,
                "durable G-Counter bump latency exceeded 5 ms: {} ns",
                m["ns"]
            );
            let tx = fs::metadata(root.join("writer-0.transaction"))
                .unwrap()
                .len();
            drop(r);
            let restarted = DurableReplica::restart_counter(root, cfg).unwrap();
            assert_eq!(restarted.state().state()[0], tally);
            r = restarted;
            emit(
                m,
                json!({"rep":k,"condition":"warm","history_records":n+k,"history_encoded_bytes":history,"payload_bytes":8,"transaction_bytes":tx,"serialized_write_amplification":tx as f64/8.0,"assertion":"durable G-Counter bump survives restart"}),
            );
        }
    } else {
        panic!("unknown family");
    }
}
fn evict(path: &Path) {
    use std::os::fd::AsRawFd;
    unsafe extern "C" {
        fn posix_fadvise(fd: i32, offset: i64, len: i64, advice: i32) -> i32;
    }
    let file = fs::File::open(path).unwrap();
    file.sync_all().unwrap();
    assert_eq!(unsafe { posix_fadvise(file.as_raw_fd(), 0, 0, 4) }, 0);
}
