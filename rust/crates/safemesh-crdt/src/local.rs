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

/// Errors from local writer and durable store operations.
///
/// Downstream callers must allow for future variants:
///
/// ```compile_fail,E0004
/// use safemesh_crdt::local::LocalError;
/// fn classify(error: LocalError) -> &'static str {
///     match error {
///         LocalError::Refused => "refused",
///         LocalError::Exhausted => "exhausted",
///         LocalError::PeerWriterAhead => "writer ahead",
///         LocalError::RecoveryRequired => "recovery",
///         LocalError::Configuration => "configuration",
///         LocalError::CounterWidth(_) => "counter width",
///         LocalError::InvalidRecord(_) => "record",
///         LocalError::History(_) => "history",
///         LocalError::InvalidHistory => "invalid history",
///         LocalError::Io(_) => "io",
///         LocalError::AncestorSync { .. } => "ancestor sync",
///         LocalError::RecordLimitExceeded { .. } => "record limit",
///     }
/// }
/// ```
#[derive(Debug)]
#[non_exhaustive]
pub enum LocalError {
    Refused,
    Exhausted,
    /// A peer supplied an own-writer record above the committed high-water.
    PeerWriterAhead,
    RecoveryRequired,
    Configuration,
    /// The requested counter width exceeds the core constructor domain.
    CounterWidth(crate::CoordinateError),
    InvalidRecord(WireError),
    History(WireError),
    InvalidHistory,
    Io(io::Error),
    /// First creation could not open or sync an ancestor; no fence is written.
    AncestorSync {
        path: PathBuf,
        source: io::Error,
    },
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
            Self::PeerWriterAhead => f.write_str("a peer returned a record for this writer above its durable high-water; local writes on this open replica are stopped; reopening clears this stop and an old-identity restore is detected only when a peer returns such a record; open a restored store with a new writer identity"),
            Self::RecoveryRequired => f.write_str("local store requires recovery"),
            Self::CounterWidth(error) => error.fmt(f),
            Self::Configuration => f.write_str("invalid or mismatched local writer configuration"),
            Self::InvalidRecord(error) => error.fmt(f),
            Self::History(error) => write!(f, "local history wire validation failed: {error}"),
            Self::InvalidHistory => {
                f.write_str("local history failed replay or sequence validation")
            }
            Self::AncestorSync { path, source } => write!(f, "AncestorSync: cannot sync store ancestor {}: {source}", path.display()),
            Self::Io(error) => write!(f, "local store I/O failed: {error}"),
            Self::RecordLimitExceeded { max_records } => write!(
                f,
                "local history exceeds restart record budget: RecordLimitExceeded: {max_records}; pass DecodeLimits::UNLIMITED to reopen"
            ),
        }
    }
}

impl core::error::Error for LocalError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::InvalidRecord(error) | Self::History(error) => Some(error),
            Self::Io(error) | Self::AncestorSync { source: error, .. } => Some(error),
            Self::CounterWidth(error) => Some(error),
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
    peer_writer_ahead: bool,
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

// Without a fence, existing directories may be leftovers from interrupted mkdir.
// No persistent provenance identifies a safe stopping ancestor, so sync the
// resolved and traversed parent chains through / before creating the fence.
fn prepare_durable_root(root: &Path, config: WriterConfig) -> Result<(), LocalError> {
    if root
        .join(format!("writer-{}.fence", config.writer))
        .try_exists()?
    {
        return Ok(());
    }
    create_durable_root(root)
}
fn create_durable_root(root: &Path) -> Result<(), LocalError> {
    create_durable_root_with(root, |parent| File::open(parent)?.sync_all())
}

fn create_durable_root_with(
    root: &Path,
    sync_parent: impl FnMut(&Path) -> io::Result<()>,
) -> Result<(), LocalError> {
    create_durable_root_using(root, |directory| fs::create_dir(directory), sync_parent)
}

