// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0
//! Durable history retention benchmark: restart wall time, peak RSS, bytes on
//! disk and write amplification for G-Counter and UTF-8 OR-Set stores.
//!
//! Each size is measured in both store formats: appends to the
//! `writer-<id>.transaction` format written before the append log (every
//! commit rewrites the whole history), then, after an explicit
//! `migrate_*_to_append_log`, restart and appends on the append log
//! (`writer-<id>.journal`, every commit appends one record).
//!
//! ```sh
//! cd rust
//! cargo run --release --locked -p safemesh-crdt --features local-writer \
//!     --example retention_bench
//! ```
//!
//! Options: `--sizes 1000,10000,100000,1000000`, `--runs 3` (restarts per
//! size; the median time is reported), `--appends 10`, `--dir <scratch>` and
//! `--out <markdown>` (default `evidence/retention/results.md`).
//!
//! Every restart and append measurement runs in a fresh child process so that
//! its peak RSS (`VmHWM`) and write counter (`wchar`) belong to that step alone.
//! A plain example is used instead of a Criterion bench: a 1M-record restart
//! is one multi-second sample whose memory high-water mark needs its own
//! process, and it adds no dependency to the locked workspace.
#[cfg(all(feature = "local-writer", target_os = "linux"))]
mod bench {
    use safemesh_crdt::{
        local::{DurableReplica, LocalReplica},
        ownership::WriterConfig,
        WireEncode,
    };
    use std::{
        fmt::Write as _,
        fs,
        path::{Path, PathBuf},
        process::Command,
        time::Instant,
    };

    const CONFIG: WriterConfig = WriterConfig {
        writers: 3,
        writer: 0,
    };
    const KINDS: [&str; 2] = ["gcounter", "utf8-orset"];

    fn element(index: u64) -> String {
        format!("member-{index:07}")
    }

    #[derive(Clone, Copy)]
    enum Format {
        /// `writer-<id>.transaction`, as written before the append log.
        Transaction,
        /// `writer-<id>.journal` grown by one entry per record.
        Journal,
    }

    // Build the history in memory with the in-memory local writer (one append
    // costs no persistence), then store it in `format` without paying a durable
    // append per record. Restart replays and checks every record either way.
    fn seed(kind: &str, records: u64, store: &Path, format: Format) {
        let scratch = store.with_extension("seed");
        let (log, entries) = match kind {
            "gcounter" => {
                let mut replica = LocalReplica::counter(&scratch, CONFIG).unwrap();
                for tally in 1..=records {
                    replica.bump(replica.ticket(), tally).unwrap();
                }
                drop(DurableReplica::counter(store, CONFIG).unwrap());
                let log = replica.log();
                let entries: Vec<_> = log
                    .records()
                    .iter()
                    .map(|r| entry(r.id.sequence, &r.to_wire_bytes().unwrap()))
                    .collect();
                (log.to_wire_bytes().unwrap(), entries)
            }
            _ => {
                let mut replica = LocalReplica::utf8_set(&scratch, CONFIG).unwrap();
                for index in 0..records {
                    replica.add(replica.ticket(), element(index)).unwrap();
                }
                drop(DurableReplica::utf8_set(store, CONFIG).unwrap());
                let log = replica.log();
                let entries: Vec<_> = log
                    .records()
                    .iter()
                    .map(|r| entry(r.id.sequence, &r.to_wire_bytes().unwrap()))
                    .collect();
                (log.to_wire_bytes().unwrap(), entries)
            }
        };
        fs::remove_dir_all(&scratch).unwrap();
        let journal = store.join(format!("writer-{}.journal", CONFIG.writer));
        let (path, bytes) = match format {
            Format::Transaction => {
                // Writer count, writer and last allocated sequence, then the
                // EventLog frame. This store has no journal.
                fs::remove_file(&journal).unwrap();
                let mut bytes = [CONFIG.writers, CONFIG.writer, records]
                    .map(u64::to_le_bytes)
                    .concat();
                bytes.extend(log);
                let path = store.join(format!("writer-{}.transaction", CONFIG.writer));
                (path, bytes)
            }
            Format::Journal => {
                // The fresh constructor wrote the header and empty base frame;
                // append one entry per record after it.
                let mut bytes = fs::read(&journal).unwrap();
                bytes.extend(entries.concat());
                (journal, bytes)
            }
        };
        fs::write(&path, bytes).unwrap();
        fs::File::open(&path).unwrap().sync_all().unwrap();
    }

