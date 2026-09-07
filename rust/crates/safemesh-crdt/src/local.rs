// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0
//! Linux/local-filesystem packet-A adapter. All writers for a replica set must
//! use the same directory and fixed configuration. Keep fence files in place.
//! A previously initialized store requires checked recovery (packet C); this
//! adapter errors instead of resetting allocation on restart. LocalReplica
//! acknowledges in memory; DurableReplica adds packet-B durable commits.
use crate::{
    ownership::*, Admission, Crdt, EventLog, GCounter, GCounterDelta, OrSet, OrSetDelta, Record,
    RecordId, WireEncode, WireSchema,
};
use alloc::{format, string::String, vec::Vec};
use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub enum LocalError {
    Refused,
    Exhausted,
    RecoveryRequired,
    Configuration,
    Io(io::Error),
}
impl From<io::Error> for LocalError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// A generation captured by a caller. Renewal invalidates all older tickets.
#[derive(Clone, Copy, Debug)]
pub struct WriteTicket(u64);

/// Owns the OS lock for its entire lifetime. No Clone or mutable state/log
/// access is exposed. Contention yields a read-only instance whose writes fail.
pub struct LocalReplica<C: Crdt> {
    config: WriterConfig,
    fence: File,
    held: bool,
    generation: u64,
    state: C,
    log: EventLog<C::Delta>,
    last_sequence: u64,
}

impl<C: Crdt> Drop for LocalReplica<C> {
    fn drop(&mut self) {
        // Explicitly release our open-file-description lock before close. A
        // concurrent fork/exec can briefly inherit an FD even with CLOEXEC.
        let _ = self.fence.unlock();
    }
}

