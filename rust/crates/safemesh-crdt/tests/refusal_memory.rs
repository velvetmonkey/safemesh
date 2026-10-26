// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

use safemesh_crdt::{
    DecodeError, DecodeLimits, EventLog, GCounterDelta, OrSetDelta, Record, RecordId,
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static MEASURE: Mutex<()> = Mutex::new(());
const PEAK_BOUND: usize = 1_048_576;

#[global_allocator]
static ALLOCATOR: Counting = Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let live = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(live, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let next = unsafe { System.realloc(ptr, layout, size) };
        if !next.is_null() {
            if size >= layout.size() {
                let delta = size - layout.size();
                let live = LIVE.fetch_add(delta, Ordering::Relaxed) + delta;
                PEAK.fetch_max(live, Ordering::Relaxed);
            } else {
                LIVE.fetch_sub(layout.size() - size, Ordering::Relaxed);
            }
        }
        next
    }
}

fn refusal_peak<D>(records: Vec<Record<D>>) -> usize
where
    D: safemesh_crdt::WireEncode
        + safemesh_crdt::WireDecode
        + safemesh_crdt::WireSchema
        + PartialEq,
{
    let mut bytes = Vec::new();
    EventLog::encode_records(Some(1), &records, &mut bytes).unwrap();
    drop(records);
    let base = LIVE.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let result = EventLog::<D>::from_wire_bytes_with_limits(&bytes, DecodeLimits::default());
    let peak = PEAK.load(Ordering::Relaxed) - base;
    eprintln!("refusal frame={} peak={peak}", bytes.len());
    assert!(matches!(
        result,
        Err(DecodeError::RecordLimitExceeded {
            max_records: 100_000
        })
    ));
    peak
}

#[test]
fn gcounter_refusal_keeps_a_fixed_peak() {
    let _measure = MEASURE.lock().unwrap_or_else(|error| error.into_inner());
    let count = DecodeLimits::DEFAULT_MAX_RECORDS;
    let make = |count: usize| {
        (1..=count)
            .map(|sequence| Record {
                id: RecordId {
                    replica: 0,
                    sequence: sequence as u64,
                },
                delta: GCounterDelta {
                    replica: 0,
                    tally: sequence as u64,
                },
            })
            .collect()
    };
    let near = refusal_peak(make(count + 1));
    let ten_times = refusal_peak(make((count + 1) * 10));
    assert!(near < PEAK_BOUND, "near peak={near}");
    assert!(ten_times < PEAK_BOUND, "ten-times peak={ten_times}");
}

#[test]
fn utf8_orset_refusal_keeps_a_fixed_peak() {
    let _measure = MEASURE.lock().unwrap_or_else(|error| error.into_inner());
    let count = DecodeLimits::DEFAULT_MAX_RECORDS + 1;
    let make = |suffix: &str| {
        (1..=count)
            .map(|sequence| Record {
                id: RecordId {
                    replica: 0,
                    sequence: sequence as u64,
                },
                delta: OrSetDelta::Add {
                    element: format!("member-{sequence:07}{suffix}"),
                    token: sequence as u64,
                },
            })
            .collect()
    };
    let near = refusal_peak(make(""));
    let larger = refusal_peak(make(&"x".repeat(128)));
    assert!(near < PEAK_BOUND, "near peak={near}");
    assert!(larger < PEAK_BOUND, "larger peak={larger}");
}

#[cfg(feature = "local-writer")]
#[test]
fn journal_restart_refusal_keeps_a_fixed_peak() {
    use safemesh_crdt::{local::DurableReplica, local::LocalError, ownership::WriterConfig};

    let _measure = MEASURE.lock().unwrap_or_else(|error| error.into_inner());
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("safemesh-refusal-{nonce}"));
    std::fs::create_dir(&root).unwrap();
    let config = WriterConfig {
        writers: 1,
        writer: 0,
    };
    let mut replica = DurableReplica::counter(&root, config).unwrap();
    let deltas = (1..=DecodeLimits::DEFAULT_MAX_RECORDS + 1).map(|sequence| GCounterDelta {
        replica: 0,
        tally: sequence as u64,
    });
    replica.append_batch(replica.ticket(), deltas).unwrap();
    drop(replica);
    let base = LIVE.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let result = DurableReplica::restart_counter(&root, config);
    let peak = PEAK.load(Ordering::Relaxed) - base;
    eprintln!("journal refusal peak={peak}");
    assert!(matches!(
        result,
        Err(LocalError::RecordLimitExceeded {
            max_records: 100_000,
            ..
        })
    ));
    assert!(peak < PEAK_BOUND, "journal peak={peak}");
}
