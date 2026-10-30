---
title: SafeMesh behaviour contract (main, unreleased)
description: Measured behaviour and open 1.0 promises for the current source tree.
---

This page describes source behaviour on `main`; release-wide support remains **NOT YET, row 6**.

## Support matrix

| Journey or carrier | Current boundary | Contract |
| --- | --- | --- |
| G-Counter and UTF-8 OR-Set | The [Rust durable walk](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/README.md#L85-L96) uses both; the [crate scope](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/README.md#L1-L3) focuses on G-Counter and OR-Set. | **HOLDS** for the named source journey; complete 1.0 support matrix **NOT YET, row 6**. |
| Linux durable Rust | `DurableReplica` provides [Linux local-file append and restart](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/src/local.rs#L802-L812). | **HOLDS** for this source path; full supported-carrier exit test **NOT YET, row 6**. |
| Python and WASM | Python [exports identity bytes without disk I/O](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-python/README.md#L194-L205); WASM [requires caller storage and writer exclusion](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-wasm/README.md#L258-L270). | **HOLDS** as carriers with app-owned storage; durable binding support **NOT YET, row 6**. |
| C | The [C ABI wraps the Rust core](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-ffi/README.md#L22-L26). | **HOLDS** as a carrier API; durable C replica **NOT YET, row 6**. |
| Other carriers | The [crate README calls them experimental](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/README.md#L1-L3). | Available, **NOT YET, row 6** for supported status; full parity is outside 1.0. |

## What "saved" means

- **HOLDS:** In a fresh Linux durable store, each `Accepted` record follows a journal append and [file sync](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/src/local.rs#L802-L812); the append path calls [`sync_data`](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/src/local.rs#L930-L954).
- **HOLDS:** Older transaction stores still use [file and directory sync](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/src/persistence.rs#L40-L57); an interrupted `append_batch` [can require recovery](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/src/local.rs#L1044-L1046).
- **NOT YET, row 2:** A power-loss durability or crash-atomicity guarantee is absent from the [proof boundary](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/WHAT-IS-PROVEN.md#L26-L27); process-kill recovery does not establish it.

## What "synced" means

- **HOLDS:** Equal accepted logs replay to equal live state in the [record-kernel boundary](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/WHAT-IS-PROVEN.md#L26-L27); this requires the relevant records to be delivered.
- **NOT YET, row 2:** Equal input sets containing conflicting payloads can leave different accepted sets, and [equal version prefixes can hide a fork](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/WHAT-IS-PROVEN.md#L62); a "synced" signal cannot promise agreement in that case.
- **NOT YET, row 5:** Bounded batch exchange need not progress after an early gap: the [six-record offer against a three-record limit](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/docs/src/content/docs/limits.md#L55-L65) is refused repeatedly.

## Batch refusal

- **HOLDS:** [`merge_log_bytes`](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/src/replica.rs#L135-L160) decodes the whole batch before admission, so a decode error admits none; after successful decode it admits records one by one and returns one `Admission` per record, so the decoded batch is not atomic.
- **NOT YET, row 2:** One cross-binding refusal rule for invalid records, collisions, overflow and mixed batches remains the [row 2 exit test](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/WHAT-IS-PROVEN.md#L29-L29), not a promise of this page.

## Capacity

- **HOLDS:** The [default decode cap is 100,000 occurrences](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/src/codec.rs#L235-L248); record 100,001 in a loaded log [returns `RecordLimitExceeded` before admission](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/docs/src/content/docs/limits.md#L102-L109).
- **HOLDS:** Ordinary durable restart uses that [same cap](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/src/local.rs#L794-L800) and [refuses a declared over-budget history before replay](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/src/local.rs#L1202-L1218).
- **NOT YET, row 5:** The [durable append paths](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/src/local.rs#L985-L1000) and [`append_batch`](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/src/local.rs#L1047-L1128) have no matching record-count guard, so they can save history that ordinary restart refuses.
- **HOLDS:** There is [no public snapshot or compaction API](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/docs/src/content/docs/limits.md#L176-L178); retained history grows until the application stops writing.

## Upgrade

- **HOLDS:** A newer tagged fence or transaction format gets [`UnsupportedFormat`](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/src/local.rs#L126-L162), while durable restart [does not migrate](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/src/local.rs#L802-L812); retained compatibility is exercised by [`v010_compat.rs`](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/tests/v010_compat.rs), [`v021_compat.rs`](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/tests/v021_compat.rs) and [`wrapper_version.rs`](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/rust/crates/safemesh-crdt/tests/wrapper_version.rs).
- **NOT YET, row 7:** A complete release upgrade guarantee, including all retained wrappers and a compatibility window, remains the [documented upgrade boundary](https://github.com/velvetmonkey/safemesh/blob/a889858fc224b60b98713d5e148528a10b5f3ac5/docs/src/content/docs/persist-and-restart.md#L449-L455).