impl<C: Crdt> LocalReplica<C>
where
    C::Delta: OwnedDelta + Clone + PartialEq,
{
    fn fresh(root: &Path, config: WriterConfig, state: C) -> Result<Self, LocalError> {
        config.validate().map_err(|_| LocalError::Configuration)?;
        fs::create_dir_all(root)?;
        let mut fence = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join(format!("writer-{}.fence", config.writer)))?;
        let held = match fence.try_lock() {
            Ok(()) => true,
            Err(TryLockError::WouldBlock) => false,
            Err(TryLockError::Error(e)) => return Err(e.into()),
        };
        let mut bytes = Vec::new();
        fence.read_to_end(&mut bytes)?;
        let generation = if bytes.is_empty() && held {
            // Reserve the store even before the first edit. Resetting an old
            // allocation is never an implicit recovery policy.
            fence.write_all(&config.writers.to_le_bytes())?;
            fence.write_all(&config.writer.to_le_bytes())?;
            fence.write_all(&1u64.to_le_bytes())?;
            fence.sync_all()?;
            1
        } else {
            if bytes.len() != 24 {
                return Err(LocalError::RecoveryRequired);
            }
            let word = |i| u64::from_le_bytes(bytes[i..i + 8].try_into().unwrap());
            if word(0) != config.writers || word(8) != config.writer {
                return Err(LocalError::Configuration);
            }
            if held {
                return Err(LocalError::RecoveryRequired);
            }
            word(16)
        };
        Ok(Self {
            config,
            fence,
            held,
            generation,
            log: EventLog::for_crdt(&state),
            state,
            last_sequence: 0,
        })
    }

    pub fn state(&self) -> &C {
        &self.state
    }
    pub fn log(&self) -> &EventLog<C::Delta> {
        &self.log
    }
    pub fn ticket(&self) -> WriteTicket {
        WriteTicket(self.generation)
    }
    /// Ordinary allocation metadata, independent of the lock generation.
    pub fn allocation_bytes(&self) -> Vec<u8> {
        [
            self.config.writers.to_le_bytes(),
            self.config.writer.to_le_bytes(),
            self.last_sequence.to_le_bytes(),
        ]
        .concat()
    }
    fn context(&self, ticket: WriteTicket, local: bool) -> WriteContext {
        WriteContext {
            config: self.config,
            held: self.held,
            generation: ticket.0,
            current_generation: self.generation,
            local,
        }
    }

    /// Revoke outstanding tickets while retaining the same exclusive OS lock.
    /// I/O uncertainty disables writes. This does not recover a replica.
    pub fn renew(&mut self, ticket: WriteTicket) -> Result<WriteTicket, LocalError> {
        if refuses(
            self.context(ticket, true),
            RecordId {
                replica: self.config.writer,
                sequence: 1,
            },
            OwnedPayload::Remove,
        ) {
            return Err(LocalError::Refused);
        }
        let next = next_sequence(self.generation).ok_or(LocalError::Exhausted)?;
        self.held = false;
        self.fence.seek(SeekFrom::Start(16))?;
        self.fence.write_all(&next.to_le_bytes())?;
        self.fence.sync_all()?;
        self.generation = next;
        self.held = true;
        Ok(self.ticket())
    }

    /// Validate ownership and the local lease BEFORE calling M1 admission.
    /// Remote redelivery and reordering do not require the receiver to own the
    /// author's coordinate. They require the record to carry its author's space.
    pub fn receive(
        &mut self,
        ticket: WriteTicket,
        record: Record<C::Delta>,
    ) -> Result<Admission, LocalError> {
        self.admit(ticket, record, false)
    }
    fn admit(
        &mut self,
        ticket: WriteTicket,
        record: Record<C::Delta>,
        local: bool,
    ) -> Result<Admission, LocalError> {
        self.admit_committed(ticket, record, local, |_, _| Ok(()))
    }
    fn admit_committed(
        &mut self,
        ticket: WriteTicket,
        record: Record<C::Delta>,
        local: bool,
        commit: impl FnOnce(&EventLog<C::Delta>, u64) -> Result<(), LocalError>,
    ) -> Result<Admission, LocalError> {
        if refuses(
            self.context(ticket, local),
            record.id,
            record.delta.owned_payload(),
        ) {
            return Err(LocalError::Refused);
        }
        let mut candidate = self.log.clone();
        let outcome = candidate.insert_record(record.clone());
        if outcome != Admission::Accepted {
            return Ok(outcome);
        }
        let sequence = if record.id.replica == self.config.writer {
            self.last_sequence.max(record.id.sequence)
        } else {
            self.last_sequence
        };
        // Keep the OS lock but revoke writes before any fallible persistence.
        // An error (including an ambiguous rename/sync) cannot be renewed away.
        self.held = false;
        commit(&candidate, sequence)?;
        self.state.apply_delta(record.delta);
        self.log = candidate;
        self.last_sequence = sequence;
        self.held = true;
        Ok(outcome)
    }
    /// Append an existing delta in the configured writer's space. No allocation
    /// or state mutation occurs on refusal, exhaustion, duplicate or collision.
    pub fn append(
        &mut self,
        ticket: WriteTicket,
        delta: C::Delta,
    ) -> Result<Record<C::Delta>, LocalError> {
        let sequence = next_sequence(self.last_sequence).ok_or(LocalError::Exhausted)?;
        let record = Record {
            id: RecordId {
                replica: self.config.writer,
                sequence,
            },
            delta,
        };
        match self.admit(ticket, record.clone(), true)? {
            Admission::Accepted => Ok(record),
            _ => Err(LocalError::Refused),
        }
    }
}

impl LocalReplica<GCounter> {
    pub fn counter(root: &Path, config: WriterConfig) -> Result<Self, LocalError> {
        config.validate().map_err(|_| LocalError::Configuration)?;
        let n = usize::try_from(config.writers).map_err(|_| LocalError::Configuration)?;
        Self::fresh(root, config, GCounter::new(n))
    }
    pub fn bump(
        &mut self,
        ticket: WriteTicket,
        tally: u64,
    ) -> Result<Record<GCounterDelta>, LocalError> {
        self.append(
            ticket,
            GCounterDelta {
                replica: self.config.writer as usize,
                tally,
            },
        )
    }
}
impl LocalReplica<OrSet<String, u64>> {
    pub fn utf8_set(root: &Path, config: WriterConfig) -> Result<Self, LocalError> {
        Self::fresh(root, config, OrSet::new())
    }
    pub fn add(
        &mut self,
        ticket: WriteTicket,
        element: String,
    ) -> Result<Record<OrSetDelta<String, u64>>, LocalError> {
        let sequence = next_sequence(self.last_sequence).ok_or(LocalError::Exhausted)?;
        let token = allocate_token(self.config.writers, self.config.writer, sequence)
            .ok_or(LocalError::Exhausted)?;
        self.append(ticket, OrSetDelta::Add { element, token })
    }
    pub fn remove(
        &mut self,
        ticket: WriteTicket,
        element: &String,
    ) -> Result<Record<OrSetDelta<String, u64>>, LocalError> {
        self.append(
            ticket,
            OrSetDelta::Remove {
                tokens: self.state.observed_tokens(element).into_iter().collect(),
            },
        )
    }
}

