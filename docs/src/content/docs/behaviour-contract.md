---
title: SafeMesh behaviour contract (main, unreleased)
description: Measured behaviour and open 1.0 promises for the current source tree.
---

This page describes source behaviour on `main`; release-wide support remains **NOT YET, row 6**.

## Support matrix

| Journey or carrier | Current boundary | Contract |
| --- | --- | --- |
| G-Counter and UTF-8 OR-Set | The [Rust durable walk](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/README.md?plain=1#LC85) uses both; the [crate scope](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/README.md?plain=1#LC1) focuses on G-Counter and OR-Set. | **HOLDS** for the named source journey; complete 1.0 support matrix **NOT YET, row 6**. |
| Linux durable Rust | `DurableReplica` provides [Linux local-file append and restart](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/src/local.rs#LC802). | **HOLDS** for this source path; full supported-carrier exit test **NOT YET, row 6**. |
| Python and WASM | Python [exports identity bytes without disk I/O](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-python/README.md?plain=1#LC194); WASM [requires caller storage and writer exclusion](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-wasm/README.md?plain=1#LC258). | **HOLDS** as carriers with app-owned storage; durable binding support **NOT YET, row 6**. |
| C | The [C ABI wraps the Rust core](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-ffi/README.md?plain=1#LC22). | **HOLDS** as a carrier API; durable C replica **NOT YET, row 6**. |
| Other carriers | The [crate README calls them experimental](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/README.md?plain=1#LC1). | Available, **NOT YET, row 6** for supported status; full parity is outside 1.0. |

## What "saved" means

- **HOLDS:** In a fresh Linux durable store, each `Accepted` record follows a journal append and [file sync](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/src/local.rs#LC802); the append path calls [`sync_data`](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/src/local.rs#LC930).
- **HOLDS:** Older transaction stores still use [file and directory sync](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/src/persistence.rs#LC40); an interrupted `append_batch` [can require recovery (local.rs:1044-1046)](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/src/local.rs).
- **NOT YET, row 2:** A power-loss durability or crash-atomicity guarantee is absent from the [proof boundary](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/WHAT-IS-PROVEN.md?plain=1#LC26); process-kill recovery does not establish it.

## What "synced" means

- **HOLDS:** Equal accepted logs replay to equal live state in the [record-kernel boundary](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/WHAT-IS-PROVEN.md?plain=1#LC26); this requires the relevant records to be delivered.
- **NOT YET, row 2:** Equal input sets containing conflicting payloads can leave different accepted sets, and [equal version prefixes can hide a fork](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/WHAT-IS-PROVEN.md?plain=1#LC62); a "synced" signal cannot promise agreement in that case.
- **NOT YET, row 5:** Bounded batch exchange need not progress after an early gap: the [six-record offer against a three-record limit](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/docs/src/content/docs/limits.md?plain=1#LC55) is refused repeatedly.

## Batch refusal

- **HOLDS:** [`merge_log_bytes`](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/src/replica.rs#LC135) decodes the whole batch before admission, so a decode error admits none; after successful decode it admits records one by one and returns one `Admission` per record, so the decoded batch is not atomic.
- **NOT YET, row 2:** One cross-binding refusal rule for invalid records, collisions, overflow and mixed batches remains the [row 2 exit test](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/WHAT-IS-PROVEN.md?plain=1#LC29), not a promise of this page.

## Capacity

- **HOLDS:** The [default decode cap is 100,000 occurrences](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/src/codec.rs#LC235); record 100,001 in a loaded log [returns `RecordLimitExceeded` before admission](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/docs/src/content/docs/limits.md?plain=1#LC102).
- **HOLDS:** Ordinary durable restart uses that [same cap](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/src/local.rs#LC794) and [refuses a declared over-budget history before replay (local.rs:1202-1218)](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/src/local.rs).
- **NOT YET, row 5:** The [durable append paths](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/src/local.rs#LC985) and [`append_batch` (local.rs:1047-1128)](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/src/local.rs) have no matching record-count guard, so they can save history that ordinary restart refuses.
- **HOLDS:** There is [no public snapshot or compaction API](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/docs/src/content/docs/limits.md?plain=1#LC176); retained history grows until the application stops writing.

## Upgrade

- **HOLDS:** A newer tagged fence or transaction format gets [`UnsupportedFormat`](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/src/local.rs#LC126), while durable restart [does not migrate](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/src/local.rs#LC802); retained compatibility is exercised by [`v010_compat.rs`](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/tests/v010_compat.rs), [`v021_compat.rs`](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/tests/v021_compat.rs) and [`wrapper_version.rs`](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/tests/wrapper_version.rs).
- **NOT YET, row 7:** A complete release upgrade guarantee, including all retained wrappers and a compatibility window, remains the [documented upgrade boundary](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/docs/src/content/docs/persist-and-restart.md?plain=1#LC449).
