// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0
//! Linux/local-filesystem packet-A adapter. All writers for a replica set must
//! use the same directory and fixed configuration. Keep fence files in place.
//! LocalReplica acknowledges in memory; DurableReplica commits before acknowledgement
//! and offers checked ordinary restart of an existing committed store.
use crate::{
    ownership::*, Admission, Crdt, EventLog, GCounter, GCounterDelta, OrSet, OrSetDelta, Record,
    RecordId, WireDecode, WireEncode, WireError, WireSchema,
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
    InvalidRecord(WireError),
    History(WireError),
    InvalidHistory,
    Io(io::Error),
    /// A `_with_limits` restart found a committed history that declares more
    /// records than [`DecodeLimits::max_records`](crate::DecodeLimits). Checked
    /// from the frame header before any record is read; the store is unchanged.
    RecordLimitExceeded {
        max_records: usize,
    },
}

impl core::fmt::Display for LocalError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Refused => {
                f.write_str("local writer operation refused by ownership or lease checks")
            }
            Self::Exhausted => {
                f.write_str("local writer sequence, generation, or token allocation exhausted")
            }
            Self::RecoveryRequired => f.write_str("local store requires recovery"),
            Self::Configuration => f.write_str("invalid or mismatched local writer configuration"),
            Self::InvalidRecord(error) => error.fmt(f),
            Self::History(error) => write!(f, "local history wire validation failed: {error}"),
            Self::InvalidHistory => {
                f.write_str("local history failed replay or sequence validation")
            }
            Self::Io(error) => write!(f, "local store I/O failed: {error}"),
            Self::RecordLimitExceeded { max_records } => write!(
                f,
                "local history exceeds restart record budget: RecordLimitExceeded: {max_records}"
            ),
        }
    }
}

impl core::error::Error for LocalError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::InvalidRecord(error) | Self::History(error) => Some(error),
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
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