#[path = "persistence.rs"]
mod persistence;

/// One committed local transaction. The suffix is the existing, unchanged log
/// wire encoding; the 24-byte prefix is LocalReplica::allocation_bytes(). This
/// storage container is not a new transport encoding. Reading it grants no
/// write lease and does not implement packet C's checked writable restart.
#[derive(Debug, PartialEq, Eq)]
pub struct CommittedTransaction {
    pub config: WriterConfig,
    pub last_sequence: u64,
    pub log_bytes: Vec<u8>,
}
impl CommittedTransaction {
    pub fn read(root: &Path, config: WriterConfig) -> Result<Self, LocalError> {
        config.validate().map_err(|_| LocalError::Configuration)?;
        let bytes = fs::read(transaction_path(root, config))?;
        if bytes.len() < 24 {
            return Err(LocalError::RecoveryRequired);
        }
        let word = |i| u64::from_le_bytes(bytes[i..i + 8].try_into().unwrap());
        if word(0) != config.writers || word(8) != config.writer {
            return Err(LocalError::Configuration);
        }
        Ok(Self {
            config,
            last_sequence: word(16),
            log_bytes: bytes[24..].to_vec(),
        })
    }
}
fn transaction_path(root: &Path, config: WriterConfig) -> PathBuf {
    root.join(format!("writer-{}.transaction", config.writer))
}

