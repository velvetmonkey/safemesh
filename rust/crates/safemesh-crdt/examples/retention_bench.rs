// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0
//! Durable history retention benchmark: restart wall time, peak RSS, bytes on
//! disk and write amplification for G-Counter and UTF-8 OR-Set stores.
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

    // Build the history in memory with the in-memory local writer (one append
    // costs no persistence), then commit it as the durable store's single
    // transaction. Growing a durable store append by append rewrites the whole
    // history each time, which is exactly the cost this benchmark reports.
    fn seed(kind: &str, records: u64, dir: &Path) -> PathBuf {
        let scratch = dir.join("seed");
        let store = dir.join("store");
        let log = match kind {
            "gcounter" => {
                let mut replica = LocalReplica::counter(&scratch, CONFIG).unwrap();
                for tally in 1..=records {
                    replica.bump(replica.ticket(), tally).unwrap();
                }
                drop(DurableReplica::counter(&store, CONFIG).unwrap());
                replica.log().to_wire_bytes().unwrap()
            }
            _ => {
                let mut replica = LocalReplica::utf8_set(&scratch, CONFIG).unwrap();
                for index in 0..records {
                    replica.add(replica.ticket(), element(index)).unwrap();
                }
                drop(DurableReplica::utf8_set(&store, CONFIG).unwrap());
                replica.log().to_wire_bytes().unwrap()
            }
        };
        fs::remove_dir_all(&scratch).unwrap();
        // The committed transaction: writer count, writer and last allocated
        // sequence, then the EventLog frame. Restart replays and checks it all.
        let mut bytes = [CONFIG.writers, CONFIG.writer, records]
            .map(u64::to_le_bytes)
            .concat();
        bytes.extend(log);
        let path = store.join(format!("writer-{}.transaction", CONFIG.writer));
        fs::write(&path, bytes).unwrap();
        fs::File::open(&path).unwrap().sync_all().unwrap();
        store
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
    // the bytes this process passed to write(2) (`wchar`) and the wall time.
    fn child_append(kind: &str, store: &Path, records: u64, appends: u64) {
        let written_before;
        let start;
        match kind {
            "gcounter" => {
                let mut replica = DurableReplica::restart_counter(store, CONFIG).unwrap();
                written_before = proc_field("/proc/self/io", "wchar:");
                start = Instant::now();
                for tally in records + 1..=records + appends {
                    replica.bump(replica.ticket(), tally).unwrap();
                }
            }
            _ => {
                let mut replica = DurableReplica::restart_utf8_set(store, CONFIG).unwrap();
                written_before = proc_field("/proc/self/io", "wchar:");
                start = Instant::now();
                for index in records..records + appends {
                    replica.add(replica.ticket(), element(index)).unwrap();
                }
            }
        }
        let micros = start.elapsed().as_micros();
        let written = proc_field("/proc/self/io", "wchar:") - written_before;
        println!("append {micros} {written}");
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

    struct Row {
        kind: &'static str,
        records: u64,
        restart_ms: f64,
        peak_rss_mib: f64,
        disk_bytes: u64,
        written_per_append: f64,
        append_growth: f64,
        append_ms: f64,
    }

    fn measure(kind: &'static str, records: u64, runs: usize, appends: u64, dir: &Path) -> Row {
        let _ = fs::remove_dir_all(dir);
        fs::create_dir_all(dir).unwrap();
        let store = seed(kind, records, dir);
        let transaction = store.join(format!("writer-{}.transaction", CONFIG.writer));
        let fence = store.join(format!("writer-{}.fence", CONFIG.writer));
        let disk_bytes = file_len(&transaction) + file_len(&fence);
        let store_arg = store.to_str().unwrap().to_owned();
        let mut samples: Vec<(u128, u128)> = (0..runs)
            .map(|_| {
                let fields = run_child(&[
                    "--child-restart".into(),
                    kind.into(),
                    store_arg.clone(),
                    records.to_string(),
                ]);
                (fields[0], fields[1])
            })
            .collect();
        samples.sort();
        let (restart_us, _) = samples[runs / 2];
        let peak_kib = samples.iter().map(|&(_, peak)| peak).max().unwrap();
        let before = file_len(&transaction);
        let fields = run_child(&[
            "--child-append".into(),
            kind.into(),
            store_arg,
            records.to_string(),
            appends.to_string(),
        ]);
        let growth = (file_len(&transaction) - before) as f64 / appends as f64;
        fs::remove_dir_all(dir).unwrap();
        Row {
            kind,
            records,
            restart_ms: restart_us as f64 / 1e3,
            peak_rss_mib: peak_kib as f64 / 1024.0,
            disk_bytes,
            written_per_append: fields[1] as f64 / appends as f64,
            append_growth: growth,
            append_ms: fields[0] as f64 / 1e3 / appends as f64,
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
        let mut out = format!(
            "| CRDT | records | restart (median of {runs}) | peak RSS | bytes on disk | disk bytes/record | bytes written per appended record | file growth per appended record | write amplification | durable append latency |\n\
             |---|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n"
        );
        for row in rows {
            writeln!(
                out,
                "| {} | {} | {:.1} ms | {:.1} MiB | {} | {:.1} | {:.0} | {:.1} | {:.0}x | {:.2} ms |",
                row.kind,
                row.records,
                row.restart_ms,
                row.peak_rss_mib,
                row.disk_bytes,
                row.disk_bytes as f64 / row.records as f64,
                row.written_per_append,
                row.append_growth,
                row.written_per_append / row.append_growth,
                row.append_ms,
            )
            .unwrap();
        }
        writeln!(
            out,
            "\nBytes written are `wchar` from `/proc/self/io` over {appends} durable \
             appends in a child process, divided by {appends}. Write amplification is \
             bytes written per appended record divided by the file growth per appended \
             record. Peak RSS is the largest restart child's `VmHWM`, including the \
             process baseline."
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
             elements `member-0000000`... Each store is one committed transaction plus its fence.\n\n\
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
                let store = super::seed(kind, 1_000, &dir);
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