// A private tentative insertion. Only version metadata (bounded by configured
// writers), not historical payloads or the identity index, is copied. Restore on
// errors and unwinding; persistence still sees the exact candidate EventLog.
struct PendingInsertion<'a, D> {
    log: &'a mut EventLog<D>,
    id: RecordId,
    len: usize,
    replica_count: Option<usize>,
    shape_bound: bool,
    version: Option<crate::VersionVector>,
}
impl<'a, D> PendingInsertion<'a, D> {
    fn new(log: &'a mut EventLog<D>, id: RecordId) -> Self {
        Self {
            id,
            len: log.records.len(),
            replica_count: log.replica_count,
            shape_bound: log.shape_bound,
            version: Some(log.version.clone()),
            log,
        }
    }
    fn accept(mut self) {
        self.version = None;
    }
}
impl<D> Drop for PendingInsertion<'_, D> {
    fn drop(&mut self) {
        if let Some(version) = self.version.take() {
            self.log.seen.remove(&self.id);
            self.log.records.truncate(self.len);
            self.log.replica_count = self.replica_count;
            self.log.shape_bound = self.shape_bound;
            self.log.version = version;
        }
    }
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
        // Preserve lease and ownership precedence, then surface the carrier cause
        // for a received OR-Set sequence-zero add or remove.
        if !local
            && record.id.sequence == 0
            && self.held
            && ticket.0 == self.generation
            && self.generation > 0
            && record.id.replica < self.config.writers
        {
            if let Err(
                error @ (WireError::ZeroSequenceAdd { .. } | WireError::ZeroSequenceRemove { .. }),
            ) = self.state.validate_record(record.id, &record.delta)
            {
                return Err(LocalError::InvalidRecord(error));
            }
        }
        if refuses(
            self.context(ticket, local),
            record.id,
            record.delta.owned_payload(),
        ) {
            return Err(LocalError::Refused);
        }
        let outcome = self.log.admission(&self.state, &record);
        if outcome != Admission::Accepted {
            return Ok(outcome);
        }
        let candidate = PendingInsertion::new(&mut self.log, record.id);
        let outcome = candidate.log.insert_record(&self.state, record.clone());
        let sequence = if record.id.replica == self.config.writer {
            self.last_sequence.max(record.id.sequence)
        } else {
            self.last_sequence
        };
        // Keep the OS lock but revoke writes before any fallible persistence.
        // An error (including an ambiguous rename/sync) cannot be renewed away.
        self.held = false;
        commit(candidate.log, sequence)?;
        self.state.apply_delta(record.delta);
        candidate.accept();
        self.last_sequence = sequence;
        self.held = true;
        Ok(outcome)
    }
    // Only for the private restart candidate. Failure discards the entire
    // candidate, so replay can admit in place without cloning a growing log.
    fn replay_validated(&mut self, record: Record<C::Delta>) -> Result<(), LocalError> {
        if refuses(
            self.context(self.ticket(), false),
            record.id,
            record.delta.owned_payload(),
        ) {
            return Err(LocalError::Refused);
        }
        let id = record.id;
        let outcome = self
            .log
            .admit_with(&mut self.state, record, |state, delta| {
                state.apply_delta(delta.clone())
            });
        if outcome != Admission::Accepted {
            return Err(LocalError::InvalidHistory);
        }
        if id.replica == self.config.writer {
            self.last_sequence = self.last_sequence.max(id.sequence);
        }
        Ok(())
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

// Read at most `len` leading bytes; a shorter file yields all of its bytes.
fn read_prefix(path: &Path, len: u64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?.take(len).read_to_end(&mut bytes)?;
    Ok(bytes)
}

// The record count a committed transaction declares, from a bounded prefix: the
// 24-byte allocation header, then the current EventLog frame's tag, length pair,
// shape header and count. Neither the frame CRC nor any record is read, so a
// corrupted count can be refused by budget before the full read would report it.
// `None` means the prefix is not a current frame for this writer and delta
// schema; the full checked read and decode then name the failure.
fn declared_records<D: WireSchema>(
    path: &Path,
    config: WriterConfig,
) -> Result<Option<usize>, LocalError> {
    let schema = D::wire_schema();
    let len = 24 + 1 + 8 + 4 + 4 + schema.len() + 1 + 8 + 4;
    let bytes = read_prefix(path, len as u64)?;
    let mut cursor = crate::WireCursor::new(&bytes);
    let mut parse = || -> Result<Option<usize>, WireError> {
        if cursor.read_u64()? != config.writers || cursor.read_u64()? != config.writer {
            return Ok(None);
        }
        cursor.read_u64()?;
        if cursor.read_u8()? != crate::codec::TAG_EVENT_LOG {
            return Ok(None);
        }
        let body = cursor.read_u32()?;
        if cursor.read_u32()? != !body || cursor.read_u32()? != u32::MAX {
            return Ok(None);
        }
        let schema_len = cursor.read_len()?;
        if cursor.read_exact(schema_len)? != schema.as_ref() {
            return Ok(None);
        }
        match cursor.read_u8()? {
            0 if !D::REQUIRES_ARITY => {}
            1 => {
                cursor.read_u64()?;
            }
            _ => return Ok(None),
        }
        cursor.read_len().map(Some)
    };
    Ok(parse().unwrap_or(None))
}

/// Largest durable history, in records, that SafeMesh supports. Histories are
/// retained forever and every durable append rewrites the whole transaction, so
/// the append cost grows with this size; `evidence/retention/results.md` records
/// the benchmark that sets it. Pass it as [`DecodeLimits::max_records`](crate::DecodeLimits)
/// to a `_with_limits` restart to refuse larger stores by name.
pub const SUPPORTED_MAX_RECORDS: usize = 100_000;

/// Additive durable API for Linux local filesystems. Fresh constructors create
/// a missing root; the root must then remain in place. All writers use the same
/// fixed root and configuration. Each Accepted/Ok(record) follows full
/// transaction replacement and file + directory sync. Any persistence error
/// permanently disables this instance's writes, retaining its fence until drop.
/// Use the explicit restart
/// constructors for existing stores; fresh constructors never reset a store.
pub struct DurableReplica<C: Crdt> {
    inner: LocalReplica<C>,
    path: PathBuf,
}

// A failed restart has no LocalReplica to unlock its fence. Release the lock
// before closing the file: a concurrent fork may briefly retain a duplicate
// of the open file description, even when the descriptor is close-on-exec.
struct RestartFence(Option<File>);

impl RestartFence {
    fn file(&mut self) -> &mut File {
        self.0.as_mut().unwrap()
    }

    fn into_file(mut self) -> File {
        self.0.take().unwrap()
    }
}

impl Drop for RestartFence {
    fn drop(&mut self) {
        if let Some(file) = &self.0 {
            let _ = file.unlock();
        }
    }
}

#[cfg(test)]
std::thread_local! {
    static RETAIN_RESTART_FENCE: std::cell::RefCell<Option<Option<File>>> = const { std::cell::RefCell::new(None) };
}

impl<C: Crdt> DurableReplica<C>
where
    C::Delta: OwnedDelta + Clone + PartialEq + WireEncode + WireSchema,
{
    fn fresh(root: &Path, config: WriterConfig, state: C) -> Result<Self, LocalError> {
        fs::create_dir_all(root)?;
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
impl<C: Crdt> DurableReplica<C>
where
    C::Delta: OwnedDelta + Clone + PartialEq + WireEncode + WireDecode + WireSchema,
{
    fn restart(root: &Path, config: WriterConfig, state: C) -> Result<Self, LocalError> {
        Self::restart_with_limits(root, config, state, crate::DecodeLimits::default())
    }

    // `max_collection_elements: None` keeps the ordinary stored-byte budget below.
    fn restart_with_limits(
        root: &Path,
        config: WriterConfig,
        state: C,
        limits: crate::DecodeLimits,
    ) -> Result<Self, LocalError> {
        config.validate().map_err(|_| LocalError::Configuration)?;
        let root = root.canonicalize()?;
        let fence = Self::lock_restart_fence(&root, config.writer)?;
        Self::restart_locked(&root, fence, config, state, limits)
    }

    fn lock_restart_fence(root: &Path, writer: u64) -> Result<RestartFence, LocalError> {
        // Opening without create is deliberate: missing ownership is not a new store.
        let fence = OpenOptions::new()
            .read(true)
            .write(true)
            .open(root.join(format!("writer-{writer}.fence")))?;
        match fence.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(LocalError::Refused),
            Err(TryLockError::Error(e)) => return Err(e.into()),
        }
        #[cfg(test)]
        RETAIN_RESTART_FENCE.with(|slot| {
            if let Some(duplicate) = slot.borrow_mut().as_mut() {
                *duplicate = Some(fence.try_clone()?);
            }
            Ok::<(), io::Error>(())
        })?;
        Ok(RestartFence(Some(fence)))
    }

    fn restart_locked(
        root: &Path,
        mut fence: RestartFence,
        config: WriterConfig,
        state: C,
        limits: crate::DecodeLimits,
    ) -> Result<Self, LocalError> {
        let mut bytes = Vec::new();
        fence.file().read_to_end(&mut bytes)?;
        if bytes.len() != 24 {
            return Err(LocalError::RecoveryRequired);
        }
        let word = |i| u64::from_le_bytes(bytes[i..i + 8].try_into().unwrap());
        if word(0) != config.writers || word(8) != config.writer {
            return Err(LocalError::Configuration);
        }
        let generation = word(16);
        if generation == 0 {
            return Err(LocalError::RecoveryRequired);
        }
        let max_records = limits.max_records;
        // Refuse an over-budget history from its declared count, before the
        // transaction is read in full or any record is decoded or replayed.
        if let Some(max_records) = max_records {
            let declared = declared_records::<C::Delta>(&transaction_path(root, config), config)?;
            if declared.is_some_and(|records| records > max_records) {
                return Err(LocalError::RecordLimitExceeded { max_records });
            }
        }
        // The lock covers reading, checking and replaying the complete transaction.
        let transaction = CommittedTransaction::read(root, config)?;
        // This is the locally committed transaction, whose bytes are already in
        // memory. Every wire collection element occupies at least one byte, so
        // its length bounds any count without imposing a new writer-lifetime
        // limit on stores created before collection ceilings were introduced.
        // Peer wire decoders retain their independent 4,096-element default.
        let max_elements = limits.max_collection_elements.unwrap_or_else(|| {
            transaction
                .log_bytes
                .len()
                .max(crate::CollectionLimits::WIRE_DEFAULT.max_elements.unwrap())
        });
        let log = EventLog::<C::Delta>::from_wire_bytes_for_with_limits(
            &transaction.log_bytes,
            &state,
            crate::DecodeLimits {
                max_records,
                max_collection_elements: Some(max_elements),
            },
        )
        .map_err(|error| match error {
            crate::DecodeError::Wire(error) => LocalError::History(error),
            crate::DecodeError::RecordLimitExceeded { max_records } => {
                LocalError::RecordLimitExceeded { max_records }
            }
        })?;
        let mut inner = LocalReplica {
            config,
            fence: fence.into_file(),
            held: true,
            generation,
            log: EventLog::for_crdt(&state),
            state,
            last_sequence: 0,
        };
        // Compose packet A's corpus-bound ownedStep with M1 admission/replay.
        // This candidate is private until every record and allocation check passes.
        for record in log.records() {
            inner.replay_validated(record.clone())?;
        }
        if inner.last_sequence != transaction.last_sequence {
            return Err(LocalError::InvalidHistory);
        }
        // A ticket from before restart must not authorize the newly acquired lease.
        inner.renew(inner.ticket())?;
        Ok(Self {
            inner,
            path: transaction_path(root, config),
        })
    }
}

impl DurableReplica<GCounter> {
    /// Reacquire ownership, validate the committed history and replay fresh state.
    /// Any error returns no replica and grants no write ticket.
    pub fn restart_counter(root: &Path, config: WriterConfig) -> Result<Self, LocalError> {
        Self::restart_counter_with_limits(root, config, crate::DecodeLimits::default())
    }
    /// [`restart_counter`](Self::restart_counter) with a restart budget.
    /// `limits.max_records` refuses a history declaring more records with
    /// [`LocalError::RecordLimitExceeded`] before any record is read, leaving
    /// the store unchanged. `max_collection_elements: None` keeps the ordinary
    /// stored-byte collection budget rather than the 4,096 wire default.
    pub fn restart_counter_with_limits(
        root: &Path,
        config: WriterConfig,
        limits: crate::DecodeLimits,
    ) -> Result<Self, LocalError> {
        config.validate().map_err(|_| LocalError::Configuration)?;
        let n = usize::try_from(config.writers).map_err(|_| LocalError::Configuration)?;
        Self::restart_with_limits(root, config, GCounter::new(n), limits)
    }
    /// Reacquire the writer, read its committed writer count, then run checked replay.
    /// The root must already contain a durable counter store for this writer.
    pub fn restart_counter_from_store(root: &Path, writer: u64) -> Result<Self, LocalError> {
        Self::restart_counter_from_store_with_limits(root, writer, crate::DecodeLimits::default())
    }
    /// [`restart_counter_from_store`](Self::restart_counter_from_store) with the
    /// restart budget of [`restart_counter_with_limits`](Self::restart_counter_with_limits).
    pub fn restart_counter_from_store_with_limits(
        root: &Path,
        writer: u64,
        limits: crate::DecodeLimits,
    ) -> Result<Self, LocalError> {
        let root = root.canonicalize()?;
        let fence = Self::lock_restart_fence(&root, writer)?;
        // The writer lock covers the metadata read and the same checked replay
        // used by restart_counter. A missing or truncated transaction is not fresh.
        let bytes = read_prefix(&root.join(format!("writer-{writer}.transaction")), 24)?;
        if bytes.len() < 24 {
            return Err(LocalError::RecoveryRequired);
        }
        let writers = u64::from_le_bytes(bytes[..8].try_into().unwrap());
        let stored_writer = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
        WriterConfig {
            writers,
            writer: stored_writer,
        }
        .validate()
        .map_err(|_| LocalError::Configuration)?;
        let config = WriterConfig { writers, writer };
        if stored_writer != writer {
            return Err(LocalError::Configuration);
        }
        let n = usize::try_from(writers).map_err(|_| LocalError::Configuration)?;
        Self::restart_locked(&root, fence, config, GCounter::new(n), limits)
    }
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
    /// Checked ordinary restart, including tombstones and token allocation.
    /// Budgets stored collection counts from the committed transaction length.
    pub fn restart_utf8_set(root: &Path, config: WriterConfig) -> Result<Self, LocalError> {
        Self::restart(root, config, OrSet::new())
    }
    /// [`restart_utf8_set`](Self::restart_utf8_set) with a restart budget.
    /// `limits.max_records` refuses a history declaring more records with
    /// [`LocalError::RecordLimitExceeded`] before any record is read, leaving
    /// the store unchanged. `max_collection_elements: None` keeps the ordinary
    /// stored-byte collection budget; `Some(n)` matches
    /// [`restart_utf8_set_with_max_collection_elements`](Self::restart_utf8_set_with_max_collection_elements).
    pub fn restart_utf8_set_with_limits(
        root: &Path,
        config: WriterConfig,
        limits: crate::DecodeLimits,
    ) -> Result<Self, LocalError> {
        Self::restart_with_limits(root, config, OrSet::new(), limits)
    }
    /// Restart a stored set with an explicit collection ceiling instead of the
    /// ordinary stored-byte budget.
    pub fn restart_utf8_set_with_max_collection_elements(
        root: &Path,
        config: WriterConfig,
        max_elements: usize,
    ) -> Result<Self, LocalError> {
        Self::restart_with_limits(
            root,
            config,
            OrSet::new(),
            crate::DecodeLimits {
                max_records: None,
                max_collection_elements: Some(max_elements),
            },
        )
    }
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
    use alloc::vec;
    use std::{
        process::Command,
        sync::atomic::{AtomicU64, Ordering},
    };

    // IDs are scoped to a CRDT log; tokens are scoped to its set. Capture
    // allocations at issuance, not received copies (which intentionally repeat).
    fn unique<T: PartialEq>(values: &[T]) -> bool {
        values
            .iter()
            .enumerate()
            .all(|(i, x)| !values[..i].contains(x))
    }
    fn allocations<D>(records: &[Record<D>]) -> Vec<RecordId> {
        records.iter().map(|r| r.id).collect()
    }
    fn tokens(records: &[Record<OrSetDelta<String, u64>>]) -> Vec<u64> {
        records
            .iter()
            .filter_map(|r| match r.delta {
                OrSetDelta::Add { token, .. } => Some(token),
                OrSetDelta::Remove { .. } => None,
            })
            .collect()
    }
    fn check_allocations<T: PartialEq + Clone>(name: &str, before: &[T], after: &[T]) {
        assert!(!before.is_empty() && !after.is_empty());
        let mut all = before.to_vec();
        all.extend_from_slice(after);
        assert!(unique(&all), "{name}: allocation reused");
        assert!(!before.iter().any(|x| after.contains(x)));
        all.push(before[0].clone());
        assert!(!unique(&all), "{name}: planted duplicate escaped collector");
        std::println!(
            "{name} before={} after={} overlap=0 planted-duplicate=detected",
            before.len(),
            after.len()
        );
    }
    fn joined_exchange<C>(
        a: &mut DurableReplica<C>,
        b: &mut DurableReplica<C>,
        empty: impl Fn() -> C,
    ) where
        C: Crdt + PartialEq + std::fmt::Debug,
        C::Delta: OwnedDelta + Clone + PartialEq + WireEncode + WireDecode + WireSchema,
    {
        let ab: Vec<_> = a
            .log()
            .since(b.log().version())
            .iter()
            .map(|r| r.to_wire_bytes().unwrap())
            .collect();
        let ba: Vec<_> = b
            .log()
            .since(a.log().version())
            .iter()
            .map(|r| r.to_wire_bytes().unwrap())
            .collect();
        assert!(!ab.is_empty() && !ba.is_empty());
        // Same two batches, opposite delivery orders, through M1 admission.
        let mut finals = Vec::new();
        for batches in [[&ab, &ba], [&ba, &ab]] {
            let mut state = empty();
            let mut log = EventLog::for_crdt(&state);
            for batch in batches {
                for packet in batch {
                    let record = Record::<C::Delta>::from_wire_bytes(packet).unwrap();
                    assert_eq!(
                        log.admit_with(&mut state, record, |state, d| state.apply_delta(d.clone())),
                        Admission::Accepted
                    );
                }
            }
            finals.push(state);
        }
        assert_eq!(finals[0], finals[1]);
        for (receiver, packets) in [(b, ab), (a, ba)] {
            for packet in packets {
                assert_eq!(
                    receiver
                        .receive(
                            receiver.ticket(),
                            Record::<C::Delta>::from_wire_bytes(&packet).unwrap()
                        )
                        .unwrap(),
                    Admission::Accepted
                );
            }
            assert_eq!(receiver.state(), &finals[0]);
        }
    }
    fn joined_config(writer: u64) -> WriterConfig {
        WriterConfig { writers: 2, writer }
    }
    fn joined_finish(root: &Path, loss: bool) {
        let counter_root = root.join("counter");
        let set_root = root.join("set");
        let before_c = EventLog::<GCounterDelta>::from_wire_bytes_for(
            &fs::read(root.join("counter-issued")).unwrap(),
            &GCounter::new(2),
        )
        .unwrap();
        let before_s = EventLog::<OrSetDelta<String, u64>>::from_wire_bytes_for(
            &fs::read(root.join("set-issued")).unwrap(),
            &OrSet::new(),
        )
        .unwrap();
        assert_eq!(before_c.records().len(), 2);
        assert_eq!(before_s.records().len(), 2);
        let mut a = DurableReplica::restart_counter(&counter_root, joined_config(0)).unwrap();
        let mut sa = DurableReplica::restart_utf8_set(&set_root, joined_config(0)).unwrap();
        if loss {
            assert_eq!(a.state().state(), &[0, 0]);
            assert!(!sa.state().contains(&"café☕".into()));
            assert!(!sa.state().contains(&"東京".into()));
            assert!(a.log().records().is_empty() && sa.log().records().is_empty());
            std::println!(
                "joined LOSS: acknowledged counter=9 and UTF-8 edits absent after process restart"
            );
            return;
        }
        assert_eq!(a.state().state(), &[9, 0]);
        for word in ["café☕", "東京"] {
            assert!(sa.state().contains(&word.into()));
        }
        let mut b = DurableReplica::restart_counter(&counter_root, joined_config(1)).unwrap();
        let mut sb = DurableReplica::restart_utf8_set(&set_root, joined_config(1)).unwrap();
        let after_c = [
            a.bump(a.ticket(), 12).unwrap(),
            b.bump(b.ticket(), 7).unwrap(),
        ];
        let after_s = [
            sa.add(sa.ticket(), "naïve".into()).unwrap(),
            sb.add(sb.ticket(), "γειά".into()).unwrap(),
        ];
        check_allocations(
            "counter IDs",
            &allocations(before_c.records()),
            &allocations(&after_c),
        );
        check_allocations(
            "set IDs",
            &allocations(before_s.records()),
            &allocations(&after_s),
        );
        check_allocations("set tokens", &tokens(before_s.records()), &tokens(&after_s));
        joined_exchange(&mut a, &mut b, || GCounter::new(2));
        joined_exchange(&mut sa, &mut sb, OrSet::new);
        assert_eq!(a.state(), b.state());
        assert_eq!(a.state().state(), &[12, 7]);
        assert_eq!(sa.state(), sb.state());
        for word in ["café☕", "東京", "naïve", "γειά"] {
            assert!(sa.state().contains(&word.into()));
        }
        for r in before_c.records() {
            assert!(a.log().records().contains(r) && b.log().records().contains(r));
        }
        for r in before_s.records() {
            assert!(sa.log().records().contains(r) && sb.log().records().contains(r));
        }
        let state_c = a.state().clone();
        let state_s = sa.state().clone();
        drop((a, b, sa, sb));
        for writer in 0..2 {
            assert_eq!(
                DurableReplica::restart_counter(&counter_root, joined_config(writer))
                    .unwrap()
                    .state(),
                &state_c
            );
            assert_eq!(
                DurableReplica::restart_utf8_set(&set_root, joined_config(writer))
                    .unwrap()
                    .state(),
                &state_s
            );
        }
        std::println!("joined HAPPY: all acknowledged records survive; both exchange orders converge; second restart survives");
    }

    fn joined_start(root: &Path, loss: bool) {
        for name in ["counter", "set"] {
            fs::create_dir_all(root.join(name)).unwrap();
        }
        // Make the newly created store directories durable before edits.
        fs::File::open(root).unwrap().sync_all().unwrap();
        // Both replicas exist before the offline edits; neither exchanges yet.
        let _b = DurableReplica::counter(&root.join("counter"), joined_config(1)).unwrap();
        let _sb = DurableReplica::utf8_set(&root.join("set"), joined_config(1)).unwrap();
        let mut c = DurableReplica::counter(&root.join("counter"), joined_config(0)).unwrap();
        let mut s = DurableReplica::utf8_set(&root.join("set"), joined_config(0)).unwrap();
        if loss {
            // Deliberately bypass only the commit: same owned allocation and
            // admission, falsely acknowledged by this negative-control caller.
            c.inner.bump(c.ticket(), 5).unwrap();
            c.inner.bump(c.ticket(), 9).unwrap();
            s.inner.add(s.ticket(), "café☕".into()).unwrap();
            s.inner.add(s.ticket(), "東京".into()).unwrap();
        } else {
            c.bump(c.ticket(), 5).unwrap();
            c.bump(c.ticket(), 9).unwrap();
            s.add(s.ticket(), "café☕".into()).unwrap();
            s.add(s.ticket(), "東京".into()).unwrap();
        }
        // External audit only: restart never reads these files as recovery data.
        fs::write(
            root.join("counter-issued"),
            c.log().to_wire_bytes().unwrap(),
        )
        .unwrap();
        fs::write(root.join("set-issued"), s.log().to_wire_bytes().unwrap()).unwrap();
        std::println!("joined ACK counter=5,9 set=café☕,東京 loss={loss}");
        std::io::stdout().flush().unwrap();
        // Intentionally bypass destructors: a real process ends after ACK.
        std::process::exit(77);
    }
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
    #[test]
    fn durable_orset_zero_sequence_remove_refused_on_receive_and_restart() {
        let root = root();
        let mut replica = DurableReplica::utf8_set(&root, config()).unwrap();
        let remove = Record {
            id: RecordId {
                replica: 1,
                sequence: 0,
            },
            delta: OrSetDelta::Remove { tokens: vec![2] },
        };
        let before = fs::read(transaction_path(&root, config())).unwrap();
        let log_before = replica.log().to_wire_bytes().unwrap();
        assert!(matches!(
            replica.receive(replica.ticket(), remove.clone()),
            Err(LocalError::InvalidRecord(WireError::ZeroSequenceRemove {
                replica: 1
            }))
        ));
        assert_eq!(replica.log().to_wire_bytes().unwrap(), log_before);
        assert!(replica.state().tombstones().is_empty());
        assert_eq!(fs::read(transaction_path(&root, config())).unwrap(), before);
        drop(replica);

        let mut bytes = Vec::new();
        EventLog::encode_records(None, &[remove], &mut bytes).unwrap();
        let inert = EventLog::<OrSetDelta<String, u64>>::from_wire_bytes(&bytes).unwrap();
        DurableReplica::<OrSet<String, u64>>::commit(
            &transaction_path(&root, config()),
            config(),
            &inert,
            0,
        )
        .unwrap();
        let stored = fs::read(transaction_path(&root, config())).unwrap();
        let error = match DurableReplica::restart_utf8_set(&root, config()) {
            Err(error) => error,
            Ok(_) => panic!("restarted from sequence-0 remove"),
        };
        assert!(matches!(
            error,
            LocalError::History(WireError::ZeroSequenceRemove { replica: 1 })
        ));
        assert!(error.to_string().contains("Recovery: "));
        assert_eq!(fs::read(transaction_path(&root, config())).unwrap(), stored);
    }
    #[test]
    fn durable_receive_zero_sequence_add_reports_cause_without_writing() {
        let root = root();
        let mut replica = DurableReplica::utf8_set(&root, config()).unwrap();
        let transaction = transaction_path(&root, config());
        let store_before = fs::read(&transaction).unwrap();
        let log_before = replica.log().to_wire_bytes().unwrap();
        let error = replica
            .receive(
                replica.ticket(),
                Record {
                    id: RecordId {
                        replica: 1,
                        sequence: 0,
                    },
                    delta: OrSetDelta::Add {
                        element: "water".into(),
                        token: 1,
                    },
                },
            )
            .unwrap_err();
        assert!(matches!(
            error,
            LocalError::InvalidRecord(WireError::ZeroSequenceAdd { replica: 1 })
        ));
        assert_eq!(
            error.to_string(),
            WireError::ZeroSequenceAdd { replica: 1 }.to_string()
        );
        assert_eq!(fs::read(&transaction).unwrap(), store_before);
        assert_eq!(replica.log().to_wire_bytes().unwrap(), log_before);
        assert!(replica.state().elements().is_empty());
        drop(replica);

        let add: Record<OrSetDelta<String, u64>> = Record {
            id: RecordId {
                replica: 1,
                sequence: 0,
            },
            delta: OrSetDelta::Add {
                element: "water".into(),
                token: 1u64,
            },
        };
        let mut bytes = Vec::new();
        EventLog::encode_records(None, &[add], &mut bytes).unwrap();
        let inert = EventLog::<OrSetDelta<String, u64>>::from_wire_bytes(&bytes).unwrap();
        DurableReplica::<OrSet<String, u64>>::commit(&transaction, config(), &inert, 0).unwrap();
        let stored = fs::read(&transaction).unwrap();
        let error = match DurableReplica::restart_utf8_set(&root, config()) {
            Err(error) => error,
            Ok(_) => panic!("restarted from sequence-0 add"),
        };
        assert!(matches!(
            error,
            LocalError::History(WireError::ZeroSequenceAdd { replica: 1 })
        ));
        assert_eq!(fs::read(&transaction).unwrap(), stored);
        std::println!(
            "store={} log-bytes={}",
            transaction.display(),
            log_before.len()
        );
    }
    #[test]
    fn receive_other_refusals_keep_ownership_text() {
        let expected = LocalError::Refused.to_string();
        let mut set = DurableReplica::utf8_set(&root(), config()).unwrap();
        let remove = set
            .receive(
                set.ticket(),
                Record {
                    id: RecordId {
                        replica: 1,
                        sequence: 0,
                    },
                    delta: OrSetDelta::Remove {
                        tokens: alloc::vec![1],
                    },
                },
            )
            .unwrap_err();
        assert!(matches!(
            remove,
            LocalError::InvalidRecord(WireError::ZeroSequenceRemove { replica: 1 })
        ));
        assert_eq!(
            remove.to_string(),
            WireError::ZeroSequenceRemove { replica: 1 }.to_string()
        );
        let outside = set
            .receive(
                set.ticket(),
                Record {
                    id: RecordId {
                        replica: 2,
                        sequence: 1,
                    },
                    delta: OrSetDelta::Add {
                        element: "water".into(),
                        token: 4,
                    },
                },
            )
            .unwrap_err();
        assert!(matches!(outside, LocalError::Refused));
        assert_eq!(outside.to_string(), expected);
        let outside_zero = set
            .receive(
                set.ticket(),
                Record {
                    id: RecordId {
                        replica: 2,
                        sequence: 0,
                    },
                    delta: OrSetDelta::Add {
                        element: "water".into(),
                        token: 2,
                    },
                },
            )
            .unwrap_err();
        assert!(matches!(outside_zero, LocalError::Refused));
        assert_eq!(outside_zero.to_string(), expected);
        let stale = set.ticket();
        set.renew(stale).unwrap();
        let stale_zero = set
            .receive(
                stale,
                Record {
                    id: RecordId {
                        replica: 1,
                        sequence: 0,
                    },
                    delta: OrSetDelta::Add {
                        element: "water".into(),
                        token: 1,
                    },
                },
            )
            .unwrap_err();
        assert!(matches!(stale_zero, LocalError::Refused));
        assert_eq!(stale_zero.to_string(), expected);

        let mut counter = DurableReplica::counter(&root(), config()).unwrap();
        let gcounter = counter
            .receive(
                counter.ticket(),
                Record {
                    id: RecordId {
                        replica: 1,
                        sequence: 0,
                    },
                    delta: GCounterDelta {
                        replica: 1,
                        tally: 7,
                    },
                },
            )
            .unwrap_err();
        assert!(matches!(gcounter, LocalError::Refused));
        assert_eq!(gcounter.to_string(), expected);

        let mut pn = DurableReplica::fresh(&root(), config(), crate::PnCounter::new(2)).unwrap();
        let pncounter = pn
            .receive(
                pn.ticket(),
                Record {
                    id: RecordId {
                        replica: 1,
                        sequence: 0,
                    },
                    delta: crate::PnCounterDelta::Inc {
                        replica: 1,
                        tally: 7,
                    },
                },
            )
            .unwrap_err();
        assert!(matches!(pncounter, LocalError::Refused));
        assert_eq!(pncounter.to_string(), expected);
    }
    #[test]
    fn restart_counter_from_store_replays_without_writer_count() {
        let root = root();
        let mut replica = DurableReplica::counter(&root, config()).unwrap();
        replica.bump(replica.ticket(), 5).unwrap();
        drop(replica);

        let replica = DurableReplica::restart_counter_from_store(&root, 0).unwrap();
        assert_eq!(replica.state().value(), 5);
        assert_eq!(replica.allocation_bytes()[..8], 2u64.to_le_bytes());
    }
    #[test]
    fn restart_counter_from_store_rejects_missing_and_invalid_metadata() {
        let root = root();
        assert!(matches!(
            DurableReplica::restart_counter_from_store(&root, 0),
            Err(LocalError::Io(_))
        ));
        let replica = DurableReplica::counter(&root, config()).unwrap();
        drop(replica);
        let path = transaction_path(&root, config());
        let original = fs::read(&path).unwrap();

        fs::remove_file(&path).unwrap();
        assert!(matches!(
            DurableReplica::restart_counter_from_store(&root, 0),
            Err(LocalError::Io(_))
        ));
        fs::write(&path, &original[..8]).unwrap();
        assert!(matches!(
            DurableReplica::restart_counter_from_store(&root, 0),
            Err(LocalError::RecoveryRequired)
        ));

        let mut invalid = original.clone();
        invalid[..8].copy_from_slice(&1u64.to_le_bytes());
        invalid[8..16].copy_from_slice(&1u64.to_le_bytes());
        fs::write(&path, invalid).unwrap();
        assert!(matches!(
            DurableReplica::restart_counter_from_store(&root, 0),
            Err(LocalError::Configuration)
        ));
        fs::write(&path, original).unwrap();
        assert!(DurableReplica::restart_counter_from_store(&root, 0).is_ok());
    }
    #[test]
    fn restart_counter_from_store_checks_writer_and_live_lease() {
        let root = root();
        let replica = DurableReplica::counter(&root, config()).unwrap();
        assert!(matches!(
            DurableReplica::restart_counter_from_store(&root, 0),
            Err(LocalError::Refused)
        ));
        drop(replica);

        fs::copy(root.join("writer-0.fence"), root.join("writer-1.fence")).unwrap();
        fs::copy(
            root.join("writer-0.transaction"),
            root.join("writer-1.transaction"),
        )
        .unwrap();
        assert!(matches!(
            DurableReplica::restart_counter_from_store(&root, 1),
            Err(LocalError::Configuration)
        ));
        assert!(DurableReplica::restart_counter_from_store(&root, 0).is_ok());
    }
    #[test]
    fn restart_counter_from_archived_store() {
        // These committed files were generated by the archived 3a9179b library.
        let root = root();
        fs::write(
            root.join("writer-0.fence"),
            include_bytes!("../tests/fixtures/bootstrap/counter.fence"),
        )
        .unwrap();
        fs::write(
            root.join("writer-0.transaction"),
            include_bytes!("../tests/fixtures/bootstrap/counter.transaction"),
        )
        .unwrap();
        let replica = DurableReplica::restart_counter_from_store(&root, 0).unwrap();
        assert_eq!(replica.state().state(), &[5, 7]);
    }
    #[test]
    fn fresh_durable_counter_creates_missing_root() {
        let store = root().join("fresh-counter");
        assert!(!store.exists());
        let replica = DurableReplica::counter(&store, config()).unwrap();
        assert!(store.is_dir());
        assert_eq!(replica.state().value(), 0);
    }
    #[test]
    fn fresh_durable_set_creates_missing_root() {
        let store = root().join("fresh-set");
        assert!(!store.exists());
        let replica = DurableReplica::utf8_set(&store, config()).unwrap();
        assert!(store.is_dir());
        assert!(replica.state().elements().is_empty());
    }
    #[test]
    fn fresh_durable_root_inputs() {
        let parent = root();
        let nested = parent.join("missing-parent/store");
        assert!(DurableReplica::counter(&nested, config()).is_ok());
        assert!(nested.is_dir());

        let file = parent.join("regular-file");
        fs::write(&file, b"file").unwrap();
        assert!(matches!(
            DurableReplica::counter(&file, config()),
            Err(LocalError::Io(_))
        ));

        let dangling = parent.join("dangling-link");
        std::os::unix::fs::symlink(parent.join("missing-target"), &dangling).unwrap();
        assert!(matches!(
            DurableReplica::counter(&dangling, config()),
            Err(LocalError::Io(_))
        ));
        assert!(!parent.join("missing-target").exists());

        let missing = parent.join("restart-missing");
        assert!(matches!(
            DurableReplica::restart_counter(&missing, config()),
            Err(LocalError::Io(_))
        ));
        assert!(matches!(
            DurableReplica::restart_utf8_set(&missing, config()),
            Err(LocalError::Io(_))
        ));
        assert!(!missing.exists());
    }
    #[test]
    fn fresh_durable_race_and_live_writer() {
        use std::sync::{Arc, Barrier};

        let store = root().join("raced-store");
        let start = Arc::new(Barrier::new(2));
        let finish = Arc::new(Barrier::new(2));
        let workers: Vec<_> = (0..2)
            .map(|_| {
                let store = store.clone();
                let start = start.clone();
                let finish = finish.clone();
                std::thread::spawn(move || {
                    start.wait();
                    let result = DurableReplica::counter(&store, config());
                    let outcome = match &result {
                        Ok(_) => "ok",
                        Err(LocalError::RecoveryRequired) => "recovery",
                        Err(LocalError::Refused) => "refused",
                        Err(_) => "unexpected",
                    };
                    finish.wait();
                    outcome
                })
            })
            .collect();
        let outcomes: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        assert_eq!(
            outcomes.iter().filter(|&&outcome| outcome == "ok").count(),
            1
        );
        assert!(outcomes.iter().all(|outcome| *outcome != "unexpected"));

        let live_root = root().join("live");
        let _live = DurableReplica::counter(&live_root, config()).unwrap();
        assert!(matches!(
            DurableReplica::counter(&live_root, config()),
            Err(LocalError::RecoveryRequired)
        ));
    }
    #[test]
    fn fresh_durable_read_only_parent() {
        use std::os::unix::fs::PermissionsExt;

        let parent = root();
        let original = fs::metadata(&parent).unwrap().permissions();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o555)).unwrap();
        let result = DurableReplica::counter(&parent.join("store"), config());
        fs::set_permissions(&parent, original).unwrap();
        assert!(
            matches!(result, Err(LocalError::Io(ref error)) if error.kind() == io::ErrorKind::PermissionDenied)
        );
        assert!(!parent.join("store").exists());
    }
    // Seed a committed store of `records` records in one transaction: allocate
    // and admit in memory through the owned writer, then commit once. Growing
    // it by durable appends would rewrite the whole history per record.
    fn seeded<C: Crdt>(
        mut replica: DurableReplica<C>,
        records: usize,
        mut edit: impl FnMut(&mut LocalReplica<C>, u64),
    ) -> DurableReplica<C>
    where
        C::Delta: OwnedDelta + Clone + PartialEq + WireEncode + WireSchema,
    {
        for index in 0..records as u64 {
            edit(&mut replica.inner, index);
        }
        let inner = &replica.inner;
        DurableReplica::<C>::commit(&replica.path, inner.config, &inner.log, inner.last_sequence)
            .unwrap();
        replica
    }
    fn store_files(root: &Path) -> Vec<(PathBuf, Vec<u8>, std::time::SystemTime)> {
        let mut files: Vec<_> = fs::read_dir(root)
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                let modified = fs::metadata(&path).unwrap().modified().unwrap();
                (path.clone(), fs::read(&path).unwrap(), modified)
            })
            .collect();
        files.sort();
        files
    }
    fn budget(max_records: usize) -> crate::DecodeLimits {
        crate::DecodeLimits {
            max_records: Some(max_records),
            max_collection_elements: None,
        }
    }
    fn assert_refused_by_name<C: Crdt>(
        result: Result<DurableReplica<C>, LocalError>,
        max_records: usize,
    ) {
        match result {
            Err(error @ LocalError::RecordLimitExceeded { max_records: named }) => {
                assert_eq!(named, max_records);
                assert_eq!(
                    error.to_string(),
                    format!(
                        "local history exceeds restart record budget: RecordLimitExceeded: {max_records}"
                    )
                );
            }
            Err(other) => panic!("expected RecordLimitExceeded, got {other:?}"),
            Ok(replica) => panic!(
                "store of {} records opened under a {max_records}-record budget",
                replica.inner.log.records().len()
            ),
        }
    }
    #[test]
    fn restart_budget_opens_supported_size_and_refuses_one_more() {
        const N: usize = SUPPORTED_MAX_RECORDS;
        // G-Counter: a store of N opens; one more durable bump makes N + 1.
        let root = self::root();
        let fresh = DurableReplica::counter(&root, config()).unwrap();
        drop(seeded(fresh, N, |inner, index| {
            inner.bump(inner.ticket(), index + 1).unwrap();
        }));
        let mut counter =
            DurableReplica::restart_counter_with_limits(&root, config(), budget(N)).unwrap();
        assert_eq!(counter.log().records().len(), N);
        counter.bump(counter.ticket(), N as u64 + 1).unwrap();
        drop(counter);
        let before = store_files(&root);
        assert_refused_by_name(
            DurableReplica::restart_counter_with_limits(&root, config(), budget(N)),
            N,
        );
        assert_refused_by_name(
            DurableReplica::restart_counter_from_store_with_limits(&root, 0, budget(N)),
            N,
        );
        assert_eq!(
            store_files(&root),
            before,
            "refusal leaves the store untouched"
        );
        // Keep forever: the budget is the caller's; without it the store opens.
        let reopened = DurableReplica::restart_counter(&root, config()).unwrap();
        assert_eq!(reopened.log().records().len(), N + 1);
        assert_eq!(reopened.state().value(), N as u128 + 1);
        drop(reopened);
        let reopened = DurableReplica::restart_counter_from_store(&root, 0).unwrap();
        assert_eq!(reopened.log().records().len(), N + 1);

        // UTF-8 OR-Set: the same boundary through one more durable add.
        let root = self::root();
        let fresh = DurableReplica::utf8_set(&root, config()).unwrap();
        drop(seeded(fresh, N, |inner, index| {
            inner.add(inner.ticket(), format!("m{index}")).unwrap();
        }));
        let mut set =
            DurableReplica::restart_utf8_set_with_limits(&root, config(), budget(N)).unwrap();
        assert_eq!(set.log().records().len(), N);
        set.add(set.ticket(), "one more".into()).unwrap();
        drop(set);
        let before = store_files(&root);
        assert_refused_by_name(
            DurableReplica::restart_utf8_set_with_limits(&root, config(), budget(N)),
            N,
        );
        assert_eq!(
            store_files(&root),
            before,
            "refusal leaves the store untouched"
        );
        let reopened = DurableReplica::restart_utf8_set(&root, config()).unwrap();
        assert_eq!(reopened.log().records().len(), N + 1);
    }
    #[test]
    fn restart_budget_refuses_before_reading_records() {
        let root = self::root();
        let mut counter = DurableReplica::counter(&root, config()).unwrap();
        for tally in 1..=3 {
            counter.bump(counter.ticket(), tally).unwrap();
        }
        drop(counter);
        // Keep the header through the declared count (3); drop every record and
        // the CRC. Only a refusal from the declared count can name the budget.
        let path = transaction_path(&root, config());
        let bytes = fs::read(&path).unwrap();
        let schema = <GCounterDelta as WireSchema>::wire_schema().len();
        let header = 24 + 1 + 8 + 4 + 4 + schema + 1 + 8 + 4;
        assert_eq!(bytes[header - 4..header], 3u32.to_le_bytes());
        fs::write(&path, &bytes[..header]).unwrap();
        let before = store_files(&root);
        assert_refused_by_name(
            DurableReplica::restart_counter_with_limits(&root, config(), budget(2)),
            2,
        );
        assert_eq!(store_files(&root), before);
        // Within budget, the full checked read reaches the missing records.
        assert!(matches!(
            DurableReplica::restart_counter_with_limits(&root, config(), budget(3)),
            Err(LocalError::History(WireError::UnexpectedEof))
        ));
        fs::write(&path, &bytes).unwrap();
        assert_eq!(
            DurableReplica::restart_counter_with_limits(&root, config(), budget(3))
                .unwrap()
                .state()
                .value(),
            3
        );
    }
    #[test]
    fn oversized_stored_orset_restarts_ordinary() {
        let root = self::root();
        let mut replica = DurableReplica::utf8_set(&root, config()).unwrap();
        let element = "bulk".to_string();
        for _ in 0..4097 {
            replica.add(replica.ticket(), element.clone()).unwrap();
        }
        assert_eq!(replica.state().observed_tokens(&element).len(), 4097);
        replica.remove(replica.ticket(), &element).unwrap();
        drop(replica);
        let mut restored = DurableReplica::restart_utf8_set(&root, config()).unwrap();
        assert_eq!(restored.state().observed_tokens(&element).len(), 0);
        assert_eq!(restored.state().tombstones().len(), 4097);
        assert_eq!(
            restored
                .receive(
                    restored.ticket(),
                    Record {
                        id: RecordId {
                            replica: 1,
                            sequence: 1,
                        },
                        delta: OrSetDelta::Add {
                            element: "peer".into(),
                            token: 3,
                        },
                    },
                )
                .unwrap(),
            Admission::Accepted
        );
        drop(restored);
        let restored = DurableReplica::restart_utf8_set(&root, config()).unwrap();
        assert_eq!(restored.state().tombstones().len(), 4097);
        assert!(restored.state().contains(&"peer".to_string()));
    }
    #[test]
    fn duplicate_and_replay_do_not_clone_history() {
        use std::{cell::Cell, rc::Rc};

        struct Delta(u64, Rc<Cell<usize>>);
        impl Clone for Delta {
            fn clone(&self) -> Self {
                self.1.set(self.1.get() + 1);
                Self(self.0, self.1.clone())
            }
        }
        impl PartialEq for Delta {
            fn eq(&self, other: &Self) -> bool {
                self.0 == other.0
            }
        }
        impl OwnedDelta for Delta {
            fn owned_payload(&self) -> OwnedPayload {
                OwnedPayload::Remove
            }
        }
        struct State(u64);
        impl crate::Mergeable for State {
            fn merge(&mut self, other: &Self) -> Result<(), crate::MergeError> {
                self.0 = self.0.max(other.0);
                Ok(())
            }
        }
        impl Crdt for State {
            type Delta = Delta;
            fn validate_record(&self, _: RecordId, _: &Delta) -> Result<(), WireError> {
                Ok(())
            }
            fn apply_delta(&mut self, delta: Delta) {
                self.0 = self.0.max(delta.0);
            }
        }

        let clones = Rc::new(Cell::new(0));
        let record = |sequence, value| Record {
            id: RecordId {
                replica: 0,
                sequence,
            },
            delta: Delta(value, clones.clone()),
        };
        let mut r = LocalReplica::fresh(&root(), config(), State(0)).unwrap();
        for sequence in 1..=128 {
            r.replay_validated(record(sequence, sequence)).unwrap();
        }
        // Only the current delta is copied for application, never the prefix.
        assert_eq!(clones.get(), 128);
        assert_eq!(r.last_sequence, 128);
        assert_eq!(r.state().0, 128);
        clones.set(0);
        for (value, expected) in [(128, Admission::Duplicate), (999, Admission::Collision)] {
            assert_eq!(
                r.admit_committed(r.ticket(), record(128, value), false, |_, _| {
                    panic!("non-admission must not commit")
                })
                .unwrap(),
                expected
            );
            assert!(matches!(
                r.replay_validated(record(128, value)),
                Err(LocalError::InvalidHistory)
            ));
        }
        assert_eq!(clones.get(), 0);
        assert_eq!(r.log().records().len(), 128);
        assert_eq!(r.last_sequence, 128);
        assert_eq!(r.state().0, 128);
        assert_eq!(
            r.admit_committed(r.ticket(), record(129, 129), false, |log, sequence| {
                assert_eq!(log.records().len(), 129);
                assert_eq!(sequence, 129);
                Ok(())
            })
            .unwrap(),
            Admission::Accepted
        );
        assert_eq!(clones.get(), 1, "accepted writes must not clone history");
        let before = r.log.version().clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = r.admit_committed(r.ticket(), record(130, 130), false, |_, _| {
                panic!("commit unwind")
            });
        }));
        assert!(result.is_err());
        assert_eq!(r.log.records().len(), 129);
        assert_eq!(r.log.version(), &before);
        assert_eq!(r.state().0, 129);
        assert_eq!(r.last_sequence, 129);
        assert!(!r.held);
        assert_eq!(
            r.log.admission(&r.state, &record(130, 130)),
            Admission::Accepted
        );
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
    fn restart_still_works_and_fresh_allocation() {
        let root = root();
        let mut r = DurableReplica::counter(&root, config()).unwrap();
        let before = r.bump(r.ticket(), 5).unwrap();
        r.receive(
            r.ticket(),
            Record {
                id: RecordId {
                    replica: 1,
                    sequence: 40,
                },
                delta: GCounterDelta {
                    replica: 1,
                    tally: 7,
                },
            },
        )
        .unwrap();
        let old = r.ticket();
        let state = r.state().state().to_vec();
        let log = r.log().to_wire_bytes().unwrap();
        let allocation = r.allocation_bytes();
        drop(r);
        let mut r = DurableReplica::restart_counter(&root, config()).unwrap();
        assert_eq!(r.state().state(), state);
        assert_eq!(r.log().to_wire_bytes().unwrap(), log);
        assert_eq!(r.allocation_bytes(), allocation);
        assert!(matches!(r.bump(old, 9), Err(LocalError::Refused)));
        let after = r.bump(r.ticket(), 9).unwrap();
        assert_eq!(
            before.id,
            RecordId {
                replica: 0,
                sequence: 1
            }
        );
        assert_eq!(
            after.id,
            RecordId {
                replica: 0,
                sequence: 2
            }
        );
        assert_eq!(r.state().state(), &[9, 7]);
        std::println!(
            "controls=7,2 counter replay={state:?} writable=true IDs {:?} -> {:?}",
            before.id,
            after.id
        );
        drop(r);
        assert_eq!(
            DurableReplica::restart_counter(&root, config())
                .unwrap()
                .state()
                .state(),
            &[9, 7]
        );

        let root = self::root();
        let mut r = DurableReplica::utf8_set(&root, config()).unwrap();
        let before = r.add(r.ticket(), "café☕".into()).unwrap();
        let removed = r.remove(r.ticket(), &"café☕".into()).unwrap();
        r.receive(
            r.ticket(),
            Record {
                id: RecordId {
                    replica: 1,
                    sequence: 40,
                },
                delta: OrSetDelta::Add {
                    element: "東京".into(),
                    token: 81,
                },
            },
        )
        .unwrap();
        let state = r.state().clone();
        let log = r.log().to_wire_bytes().unwrap();
        let allocation = r.allocation_bytes();
        let old = r.ticket();
        drop(r);
        let mut r = DurableReplica::restart_utf8_set(&root, config()).unwrap();
        assert_eq!(r.state(), &state);
        assert_eq!(r.log().to_wire_bytes().unwrap(), log);
        assert_eq!(r.allocation_bytes(), allocation);
        assert!(matches!(
            r.add(old, "stale".into()),
            Err(LocalError::Refused)
        ));
        let after = r.add(r.ticket(), "café☕".into()).unwrap();
        assert_eq!(
            before.id,
            RecordId {
                replica: 0,
                sequence: 1
            }
        );
        assert_eq!(
            removed.id,
            RecordId {
                replica: 0,
                sequence: 2
            }
        );
        assert_eq!(
            after.id,
            RecordId {
                replica: 0,
                sequence: 3
            }
        );
        assert!(matches!(before.delta, OrSetDelta::Add { token: 2, .. }));
        assert!(matches!(after.delta, OrSetDelta::Add { token: 6, .. }));
        assert!(r.state().contains(&"café☕".into()));
        assert!(r.state().contains(&"東京".into()));
        assert!(r.state().tombstones().contains(&2));
        std::println!("controls=7,2 set replay=equal including tombstone 2 and remote token 81; writable=true IDs {:?}, {:?} -> {:?}; tokens 2 -> 6", before.id, removed.id, after.id);
        drop(r);
        let r = DurableReplica::restart_utf8_set(&root, config()).unwrap();
        assert!(r.state().contains(&"café☕".into()));
        assert!(r.state().tombstones().contains(&2));
    }
    fn initialized(kind: &str) -> PathBuf {
        let root = root();
        if kind == "counter" {
            let mut r = DurableReplica::counter(&root, config()).unwrap();
            r.bump(r.ticket(), 5).unwrap();
        } else {
            let mut r = DurableReplica::utf8_set(&root, config()).unwrap();
            r.add(r.ticket(), "café☕".into()).unwrap();
        }
        root
    }
    // Success proves the returned replica can commit a fresh edit.
    fn restart_and_write(kind: &str, root: &Path, config: WriterConfig) -> Result<(), LocalError> {
        if kind == "counter" {
            let mut r = DurableReplica::restart_counter(root, config)?;
            r.bump(r.ticket(), 9)?;
        } else {
            let mut r = DurableReplica::restart_utf8_set(root, config)?;
            r.add(r.ticket(), "new".into())?;
        }
        Ok(())
    }
    #[test]
    fn restart_corrupt_history() {
        for kind in ["counter", "set"] {
            let root = initialized(kind);
            let path = transaction_path(&root, config());
            let mut bytes = fs::read(&path).unwrap();
            // Damage the checksum; the well-formed payload permits revert control 8.
            *bytes.last_mut().unwrap() ^= 1;
            fs::write(&path, &bytes).unwrap();
            let result = restart_and_write(kind, &root, config());
            std::println!(
                "control=3 {kind} result={result:?} writable={}",
                result.is_ok()
            );
            assert!(matches!(
                result,
                Err(LocalError::History(WireError::IntegrityMismatch))
            ));
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
    #[test]
    fn restart_mismatched_history() {
        for kind in ["counter", "set"] {
            let root = initialized(kind);
            let path = transaction_path(&root, config());
            let original = fs::read(&path).unwrap();
            let mut bytes = original.clone();
            bytes[..8].copy_from_slice(&3u64.to_le_bytes());
            fs::write(&path, &bytes).unwrap();
            let result = restart_and_write(kind, &root, config());
            std::println!(
                "control=4 {kind} result={result:?} writable={}",
                result.is_ok()
            );
            assert!(matches!(result, Err(LocalError::Configuration)));
            assert_eq!(fs::read(&path).unwrap(), bytes);
            fs::write(&path, original).unwrap();
            let result = restart_and_write(
                kind,
                &root,
                WriterConfig {
                    writers: 3,
                    writer: 0,
                },
            );
            std::println!("control=4 {kind} mismatched writer result={result:?}");
            assert!(
                matches!(result, Err(LocalError::Configuration)),
                "{kind}: mismatched writer restart returned {result:?}"
            );
        }
        let root = initialized("counter");
        assert!(matches!(
            DurableReplica::restart_utf8_set(&root, config()),
            Err(LocalError::History(WireError::DeltaTypeMismatch))
        ));
    }
    #[test]
    fn restart_invalid_history() {
        for kind in ["counter", "set"] {
            let root = initialized(kind);
            let path = transaction_path(&root, config());
            // Serialize with the product: valid framing, invalid ownership.
            if kind == "counter" {
                let record = Record {
                    id: RecordId {
                        replica: 0,
                        sequence: 1,
                    },
                    delta: GCounterDelta {
                        replica: 1,
                        tally: 5,
                    },
                };
                let mut log = EventLog::for_crdt(&GCounter::new(2));
                assert_eq!(
                    log.insert_record(&GCounter::new(2), record.clone()),
                    Admission::Invalid(WireError::OwnershipViolation)
                );
                let mut bytes = Vec::new();
                EventLog::encode_records(Some(2), &[record], &mut bytes).unwrap();
                let log = EventLog::from_wire_bytes(&bytes).unwrap();
                DurableReplica::<GCounter>::commit(&path, config(), &log, 1).unwrap();
            } else {
                let mut log = EventLog::for_crdt(&OrSet::<String, u64>::new());
                assert_eq!(
                    log.insert_record(
                        &OrSet::<String, u64>::new(),
                        Record {
                            id: RecordId {
                                replica: 0,
                                sequence: 1
                            },
                            delta: OrSetDelta::Add {
                                element: "foreign".into(),
                                token: 3
                            },
                        }
                    ),
                    Admission::Accepted
                );
                DurableReplica::<OrSet<String, u64>>::commit(&path, config(), &log, 1).unwrap();
            }
            let bytes = fs::read(&path).unwrap();
            let result = restart_and_write(kind, &root, config());
            std::println!(
                "control=5 {kind} result={result:?} writable={}",
                result.is_ok()
            );
            assert!(matches!(
                result,
                Err(LocalError::Refused) | Err(LocalError::History(_))
            ));
            assert_eq!(fs::read(&path).unwrap(), bytes);
            let root = initialized(kind);
            let path = transaction_path(&root, config());
            let original = fs::read(&path).unwrap();
            for last in [0u64, 2, u64::MAX] {
                let mut bytes = original.clone();
                bytes[16..24].copy_from_slice(&last.to_le_bytes());
                fs::write(&path, &bytes).unwrap();
                assert!(matches!(
                    restart_and_write(kind, &root, config()),
                    Err(LocalError::InvalidHistory)
                ));
                assert_eq!(fs::read(&path).unwrap(), bytes);
            }
        }
    }
    #[test]
    fn restart_unavailable_ownership() {
        let root = root();
        let r = DurableReplica::counter(&root, config()).unwrap();
        assert!(matches!(
            DurableReplica::restart_counter(&root, config()),
            Err(LocalError::Refused)
        ));
        drop(r);
        restart_and_write("counter", &root, config()).unwrap();
        let root = self::root();
        let r = DurableReplica::utf8_set(&root, config()).unwrap();
        assert!(matches!(
            DurableReplica::restart_utf8_set(&root, config()),
            Err(LocalError::Refused)
        ));
        drop(r);
        restart_and_write("set", &root, config()).unwrap();
        std::println!("control=6 both primitives: held fence Refused, no replica; released fence and empty histories: writable");
    }

    #[test]
    fn failed_restart_unlocks_with_duplicated_fence_handle() {
        let root = initialized("counter");
        let transaction = transaction_path(&root, config());
        let withheld = root.join("withheld-transaction");
        fs::rename(&transaction, &withheld).unwrap();

        RETAIN_RESTART_FENCE.with(|slot| *slot.borrow_mut() = Some(None));
        let first = DurableReplica::restart_counter(&root, config());
        let duplicate =
            RETAIN_RESTART_FENCE.with(|slot| slot.borrow_mut().take().unwrap().unwrap());
        assert!(matches!(first, Err(LocalError::Io(_))));
        assert!(!transaction.exists());

        let second = DurableReplica::restart_counter(&root, config());
        assert!(
            matches!(second, Err(LocalError::Io(_))),
            "next restart was Refused"
        );
        drop(duplicate);
        fs::rename(&withheld, &transaction).unwrap();
        restart_and_write("counter", &root, config()).unwrap();
    }

    #[test]
    fn durable_child() {
        if let Ok(root) = std::env::var("SAFEMESH_JOINED_ROOT") {
            joined_start(
                Path::new(&root),
                std::env::var_os("SAFEMESH_JOINED_LOSS").is_some(),
            );
        }
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
        for loss in [false, true] {
            let root = root();
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "local::durable_tests::durable_child",
                    "--nocapture",
                ])
                .env("SAFEMESH_JOINED_ROOT", &root);
            if loss {
                command.env("SAFEMESH_JOINED_LOSS", "1");
            }
            let output = command.output().unwrap();
            fs::write(
                root.join("joined.exit"),
                output.status.code().unwrap().to_string(),
            )
            .unwrap();
            assert_eq!(fs::read_to_string(root.join("joined.exit")).unwrap(), "77");
            assert!(String::from_utf8_lossy(&output.stdout).contains("joined ACK"));
            joined_finish(&root, loss);
        }

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
    fn failure<C: Crdt + std::fmt::Debug>(
        mut r: DurableReplica<C>,
        delta: C::Delta,
        boundary: u8,
        prior: Option<C::Delta>,
    ) where
        C::Delta: OwnedDelta + Clone + PartialEq + WireEncode + WireSchema,
    {
        if let Some(prior) = prior {
            r.append(r.ticket(), prior).unwrap();
        }
        let version = r.log().version().clone();
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
        assert_eq!(r.log().version(), &version);
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
        for (boundary, populated) in
            (2..=6).flat_map(|boundary| [false, true].map(|populated| (boundary, populated)))
        {
            failure(
                DurableReplica::counter(&root(), config()).unwrap(),
                GCounterDelta {
                    replica: 0,
                    tally: 9,
                },
                boundary,
                populated.then_some(GCounterDelta {
                    replica: 0,
                    tally: 1,
                }),
            );
            failure(
                DurableReplica::utf8_set(&root(), config()).unwrap(),
                OrSetDelta::Add {
                    element: "東京".into(),
                    token: if populated { 4 } else { 2 },
                },
                boundary,
                populated.then_some(OrSetDelta::Add {
                    element: "prior".into(),
                    token: 2,
                }),
            );
            std::println!(
                "controls=4,5 write/sync/uncertain-boundary={boundary} error+disabled=PASS"
            );
        }
        // Closing a remote gap can advance over several existing records.
        // Failure must restore that prefix as well as the identity index.
        for prior in [&[][..], &[2, 3][..]] {
            let mut r = LocalReplica::counter(&root(), config()).unwrap();
            let remote = |sequence| Record {
                id: RecordId {
                    replica: 1,
                    sequence,
                },
                delta: GCounterDelta {
                    replica: 1,
                    tally: sequence,
                },
            };
            for &sequence in prior {
                assert_eq!(
                    r.receive(r.ticket(), remote(sequence)).unwrap(),
                    Admission::Accepted
                );
            }
            let before = r.log().clone();
            let state = r.state().clone();
            assert!(matches!(
                r.admit_committed(r.ticket(), remote(1), false, |log, sequence| {
                    assert_eq!(log.version().get(1), prior.last().copied().unwrap_or(1));
                    assert_eq!(sequence, 0);
                    Err(LocalError::Io(io::Error::other("commit failed")))
                }),
                Err(LocalError::Io(_))
            ));
            assert_eq!(r.log(), &before);
            assert_eq!(r.state(), &state);
            assert_eq!(r.last_sequence, 0);
            assert!(!r.held);
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
