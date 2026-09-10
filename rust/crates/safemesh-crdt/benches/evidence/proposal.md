# Measurement evidence and candidate ranges

OR-4 remains outstanding. These are measurements and candidate ranges for operator review, not numeric product commitments.

Source: 9187ea22f232ce710082285b9b5c7d9fe92f7b5a (product base 8911fc0). Machine identity, toolchain, filesystem, seed 42, condition, seven repetitions, and every brake sample are in the adjacent raw metadata files. The Rust workload source is unchanged between this source commit and the evidence commit.

Percentiles are nearest-rank p50/p95/p99 in milliseconds. With seven samples p95 and p99 are both the maximum; they do not estimate population tails. Only complete rungs enter these tables. Workload dimensions co-vary geometrically; these are not isolated causal effects or multidimensional envelope guarantees.

Allocator peak includes the instrument and setup objects; process RSS is Linux VmHWM for the whole process, including setup. Cumulative allocation counts requested bytes, including allocator reallocation through alloc/copy/dealloc. Copy traffic itself is NOT MEASURED. Kernel write_bytes is process-attributed storage-layer accounting, not hardware/media or journal traffic. Clone allocation measures the clone representation, not original allocator capacity.

## T1 — Reproduction and measurement method

HARNESS binary. WHY: a harness-free release benchmark uses the existing dev dependency, gives explicit control over filesystem setup and allocation metering, and never runs on the default test path. PATH rust/crates/safemesh-crdt/benches/envelope.rs. IN CI no.

RECORDED PER RUN: source SHA, crate version, rustc toolchain, OS, CPU model, logical core count and affinity, filesystem/mount options, seed, warm or eviction-requested condition, repetition count, workload parameters and brake readings. SEED MECHANISM: xorshift64 with seed 42 and deterministic record-index mixing; unique UTF-8 values carry a fixed-width ID. RAW OUTPUT RETAINED AT /home/monkey/scratch/benchraw/ and in this evidence directory. The first sweep metadata's dirty flag reflects generated Python cache or the new summary script, not changes to the committed Rust workload. Executable hashes and the resolved dependency lock are retained in checks/; future runner executions also record the binary hash per run.

Build from a clean checkout of the recorded source (or this branch, whose Rust workload is identical):

```sh
export PATH=/home/monkey/bin:$PATH
export TMPDIR=/home/monkey/scratch/benchraw
export CARGO_TARGET_DIR=/home/monkey/scratch/benchraw/target
nice -n 15 cargo build --manifest-path rust/Cargo.toml -j 1 -p safemesh-crdt --bench envelope --release --features local-writer
```

Use a separate CARGO_TARGET_DIR for each source worktree, especially a mutated control: a shared target cache can retain the wrong executable despite a successful build. Verify the recorded executable hash. Locate the executable envelope-* under the target/release/deps directory, then run its `self-test`. Run `python3 rust/crates/safemesh-crdt/benches/bin/run.py --self-test`. Invoke `run.py <absolute-binary> <new-permitted-scratch-output>` for the six-family sweep. Modes `--mode noise`, `--mode trace`, and `--mode single` run controls; `--family B6` selects the decoder control instead of B1. Every child command is retained in metadata. The runner nices one process, checks every prescribed brake before every rung/control, caps address space at 512 MiB, and stops a rung at 20 seconds; these additional brakes protect the shared box and are not product limits. Initial sizes are 8 records/32-byte payload/one writer/one-record batch, multiplying by 4/2/2/2 at each rung. Byte cap is batch * (payload + 64). B5 and B6 always use one writer. Seed and repetition count remain fixed at 42 and 7.

Run `summarize.py --self-test`, then `summarize.py <evidence/raw> <new-output-file>` to regenerate the measured tables and controls. Sibling control directories are required. No default CI command or check was added or removed.

B1 repetitions are successive accepted appends, so retained history increases by one between samples. B2 preserves actual outcomes rather than equating requested duplicate ratio with measured ratio. B3 uses posix_fadvise(DONTNEED) on the transaction only; fence and device caches are not made cold. B4 measures caller batching over an unbounded export and borrowed typed record import; wire bytes are encoded record lengths, and encode time is outside export/import timers. B5 times removal/query loops and records clone allocation as an explicit memory proxy. B6 times candidate decoding of generated valid and damaged frames. No production source or persisted format is modified.

## T2 — Six families

