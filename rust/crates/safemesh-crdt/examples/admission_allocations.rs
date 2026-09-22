// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0
//! Run with --release --features local-writer; pass an existing scratch directory.
#[cfg(all(feature = "local-writer", target_os = "linux"))]
mod benchmark {
    use safemesh_crdt::{local::DurableReplica, ownership::WriterConfig};
    use std::{
        alloc::{GlobalAlloc, Layout, System},
        sync::atomic::{AtomicUsize, Ordering},
        time::Instant,
    };
    struct Counting;
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    static BYTES: AtomicUsize = AtomicUsize::new(0);
    #[global_allocator]
    static ALLOCATOR: Counting = Counting;
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            CALLS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(layout.size(), Ordering::Relaxed);
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            CALLS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(size, Ordering::Relaxed);
            unsafe { System.realloc(ptr, layout, size) }
        }
    }
    pub fn run() {
        let parent =
            std::path::PathBuf::from(std::env::args_os().nth(1).expect("scratch directory"));
        for n in [128, 512, 2048] {
            let root = parent.join(format!("admission-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&root).unwrap();
            let mut replica = DurableReplica::counter(
                &root,
                WriterConfig {
                    writers: 1,
                    writer: 0,
                },
            )
            .unwrap();
            CALLS.store(0, Ordering::Relaxed);
            BYTES.store(0, Ordering::Relaxed);
            let start = Instant::now();
            for tally in 1..=n {
                replica.bump(replica.ticket(), tally).unwrap();
            }
            let elapsed = start.elapsed();
            let calls = CALLS.load(Ordering::Relaxed);
            let bytes = BYTES.load(Ordering::Relaxed);
            assert_eq!(replica.log().records().len(), n as usize);
            println!(
                "N={n} allocations={calls} allocated_bytes={bytes} elapsed_us={}",
                elapsed.as_micros()
            );
        }
    }
}
fn main() {
    #[cfg(all(feature = "local-writer", target_os = "linux"))]
    benchmark::run();
}
