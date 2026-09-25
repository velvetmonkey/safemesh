// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0
//! Check or migrate one persisted EventLog file.
//!
//!   migrate_event_log <delta> <replica-count|unbounded> <input>            check only
//!   migrate_event_log <delta> <replica-count|unbounded> <input> <output>   migrate
//!
//! `<delta>`: gcounter, pncounter, orset-u64, orset-utf8, lww-register-u64,
//! enable-wins-flag-u64 or lww-map-u64. Counters take the original replica
//! count; the other deltas take `unbounded`. The input is never modified and
//! the output must not exist. Exit 0 on success, 1 on a refused file, 2 on usage.
use safemesh_crdt::{
    Crdt, EnableWinsFlag, EventLog, GCounter, LwwMap, LwwRegister, OrSet, PnCounter, WireDecode,
    WireEncode, WireError, WireSchema,
};
use std::{fs, io::Write, process::ExitCode};

const USAGE: &str = "usage: migrate_event_log <delta> <replica-count|unbounded> <input> [<output>]";

fn run<C: Crdt>(state: C, input: &[u8], output: Option<&str>) -> Result<String, WireError>
where
    C::Delta: WireDecode + WireEncode + WireSchema + PartialEq,
{
    let Some(output) = output else {
        let log = EventLog::<C::Delta>::from_wire_bytes_for(input, &state)?;
        return Ok(format!("current frame: {} records", log.records().len()));
    };
    let migrated = EventLog::<C::Delta>::migrate_legacy_wire_bytes_for(input, &state)?;
    let records = EventLog::<C::Delta>::from_wire_bytes_for(&migrated, &state)?
        .records()
        .len();
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .unwrap_or_else(|error| panic!("cannot create {output}: {error}"));
    file.write_all(&migrated).unwrap();
    file.sync_all().unwrap();
    Ok(format!("wrote {output}: {records} records, current frame"))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !(3..=4).contains(&args.len()) {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let fixed = args[1].parse::<usize>().ok();
    let unbounded = args[1] == "unbounded";
    let input =
        fs::read(&args[2]).unwrap_or_else(|error| panic!("cannot read {}: {error}", args[2]));
    let output = args.get(3).map(String::as_str);
    let result = match (args[0].as_str(), fixed, unbounded) {
        ("gcounter", Some(n), _) => run(GCounter::new(n), &input, output),
        ("pncounter", Some(n), _) => run(PnCounter::new(n), &input, output),
        ("orset-u64", _, true) => run(OrSet::<u64, u64>::new(), &input, output),
        ("orset-utf8", _, true) => run(OrSet::<String, u64>::new(), &input, output),
        ("lww-register-u64", _, true) => run(LwwRegister::<u64>::new(), &input, output),
        ("enable-wins-flag-u64", _, true) => run(EnableWinsFlag::<u64>::new(), &input, output),
        ("lww-map-u64", _, true) => run(LwwMap::<u64, u64>::new(), &input, output),
        _ => {
            eprintln!("{USAGE}\ncounters take a replica count; other deltas take `unbounded`");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(message) => {
            println!("{}: {message}", args[2]);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}: {error}", args[2]);
            ExitCode::FAILURE
        }
    }
}