    // One journal entry, as a durable append writes it: payload length and its
    // complement, the allocation sequence after the record, the record's wire
    // bytes, then CRC-32 (ISO-HDLC) over everything before it.
    fn entry(sequence: u64, record: &[u8]) -> Vec<u8> {
        let len = (8 + record.len()) as u32;
        let mut bytes = [len.to_le_bytes(), (!len).to_le_bytes()].concat();
        bytes.extend(sequence.to_le_bytes());
        bytes.extend(record);
        let mut crc = u32::MAX;
        for &byte in &bytes {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
            }
        }
        bytes.extend((!crc).to_le_bytes());
        bytes
    }

    fn migrate(kind: &str, store: &Path) -> usize {
        match kind {
            "gcounter" => DurableReplica::migrate_counter_to_append_log(store, CONFIG)
                .unwrap()
                .log()
                .records()
                .len(),
            _ => DurableReplica::migrate_utf8_set_to_append_log(store, CONFIG)
                .unwrap()
                .log()
                .records()
                .len(),
        }
    }

    fn restart(kind: &str, store: &Path) -> usize {
        match kind {
            "gcounter" => DurableReplica::restart_counter(store, CONFIG)
                .unwrap()
                .log()
                .records()
                .len(),
            _ => DurableReplica::restart_utf8_set(store, CONFIG)
                .unwrap()
                .log()
                .records()
                .len(),
        }
    }

    fn proc_field(file: &str, key: &str) -> u64 {
        let text = fs::read_to_string(file).unwrap();
        let line = text.lines().find(|line| line.starts_with(key)).unwrap();
        line[key.len()..]
            .trim()
            .trim_end_matches(" kB")
            .parse()
            .unwrap()
    }

    // Child: one timed restart; report wall time and the process's peak RSS.
    fn child_restart(kind: &str, store: &Path, records: usize) {
        let base_kib = proc_field("/proc/self/status", "VmHWM:");
        let start = Instant::now();
        let replayed = restart(kind, store);
        let micros = start.elapsed().as_micros();
        assert_eq!(replayed, records, "restart replayed every retained record");
        let peak_kib = proc_field("/proc/self/status", "VmHWM:");
        println!("restart {micros} {peak_kib} {base_kib}");
    }

    // Child: restart untimed, then durably append `appends` records and report
    // the bytes this process passed to write(2) (`wchar`) and each append's
    // wall time.
    fn child_append(kind: &str, store: &Path, records: u64, appends: u64) {
        let written_before;
        let mut micros = Vec::new();
        match kind {
            "gcounter" => {
                let mut replica = DurableReplica::restart_counter(store, CONFIG).unwrap();
                written_before = proc_field("/proc/self/io", "wchar:");
                for tally in records + 1..=records + appends {
                    let start = Instant::now();
                    replica.bump(replica.ticket(), tally).unwrap();
                    micros.push(start.elapsed().as_micros());
                }
            }
            _ => {
                let mut replica = DurableReplica::restart_utf8_set(store, CONFIG).unwrap();
                written_before = proc_field("/proc/self/io", "wchar:");
                for index in records..records + appends {
                    let start = Instant::now();
                    replica.add(replica.ticket(), element(index)).unwrap();
                    micros.push(start.elapsed().as_micros());
                }
            }
        }
        let written = proc_field("/proc/self/io", "wchar:") - written_before;
        let times: Vec<String> = micros.iter().map(u128::to_string).collect();
        println!("append {written} {}", times.join(" "));
    }

    fn run_child(args: &[String]) -> Vec<u128> {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "child {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .unwrap()
            .split_whitespace()
            .skip(1)
            .map(|field| field.parse().unwrap())
            .collect()
    }

    fn file_len(path: &Path) -> u64 {
        fs::metadata(path).unwrap().len()
    }

    // Per-append cost in one store format.
    struct Append {
        written: f64,
        growth: f64,
        median_ms: f64,
        max_ms: f64,
    }

    // Median restart time and the largest peak RSS over `runs` children.
    struct Restart {
        ms: f64,
        peak_rss_mib: f64,
    }

    struct Row {
        kind: &'static str,
        records: u64,
        transaction: Append,
        journal: Append,
        grown: Restart,
        migrated: Restart,
        disk_bytes: u64,
    }

    fn append(kind: &str, store: &Path, file: &Path, first: u64, appends: u64) -> Append {
        let before = file_len(file);
        let fields = run_child(&[
            "--child-append".into(),
            kind.into(),
            store.to_str().unwrap().into(),
            first.to_string(),
            appends.to_string(),
        ]);
        let mut times = fields[1..].to_vec();
        times.sort();
        Append {
            written: fields[0] as f64 / appends as f64,
            growth: (file_len(file) - before) as f64 / appends as f64,
            median_ms: times[times.len() / 2] as f64 / 1e3,
            max_ms: *times.last().unwrap() as f64 / 1e3,
        }
    }

    fn restarts(kind: &str, store: &Path, records: u64, runs: usize) -> Restart {
        let mut samples: Vec<(u128, u128)> = (0..runs)
            .map(|_| {
                let fields = run_child(&[
                    "--child-restart".into(),
                    kind.into(),
                    store.to_str().unwrap().into(),
                    records.to_string(),
                ]);
                (fields[0], fields[1])
            })
            .collect();
        samples.sort();
        Restart {
            ms: samples[runs / 2].0 as f64 / 1e3,
            peak_rss_mib: samples.iter().map(|&(_, peak)| peak).max().unwrap() as f64 / 1024.0,
        }
    }

    fn measure(kind: &'static str, records: u64, runs: usize, appends: u64, dir: &Path) -> Row {
        let _ = fs::remove_dir_all(dir);
        fs::create_dir_all(dir).unwrap();
        let file =
            |store: &Path, suffix: &str| store.join(format!("writer-{}.{suffix}", CONFIG.writer));

        // Before: appends to a transaction-format store, then the explicit move.
        let old = dir.join("transaction");
        seed(kind, records, &old, Format::Transaction);
        let transaction = append(kind, &old, &file(&old, "transaction"), records, appends);
        assert_eq!(migrate(kind, &old), (records + appends) as usize);
        assert!(!file(&old, "transaction").exists());
        let migrated = restarts(kind, &old, records + appends, runs);

        // After: a journal grown by one entry per record, as a fresh store grows.
        let new = dir.join("journal");
        seed(kind, records, &new, Format::Journal);
        let grown = restarts(kind, &new, records, runs);
        let disk_bytes = file_len(&file(&new, "journal")) + file_len(&file(&new, "fence"));
        let journal = append(kind, &new, &file(&new, "journal"), records, appends);
        fs::remove_dir_all(dir).unwrap();
        Row {
            kind,
            records,
            transaction,
            journal,
            grown,
            migrated,
            disk_bytes,
        }
    }

    fn command(program: &str, args: &[&str]) -> String {
        Command::new(program)
            .args(args)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
            .unwrap_or_else(|| "unknown".into())
    }

    fn machine(dir: &Path) -> String {
        let cpuinfo = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
        let cpu = cpuinfo
            .lines()
            .find_map(|line| line.strip_prefix("model name"))
            .map(|rest| rest.trim_start_matches([' ', '\t', ':']).to_owned())
            .unwrap_or_else(|| "unknown".into());
        let threads = std::thread::available_parallelism().map_or(0, |n| n.get());
        let memory = proc_field("/proc/meminfo", "MemTotal:") / 1024;
        let kernel = fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
        let filesystem = command("stat", &["-f", "-c", "%T", dir.to_str().unwrap()]);
        let rustc = command("rustc", &["--version"]);
        let cargo = command("cargo", &["--version"]);
        let commit = command("git", &["rev-parse", "HEAD"]);
        let dirty = !command("git", &["status", "--porcelain", "--untracked-files=no"]).is_empty();
        format!(
            "- CPU: {cpu} ({threads} threads available)\n\
             - Memory: {memory} MiB\n\
             - Kernel: Linux {}\n\
             - Store filesystem: {filesystem}\n\
             - Toolchain: {rustc}; {cargo}; release profile\n\
             - Source: {commit}{}\n",
            kernel.trim(),
            if dirty {
                " plus uncommitted changes"
            } else {
                ""
            },
        )
    }

    fn table(rows: &[Row], appends: u64, runs: usize) -> String {
        let mut out = String::from(
            "### Append cost: transaction format (before) and append log (after)\n\n\
             | CRDT | records | before: bytes written per appended record | before: write amplification | before: median append | before: slowest append | after: bytes written per appended record | after: write amplification | after: median append | after: slowest append |\n\
             |---|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n",
        );
        for row in rows {
            let (before, after) = (&row.transaction, &row.journal);
            writeln!(
                out,
                "| {} | {} | {:.0} | {:.0}x | {:.2} ms | {:.2} ms | {:.0} | {:.1}x | {:.2} ms | {:.2} ms |",
                row.kind,
                row.records,
                before.written,
                before.written / before.growth,
                before.median_ms,
                before.max_ms,
                after.written,
                after.written / after.growth,
                after.median_ms,
                after.max_ms,
            )
            .unwrap();
        }
        writeln!(
            out,
            "\n### Append-log store: restart and size\n\n\
             | CRDT | records | restart, one entry per record (median of {runs}) | peak RSS | bytes on disk | disk bytes/record | restart after migration (median of {runs}) | peak RSS after migration |\n\
             |---|---:|---:|---:|---:|---:|---:|---:|"
        )
        .unwrap();
        for row in rows {
            writeln!(
                out,
                "| {} | {} | {:.1} ms | {:.1} MiB | {} | {:.1} | {:.1} ms | {:.1} MiB |",
                row.kind,
                row.records,
                row.grown.ms,
                row.grown.peak_rss_mib,
                row.disk_bytes,
                row.disk_bytes as f64 / row.records as f64,
                row.migrated.ms,
                row.migrated.peak_rss_mib,
            )
            .unwrap();
        }
        writeln!(
            out,
            "\nBytes written are `wchar` from `/proc/self/io` over {appends} durable \
             appends in a child process, divided by {appends}; append times are the median \
             and slowest of those {appends}. Write amplification is bytes written per \
             appended record divided by the file growth per appended record. \"Before\" \
             appends go to the transaction-format store, which is then moved to the append \
             log by `migrate_*_to_append_log` (records plus {appends}, the whole history in \
             the journal's base frame). \"After\" appends, restart and size are for a \
             journal holding one entry per record, as a fresh store grows. Peak RSS is the \
             largest restart child's `VmHWM`, including the process baseline."
        )
        .unwrap();
        out
    }

    fn option<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
        args.iter()
            .position(|arg| arg == name)
            .map(|index| args[index + 1].as_str())
    }

    pub fn main() {
        let args: Vec<String> = std::env::args().skip(1).collect();
        match args.first().map(String::as_str) {
            Some("--child-restart") => {
                return child_restart(&args[1], Path::new(&args[2]), args[3].parse().unwrap())
            }
            Some("--child-append") => {
                return child_append(
                    &args[1],
                    Path::new(&args[2]),
                    args[3].parse().unwrap(),
                    args[4].parse().unwrap(),
                )
            }
            _ => {}
        }
        let sizes: Vec<u64> = option(&args, "--sizes")
            .unwrap_or("1000,10000,100000,1000000")
            .split(',')
            .map(|size| size.parse().unwrap())
            .collect();
        let runs: usize = option(&args, "--runs").map_or(3, |runs| runs.parse().unwrap());
        let appends: u64 = option(&args, "--appends").map_or(10, |n| n.parse().unwrap());
        let dir = option(&args, "--dir").map_or_else(
            || std::env::temp_dir().join(format!("safemesh-retention-{}", std::process::id())),
            PathBuf::from,
        );
        let out = option(&args, "--out").map_or_else(
            || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../evidence/retention/results.md"),
            PathBuf::from,
        );
        fs::create_dir_all(&dir).unwrap();
        let machine = machine(&dir);
        let mut rows = Vec::new();
        for kind in KINDS {
            for &records in &sizes {
                eprintln!("measuring {kind} at {records} records");
                rows.push(measure(kind, records, runs, appends, &dir.join("case")));
            }
        }
        let _ = fs::remove_dir_all(&dir);
        let table = table(&rows, appends, runs);
        println!("{table}");
        let report = format!(
            "# Durable history retention benchmark\n\n\
             Generated by `cargo run --release --locked -p safemesh-crdt --features local-writer \
             --example retention_bench` (`rust/crates/safemesh-crdt/examples/retention_bench.rs`). \
             Do not edit by hand; rerun to regenerate.\n\n\
             Workload: one writer of three (`writers: 3, writer: 0`). G-Counter records are \
             `bump` tallies 1..=n; UTF-8 OR-Set records are `add` of distinct 14-byte \
             elements `member-0000000`... Each size is seeded twice, without a durable \
             append per record: once as a transaction-format store (the format written \
             before the append log) and once as an append log with one entry per \
             record.\n\n\
             ## Machine\n\n{machine}\n## Results\n\n{table}"
        );
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&out, report).unwrap();
        eprintln!("wrote {}", out.display());
    }

    #[cfg(test)]
    mod tests {
        // Keeps the harness honest at a size cheap enough for `cargo test --examples`.
        #[test]
        fn seeded_stores_restart_with_every_record() {
            let dir =
                std::env::temp_dir().join(format!("safemesh-retention-t{}", std::process::id()));
            for kind in super::KINDS {
                let _ = std::fs::remove_dir_all(&dir);
                std::fs::create_dir_all(&dir).unwrap();
                let store = dir.join("transaction");
                super::seed(kind, 1_000, &store, super::Format::Transaction);
                assert_eq!(super::restart(kind, &store), 1_000);
                assert_eq!(super::migrate(kind, &store), 1_000);
                assert_eq!(super::restart(kind, &store), 1_000);
                let store = dir.join("journal");
                super::seed(kind, 1_000, &store, super::Format::Journal);
                assert_eq!(super::restart(kind, &store), 1_000);
            }
            std::fs::remove_dir_all(&dir).unwrap();
        }
    }
}

fn main() {
    #[cfg(all(feature = "local-writer", target_os = "linux"))]
    bench::main();
    #[cfg(not(all(feature = "local-writer", target_os = "linux")))]
    eprintln!("retention_bench requires Linux and --features local-writer");
}