FAMILY B1 RAN yes; CARRIERS OR-Set UTF-8; RUNGS retained setup records 8, 32, 128, 512; STOPPED BY box brake: swap activity above 1000 KB/s; REPETITIONS 7 per condition. Top complete rung B1-r3: payload 256 bytes, actual writer domain 8.

| Operation / condition | p50/p95/p99 ms | Peak RSS bytes | Peak allocator bytes | Cumulative allocation bytes, min–max | Kernel write bytes, min–max |
|---|---:|---:|---:|---:|---:|
| append / warm  | 2.584374/2.617129/2.617129 | 3751936 | 1211871 | 1762892–1779450 | 151552–155648 |

BYTES transaction 150904–152668; amplification 589.469–596.359 serialized; 592.000–608.000 kernel. ASSERTION accepted history grows; full-history rewrite is exposed by actual resulting transaction lengths and storage accounting. RESULT held. Sync counts are in the separate traced control, not assumed per untraced rung.

FAMILY B2 RAN yes; CARRIERS OR-Set UTF-8; RUNGS retained setup records 8, 32, 128; STOPPED BY box brake: swap activity above 1000 KB/s; REPETITIONS 7 per condition. Top complete rung B2-r2: payload 128 bytes, actual writer domain 4.

| Operation / condition | p50/p95/p99 ms | Peak RSS bytes | Peak allocator bytes | Cumulative allocation bytes, min–max | Kernel write bytes, min–max |
|---|---:|---:|---:|---:|---:|
| duplicate_batch / warm 100 | 0.030942/0.039033/0.039033 | 2846720 | 157198 | 113828–113828 | 0–0 |
| mixed_batch / warm 0 | 1.909800/2.029626/2.029626 | 2932736 | 302192 | 632737–677889 | 49152–57344 |
| mixed_batch / warm 50 | 2.097390/2.256611/2.256611 | 2899968 | 277396 | 581437–625085 | 49152–49152 |

BYTES pre-batch log up to 25622. ASSERTION pure duplicate state, wire log, version and allocation sequence unchanged: RESULT held. Mixed rows retain actual accepted/duplicate counts. Requested 0% duplicate batches can redeliver IDs used by the earlier 50% condition; interpret actual counts rather than the requested ratio. Accepted-only single-append latency is B1; this family measures batches.

FAMILY B3 RAN yes; CARRIERS OR-Set UTF-8; RUNGS retained setup records 8, 32, 128, 512, 2048; STOPPED BY box brake: swap activity above 1000 KB/s; REPETITIONS 7 per condition. Top complete rung B3-r4: payload 512 bytes, actual writer domain 16.

| Operation / condition | p50/p95/p99 ms | Peak RSS bytes | Peak allocator bytes | Cumulative allocation bytes, min–max | Kernel write bytes, min–max |
|---|---:|---:|---:|---:|---:|
| restart / file-cache-eviction-requested  | 163.025591/170.220702/170.220702 | 15249408 | 9759523 | 1479014636–1479014636 | 4096–4096 |
| restart / warm  | 161.869171/165.943636/165.943636 | 15204352 | 9759523 | 1479014636–1479014636 | 4096–4096 |

Descriptive log-log fit of warm median against record count along this co-varying workload path: slope 1.240. Payload and writer count also increase. This is no asymptotic proof.

BYTES replay frame 1126458. ASSERTION exact state, encoded log and recovered local sequence: RESULT held. File-cache eviction was requested with successful posix_fadvise(DONTNEED); actual cache residency and device-cold restart are NOT MEASURED. Cache condition order is warm first, eviction-requested second.

FAMILY B4 RAN yes; CARRIERS OR-Set UTF-8; RUNGS retained setup records 8, 32, 128, 512; STOPPED BY box brake: swap activity above 1000 KB/s; REPETITIONS 7 per condition. Top complete rung B4-r3: payload 256 bytes, actual writer domain 8.

| Operation / condition | p50/p95/p99 ms | Peak RSS bytes | Peak allocator bytes | Cumulative allocation bytes, min–max | Kernel write bytes, min–max |
|---|---:|---:|---:|---:|---:|
| export / warm  | 0.011876/0.021570/0.023282 | 2990080 | 372669 | 2624–180032 | 0–0 |
| import / warm  | 0.001241/0.001842/0.002734 | 2990080 | 362605 | 0–25136 | 0–0 |