fn create_durable_root_using(
    root: &Path,
    mut create: impl FnMut(&Path) -> io::Result<()>,
    mut sync_parent: impl FnMut(&Path) -> io::Result<()>,
) -> Result<(), LocalError> {
    let root = std::path::absolute(root)?;
    let mut missing = Vec::new();
    let mut ancestor = root.as_path();
    loop {
        match fs::metadata(ancestor) {
            Ok(metadata) if metadata.is_dir() => break,
            Ok(_) => return Err(io::Error::from(io::ErrorKind::NotADirectory).into()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                missing.push(ancestor.to_path_buf());
                ancestor = ancestor
                    .parent()
                    .ok_or_else(|| io::Error::other("no ancestor"))?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    for directory in missing.iter().rev() {
        match create(directory) {
            Ok(()) => {}
            // A concurrent creator may have installed this directory after the walk.
            Err(error)
                if error.kind() == io::ErrorKind::AlreadyExists
                    && fs::metadata(directory)?.is_dir() => {}
            Err(error) => return Err(error.into()),
        }
    }
    let resolved = root.canonicalize()?;
    let mut parents: Vec<_> = resolved
        .ancestors()
        .skip(1)
        .map(Path::to_path_buf)
        .collect();
    // Keep entries needed to traverse symlinks or cancelled `..` components
    // durable too, even if an interrupted earlier call created them.
    for parent in root.ancestors().skip(1) {
        let parent = parent
            .canonicalize()
            .map_err(|source| LocalError::AncestorSync {
                path: parent.to_path_buf(),
                source,
            })?;
        if !parents.contains(&parent) {
            parents.push(parent);
        }
    }
    parents.sort_by_key(|parent| core::cmp::Reverse(parent.components().count()));
    for parent in parents {
        sync_parent(&parent).map_err(|source| LocalError::AncestorSync {
            path: parent,
            source,
        })?;
    }
    Ok(())
}

impl<C: Crdt> LocalReplica<C>
where
    C::Delta: OwnedDelta + Clone + PartialEq,
{
    fn fresh(root: &Path, config: WriterConfig, state: C) -> Result<Self, LocalError> {
        config.validate().map_err(|_| LocalError::Configuration)?;
        prepare_durable_root(root, config)?;
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
            peer_writer_ahead: false,
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
        self.admit_committed(ticket, record, local, |_, _| Ok(()), |_| Ok(()))
    }
    fn admit_committed(
        &mut self,
        ticket: WriteTicket,
        record: Record<C::Delta>,
        local: bool,
        commit: impl FnOnce(&EventLog<C::Delta>, u64) -> Result<(), LocalError>,
        save_alarm: impl FnOnce(&EventLog<C::Delta>) -> Result<(), LocalError>,
    ) -> Result<Admission, LocalError> {
        if local && self.peer_writer_ahead {
            return Err(LocalError::PeerWriterAhead);
        }
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
        // Peer history cannot allocate IDs in this writer's space. A backup
        // may have lost locally issued IDs, so stop this open writer as well.
        if !local
            && record.id.replica == self.config.writer
            && record.id.sequence > self.last_sequence
        {
            self.peer_writer_ahead = true;
            return Err(LocalError::PeerWriterAhead);
        }
        let outcome = self.log.admission(&self.state, &record);
        if outcome != Admission::Accepted {
            let prior = self.log.collisions.len();
            self.log.raise_on_collision(outcome, record);
            if self.log.collisions.len() != prior {
                self.held = false;
                save_alarm(&self.log)?;
                self.held = true;
            }
            return Ok(outcome);
        }
        let candidate = PendingInsertion::new(&mut self.log, record.id);
        let outcome = candidate.log.insert_record(&self.state, record.clone());
        let sequence = if local {
            record.id.sequence
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
        if self.peer_writer_ahead {
            return Err(LocalError::PeerWriterAhead);
        }
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
        Self::fresh(
            root,
            config,
            GCounter::try_new(n).map_err(LocalError::CounterWidth)?,
        )
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
        if self.peer_writer_ahead {
            return Err(LocalError::PeerWriterAhead);
        }
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

#[path = "journal.rs"]
mod journal;
#[path = "rollback.rs"]
mod rollback;

/// One writer's committed history. The suffix is the existing, unchanged log
/// wire encoding; the 24-byte prefix is LocalReplica::allocation_bytes(). This
/// storage container is not a new transport encoding. Reading it grants no
/// write lease and does not implement packet C's checked writable restart.
/// For an append-log store, the journal's base frame and every complete record
/// are reassembled into the frame a whole-history transaction would hold; a
/// torn final record is excluded and the journal is not modified.
#[derive(Debug, PartialEq, Eq)]
pub struct CommittedTransaction {
    pub config: WriterConfig,
    pub last_sequence: u64,
    pub log_bytes: Vec<u8>,
}
impl CommittedTransaction {
    pub fn read(root: &Path, config: WriterConfig) -> Result<Self, LocalError> {
        config.validate().map_err(|_| LocalError::Configuration)?;
        if store_format(root, config.writer)? == Format::Journal {
            let bytes = fs::read(journal::path(root, config.writer))?;
            let parsed = journal::parse(&bytes, config)?;
            let records: Vec<&[u8]> = parsed.entries.iter().map(|&(_, record)| record).collect();
            return Ok(Self {
                config,
                last_sequence: parsed
                    .entries
                    .last()
                    .map_or(parsed.base_sequence, |&(sequence, _)| sequence),
                log_bytes: journal::assemble(parsed.base_frame, &records)
                    .map_err(LocalError::History)?,
            });
        }
        legacy_transaction(root, config)
    }
}
fn transaction_path(root: &Path, config: WriterConfig) -> PathBuf {
    root.join(format!("writer-{}.transaction", config.writer))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    /// `writer-<id>.transaction`, written before the append log: every commit
    /// replaces the whole file.
    Transaction,
    /// `writer-<id>.journal`: every commit appends one record and syncs it.
    Journal,
}

fn exists(path: &Path) -> Result<bool, LocalError> {
    match fs::metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

// A writer has one committed store file. Both files mean an explicit migration
// stopped after its journal was complete; only migration resolves that. With
// neither, the transaction read reports NotFound exactly as before.
fn store_format(root: &Path, writer: u64) -> Result<Format, LocalError> {
    let transaction = exists(&root.join(format!("writer-{writer}.transaction")))?;
    match (transaction, exists(&journal::path(root, writer))?) {
        (true, true) => Err(LocalError::RecoveryRequired),
        (false, true) => Ok(Format::Journal),
        _ => Ok(Format::Transaction),
    }
}

fn encode_error(error: WireError) -> LocalError {
    LocalError::Io(io::Error::other(format!("{error:?}")))
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
    let bytes = read_prefix(path, (24 + frame_prefix::<D>()) as u64)?;
    let mut cursor = crate::WireCursor::new(&bytes);
    let mut parse = || -> Result<Option<usize>, WireError> {
        if cursor.read_u64()? != config.writers || cursor.read_u64()? != config.writer {
            return Ok(None);
        }
        cursor.read_u64()?;
        declared_frame_records::<D>(&mut cursor)
    };
    Ok(parse().unwrap_or(None))
}

// Tag, length pair, shape header and record count of a current EventLog frame.
fn frame_prefix<D: WireSchema>() -> usize {
    1 + 8 + 4 + 4 + D::wire_schema().len() + 1 + 8 + 4
}

fn declared_frame_records<D: WireSchema>(
    cursor: &mut crate::WireCursor<'_>,
) -> Result<Option<usize>, WireError> {
    if cursor.read_u8()? != crate::codec::TAG_EVENT_LOG {
        return Ok(None);
    }
    let body = cursor.read_u32()?;
    if cursor.read_u32()? != !body || cursor.read_u32()? != u32::MAX {
        return Ok(None);
    }
    let schema_len = cursor.read_len()?;
    if cursor.read_exact(schema_len)? != D::wire_schema().as_ref() {
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
}

// The same count for an append-log store: the base frame's declared records
// plus complete journal entries, counted through a fixed buffer by their length
// pairs. Stops once the budget is exceeded; no record is read or decoded.
fn declared_journal_records<D: WireSchema>(
    root: &Path,
    config: WriterConfig,
    max_records: usize,
) -> Result<Option<usize>, LocalError> {
    let file = File::open(journal::path(root, config.writer))?;
    let file_len = file.metadata()?.len();
    let mut reader = io::BufReader::new(file);
    let mut header = [0u8; journal::HEADER];
    if reader.read_exact(&mut header).is_err() {
        return Ok(None);
    }
    let Ok((_, base_len)) = journal::check_header(&header, config) else {
        return Ok(None);
    };
    let prefix_len = frame_prefix::<D>().min(base_len);
    let mut prefix = alloc::vec![0u8; prefix_len];
    if reader.read_exact(&mut prefix).is_err() {
        return Ok(None);
    }
    let Ok(Some(base)) = declared_frame_records::<D>(&mut crate::WireCursor::new(&prefix)) else {
        return Ok(None);
    };
    if base > max_records {
        return Ok(Some(base));
    }
    reader.seek_relative((base_len - prefix_len) as i64)?;
    let entries = journal::count_entries(
        &mut reader,
        (journal::HEADER + base_len) as u64,
        file_len,
        max_records - base + 1,
    )?;
    Ok(Some(base + entries))
}

/// Largest durable history, in records, that SafeMesh supports. Histories are
/// retained forever and restart replays all of them, so restart time and memory
/// grow with this size, as does the append cost of a store not yet migrated to
/// the append log; `evidence/retention/results.md` records the benchmark that
/// sets it. Ordinary restarts use this as their record cap; `_with_limits`
/// accepts a caller budget or [`DecodeLimits::UNLIMITED`](crate::DecodeLimits).
pub const SUPPORTED_MAX_RECORDS: usize = crate::DecodeLimits::DEFAULT_MAX_RECORDS;

/// Additive durable API for Linux local filesystems. Fresh constructors create
/// a missing root; the root must then remain in place. All writers use the same
/// fixed root and configuration. Fresh stores keep an append-only
/// `writer-<id>.journal`: each Accepted/Ok(record) follows appending that one
/// record and syncing the file. Stores written before the append log keep
/// `writer-<id>.transaction`, which restart still opens and each commit still
/// replaces whole, with file + directory sync, until an explicit
/// `migrate_*_to_append_log` call; restart never migrates. Any persistence
/// error permanently disables this instance's writes, retaining its fence until
/// drop. Use the explicit restart constructors for existing stores; fresh
/// constructors never reset a store.
pub struct DurableReplica<C: Crdt> {
    inner: LocalReplica<C>,
    store: Store,
    root: PathBuf,
    torn_tail: Option<TornTail>,
    rollback: Option<rollback::Counter>,
}

enum Store {
    Transaction(PathBuf),
    /// The journal and the length of its complete entries, where the next
    /// entry is written.
    Journal {
        file: File,
        len: u64,
    },
}

/// Bytes after the last complete record of an append-log store that restart
/// discarded: an entry cut short, zero fill, or a final entry whose checksum
/// fails. Such an append was interrupted before its file sync, so it was never
/// acknowledged. Restart truncates the journal to `offset` and syncs it. A
/// damaged entry followed by other data is not a torn tail; restart refuses it
/// as `History(IntegrityMismatch)` and leaves the journal unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct TornTail {
    /// Journal length after truncation: the end of the last complete record.
    pub offset: u64,
    /// Bytes removed from the end of the journal.
    pub discarded_bytes: u64,
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
        prepare_durable_root(root, config)?;
        let root = root.canonicalize()?;
        // Never overwrite a history whose fence is missing, in either format.
        let path = journal::path(&root, config.writer);
        if exists(&transaction_path(&root, config))? || exists(&path)? {
            return Err(LocalError::RecoveryRequired);
        }
        let mut inner = LocalReplica::fresh(&root, config, state)?;
        if !inner.held {
            return Err(LocalError::Refused);
        }
        inner.held = false;
        // The header and empty base frame appear whole or not at all.
        let header = journal::header(config, 0, &inner.log.to_wire_bytes().map_err(encode_error)?)?;
        persistence::replace(&path, &header)?;
        let file = OpenOptions::new().read(true).write(true).open(&path)?;
        let rollback = rollback::Counter::create(&root, config)?;
        inner.held = true;
        Ok(Self {
            inner,
            root,
            store: Store::Journal {
                file,
                len: header.len() as u64,
            },
            torn_tail: None,
            rollback: Some(rollback),
        })
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
        bytes.extend(log.to_wire_bytes().map_err(encode_error)?);
        persistence::replace(path, &bytes)?;
        Ok(())
    }
    // Append the candidate's new last record at the end of the complete entries,
    // then sync. The entry becomes part of `len` only after the sync returns.
    fn append_entry(
        file: &File,
        len: &mut u64,
        log: &EventLog<C::Delta>,
        sequence: u64,
    ) -> Result<(), LocalError> {
        use std::os::unix::fs::FileExt;
        let record = log.records().last().ok_or(LocalError::InvalidHistory)?;
        let bytes = journal::entry(sequence, &record.to_wire_bytes().map_err(encode_error)?)?;
        persistence::checkpoint(11)?;
        // Leave half an entry on disk, as an interrupted write would.
        #[cfg(test)]
        if persistence::FAULT.with(|fault| fault.get().0) == 12 {
            file.write_all_at(&bytes[..bytes.len() / 2], *len)?;
        }
        persistence::checkpoint(12)?;
        file.write_all_at(&bytes, *len)?;
        persistence::checkpoint(13)?;
        // fdatasync also persists the file length needed to read the entry back.
        file.sync_data()?;
        persistence::checkpoint(14)?;
        *len += bytes.len() as u64;
        Ok(())
    }
    /// The torn tail this instance's restart discarded from an append-log
    /// store, if any. `None` for a fresh store, a transaction-format store and
    /// a journal that ended on a complete record.
    pub fn torn_tail(&self) -> Option<TornTail> {
        self.torn_tail
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
        let store = &mut self.store;
        let rollback = &mut self.rollback;
        let config = self.inner.config;
        let alarm_path = self.root.join(format!("writer-{}.alarms", config.writer));
        let outcome = self.inner.admit_committed(
            ticket,
            record,
            local,
            |log, sequence| {
                if local {
                    if let Some(counter) = rollback {
                        counter.advance(sequence)?;
                    }
                }
                match store {
                    Store::Transaction(path) => Self::commit(path, config, log, sequence),
                    Store::Journal { file, len } => Self::append_entry(file, len, log, sequence),
                }
            },
            |log| {
                let bytes = log
                    .collision_report_bytes()
                    .map_err(encode_error)?
                    .ok_or(LocalError::InvalidHistory)?;
                persistence::replace(&alarm_path, &bytes)?;
                Ok(())
            },
        )?;
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
        if self.inner.peer_writer_ahead {
            return Err(LocalError::PeerWriterAhead);
        }
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
        let format = store_format(root, config.writer)?;
        // Refuse an over-budget history from its declared count, before the
        // history is read in full or any record is decoded or replayed.
        if let Some(max_records) = max_records {
            let declared = match format {
                Format::Transaction => {
                    declared_records::<C::Delta>(&transaction_path(root, config), config)?
                }
                Format::Journal => declared_journal_records::<C::Delta>(root, config, max_records)?,
            };
            if declared.is_some_and(|records| records > max_records) {
                return Err(LocalError::RecordLimitExceeded { max_records });
            }
        }
        let decode_error = |error| match error {
            crate::DecodeError::Wire(error) => LocalError::History(error),
            crate::DecodeError::RecordLimitExceeded { max_records } => {
                LocalError::RecordLimitExceeded { max_records }
            }
        };
        // The history's bytes are already in memory. Every wire collection
        // element occupies at least one byte, so their length bounds any count
        // without imposing a new writer-lifetime limit on stores created before
        // collection ceilings were introduced. Peer wire decoders retain their
        // independent 4,096-element default.
        let max_elements = |stored: usize| {
            limits.max_collection_elements.unwrap_or_else(|| {
                stored.max(crate::CollectionLimits::WIRE_DEFAULT.max_elements.unwrap())
            })
        };
        // The lock covers reading, checking and replaying the complete history.
        let (base, base_sequence, entries, store, torn_tail) = match format {
            Format::Transaction => {
                let transaction = CommittedTransaction::read(root, config)?;
                let log = EventLog::<C::Delta>::from_wire_bytes_for_with_limits(
                    &transaction.log_bytes,
                    &state,
                    crate::DecodeLimits {
                        max_records,
                        max_collection_elements: Some(max_elements(transaction.log_bytes.len())),
                    },
                )
                .map_err(decode_error)?;
                let store = Store::Transaction(transaction_path(root, config));
                (log, transaction.last_sequence, Vec::new(), store, None)
            }
            Format::Journal => {
                let mut file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(journal::path(root, config.writer))?;
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes)?;
                let parsed = journal::parse(&bytes, config)?;
                let max_elements = max_elements(bytes.len());
                let log = EventLog::<C::Delta>::from_wire_bytes_for_with_limits(
                    parsed.base_frame,
                    &state,
                    crate::DecodeLimits {
                        max_records,
                        max_collection_elements: Some(max_elements),
                    },
                )
                .map_err(decode_error)?;
                if let Some(max_records) = max_records {
                    if log.records().len() + parsed.entries.len() > max_records {
                        return Err(LocalError::RecordLimitExceeded { max_records });
                    }
                }
                let entries = parsed
                    .entries
                    .iter()
                    .map(|&(sequence, bytes)| {
                        Record::<C::Delta>::from_wire_bytes_with_collection_limits(
                            bytes,
                            crate::CollectionLimits {
                                max_elements: Some(max_elements),
                            },
                        )
                        .map(|record| (sequence, record))
                        .map_err(LocalError::History)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let torn_tail = parsed.tail.map(|tail| TornTail {
                    offset: tail.offset,
                    discarded_bytes: tail.bytes,
                });
                let store = Store::Journal {
                    file,
                    len: parsed.valid_len,
                };
                (log, parsed.base_sequence, entries, store, torn_tail)
            }
        };
        let mut inner = LocalReplica {
            config,
            fence: fence.into_file(),
            held: true,
            generation,
            log: EventLog::for_crdt(&state),
            state,
            last_sequence: 0,
            peer_writer_ahead: false,
        };
        // Compose packet A's corpus-bound ownedStep with M1 admission/replay.
        // This candidate is private until every record and allocation check passes.
        for record in base.records() {
            inner.replay_validated(record.clone())?;
        }
        if inner.last_sequence != base_sequence {
            return Err(LocalError::InvalidHistory);
        }
        drop(base);
        for (sequence, record) in entries {
            inner.replay_validated(record)?;
            if inner.last_sequence != sequence {
                return Err(LocalError::InvalidHistory);
            }
        }
        let rollback = rollback::Counter::open(root, config, inner.last_sequence)?;
        // The alarm file is a complete collision-report frame. A damaged
        // frame is refused; it is never treated as a journal torn tail.
        let alarm_path = root.join(format!("writer-{}.alarms", config.writer));
        match fs::read(alarm_path) {
            Ok(bytes) => {
                let report =
                    crate::CollisionReport::<C::Delta>::from_wire_bytes_with_limits(&bytes, limits)
                        .map_err(decode_error)?;
                for (id, verdict) in inner.log.merge_collision_report(report) {
                    if verdict != crate::CollisionVerdict::Recorded {
                        let _ = id;
                        return Err(LocalError::InvalidHistory);
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        // Only a restart that validated every complete record drops the torn
        // tail, so the next entry is written where the last complete one ends.
        if let (Some(tail), Store::Journal { file, .. }) = (torn_tail, &store) {
            file.set_len(tail.offset)?;
            file.sync_all()?;
        }
        // A ticket from before restart must not authorize the newly acquired lease.
        inner.renew(inner.ticket())?;
        Ok(Self {
            inner,
            root: root.to_path_buf(),
            store,
            torn_tail,
            rollback,
        })
    }

    // Explicit, one-way move of a transaction-format store to the append log.
    // Its complete history becomes the journal's base frame. A crash before the
    // journal is in place leaves the transaction store; after it, both files
    // exist, which ordinary restart refuses and a repeated migration completes.
    fn migrate(root: &Path, config: WriterConfig, state: C) -> Result<Self, LocalError> {
        config.validate().map_err(|_| LocalError::Configuration)?;
        let root = root.canonicalize()?;
        let fence = Self::lock_restart_fence(&root, config.writer)?;
        let transaction = transaction_path(&root, config);
        let path = journal::path(&root, config.writer);
        if exists(&transaction)? && exists(&path)? {
            // The journal must hold exactly the transaction's history with
            // nothing appended, as an interrupted migration leaves it.
            let committed = legacy_transaction(&root, config)?;
            let bytes = fs::read(&path)?;
            let parsed = journal::parse(&bytes, config)?;
            let limits = crate::DecodeLimits {
                // Recovery comparison must inspect both complete stored histories.
                max_records: crate::DecodeLimits::UNLIMITED.max_records,
                max_collection_elements: Some(bytes.len().max(4096)),
            };
            let decode = |frame| {
                EventLog::<C::Delta>::from_wire_bytes_for_with_limits(frame, &state, limits)
                    .map_err(|_| LocalError::RecoveryRequired)
            };
            if !parsed.entries.is_empty()
                || parsed.tail.is_some()
                || parsed.base_sequence != committed.last_sequence
                || decode(parsed.base_frame)? != decode(&committed.log_bytes)?
            {
                return Err(LocalError::RecoveryRequired);
            }
            fs::remove_file(&transaction)?;
            File::open(&root)?.sync_all()?;
        }
        let mut replica = Self::restart_locked(&root, fence, config, state, Default::default())?;
        if let Store::Transaction(_) = replica.store {
            let header = journal::header(
                config,
                replica.inner.last_sequence,
                &replica.inner.log.to_wire_bytes().map_err(encode_error)?,
            )?;
            persistence::replace(&path, &header)?;
            persistence::checkpoint(15)?;
            fs::remove_file(&transaction)?;
            File::open(&root)?.sync_all()?;
            let file = OpenOptions::new().read(true).write(true).open(&path)?;
            replica.store = Store::Journal {
                file,
                len: header.len() as u64,
            };
        }
        Ok(replica)
    }
}

fn legacy_transaction(
    root: &Path,
    config: WriterConfig,
) -> Result<CommittedTransaction, LocalError> {
    let bytes = fs::read(transaction_path(root, config))?;
    if bytes.len() < 24 {
        return Err(LocalError::RecoveryRequired);
    }
    let word = |i| u64::from_le_bytes(bytes[i..i + 8].try_into().unwrap());
    if word(0) != config.writers || word(8) != config.writer {
        return Err(LocalError::Configuration);
    }
    Ok(CommittedTransaction {
        config,
        last_sequence: word(16),
        log_bytes: bytes[24..].to_vec(),
    })
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
        Self::restart_with_limits(
            root,
            config,
            GCounter::try_new(n).map_err(LocalError::CounterWidth)?,
            limits,
        )
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
        let (bytes, at) = match store_format(&root, writer)? {
            Format::Transaction => (
                read_prefix(&root.join(format!("writer-{writer}.transaction")), 24)?,
                0,
            ),
            Format::Journal => {
                let bytes = read_prefix(&journal::path(&root, writer), journal::HEADER as u64)?;
                if bytes.len() < journal::HEADER || bytes[..8] != journal::MAGIC {
                    return Err(LocalError::RecoveryRequired);
                }
                (bytes, 8)
            }
        };
        if bytes.len() < at + 24 {
            return Err(LocalError::RecoveryRequired);
        }
        let writers = u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
        let stored_writer = u64::from_le_bytes(bytes[at + 8..at + 16].try_into().unwrap());
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
        Self::restart_locked(
            &root,
            fence,
            config,
            GCounter::try_new(n).map_err(LocalError::CounterWidth)?,
            limits,
        )
    }
    /// Explicitly move a counter store written before the append log
    /// (`writer-<id>.transaction`) to the append log (`writer-<id>.journal`),
    /// after the same checked replay as [`restart_counter`](Self::restart_counter),
    /// and return it ready to write. Every record is kept. Restart never
    /// migrates. If a crash leaves both files, restart refuses with
    /// `RecoveryRequired` and calling this again completes the move. An
    /// append-log store is restarted unchanged.
    pub fn migrate_counter_to_append_log(
        root: &Path,
        config: WriterConfig,
    ) -> Result<Self, LocalError> {
        config.validate().map_err(|_| LocalError::Configuration)?;
        let n = usize::try_from(config.writers).map_err(|_| LocalError::Configuration)?;
        Self::migrate(root, config, GCounter::new(n))
    }
    pub fn counter(root: &Path, config: WriterConfig) -> Result<Self, LocalError> {
        config.validate().map_err(|_| LocalError::Configuration)?;
        let n = usize::try_from(config.writers).map_err(|_| LocalError::Configuration)?;
        Self::fresh(
            root,
            config,
            GCounter::try_new(n).map_err(LocalError::CounterWidth)?,
        )
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
                max_records: crate::DecodeLimits::default().max_records,
                max_collection_elements: Some(max_elements),
            },
        )
    }
    pub fn utf8_set(root: &Path, config: WriterConfig) -> Result<Self, LocalError> {
        Self::fresh(root, config, OrSet::new())
    }
    /// Explicitly move a UTF-8 OR-Set store written before the append log to
    /// the append log, as
    /// [`migrate_counter_to_append_log`](DurableReplica::migrate_counter_to_append_log)
    /// does for a counter, after the same checked replay as
    /// [`restart_utf8_set`](Self::restart_utf8_set).
    pub fn migrate_utf8_set_to_append_log(
        root: &Path,
        config: WriterConfig,
    ) -> Result<Self, LocalError> {
        Self::migrate(root, config, OrSet::new())
    }
    pub fn add(
        &mut self,
        ticket: WriteTicket,
        element: String,
    ) -> Result<Record<OrSetDelta<String, u64>>, LocalError> {
        if self.inner.peer_writer_ahead {
            return Err(LocalError::PeerWriterAhead);
        }
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
        root.canonicalize().unwrap()
    }
    #[test]
    fn fresh_root_syncs_exact_created_parents() {
        let existing = root();
        let nested = existing.join("ancestor/store");
        let mut synced = Vec::new();
        create_durable_root_with(&nested, |parent| {
            synced.push(parent.to_path_buf());
            Ok(())
        })
        .unwrap();
        let expected: Vec<_> = nested
            .canonicalize()
            .unwrap()
            .ancestors()
            .skip(1)
            .map(Path::to_path_buf)
            .collect();
        assert_eq!(synced, expected);
        synced.clear();
        create_durable_root_with(&nested, |parent| {
            synced.push(parent.to_path_buf());
            Ok(())
        })
        .unwrap();
        assert_eq!(synced, expected);
        let failed = existing.join("failed/store");
        let error = create_durable_root_with(&failed, |_| {
            Err(io::Error::other("injected parent sync failure"))
        })
        .unwrap_err();
        assert!(
            matches!(error, LocalError::AncestorSync { path, source } if path == failed.parent().unwrap() && source.to_string() == "injected parent sync failure")
        );
        assert!(failed.is_dir());
    }
    #[test]
    fn fresh_root_syncs_lexical_symlink_parent() {
        let base = root();
        fs::create_dir(base.join("a")).unwrap();
        fs::create_dir_all(base.join("b/real")).unwrap();
        std::os::unix::fs::symlink(base.join("b/real"), base.join("a/link")).unwrap();
        let store = base.join("a/link/store");
        let mut synced = Vec::new();
        create_durable_root_with(&store, |parent| {
            synced.push(parent.to_path_buf());
            Ok(())
        })
        .unwrap();

        // Canonical chain: b/real, b, base, and every ancestor of base.
        // Lexical chain adds a: it holds the link, but is not in the canonical chain.
        let mut expected = vec![
            base.join("b/real"),
            base.join("b"),
            base.join("a"),
            base.clone(),
        ];
        expected.extend(base.ancestors().skip(1).map(Path::to_path_buf));
        assert_eq!(synced, expected);
        fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn fresh_root_syncs_leftover_parent_chain() {
        let existing = root();
        let leftover = existing.join("interrupted/store");
        // Simulate an earlier call dying after mkdir, without any parent fsync.
        fs::create_dir_all(&leftover).unwrap();
        let mut synced = Vec::new();
        create_durable_root_with(&leftover, |parent| {
            synced.push(parent.to_path_buf());
            Ok(())
        })
        .unwrap();
        let expected: Vec<_> = leftover
            .canonicalize()
            .unwrap()
            .ancestors()
            .skip(1)
            .map(Path::to_path_buf)
            .collect();
        assert_eq!(synced, expected);
        assert_eq!(synced[0], existing.join("interrupted"));
        assert_eq!(synced[1], existing);
        let replica = DurableReplica::counter(&leftover, config()).unwrap();
        assert!(leftover.join("writer-0.fence").is_file());
        drop(replica);
    }

    #[test]
    fn fresh_root_syncs_cancelled_traversal_parents() {
        let existing = root();
        let cancelled = existing.join("interrupted/child");
        fs::create_dir_all(&cancelled).unwrap();
        let store = existing.join("interrupted/child/../../store");
        fs::create_dir(existing.join("store")).unwrap();
        let mut synced = Vec::new();
        create_durable_root_with(&store, |parent| {
            synced.push(parent.to_path_buf());
            Ok(())
        })
        .unwrap();
        assert!(synced.contains(&existing.join("interrupted")));
        assert!(synced.contains(&existing));
        assert_eq!(synced.last().unwrap(), Path::new("/"));
        let replica = DurableReplica::counter(&store, config()).unwrap();
        drop(replica);
    }

    #[test]
    fn fresh_root_syncs_concurrent_creator_parents() {
        let existing = root();
        let nested = existing.join("concurrent/store");
        let mut synced = Vec::new();
        let mut races = 0;
        create_durable_root_using(
            &nested,
            |directory| {
                fs::create_dir(directory)?;
                races += 1;
                Err(io::Error::from(io::ErrorKind::AlreadyExists))
            },
            |parent| {
                synced.push(parent.to_path_buf());
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(races, 2);
        let expected: Vec<_> = nested
            .canonicalize()
            .unwrap()
            .ancestors()
            .skip(1)
            .map(Path::to_path_buf)
            .collect();
        assert_eq!(synced, expected);
    }

    fn config() -> WriterConfig {
        WriterConfig {
            writers: 2,
            writer: 0,
        }
    }
    // Recreate the pre-protection store layout for tests of its old restart
    // behavior. The new protected-store cases have separate tamper tests.
    fn pre_protection<C: Crdt>(replica: &mut DurableReplica<C>) {
        let (marker, counter) =
            rollback::paths(&replica.root, replica.inner.config.writer).unwrap();
        fs::remove_file(marker).unwrap();
        fs::remove_file(counter).unwrap();
        replica.rollback = None;
    }
    // An empty store in the format main wrote before the append log: the same
    // fence and a whole-history `writer-0.transaction` from the unchanged
    // `commit` that main's fresh constructors called, with no journal.
    fn legacy_at(kind: &str, root: &Path) {
        let path = transaction_path(root, config());
        if kind == "counter" {
            drop(DurableReplica::counter(root, config()).unwrap());
            let log = EventLog::for_crdt(&GCounter::new(2));
            DurableReplica::<GCounter>::commit(&path, config(), &log, 0).unwrap();
        } else {
            drop(DurableReplica::utf8_set(root, config()).unwrap());
            let log = EventLog::for_crdt(&OrSet::<String, u64>::new());
            DurableReplica::<OrSet<String, u64>>::commit(&path, config(), &log, 0).unwrap();
        }
        fs::remove_file(journal::path(root, 0)).unwrap();
        let (marker, counter) = rollback::paths(root, config().writer).unwrap();
        fs::remove_file(marker).unwrap();
        fs::remove_file(counter).unwrap();
    }
    fn legacy(kind: &str) -> PathBuf {
        let root = root();
        legacy_at(kind, &root);
        root
    }
    // Whichever committed history file writer 0 has.
    fn store_file(root: &Path) -> PathBuf {
        let journal = journal::path(root, 0);
        if journal.exists() {
            journal
        } else {
            transaction_path(root, config())
        }
    }
    fn h1_record(sequence: u64) -> Record<GCounterDelta> {
        Record {
            id: RecordId {
                replica: 0,
                sequence,
            },
            delta: GCounterDelta {
                replica: 0,
                tally: 99,
            },
        }
    }
    #[test]
    fn h1_event_log_unprotected_control() {
        let mut state = GCounter::new(2);
        let mut log = EventLog::for_crdt(&state);
        assert_eq!(
            log.insert_record(&state, h1_record(u64::MAX)),
            Admission::Accepted
        );
        assert!(matches!(
            log.append(
                &mut state,
                0,
                GCounterDelta {
                    replica: 0,
                    tally: 100
                }
            ),
            Err(crate::AppendError::SequenceExhausted)
        ));
    }
    #[test]
    fn h1_local_peer_sequence_refused() {
        let mut r = LocalReplica::counter(&root(), config()).unwrap();
        let honest = r.bump(r.ticket(), 1).unwrap();
        assert_eq!(r.receive(r.ticket(), honest).unwrap(), Admission::Duplicate);
        assert_eq!(r.bump(r.ticket(), 2).unwrap().id.sequence, 2);
        let state = r.state().clone();
        let log = r.log().clone();
        let allocation = r.allocation_bytes();
        let received = r.receive(r.ticket(), h1_record(u64::MAX));
        let next = r.bump(r.ticket(), 3);
        std::println!("H1.local receive={received:?} next={next:?}");
        assert!(matches!(received, Err(LocalError::PeerWriterAhead)));
        assert_eq!(r.state(), &state);
        assert_eq!(r.log(), &log);
        assert_eq!(r.allocation_bytes(), allocation);
        assert!(matches!(
            r.bump(r.ticket(), 3),
            Err(LocalError::PeerWriterAhead)
        ));
    }
    #[test]
    fn h1_durable_peer_sequence_refused_after_restart() {
        let root = root();
        let mut r = DurableReplica::counter(&root, config()).unwrap();
        let honest = r.bump(r.ticket(), 1).unwrap();
        let path = store_file(&root);
        let bytes = fs::read(&path).unwrap();
        assert!(matches!(
            r.receive(r.ticket(), h1_record(u64::MAX)),
            Err(LocalError::PeerWriterAhead)
        ));
        assert!(matches!(
            r.bump(r.ticket(), 2),
            Err(LocalError::PeerWriterAhead)
        ));
        assert_eq!(fs::read(&path).unwrap(), bytes);
        drop(r);
        let mut r = DurableReplica::restart_counter(&root, config()).unwrap();
        // The stop is volatile: ordinary restart still continues at high-water + 1.
        assert_eq!(r.bump(r.ticket(), 2).unwrap().id.sequence, 2);
        assert_eq!(r.receive(r.ticket(), honest).unwrap(), Admission::Duplicate);
        let before = fs::read(&path).unwrap();
        // Sync re-detects the same forged record and stops this new session.
        assert!(matches!(
            r.receive(r.ticket(), h1_record(u64::MAX)),
            Err(LocalError::PeerWriterAhead)
        ));
        assert!(matches!(
            r.bump(r.ticket(), 3),
            Err(LocalError::PeerWriterAhead)
        ));
        assert_eq!(fs::read(&path).unwrap(), before);
    }
    #[test]
    fn h1_second_order_boundaries_and_complete_log_receive() {
        for sequence in [2, u64::MAX] {
            let root = root();
            let mut r = DurableReplica::counter(&root, config()).unwrap();
            let honest = r.bump(r.ticket(), 1).unwrap();
            assert_eq!(
                r.receive(r.ticket(), honest.clone()).unwrap(),
                Admission::Duplicate
            );
            let path = store_file(&root);
            let bytes = fs::read(&path).unwrap();
            let state = r.state().clone();
            let log = r.log().clone();
            let allocation = r.allocation_bytes();
            for _ in 0..2 {
                assert!(matches!(
                    r.receive(r.ticket(), h1_record(sequence)),
                    Err(LocalError::PeerWriterAhead)
                ));
                assert_eq!(r.state(), &state);
                assert_eq!(r.log(), &log);
                assert_eq!(r.allocation_bytes(), allocation);
                assert_eq!(fs::read(&path).unwrap(), bytes);
            }
            let ticket = r.renew(r.ticket()).unwrap();
            assert!(matches!(
                r.append(
                    ticket,
                    GCounterDelta {
                        replica: 0,
                        tally: 3
                    }
                ),
                Err(LocalError::PeerWriterAhead)
            ));
            assert_eq!(r.receive(ticket, honest).unwrap(), Admission::Duplicate);
            let remote = Record {
                id: RecordId {
                    replica: 1,
                    sequence: u64::MAX,
                },
                delta: GCounterDelta {
                    replica: 1,
                    tally: 7,
                },
            };
            assert_eq!(
                r.receive(ticket, remote.clone()).unwrap(),
                Admission::Accepted
            );
            assert_eq!(r.receive(ticket, remote).unwrap(), Admission::Duplicate);
            assert_eq!(r.allocation_bytes(), allocation);
            assert!(matches!(
                r.bump(ticket, 3),
                Err(LocalError::PeerWriterAhead)
            ));
        }
        let mut r = LocalReplica::counter(&root(), config()).unwrap();
        let mut incoming = EventLog::for_crdt(&GCounter::new(2));
        assert_eq!(
            incoming.insert_record(&GCounter::new(2), h1_record(u64::MAX)),
            Admission::Accepted
        );
        // Local/DurableReplica expose single-record receive, not a batch merge.
        // A complete transport log must decode then route every record to receive.
        let decoded = EventLog::<GCounterDelta>::from_wire_bytes_for(
            &incoming.to_wire_bytes().unwrap(),
            r.state(),
        )
        .unwrap();
        for record in decoded.records() {
            assert!(matches!(
                r.receive(r.ticket(), record.clone()),
                Err(LocalError::PeerWriterAhead)
            ));
        }
        assert!(matches!(
            r.bump(r.ticket(), 1),
            Err(LocalError::PeerWriterAhead)
        ));
    }
    #[test]
    fn h1_orset_local_actions_remain_stopped() {
        let mut r = DurableReplica::utf8_set(&root(), config()).unwrap();
        let element = String::from("honest");
        r.add(r.ticket(), element.clone()).unwrap();
        let ahead = Record {
            id: RecordId {
                replica: 0,
                sequence: 2,
            },
            delta: OrSetDelta::Add {
                element: String::from("lost"),
                token: 4,
            },
        };
        assert!(matches!(
            r.receive(r.ticket(), ahead),
            Err(LocalError::PeerWriterAhead)
        ));
        assert!(matches!(
            r.add(r.ticket(), String::from("next")),
            Err(LocalError::PeerWriterAhead)
        ));
        assert!(matches!(
            r.remove(r.ticket(), &element),
            Err(LocalError::PeerWriterAhead)
        ));
        let mut r = LocalReplica::utf8_set(&root(), config()).unwrap();
        let ahead = Record {
            id: RecordId {
                replica: 0,
                sequence: 1,
            },
            delta: OrSetDelta::Add {
                element: element.clone(),
                token: 2,
            },
        };
        assert!(matches!(
            r.receive(r.ticket(), ahead),
            Err(LocalError::PeerWriterAhead)
        ));
        assert!(matches!(
            r.add(r.ticket(), element.clone()),
            Err(LocalError::PeerWriterAhead)
        ));
        assert!(matches!(
            r.remove(r.ticket(), &element),
            Err(LocalError::PeerWriterAhead)
        ));
    }
    #[test]
    fn m3_restore_peer_return_never_reissues_id() {
        let root = root();
        let mut r = DurableReplica::counter(&root, config()).unwrap();
        pre_protection(&mut r);
        r.bump(r.ticket(), 1).unwrap();
        let path = store_file(&root);
        let backup = fs::read(&path).unwrap();
        let lost = r.bump(r.ticket(), 2).unwrap();
        drop(r);
        fs::write(&path, &backup).unwrap();
        let mut r = DurableReplica::restart_counter(&root, config()).unwrap();
        assert!(matches!(
            r.receive(r.ticket(), lost.clone()),
            Err(LocalError::PeerWriterAhead)
        ));
        let before = fs::read(&path).unwrap();
        assert!(
            matches!(r.bump(r.ticket(), 3), Err(LocalError::PeerWriterAhead)),
            "restored writer must stop before reissuing {:?}",
            lost.id
        );
        assert_eq!(before, backup);
        assert_eq!(fs::read(&path).unwrap(), backup);
    }
    #[test]
    fn durable_orset_zero_sequence_remove_refused_on_receive_and_restart() {
        // The stored-history half writes a whole transaction, so use that format.
        let root = legacy("set");
        let mut replica = DurableReplica::restart_utf8_set(&root, config()).unwrap();
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
        // The stored-history half writes a whole transaction, so use that format.
        let root = legacy("set");
        let mut replica = DurableReplica::restart_utf8_set(&root, config()).unwrap();
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
    fn counter_width_constructor_boundaries() {
        for writers in [0, 4096, 4097] {
            let config = WriterConfig { writers, writer: 0 };
            let local = LocalReplica::counter(&root(), config);
            let durable_root = root();
            let durable = DurableReplica::counter(&durable_root, config);
            if writers == 4097 {
                assert!(matches!(
                    local,
                    Err(LocalError::CounterWidth(
                        crate::CoordinateError::ReplicaLimitExceeded {
                            requested: 4097,
                            maximum: 4096
                        }
                    ))
                ));
                assert!(matches!(durable, Err(LocalError::CounterWidth(_))));
                assert!(matches!(
                    DurableReplica::restart_counter(&durable_root, config),
                    Err(LocalError::CounterWidth(_))
                ));
            } else if writers == 0 {
                assert!(matches!(local, Err(LocalError::Configuration)));
                assert!(matches!(durable, Err(LocalError::Configuration)));
            } else {
                assert_eq!(local.unwrap().state().len(), 4096);
                drop(durable.unwrap());
                assert_eq!(
                    DurableReplica::restart_counter(&durable_root, config)
                        .unwrap()
                        .state()
                        .len(),
                    4096
                );
                assert_eq!(
                    DurableReplica::restart_counter_from_store(&durable_root, 0)
                        .unwrap()
                        .state()
                        .len(),
                    4096
                );
            }
        }
    }

    #[test]
    fn counter_width_restart_is_named_and_preserves_store() {
        for writers in [4097, usize::MAX as u64] {
            let root = root();
            let config = WriterConfig { writers, writer: 0 };
            // Match the pre-cap fresh log shape through the product encoder.
            let log = EventLog::<GCounterDelta>::with_replica_count(writers as usize);
            let mut transaction = [
                writers.to_le_bytes(),
                0u64.to_le_bytes(),
                0u64.to_le_bytes(),
            ]
            .concat();
            transaction.extend(log.to_wire_bytes().unwrap());
            let fence = [
                writers.to_le_bytes(),
                0u64.to_le_bytes(),
                1u64.to_le_bytes(),
            ]
            .concat();
            fs::write(transaction_path(&root, config), &transaction).unwrap();
            let fence_path = root.join("writer-0.fence");
            fs::write(&fence_path, &fence).unwrap();
            for _ in 0..2 {
                let error = DurableReplica::restart_counter_from_store(&root, 0)
                    .err()
                    .unwrap();
                assert!(
                    matches!(error, LocalError::CounterWidth(crate::CoordinateError::ReplicaLimitExceeded { requested, maximum: 4096 }) if requested == writers as usize)
                );
                let message = error.to_string();
                assert!(
                    message.contains(&writers.to_string()) && message.contains("4096"),
                    "{message}"
                );
                assert_eq!(
                    fs::read(transaction_path(&root, config)).unwrap(),
                    transaction
                );
                assert_eq!(fs::read(&fence_path).unwrap(), fence);
            }
            assert!(matches!(
                DurableReplica::restart_counter(
                    &root,
                    WriterConfig {
                        writers: 2,
                        writer: 0
                    }
                ),
                Err(LocalError::Configuration)
            ));
            assert_eq!(
                fs::read(transaction_path(&root, config)).unwrap(),
                transaction
            );
            assert_eq!(fs::read(&fence_path).unwrap(), fence);
        }
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
        // The append-log header: magic, then writer count and writer.
        let path = journal::path(&root, 0);
        let original = fs::read(&path).unwrap();
        fs::write(&path, &original[..journal::HEADER - 1]).unwrap();
        assert!(matches!(
            DurableReplica::restart_counter_from_store(&root, 0),
            Err(LocalError::RecoveryRequired)
        ));
        let mut invalid = original.clone();
        invalid[8..16].copy_from_slice(&1u64.to_le_bytes());
        invalid[16..24].copy_from_slice(&1u64.to_le_bytes());
        fs::write(&path, invalid).unwrap();
        assert!(matches!(
            DurableReplica::restart_counter_from_store(&root, 0),
            Err(LocalError::Configuration)
        ));
        fs::write(&path, &original).unwrap();
        assert!(DurableReplica::restart_counter_from_store(&root, 0).is_ok());

        let root = legacy("counter");
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
        fs::copy(root.join("writer-0.journal"), root.join("writer-1.journal")).unwrap();
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
    fn fresh_root_second_order() {
        let parent = root();
        for name in ["empty", "stray", "symlink"] {
            let target = parent.join(format!("target-{name}"));
            fs::create_dir(&target).unwrap();
            if name == "stray" {
                fs::write(target.join("stray"), b"keep").unwrap();
            }
            let store = if name == "symlink" {
                let link = parent.join("link");
                std::os::unix::fs::symlink(&target, &link).unwrap();
                link
            } else {
                target.clone()
            };
            std::eprintln!("ROOT-PROBE fresh {}", store.display());
            let replica = DurableReplica::counter(&store, config()).unwrap();
            drop(replica);
            std::eprintln!("ROOT-PROBE reopen {}", store.display());
            let mut replica = DurableReplica::restart_counter(&store, config()).unwrap();
            let ticket = replica.ticket();
            replica.bump(ticket, 1).unwrap();
            drop(replica);
            std::eprintln!("ROOT-PROBE restart {}", store.display());
            let replica = DurableReplica::restart_counter(&store, config()).unwrap();
            assert_eq!(replica.state().value(), 1);
            drop(replica);
            if name == "stray" {
                assert_eq!(fs::read(target.join("stray")).unwrap(), b"keep");
            }
        }
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
    fn fresh_ancestor_sync_refusal_and_retry() {
        use std::os::unix::fs::PermissionsExt;
        if std::process::Command::new("id")
            .arg("-u")
            .output()
            .unwrap()
            .stdout
            == b"0\n"
        {
            std::eprintln!("SKIP: root bypasses execute-only ancestor permissions");
            return;
        }
        let ancestor = root().join("execute-only");
        let writable = ancestor.join("writable");
        fs::create_dir_all(&writable).unwrap();
        let ancestor = ancestor.canonicalize().unwrap();
        let store = ancestor.join("writable/store");
        fs::set_permissions(&ancestor, fs::Permissions::from_mode(0o111)).unwrap();
        std::eprintln!("ANCESTOR-PROBE refusal {}", ancestor.display());
        let result = DurableReplica::counter(&store, config());
        fs::set_permissions(&ancestor, fs::Permissions::from_mode(0o755)).unwrap();
        let error = result
            .err()
            .expect("execute-only ancestor must refuse creation");
        assert!(error.to_string().contains(&ancestor.display().to_string()));
        assert!(error.to_string().contains("Permission denied"));
        assert!(matches!(error, LocalError::AncestorSync { path, source }
            if path == ancestor && source.kind() == io::ErrorKind::PermissionDenied));
        assert!(store.is_dir());
        assert!(!store.join("writer-0.fence").exists());
        std::eprintln!("ANCESTOR-PROBE retry {}", store.display());
        let replica = DurableReplica::counter(&store, config()).unwrap();
        assert!(store.join("writer-0.fence").exists());
        drop(replica);
        std::eprintln!("ANCESTOR-PROBE reopen {}", store.display());
        drop(DurableReplica::restart_counter(&store, config()).unwrap());
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
        root: &Path,
        mut replica: DurableReplica<C>,
        records: usize,
        mut edit: impl FnMut(&mut LocalReplica<C>, u64),
    ) -> DurableReplica<C>
    where
        C::Delta: OwnedDelta + Clone + PartialEq + WireEncode + WireSchema,
    {
        pre_protection(&mut replica);
        for index in 0..records as u64 {
            edit(&mut replica.inner, index);
        }
        let DurableReplica { inner, store, .. } = &mut replica;
        match store {
            Store::Transaction(path) => {
                DurableReplica::<C>::commit(path, inner.config, &inner.log, inner.last_sequence)
                    .unwrap()
            }
            // The whole history as the journal's base frame, as migration writes it.
            Store::Journal { file, len } => {
                let path = journal::path(root, inner.config.writer);
                let log = inner.log.to_wire_bytes().unwrap();
                let header = journal::header(inner.config, inner.last_sequence, &log).unwrap();
                persistence::replace(&path, &header).unwrap();
                *file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&path)
                    .unwrap();
                *len = header.len() as u64;
            }
        }
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
                        "local history exceeds restart record budget: RecordLimitExceeded: {max_records}; pass DecodeLimits::UNLIMITED to reopen"
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
        drop(seeded(&root, fresh, N, |inner, index| {
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
        assert_refused_by_name(DurableReplica::restart_counter(&root, config()), N);
        assert_refused_by_name(DurableReplica::restart_counter_from_store(&root, 0), N);
        // Explicit unlimited preserves access to older, oversized histories.
        let reopened = DurableReplica::restart_counter_with_limits(
            &root,
            config(),
            crate::DecodeLimits::UNLIMITED,
        )
        .unwrap();
        assert_eq!(reopened.log().records().len(), N + 1);
        assert_eq!(reopened.state().value(), N as u128 + 1);
        drop(reopened);
        let reopened = DurableReplica::restart_counter_from_store_with_limits(
            &root,
            0,
            crate::DecodeLimits::UNLIMITED,
        )
        .unwrap();
        assert_eq!(reopened.log().records().len(), N + 1);

        // UTF-8 OR-Set: the same boundary through one more durable add.
        let root = self::root();
        let fresh = DurableReplica::utf8_set(&root, config()).unwrap();
        drop(seeded(&root, fresh, N, |inner, index| {
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
        assert_refused_by_name(DurableReplica::restart_utf8_set(&root, config()), N);
        // Explicit unlimited preserves access to this older, oversized set.
        let reopened = DurableReplica::restart_utf8_set_with_limits(
            &root,
            config(),
            crate::DecodeLimits::UNLIMITED,
        )
        .unwrap();
        assert_eq!(reopened.log().records().len(), N + 1);
    }
    #[test]
    fn persisted_150000_record_history_requires_explicit_unlimited_on_restart() {
        const N: usize = 150_000;
        let root = self::root();
        let fresh = DurableReplica::counter(&root, config()).unwrap();
        drop(seeded(&root, fresh, N, |inner, index| {
            inner.bump(inner.ticket(), index + 1).unwrap();
        }));
        let before = store_files(&root);
        assert_refused_by_name(
            DurableReplica::restart_counter(&root, config()),
            crate::DecodeLimits::DEFAULT_MAX_RECORDS,
        );
        assert_eq!(store_files(&root), before);
        let reopened = DurableReplica::restart_counter_with_limits(
            &root,
            config(),
            crate::DecodeLimits::UNLIMITED,
        )
        .unwrap();
        assert_eq!(reopened.log().records().len(), N);
        assert_eq!(reopened.state().value(), N as u128);
    }
    #[test]
    fn restart_budget_refuses_before_reading_records() {
        // A transaction-format store declares its count in the frame header.
        let root = legacy("counter");
        let mut counter = DurableReplica::restart_counter(&root, config()).unwrap();
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
    fn counter_journal(tallies: &[u64]) -> PathBuf {
        let root = root();
        let mut r = DurableReplica::counter(&root, config()).unwrap();
        pre_protection(&mut r);
        for &tally in tallies {
            r.bump(r.ticket(), tally).unwrap();
        }
        root
    }
    fn store_copy(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        fs::read_dir(root)
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                let bytes = fs::read(&path).unwrap();
                (path, bytes)
            })
            .collect()
    }
    fn restore_copy(copy: &[(PathBuf, Vec<u8>)]) {
        for (path, bytes) in copy {
            fs::write(path, bytes).unwrap();
        }
    }
    fn protected_counter_path(root: &Path, writer: u64) -> PathBuf {
        rollback::paths(root, writer).unwrap().1
    }
    #[test]
    fn rollback_r1_store_only_restore_refuses() {
        let root = root();
        let mut r = DurableReplica::counter(&root, config()).unwrap();
        r.bump(r.ticket(), 1).unwrap();
        let copy = store_copy(&root);
        r.bump(r.ticket(), 2).unwrap();
        drop(r);
        restore_copy(&copy);
        assert!(matches!(
            DurableReplica::restart_counter(&root, config()),
            Err(LocalError::RecoveryRequired)
        ));
    }
    #[test]
    fn rollback_r2_new_writer_after_refusal() {
        let root = root();
        let mut old = DurableReplica::counter(&root, config()).unwrap();
        old.bump(old.ticket(), 1).unwrap();
        let copy = store_copy(&root);
        old.bump(old.ticket(), 2).unwrap();
        drop(old);
        restore_copy(&copy);
        assert!(matches!(
            DurableReplica::restart_counter(&root, config()),
            Err(LocalError::RecoveryRequired)
        ));
        let new_config = WriterConfig {
            writers: 2,
            writer: 1,
        };
        let mut new = DurableReplica::counter(&root, new_config).unwrap();
        let record = new.bump(new.ticket(), 1).unwrap();
        assert_eq!(record.id.replica, 1);
    }
    #[test]
    fn rollback_r3_honest_restart_100_times() {
        let root = root();
        let mut r = DurableReplica::counter(&root, config()).unwrap();
        r.bump(r.ticket(), 1).unwrap();
        drop(r);
        for _ in 0..100 {
            let r = DurableReplica::restart_counter(&root, config()).unwrap();
            assert_eq!(r.state().value(), 1);
        }
    }
    #[test]
    fn rollback_crash_child() {
        let Ok(root) = std::env::var("SAFEMESH_ROLLBACK_CRASH_ROOT") else {
            return;
        };
        let boundary = std::env::var("SAFEMESH_ROLLBACK_CRASH_BOUNDARY")
            .unwrap()
            .parse()
            .unwrap();
        let mut r = DurableReplica::restart_counter(Path::new(&root), config()).unwrap();
        fault(boundary, true);
        r.bump(r.ticket(), 2).unwrap();
        panic!("crash checkpoint did not exit");
    }
    #[test]
    fn rollback_r4_crash_write_boundaries() {
        for (boundary, refused, value) in [(19, false, 1), (20, true, 1), (14, false, 2)] {
            let root = root();
            let mut r = DurableReplica::counter(&root, config()).unwrap();
            r.bump(r.ticket(), 1).unwrap();
            drop(r);
            let output = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "local::durable_tests::rollback_crash_child"])
                .env("SAFEMESH_ROLLBACK_CRASH_ROOT", &root)
                .env("SAFEMESH_ROLLBACK_CRASH_BOUNDARY", boundary.to_string())
                .output()
                .unwrap();
            let status = root.join("rollback-child.exit");
            fs::write(&status, output.status.code().unwrap().to_string()).unwrap();
            assert_eq!(fs::read_to_string(status).unwrap(), "77", "{output:?}");
            if refused {
                assert!(matches!(
                    DurableReplica::restart_counter(&root, config()),
                    Err(LocalError::RecoveryRequired)
                ));
                let new_config = WriterConfig {
                    writers: 2,
                    writer: 1,
                };
                assert!(DurableReplica::counter(&root, new_config).is_ok());
            } else {
                let r = DurableReplica::restart_counter(&root, config()).unwrap();
                assert_eq!(r.state().value(), value);
            }
        }
    }
    #[test]
    fn rollback_r5_deleted_counter_refuses() {
        let root = root();
        let mut r = DurableReplica::counter(&root, config()).unwrap();
        r.bump(r.ticket(), 1).unwrap();
        drop(r);
        fs::remove_file(protected_counter_path(&root, 0)).unwrap();
        assert!(matches!(
            DurableReplica::restart_counter(&root, config()),
            Err(LocalError::RecoveryRequired)
        ));
    }
    #[test]
    fn rollback_r6_counter_damage_refuses() {
        let root = root();
        let mut r = DurableReplica::counter(&root, config()).unwrap();
        r.bump(r.ticket(), 1).unwrap();
        drop(r);
        let path = protected_counter_path(&root, 0);
        let original = fs::read(&path).unwrap();
        let mut flipped = original.clone();
        flipped[40] ^= 1;
        let mut trailing = original.clone();
        trailing.push(0);
        let mut impossible = original.clone();
        impossible[40..48].copy_from_slice(&u64::MAX.to_le_bytes());
        let checksum = crate::codec::frame_crc32(&impossible[..48]);
        impossible[48..52].copy_from_slice(&checksum.to_le_bytes());
        for bytes in [Vec::new(), flipped, trailing, impossible] {
            fs::write(&path, bytes).unwrap();
            assert!(matches!(
                DurableReplica::restart_counter(&root, config()),
                Err(LocalError::RecoveryRequired)
            ));
        }
        fs::write(&path, original).unwrap();
        assert!(DurableReplica::restart_counter(&root, config()).is_ok());
    }
    #[test]
    fn rollback_counter_symlink_refuses_without_reading_target() {
        let root = root();
        drop(DurableReplica::counter(&root, config()).unwrap());
        let path = protected_counter_path(&root, 0);
        let victim = root.join("victim");
        fs::write(&victim, b"keep").unwrap();
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&victim, &path).unwrap();
        assert!(matches!(
            DurableReplica::restart_counter(&root, config()),
            Err(LocalError::RecoveryRequired)
        ));
        assert_eq!(fs::read(victim).unwrap(), b"keep");
    }
    #[test]
    fn rollback_r7_limit_whole_machine_copy_passes() {
        let root = root();
        let mut r = DurableReplica::counter(&root, config()).unwrap();
        r.bump(r.ticket(), 1).unwrap();
        let copy = store_copy(&root);
        let counter_path = protected_counter_path(&root, 0);
        let counter = fs::read(&counter_path).unwrap();
        r.bump(r.ticket(), 2).unwrap();
        drop(r);
        restore_copy(&copy);
        fs::write(counter_path, counter).unwrap();
        let r = DurableReplica::restart_counter(&root, config()).unwrap();
        assert_eq!(r.state().value(), 1);
    }
    #[test]
    fn rollback_r8_two_writers_two_counters() {
        let root = root();
        let other = WriterConfig {
            writers: 2,
            writer: 1,
        };
        let mut first = DurableReplica::counter(&root, config()).unwrap();
        let mut second = DurableReplica::counter(&root, other).unwrap();
        assert_ne!(
            protected_counter_path(&root, 0),
            protected_counter_path(&root, 1)
        );
        first.bump(first.ticket(), 1).unwrap();
        second.bump(second.ticket(), 1).unwrap();
        drop(first);
        drop(second);
        assert!(DurableReplica::restart_counter(&root, config()).is_ok());
        assert!(DurableReplica::restart_counter(&root, other).is_ok());
    }
    #[test]
    fn rollback_limit_other_writer_only_restore_passes() {
        let root = root();
        let other = WriterConfig {
            writers: 2,
            writer: 1,
        };
        let mut receiver = DurableReplica::counter(&root, config()).unwrap();
        let mut sender = DurableReplica::counter(&root, other).unwrap();
        let first = sender.bump(sender.ticket(), 1).unwrap();
        receiver.receive(receiver.ticket(), first).unwrap();
        let copy = store_copy(&root);
        let second = sender.bump(sender.ticket(), 2).unwrap();
        receiver.receive(receiver.ticket(), second).unwrap();
        drop(receiver);
        drop(sender);
        restore_copy(&copy);
        let receiver = DurableReplica::restart_counter(&root, config()).unwrap();
        assert_eq!(receiver.state().state()[1], 1);
    }
    #[test]
    fn rollback_check_precedes_saved_collision_alarm() {
        let root = root();
        let mut r = DurableReplica::counter(&root, config()).unwrap();
        r.bump(r.ticket(), 1).unwrap();
        let conflict = Record {
            id: RecordId {
                replica: 0,
                sequence: 1,
            },
            delta: GCounterDelta {
                replica: 0,
                tally: 9,
            },
        };
        assert_eq!(
            r.receive(r.ticket(), conflict).unwrap(),
            Admission::Collision
        );
        assert!(root.join("writer-0.alarms").exists());
        let copy = store_copy(&root);
        r.bump(r.ticket(), 2).unwrap();
        drop(r);
        restore_copy(&copy);
        assert!(matches!(
            DurableReplica::restart_counter(&root, config()),
            Err(LocalError::RecoveryRequired)
        ));
    }
    fn restart_state(root: &Path) -> Result<(Vec<u64>, Option<TornTail>), LocalError> {
        let r = DurableReplica::restart_counter(root, config())?;
        Ok((r.state().state().to_vec(), r.torn_tail()))
    }
    #[test]
    fn append_log_writes_one_entry_per_record() {
        let root = counter_journal(&[1]);
        let path = journal::path(&root, 0);
        let mut r = DurableReplica::restart_counter(&root, config()).unwrap();
        for tally in 2..=50 {
            let before = fs::read(&path).unwrap();
            let record = r.bump(r.ticket(), tally).unwrap();
            let after = fs::read(&path).unwrap();
            // The earlier bytes are untouched; exactly one entry follows them.
            assert_eq!(after[..before.len()], before[..]);
            let entry = journal::entry(tally, &record.to_wire_bytes().unwrap()).unwrap();
            assert_eq!(after[before.len()..], entry[..]);
        }
        assert!(!transaction_path(&root, config()).exists());
    }
    #[test]
    fn append_log_torn_tail_is_truncated_and_reported() {
        let full = fs::read(journal::path(&counter_journal(&[5, 9, 12]), 0)).unwrap();
        let two = fs::read(journal::path(&counter_journal(&[5, 9]), 0)).unwrap();
        let last = full.len() - two.len();
        let mut flipped = full.clone();
        *flipped.last_mut().unwrap() ^= 1;
        let mut zero_fill = full.clone();
        zero_fill.extend([0u8; 64]);
        let mut short_pair = full.clone();
        short_pair.extend([7u8; 3]);
        for (case, bytes, state, kept) in [
            (
                "final entry cut short",
                full[..full.len() - 7].to_vec(),
                [9, 0],
                two.len(),
            ),
            ("final entry checksum", flipped, [9, 0], two.len()),
            ("zero fill", zero_fill, [12, 0], full.len()),
            ("length pair cut short", short_pair, [12, 0], full.len()),
        ] {
            let root = counter_journal(&[]);
            let path = journal::path(&root, 0);
            fs::write(&path, &bytes).unwrap();
            let (restored, torn) = restart_state(&root).unwrap();
            std::println!("torn tail case={case} state={restored:?} torn_tail={torn:?}");
            assert_eq!(restored, state, "{case}");
            assert_eq!(
                torn,
                Some(TornTail {
                    offset: kept as u64,
                    discarded_bytes: (bytes.len() - kept) as u64
                }),
                "{case}"
            );
            assert_eq!(fs::read(&path).unwrap(), bytes[..kept], "{case}: truncated");
            let mut r = DurableReplica::restart_counter(&root, config()).unwrap();
            assert_eq!(r.torn_tail(), None, "{case}: nothing left to discard");
            r.bump(r.ticket(), 30).unwrap();
            drop(r);
            assert_eq!(restart_state(&root).unwrap(), (vec![30, 0], None), "{case}");
        }
        assert!(last > 7);
    }
    #[test]
    fn append_log_damage_before_the_tail_is_refused() {
        let root = counter_journal(&[5, 9, 12]);
        let path = journal::path(&root, 0);
        let original = fs::read(&path).unwrap();
        let first =
            journal::HEADER + u32::from_le_bytes(original[32..36].try_into().unwrap()) as usize;
        let mut payload = original.clone();
        payload[first + 8 + 8 + 4] ^= 1;
        let mut pair = original.clone();
        pair[first] ^= 1;
        let mut base = original.clone();
        base[first - 1] ^= 1;
        let mut writers = original.clone();
        writers[8..16].copy_from_slice(&3u64.to_le_bytes());
        let mut magic = original.clone();
        magic[0] ^= 1;
        for (case, bytes, expected) in [
            (
                "record before others",
                payload,
                "History(IntegrityMismatch)",
            ),
            (
                "length pair before others",
                pair,
                "History(IntegrityMismatch)",
            ),
            ("base frame", base, "History(IntegrityMismatch)"),
            ("writer count", writers, "Configuration"),
            ("magic", magic, "RecoveryRequired"),
            (
                "header cut short",
                original[..journal::HEADER - 1].to_vec(),
                "RecoveryRequired",
            ),
        ] {
            fs::write(&path, &bytes).unwrap();
            let error = restart_state(&root).unwrap_err();
            std::println!("damaged journal case={case} restart={error:?}");
            assert_eq!(format!("{error:?}"), expected, "{case}");
            assert_eq!(fs::read(&path).unwrap(), bytes, "{case}: journal unchanged");
        }
        fs::write(&path, &original).unwrap();
        assert_eq!(restart_state(&root).unwrap(), (vec![12, 0], None));
    }
    #[test]
    fn restart_budget_counts_append_log_without_decoding() {
        let root = counter_journal(&[1, 2, 3]);
        let path = journal::path(&root, 0);
        let original = fs::read(&path).unwrap();
        // Damage every record payload but keep each length pair: only a count
        // taken from the pairs can name the budget.
        let mut bytes = original.clone();
        let mut at =
            journal::HEADER + u32::from_le_bytes(original[32..36].try_into().unwrap()) as usize;
        let mut entries = 0;
        while at < bytes.len() {
            let len = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
            bytes[at + 8 + 8] ^= 0xff;
            at += 8 + len + 4;
            entries += 1;
        }
        assert_eq!(entries, 3);
        fs::write(&path, &bytes).unwrap();
        let before = store_files(&root);
        assert_refused_by_name(
            DurableReplica::restart_counter_with_limits(&root, config(), budget(2)),
            2,
        );
        assert_refused_by_name(
            DurableReplica::restart_counter_from_store_with_limits(&root, 0, budget(2)),
            2,
        );
        assert_eq!(store_files(&root), before);
        assert!(matches!(
            DurableReplica::restart_counter_with_limits(&root, config(), budget(3)),
            Err(LocalError::History(WireError::IntegrityMismatch))
        ));
        fs::write(&path, &original).unwrap();
        assert_eq!(
            DurableReplica::restart_counter_with_limits(&root, config(), budget(3))
                .unwrap()
                .state()
                .value(),
            3
        );
    }
    #[test]
    fn transaction_store_opens_appends_in_place_and_migrates_explicitly() {
        let root = legacy("set");
        let mut r = DurableReplica::restart_utf8_set(&root, config()).unwrap();
        for index in 0..5_000 {
            r.receive(
                r.ticket(),
                Record {
                    id: RecordId {
                        replica: 1,
                        sequence: index + 1,
                    },
                    delta: OrSetDelta::Add {
                        element: format!("kept-{index:04}"),
                        token: allocate_token(2, 1, index + 1).unwrap(),
                    },
                },
            )
            .unwrap();
        }
        r.add(r.ticket(), "local".into()).unwrap();
        drop(r);
        // Restart opens the old format and never migrates it.
        let transaction = transaction_path(&root, config());
        let r = DurableReplica::restart_utf8_set(&root, config()).unwrap();
        assert_eq!(r.log().records().len(), 5_001);
        let state = r.state().clone();
        drop(r);
        let committed = read(&root);
        assert!(transaction.exists() && !journal::path(&root, 0).exists());
        let mut r = DurableReplica::migrate_utf8_set_to_append_log(&root, config()).unwrap();
        assert!(!transaction.exists() && journal::path(&root, 0).exists());
        assert_eq!(r.state(), &state);
        assert_eq!(read(&root), committed, "every record kept, in order");
        let before = fs::metadata(journal::path(&root, 0)).unwrap().len();
        r.add(r.ticket(), "appended".into()).unwrap();
        let after = fs::metadata(journal::path(&root, 0)).unwrap().len();
        assert!(after - before < 100, "one entry, not the history");
        drop(r);
        let r = DurableReplica::restart_utf8_set(&root, config()).unwrap();
        assert_eq!(r.log().records().len(), 5_002);
        assert!(r.state().contains(&"kept-4999".into()) && r.state().contains(&"appended".into()));
        drop(r);
        // Migrating an append-log store restarts it unchanged.
        let r = DurableReplica::migrate_utf8_set_to_append_log(&root, config()).unwrap();
        assert_eq!(r.log().records().len(), 5_002);
        drop(r);

        // The store the archived 3a9179b library wrote migrates the same way.
        let root = self::root();
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
        let committed = read(&root);
        let mut r = DurableReplica::migrate_counter_to_append_log(&root, config()).unwrap();
        assert_eq!(r.state().state(), &[5, 7]);
        assert_eq!(read(&root), committed);
        r.bump(r.ticket(), 8).unwrap();
        drop(r);
        assert_eq!(restart_state(&root).unwrap(), (vec![8, 7], None));
    }
    #[test]
    fn interrupted_migration_is_refused_then_completed() {
        let root = initialized_legacy("counter");
        let committed = read(&root);
        // Fail after the journal is in place, before the transaction is removed.
        fault(15, false);
        let result = DurableReplica::migrate_counter_to_append_log(&root, config());
        fault(0, false);
        assert!(matches!(result, Err(LocalError::Io(_))));
        assert!(transaction_path(&root, config()).exists() && journal::path(&root, 0).exists());
        let both = store_files(&root);
        assert!(matches!(
            DurableReplica::restart_counter(&root, config()),
            Err(LocalError::RecoveryRequired)
        ));
        assert!(matches!(
            DurableReplica::restart_counter_from_store(&root, 0),
            Err(LocalError::RecoveryRequired)
        ));
        assert!(matches!(
            DurableReplica::counter(&root, config()),
            Err(LocalError::RecoveryRequired)
        ));
        assert_eq!(store_files(&root), both, "refusals change nothing");
        let r = DurableReplica::migrate_counter_to_append_log(&root, config()).unwrap();
        assert_eq!(r.state().state(), &[5, 0]);
        drop(r);
        assert!(!transaction_path(&root, config()).exists());
        assert_eq!(read(&root), committed);

        // A journal that has moved on from the transaction is not completed.
        let root = initialized_legacy("counter");
        let transaction = fs::read(transaction_path(&root, config())).unwrap();
        drop(DurableReplica::migrate_counter_to_append_log(&root, config()).unwrap());
        let mut r = DurableReplica::restart_counter(&root, config()).unwrap();
        r.bump(r.ticket(), 6).unwrap();
        drop(r);
        fs::write(transaction_path(&root, config()), &transaction).unwrap();
        assert!(matches!(
            DurableReplica::migrate_counter_to_append_log(&root, config()),
            Err(LocalError::RecoveryRequired)
        ));
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
                r.admit_committed(
                    r.ticket(),
                    record(128, value),
                    false,
                    |_, _| { panic!("non-admission must not commit") },
                    |_| Ok(())
                )
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
            r.admit_committed(
                r.ticket(),
                record(129, 129),
                true,
                |log, sequence| {
                    assert_eq!(log.records().len(), 129);
                    assert_eq!(sequence, 129);
                    Ok(())
                },
                |_| Ok(())
            )
            .unwrap(),
            Admission::Accepted
        );
        assert_eq!(clones.get(), 1, "accepted writes must not clone history");
        let before = r.log.version().clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = r.admit_committed(
                r.ticket(),
                record(130, 130),
                true,
                |_, _| panic!("commit unwind"),
                |_| Ok(()),
            );
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
        legacy: bool,
        root: &Path,
        boundary: u8,
    ) -> (CommittedTransaction, CommittedTransaction) {
        let old;
        if legacy {
            legacy_at(kind, root);
        }
        if kind == "counter" {
            let mut r = if legacy {
                DurableReplica::restart_counter(root, config()).unwrap()
            } else {
                DurableReplica::counter(root, config()).unwrap()
            };
            if !legacy {
                pre_protection(&mut r);
            }
            r.bump(r.ticket(), 5).unwrap();
            old = read(root);
            fault(boundary, true);
            assert_eq!(r.bump(r.ticket(), 9).unwrap().id.sequence, 2);
            assert_eq!(r.state().state(), &[9, 0]);
        } else {
            let mut r = if legacy {
                DurableReplica::restart_utf8_set(root, config()).unwrap()
            } else {
                DurableReplica::utf8_set(root, config()).unwrap()
            };
            if !legacy {
                pre_protection(&mut r);
            }
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
    // The same one-record history in the pre-append-log transaction format.
    fn initialized_legacy(kind: &str) -> PathBuf {
        let root = legacy(kind);
        if kind == "counter" {
            let mut r = DurableReplica::restart_counter(&root, config()).unwrap();
            r.bump(r.ticket(), 5).unwrap();
        } else {
            let mut r = DurableReplica::restart_utf8_set(&root, config()).unwrap();
            r.add(r.ticket(), "café☕".into()).unwrap();
        }
        assert!(!journal::path(&root, 0).exists());
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
            let root = initialized_legacy(kind);
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
            let root = initialized_legacy(kind);
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
        for root in [initialized("counter"), initialized_legacy("counter")] {
            assert!(matches!(
                DurableReplica::restart_utf8_set(&root, config()),
                Err(LocalError::History(WireError::DeltaTypeMismatch))
            ));
        }
    }
    #[test]
    fn restart_invalid_history() {
        for kind in ["counter", "set"] {
            let root = initialized_legacy(kind);
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
            let root = initialized_legacy(kind);
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
        let transaction = store_file(&root);
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
            let legacy = std::env::var_os("SAFEMESH_DURABLE_LEGACY").is_some();
            exercise(&kind, legacy, Path::new(&root), boundary);
        }
    }
    fn crash(kind: &str, legacy: bool, boundary: u8, root: &Path) -> std::process::Output {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "local::durable_tests::durable_child",
                "--nocapture",
            ])
            .env("SAFEMESH_DURABLE_ROOT", root)
            .env("SAFEMESH_DURABLE_KIND", kind)
            .env("SAFEMESH_DURABLE_BOUNDARY", boundary.to_string());
        if legacy {
            command.env("SAFEMESH_DURABLE_LEGACY", "1");
        }
        let output = command.output().unwrap();
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

        for (kind, legacy) in [("counter", false), ("set", false), ("counter", true)] {
            let root = root();
            let (_, expected) = exercise(kind, legacy, &root, 0);
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
    // Transaction format (whole-file replacement), boundaries 1-6: before the
    // temporary, mid-write, written, synced, renamed, directory synced.
    // Append log, boundaries 11-14: before the write, half an entry written,
    // entry written but not synced, entry synced. 7: commit returned; 8: ACK.
    #[test]
    fn durable_crash_boundaries() {
        for kind in ["counter", "set"] {
            for legacy in [true, false] {
                let (old, new) = exercise(kind, legacy, &root(), 0);
                let boundaries: &[u8] = if legacy {
                    &[1, 2, 3, 4, 5, 6, 7, 8]
                } else {
                    &[11, 12, 13, 14, 7, 8]
                };
                for &boundary in boundaries {
                    let root = root();
                    let output = crash(kind, legacy, boundary, &root);
                    let recovered = read(&root);
                    // A process exit keeps page-cache writes, so an entry written
                    // before its sync (13) reads back whole: the new state. A power
                    // cut could instead leave the old state or a torn tail.
                    let complete = if legacy {
                        boundary >= 5
                    } else {
                        !matches!(boundary, 11 | 12)
                    };
                    assert_eq!(&recovered, if complete { &new } else { &old });
                    assert_eq!(
                        String::from_utf8_lossy(&output.stdout).contains("ACK"),
                        boundary == 8
                    );
                    let restart = if legacy {
                        "transaction format".into()
                    } else {
                        let torn = restart_after_crash(kind, &root, &recovered);
                        assert_eq!(torn.is_some(), boundary == 12);
                        format!("append log, torn_tail={torn:?}")
                    };
                    std::println!(
                        "{kind} boundary={boundary} state={} ({restart})",
                        if complete { "new" } else { "old" },
                    );
                }
            }
        }
    }
    // Restart must replay exactly the recovered history, truncate a torn tail
    // to its reported offset, then accept and keep one more durable edit.
    fn restart_after_crash(
        kind: &str,
        root: &Path,
        recovered: &CommittedTransaction,
    ) -> Option<TornTail> {
        let path = journal::path(root, 0);
        let before = fs::metadata(&path).unwrap().len();
        let (torn, log, records) = if kind == "counter" {
            let mut r = DurableReplica::restart_counter(root, config()).unwrap();
            let log = r.log().to_wire_bytes().unwrap();
            let torn = r.torn_tail();
            r.bump(r.ticket(), 20).unwrap();
            (torn, log, r.log().records().len())
        } else {
            let mut r = DurableReplica::restart_utf8_set(root, config()).unwrap();
            let log = r.log().to_wire_bytes().unwrap();
            let torn = r.torn_tail();
            r.add(r.ticket(), "after".into()).unwrap();
            (torn, log, r.log().records().len())
        };
        assert_eq!(
            log, recovered.log_bytes,
            "restart replays exactly one state"
        );
        if let Some(tail) = torn {
            assert_eq!(tail.offset + tail.discarded_bytes, before);
        }
        let again = read(root);
        assert_eq!(again.last_sequence, recovered.last_sequence + 1);
        let replayed = if kind == "counter" {
            let r = DurableReplica::restart_counter(root, config()).unwrap();
            assert_eq!(r.torn_tail(), None);
            r.log().records().len()
        } else {
            let r = DurableReplica::restart_utf8_set(root, config()).unwrap();
            assert_eq!(r.torn_tail(), None);
            r.log().records().len()
        };
        assert_eq!(replayed, records);
        torn
    }
    fn acknowledged_survives(kind: &str) {
        for legacy in [true, false] {
            let root = root();
            let output = crash(kind, legacy, 8, &root);
            assert!(String::from_utf8_lossy(&output.stdout).contains("ACK"));
            replay(kind, &read(&root));
        }
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
        let counter = |legacy: bool| {
            let root = root();
            if legacy {
                legacy_at("counter", &root);
                DurableReplica::restart_counter(&root, config()).unwrap()
            } else {
                DurableReplica::counter(&root, config()).unwrap()
            }
        };
        let set = |legacy: bool| {
            let root = root();
            if legacy {
                legacy_at("set", &root);
                DurableReplica::restart_utf8_set(&root, config()).unwrap()
            } else {
                DurableReplica::utf8_set(&root, config()).unwrap()
            }
        };
        let boundaries = (2..=6)
            .map(|b| (b, true))
            .chain((11..=14).map(|b| (b, false)));
        for ((boundary, legacy), populated) in
            boundaries.flat_map(|boundary| [false, true].map(|populated| (boundary, populated)))
        {
            failure(
                counter(legacy),
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
                set(legacy),
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
                r.admit_committed(
                    r.ticket(),
                    remote(1),
                    false,
                    |log, sequence| {
                        assert_eq!(log.version().get(1), prior.last().copied().unwrap_or(1));
                        assert_eq!(sequence, 0);
                        Err(LocalError::Io(io::Error::other("commit failed")))
                    },
                    |_| Ok(())
                ),
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