/// Additive durable API for Linux local filesystems. The root must already
/// exist durably and remain in place; all writers use the same fixed root and
/// configuration. Each Accepted/Ok(record) follows full transaction replacement
/// and file + directory sync. Any persistence error permanently disables this
/// instance's writes, retaining its fence until drop. Existing stores error:
/// writable restart and history validation belong to packet C.
pub struct DurableReplica<C: Crdt> {
    inner: LocalReplica<C>,
    path: PathBuf,
}
impl<C: Crdt> DurableReplica<C>
where
    C::Delta: OwnedDelta + Clone + PartialEq + WireEncode + WireSchema,
{
    fn fresh(root: &Path, config: WriterConfig, state: C) -> Result<Self, LocalError> {
        let root = root.canonicalize()?;
        // Never overwrite a transaction whose fence is missing.
        match fs::metadata(transaction_path(&root, config)) {
            Ok(_) => return Err(LocalError::RecoveryRequired),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        let inner = LocalReplica::fresh(&root, config, state)?;
        if !inner.held {
            return Err(LocalError::Refused);
        }
        let path = transaction_path(&root, config);
        let mut replica = Self { inner, path };
        replica.inner.held = false;
        Self::commit(&replica.path, config, &replica.inner.log, 0)?;
        replica.inner.held = true;
        Ok(replica)
    }
    fn commit(
        path: &Path,
        config: WriterConfig,
        log: &EventLog<C::Delta>,
        sequence: u64,
    ) -> Result<(), LocalError> {
        let mut bytes = [
            config.writers.to_le_bytes(),
            config.writer.to_le_bytes(),
            sequence.to_le_bytes(),
        ]
        .concat();
        bytes.extend(
            log.to_wire_bytes()
                .map_err(|e| LocalError::Io(io::Error::other(format!("{e:?}"))))?,
        );
        persistence::replace(path, &bytes)?;
        Ok(())
    }
    pub fn state(&self) -> &C {
        self.inner.state()
    }
    pub fn log(&self) -> &EventLog<C::Delta> {
        self.inner.log()
    }
    pub fn allocation_bytes(&self) -> Vec<u8> {
        self.inner.allocation_bytes()
    }
    pub fn ticket(&self) -> WriteTicket {
        self.inner.ticket()
    }
    pub fn renew(&mut self, ticket: WriteTicket) -> Result<WriteTicket, LocalError> {
        self.inner.renew(ticket)
    }
    fn admit(
        &mut self,
        ticket: WriteTicket,
        record: Record<C::Delta>,
        local: bool,
    ) -> Result<Admission, LocalError> {
        let path = &self.path;
        let config = self.inner.config;
        let outcome = self
            .inner
            .admit_committed(ticket, record, local, |log, sequence| {
                Self::commit(path, config, log, sequence)
            })?;
        #[cfg(test)]
        persistence::checkpoint(7)?;
        Ok(outcome)
    }
    pub fn receive(
        &mut self,
        ticket: WriteTicket,
        record: Record<C::Delta>,
    ) -> Result<Admission, LocalError> {
        self.admit(ticket, record, false)
    }
    pub fn append(
        &mut self,
        ticket: WriteTicket,
        delta: C::Delta,
    ) -> Result<Record<C::Delta>, LocalError> {
        let sequence = next_sequence(self.inner.last_sequence).ok_or(LocalError::Exhausted)?;
        let record = Record {
            id: RecordId {
                replica: self.inner.config.writer,
                sequence,
            },
            delta,
        };
        match self.admit(ticket, record.clone(), true)? {
            Admission::Accepted => Ok(record),
            _ => Err(LocalError::Refused),
        }
    }
}
impl DurableReplica<GCounter> {
    pub fn counter(root: &Path, config: WriterConfig) -> Result<Self, LocalError> {
        config.validate().map_err(|_| LocalError::Configuration)?;
        let n = usize::try_from(config.writers).map_err(|_| LocalError::Configuration)?;
        Self::fresh(root, config, GCounter::new(n))
    }
    pub fn bump(
        &mut self,
        ticket: WriteTicket,
        tally: u64,
    ) -> Result<Record<GCounterDelta>, LocalError> {
        self.append(
            ticket,
            GCounterDelta {
                replica: self.inner.config.writer as usize,
                tally,
            },
        )
    }
}
impl DurableReplica<OrSet<String, u64>> {
    pub fn utf8_set(root: &Path, config: WriterConfig) -> Result<Self, LocalError> {
        Self::fresh(root, config, OrSet::new())
    }
    pub fn add(
        &mut self,
        ticket: WriteTicket,
        element: String,
    ) -> Result<Record<OrSetDelta<String, u64>>, LocalError> {
        let sequence = next_sequence(self.inner.last_sequence).ok_or(LocalError::Exhausted)?;
        let token = allocate_token(
            self.inner.config.writers,
            self.inner.config.writer,
            sequence,
        )
        .ok_or(LocalError::Exhausted)?;
        self.append(ticket, OrSetDelta::Add { element, token })
    }
    pub fn remove(
        &mut self,
        ticket: WriteTicket,
        element: &String,
    ) -> Result<Record<OrSetDelta<String, u64>>, LocalError> {
        self.append(
            ticket,
            OrSetDelta::Remove {
                tokens: self
                    .inner
                    .state
                    .observed_tokens(element)
                    .into_iter()
                    .collect(),
            },
        )
    }
}

#[cfg(test)]
mod durable_tests {
    use super::*;
    use alloc::string::ToString;
    use std::{
        process::Command,
        sync::atomic::{AtomicU64, Ordering},
    };
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    fn root() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "durable-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }
    fn config() -> WriterConfig {
        WriterConfig {
            writers: 2,
            writer: 0,
        }
    }
    fn read(root: &Path) -> CommittedTransaction {
        CommittedTransaction::read(root, config()).unwrap()
    }
    fn fault(boundary: u8, crash: bool) {
        persistence::FAULT.with(|f| f.set((boundary, crash)));
    }
    fn exercise(
        kind: &str,
        root: &Path,
        boundary: u8,
    ) -> (CommittedTransaction, CommittedTransaction) {
        let old;
        if kind == "counter" {
            let mut r = DurableReplica::counter(root, config()).unwrap();
            r.bump(r.ticket(), 5).unwrap();
            old = read(root);
            fault(boundary, true);
            assert_eq!(r.bump(r.ticket(), 9).unwrap().id.sequence, 2);
            assert_eq!(r.state().state(), &[9, 0]);
        } else {
            let mut r = DurableReplica::utf8_set(root, config()).unwrap();
            r.add(r.ticket(), "café☕".into()).unwrap();
            old = read(root);
            fault(boundary, true);
            assert_eq!(
                r.remove(r.ticket(), &"café☕".into()).unwrap().id.sequence,
                2
            );
            assert!(!r.state().contains(&"café☕".into()));
            assert_eq!(r.state().tombstones().len(), 1);
        }
        // The public call has returned success to its caller.
        std::println!("ACK {kind}");
        std::io::stdout().flush().unwrap();
        persistence::checkpoint(8).unwrap();
        (old, read(root))
    }
    fn replay(kind: &str, transaction: &CommittedTransaction) {
        assert_eq!(transaction.config, config());
        assert_eq!(transaction.last_sequence, 2);
        if kind == "counter" {
            let mut state = GCounter::new(2);
            let log =
                EventLog::<GCounterDelta>::from_wire_bytes_for(&transaction.log_bytes, &state)
                    .unwrap();
            assert_eq!(log.records().len(), 2);
            assert_eq!(log.to_wire_bytes().unwrap(), transaction.log_bytes);
            for record in log.records() {
                state.apply_delta(record.delta.clone());
            }
            assert_eq!(state.state(), &[9, 0]);
        } else {
            let mut state = OrSet::<String, u64>::new();
            let log = EventLog::<OrSetDelta<String, u64>>::from_wire_bytes_for(
                &transaction.log_bytes,
                &state,
            )
            .unwrap();
            assert_eq!(log.records().len(), 2);
            assert_eq!(log.to_wire_bytes().unwrap(), transaction.log_bytes);
            for record in log.records() {
                state.apply_delta(record.delta.clone());
            }
            assert!(!state.contains(&"café☕".into()));
            assert_eq!(
                state.tombstones().iter().copied().collect::<Vec<_>>(),
                alloc::vec![2]
            );
        }
    }
    #[test]
    fn durable_child() {
        if let Ok(root) = std::env::var("SAFEMESH_DURABLE_ROOT") {
            let kind = std::env::var("SAFEMESH_DURABLE_KIND").unwrap();
            if std::env::var_os("SAFEMESH_DURABLE_READ").is_some() {
                replay(&kind, &read(Path::new(&root)));
                return;
            }
            let boundary = std::env::var("SAFEMESH_DURABLE_BOUNDARY")
                .unwrap()
                .parse()
                .unwrap();
            exercise(&kind, Path::new(&root), boundary);
        }
    }
    fn crash(kind: &str, boundary: u8, root: &Path) -> std::process::Output {
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "local::durable_tests::durable_child",
                "--nocapture",
            ])
            .env("SAFEMESH_DURABLE_ROOT", root)
            .env("SAFEMESH_DURABLE_KIND", kind)
            .env("SAFEMESH_DURABLE_BOUNDARY", boundary.to_string())
            .output()
            .unwrap();
        // Save and read child exit codes, too; never derive one through a pipe.
        let status = root.join("child.exit");
        fs::write(&status, output.status.code().unwrap().to_string()).unwrap();
        assert_eq!(fs::read_to_string(status).unwrap(), "77", "{:?}", output);
        output
    }
    #[test]
    fn durable_still_works() {
        for kind in ["counter", "set"] {
            let root = root();
            let (_, expected) = exercise(kind, &root, 0);
            // All writer handles are dropped. Reopen the committed transaction
            // and replay fresh state; enabling writes on restart is packet C.
            assert_eq!(read(&root), expected);
            replay(kind, &read(&root));
            let restarted = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "local::durable_tests::durable_child",
                    "--nocapture",
                ])
                .env("SAFEMESH_DURABLE_ROOT", &root)
                .env("SAFEMESH_DURABLE_KIND", kind)
                .env("SAFEMESH_DURABLE_READ", "1")
                .output()
                .unwrap();
            let status = root.join("restart.exit");
            fs::write(&status, restarted.status.code().unwrap().to_string()).unwrap();
            assert_eq!(fs::read_to_string(status).unwrap(), "0", "{restarted:?}");
            std::println!("control=6 {kind} ordinary-write/read/restart=PASS");
        }
    }
    #[test]
    fn durable_crash_boundaries() {
        for kind in ["counter", "set"] {
            let (old, new) = exercise(kind, &root(), 0);
            for boundary in 1..=8 {
                let root = root();
                let output = crash(kind, boundary, &root);
                let recovered = read(&root);
                assert_eq!(&recovered, if boundary < 5 { &old } else { &new });
                assert_eq!(
                    String::from_utf8_lossy(&output.stdout).contains("ACK"),
                    boundary == 8
                );
                std::println!(
                    "control=2 {kind} boundary={boundary}/8 complete={}",
                    if boundary < 5 { "old" } else { "new" }
                );
            }
        }
    }
    fn acknowledged_survives(kind: &str) {
        let root = root();
        let output = crash(kind, 8, &root);
        assert!(String::from_utf8_lossy(&output.stdout).contains("ACK"));
        replay(kind, &read(&root));
        std::println!("control=3 {kind} acknowledged-crash-recovery=PASS");
    }
    #[test]
    fn durable_acknowledged_survives_counter() {
        acknowledged_survives("counter");
    }
    #[test]
    fn durable_acknowledged_survives_set() {
        acknowledged_survives("set");
    }
    fn failure<C: Crdt + std::fmt::Debug>(mut r: DurableReplica<C>, delta: C::Delta, boundary: u8)
    where
        C::Delta: OwnedDelta + Clone + PartialEq + WireEncode + WireSchema,
    {
        let state = format!("{:?}", r.state());
        let log = r.log().to_wire_bytes().unwrap();
        let allocation = r.allocation_bytes();
        let ticket = r.ticket();
        fault(boundary, false);
        assert!(matches!(
            r.append(ticket, delta.clone()),
            Err(LocalError::Io(_))
        ));
        fault(0, false);
        assert_eq!(format!("{:?}", r.state()), state);
        assert_eq!(r.log().to_wire_bytes().unwrap(), log);
        assert_eq!(r.allocation_bytes(), allocation);
        assert!(matches!(
            r.append(ticket, delta.clone()),
            Err(LocalError::Refused)
        ));
        assert!(matches!(
            r.receive(
                ticket,
                Record {
                    id: RecordId {
                        replica: 0,
                        sequence: 1
                    },
                    delta
                }
            ),
            Err(LocalError::Refused)
        ));
        assert!(matches!(r.renew(ticket), Err(LocalError::Refused)));
    }
    #[test]
    fn durable_io_failure_disables_writes() {
        for boundary in 2..=6 {
            failure(
                DurableReplica::counter(&root(), config()).unwrap(),
                GCounterDelta {
                    replica: 0,
                    tally: 9,
                },
                boundary,
            );
            failure(
                DurableReplica::utf8_set(&root(), config()).unwrap(),
                OrSetDelta::Add {
                    element: "東京".into(),
                    token: 2,
                },
                boundary,
            );
            std::println!(
                "controls=4,5 write/sync/uncertain-boundary={boundary} error+disabled=PASS"
            );
        }
    }
    #[test]
    fn durable_receive_and_refusal() {
        let root = root();
        let mut r = DurableReplica::counter(&root, config()).unwrap();
        let record = Record {
            id: RecordId {
                replica: 1,
                sequence: 1,
            },
            delta: GCounterDelta {
                replica: 1,
                tally: 7,
            },
        };
        assert_eq!(
            r.receive(r.ticket(), record.clone()).unwrap(),
            Admission::Accepted
        );
        let transaction = read(&root);
        assert_eq!(transaction.last_sequence, 0);
        assert_eq!(transaction.log_bytes, r.log().to_wire_bytes().unwrap());
        assert_eq!(
            r.receive(r.ticket(), record.clone()).unwrap(),
            Admission::Duplicate
        );
        let mut collision = record;
        collision.delta.tally = 8;
        assert_eq!(
            r.receive(r.ticket(), collision).unwrap(),
            Admission::Collision
        );
        assert!(matches!(
            r.append(
                r.ticket(),
                GCounterDelta {
                    replica: 1,
                    tally: 10
                }
            ),
            Err(LocalError::Refused)
        ));
        assert_eq!(read(&root), transaction);
        let old = r.ticket();
        let ticket = r.renew(old).unwrap();
        assert!(matches!(r.bump(old, 9), Err(LocalError::Refused)));
        r.bump(ticket, 9).unwrap();
        assert_eq!(read(&root).last_sequence, 1);
        drop(r);
        assert!(matches!(
            DurableReplica::counter(&root, config()),
            Err(LocalError::RecoveryRequired)
        ));
    }
}