BYTES maximum actual record-wire batch 2320; wire excludes an unspecified transport wrapper. ASSERTION all IDs and contiguous versions converge, every batch advances: RESULT held. Odd IDs precede even IDs, with 0 and n/4 peer prefixes. Caller batching filters already-held IDs and applies record and byte caps; the product export remains unbounded. No product pagination or continuation API is claimed.

FAMILY B5 RAN yes; CARRIERS OR-Set UTF-8; RUNGS retained setup records 8, 32, 128, 512, 2048, 8192; STOPPED BY box brake: box wall-time brake 20 seconds; REPETITIONS 7 per condition. Top complete rung B5-r5: payload 1024 bytes, actual writer domain 1.

| Operation / condition | p50/p95/p99 ms | Peak RSS bytes | Peak allocator bytes | Cumulative allocation bytes, min–max | Kernel write bytes, min–max |
|---|---:|---:|---:|---:|---:|
| query / warm  | 310.910004/396.812169/396.812169 | 66363392 | 19120194 | 16744448–16744448 | 0–0 |
| remove / warm  | 213.490000/228.708024/228.708024 | 66363392 | 19115666 | 9275728–10061184 | 0–0 |

BYTES encoded log up to 9011258; retained add count 8192; tombstones up to 8192; live membership ends at zero. State/log/tombstone clone allocation maxima: 9070328/10041856/160680 bytes. Encoded bytes per recorded operation: 550.004–720.671. Remove/query times cover the row's full loop, including seeded query-string construction; not a single-operation latency. ASSERTION membership and tombstone counts exact: RESULT held.

FAMILY B6 RAN yes; CARRIERS OR-Set UTF-8; RUNGS retained setup records 8, 32; STOPPED BY box brake: swap activity above 1000 KB/s; REPETITIONS 7 per condition. Top complete rung B6-r1: payload 64 bytes, actual writer domain 1.

| Operation / condition | p50/p95/p99 ms | Peak RSS bytes | Peak allocator bytes | Cumulative allocation bytes, min–max | Kernel write bytes, min–max |
|---|---:|---:|---:|---:|---:|
| decode / warm large-valid-frame | 0.026787/0.029842/0.029842 | 2580480 | 22640 | 6648–6648 | 0–0 |
| decode / warm malformed | 0.000030/0.000050/0.000050 | 2584576 | 17368 | 0–0 | 0–0 |
| decode / warm trailing | 0.026756/0.027198/0.027198 | 2584576 | 25962 | 6648–6648 | 0–0 |
| decode / warm truncated | 0.000051/0.000071/0.000071 | 2584576 | 17368 | 0–0 | 0–0 |

BYTES maximum tested frame 3323. Typed errors ['InvalidTag', 'TrailingBytes', 'UnexpectedEof']; large valid frames accepted. ASSERTION malformed/truncated/trailing inputs reject, separately held accepted history and version unchanged: RESULT held. This decoder creates a candidate log and has no mutating import API in this case. The refusal-boundary half of G4 cannot be measured until the limit mechanism exists, and that is a different lane. Binding paths NOT MEASURED: OR-1 owns the language surface.

Carriers NOT MEASURED: G-Set, G-Counter, PN-Counter, RGA/Text. This instrument exercises the OR-Set path to carry variable UTF-8 payloads and retained tombstones through all six families. Numeric-carrier timings cannot be inferred from it; durable constructors currently expose G-Counter and OR-Set, leaving a separate G-Counter run possible but unmeasured.

## T3 — Eight measured dimensions

| Dimension | Min | Max reached | Top measurement | Stop | Peak RSS / allocator bytes | p50/p95/p99 ms | Write amplification |
|---|---:|---:|---|---|---:|---:|---|
| history record count | 8 | 16384 | B5-r5 remove warm (n=7) | box wall-time brake 20 seconds | 66363392 / 19115666 | 216.262214/228.708024/228.708024 | N/A: no accepted durable append in this operation |
| history encoded bytes | 618 | 9011258 | B5-r5 remove warm (n=7) | box wall-time brake 20 seconds | 66363392 / 19115666 | 216.262214/228.708024/228.708024 | N/A: no accepted durable append in this operation |
| writer/replica count | 1 | 16 | B3-r4 restart warm (n=7) | swap activity above 1000 KB/s | 15204352 / 9759523 | 161.869171/165.943636/165.943636 | N/A: no accepted durable append in this operation |
| per-record payload bytes | 32 | 1024 | B5-r5 remove warm (n=14) | box wall-time brake 20 seconds | 66363392 / 19115666 | 213.490000/228.708024/228.708024 | N/A: no accepted durable append in this operation |
| records per caller import batch | 1 | 8 | B4-r3 import warm (n=784) | swap activity above 1000 KB/s | 2990080 / 362605 | 0.001241/0.001842/0.002734 | N/A: no accepted durable append in this operation |
| bytes per caller import batch | 66 | 2320 | B4-r3 import warm (n=784) | swap activity above 1000 KB/s | 2990080 / 362605 | 0.001241/0.001842/0.002734 | N/A: no accepted durable append in this operation |
| live carrier entries | 0 | 4096 | B5-r5 remove warm (n=7) | box wall-time brake 20 seconds | 66363392 / 18881082 | 213.490000/221.024499/221.024499 | N/A: no accepted durable append in this operation |
| retained tombstone count | 4 | 8192 | B5-r5 remove warm (n=7) | box wall-time brake 20 seconds | 66363392 / 19115666 | 216.262214/228.708024/228.708024 | N/A: no accepted durable append in this operation |

All eight rows are candidate observed ranges, not independently swept ceilings. Maxima come from different workloads and cannot be combined. For dimensions whose maximum was measured in a non-append operation, accepted-payload write amplification is N/A; the measured durable amplification range remains the B1 table, not an extrapolation to those maxima.

## T4 — Candidate proposal

PROPOSAL FILE rust/crates/safemesh-crdt/benches/evidence/proposal.md. DIMENSIONS PROPOSED 8, exactly the measured ranges in T3. No dimension is omitted. At the recorded source on the recorded box, these workloads completed with the memory and latency above; these are proposed candidate ranges, with operator ruling OR-4 outstanding. Accepted-append amplification is measured in B1 only and is not extrapolated to higher non-append maxima. OR-4 NAMED AS OUTSTANDING yes.

Prohibited-word scan: no occurrences in lane-authored content. The scan constructs the disallowed token from its two parts to avoid placing it in this evidence itself; the command and result are retained in checks.

## T5 — Instrument controls

PLANTED: 1,048,576-byte vector retained across a decoder call in a COPY of the benchmark. EXPECTED DIRECTION: cumulative allocation and extra peak increase by exactly 1,048,576 bytes. Baseline and copied instrument use the same public decoder. The retained patch describes the mutation.

| Control | p50/p95/p99 ms, valid frame | Allocation bytes per decode | Extra peak bytes |
|---|---:|---:|---:|
| baseline-0.jsonl | 0.005117/0.007820/0.007820 | 1336 | 1208 |
| baseline-1.jsonl | 0.005208/0.007630/0.007630 | 1336 | 1208 |
| baseline-2.jsonl | 0.005347/0.012818/0.012818 | 1336 | 1208 |
| allocation-0.jsonl | 0.014350/0.674353/0.674353 | 1049912 | 1049784 |

OBSERVED allocation delta 1048576, extra-peak delta 1048576 bytes. DETECTED yes. NOISE: three unchanged decoder runs, allocation spread 0 bytes and median latency spread 230 ns. SIGNAL EXCEEDS NOISE yes for allocation. One mutant process with seven repetitions; cross-process mutant variance is NOT MEASURED.

Unchanged B1 median latencies (ms): [0.791485, 0.779658, 0.740154]; spread 0.051331 ms (6.94% of smallest median). The 5 ms planted sleep experiment stopped on swap activity before execution; DETECTED NOT MEASURED for that attempt. The copy was reverted before the allocation control.

TREE CLEAN AFTER yes for the negative worktree: empty final status and matching Rust source hashes retained. The main worktree carries only the intended harness/evidence changes.

The first allocation-control build used the wrong working directory (exit 101), after which a stale binary was copied. That run showed no allocation movement and is retained under allocation-raw, explicitly excluded from the comparison above. The corrected build exited 0 and produced the verified control binary.

## Sync control

Traced B1 control: 33 successful fsync calls, 16 transaction-file writes totaling 9712 bytes. This includes one fence sync, fresh empty commit, eight setup commits and seven measured appends. Each of the last six isolated append intervals has exactly two fsync calls. This is one traced control process; its timings are excluded from latency tables. Sync counts at larger rungs are NOT MEASURED.

Final build verification found that the shared target directory still contained the allocation mutant even after an exit-0 baseline build. A fresh isolated target build was therefore required; its log and executable hash are retained. The original sweep/noise executable had already been copied before any mutation and is distinct from both mutant hashes.
