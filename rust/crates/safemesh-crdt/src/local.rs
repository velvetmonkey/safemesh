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
///         LocalError::StoreExists => "store exists",
///         LocalError::RecoveryRequired => "recovery",
///         LocalError::UnsupportedFormat { .. } => "unsupported format",
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
    /// A fresh writer store already exists at this root for this writer.
    StoreExists,
    /// Store metadata is damaged or disagrees with its durable history.
    RecoveryRequired,
    /// A stored wrapper has a version newer than this reader understands.
    UnsupportedFormat {
        file: &'static str,
        version: u8,
    },
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
        default_limit: bool,
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
            Self::StoreExists => f.write_str("local store already exists for this writer; restart it, or create with a new root or writer ID"),
            Self::RecoveryRequired => f.write_str("local store requires recovery: its writer fence, journal header, rollback counter or migration files are damaged or do not agree"),
            Self::UnsupportedFormat { file, version } => write!(f, "unsupported {file} version {version}; highest version this build reads is 1; use a newer SafeMesh"),
            Self::CounterWidth(error) => error.fmt(f),
            Self::Configuration => f.write_str("invalid or mismatched local writer configuration"),
            Self::InvalidRecord(error) => error.fmt(f),
            Self::History(error) => write!(f, "local history wire validation failed: {error}"),
            Self::InvalidHistory => {
                f.write_str("local history failed replay or sequence validation")
            }
            Self::AncestorSync { path, source } => write!(f, "AncestorSync: cannot sync store ancestor {}: {source}", path.display()),
            Self::Io(error) => write!(f, "local store I/O failed: {error}"),
            Self::RecordLimitExceeded { max_records, default_limit } => {
                write!(f, "local history exceeds restart record budget: RecordLimitExceeded: {max_records}")?;
                if *default_limit { f.write_str("; pass DecodeLimits::UNLIMITED to reopen") } else { Ok(()) }
            }
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

const FENCE_MAGIC: &[u8; 8] = b"SMFENCE1";
const TRANSACTION_MAGIC: &[u8; 8] = b"SMTRANS1";

fn fence_fields(bytes: &[u8]) -> Result<usize, LocalError> {
    if bytes.len() == 24 {
        return Ok(0);
    }
    if bytes.len() >= 9 && &bytes[..8] == FENCE_MAGIC {
        if bytes[8] > 1 {
            return Err(LocalError::UnsupportedFormat {
                file: "fence",
                version: bytes[8],
            });
        }
        if bytes[8] == 1 && bytes.len() == 33 {
            return Ok(9);
        }
    }
    Err(LocalError::RecoveryRequired)
}

// A valid legacy transaction has an EventLog tag at byte 24. This
// discriminator takes precedence even when its writers word equals the magic.
fn transaction_fields(bytes: &[u8]) -> Result<usize, LocalError> {
    if bytes.len() >= 25 && matches!(bytes[24], 0x02 | 0x03) {
        return Ok(0);
    }
    if bytes.len() >= 9 && &bytes[..8] == TRANSACTION_MAGIC {
        if bytes[8] > 1 {
            return Err(LocalError::UnsupportedFormat {
                file: "transaction",
                version: bytes[8],
            });
        }
        if bytes[8] == 1 && bytes.len() >= 34 {
            return Ok(9);
        }
    }
    if bytes.len() < 24 {
        return Err(LocalError::RecoveryRequired);
    }
    Ok(0)
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
            let at = fence_fields(&bytes)?;
            let word = |i| u64::from_le_bytes(bytes[at + i..at + i + 8].try_into().unwrap());
            if word(0) != config.writers || word(8) != config.writer {
                return Err(LocalError::Configuration);
            }
            if held {
                return Err(LocalError::StoreExists);
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
        let mut bytes = Vec::new();
        self.fence.seek(SeekFrom::Start(0))?;
        self.fence.read_to_end(&mut bytes)?;
        let at = fence_fields(&bytes)?;
        self.fence.seek(SeekFrom::Start((at + 16) as u64))?;
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
    let bytes = read_prefix(path, (33 + frame_prefix::<D>()) as u64)?;
    let at = transaction_fields(&bytes)?;
    let mut cursor = crate::WireCursor::new(&bytes[at..]);
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
            return Err(LocalError::StoreExists);
        }
        // The counter lives beside the store. Check it only after canonicalizing
        // the root, and before creating this writer's fence or journal.
        rollback::Counter::ensure_absent(&root, config)?;
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

    /// Commit local deltas as one durable batch. The counter records the final
    /// sequence before any journal entry is written. An interrupted batch can
    /// therefore require recovery, as an interrupted single append can.
    pub fn append_batch(
        &mut self,
        ticket: WriteTicket,
        deltas: impl IntoIterator<Item = C::Delta>,
    ) -> Result<Vec<Record<C::Delta>>, LocalError>
    where
        C: Clone,
    {
        let deltas: Vec<_> = deltas.into_iter().collect();
        if deltas.is_empty() {
            return Ok(Vec::new());
        }
        if self.inner.peer_writer_ahead {
            return Err(LocalError::PeerWriterAhead);
        }
        let count = u64::try_from(deltas.len()).map_err(|_| LocalError::Exhausted)?;
        let last = self
            .inner
            .last_sequence
            .checked_add(count)
            .ok_or(LocalError::Exhausted)?;
        let first = self.inner.last_sequence + 1;
        let mut state = self.inner.state.clone();
        let mut log = self.inner.log.clone();
        let mut records = Vec::with_capacity(deltas.len());
        let mut entries = Vec::with_capacity(deltas.len());
        for (index, delta) in deltas.into_iter().enumerate() {
            let sequence = first + index as u64;
            let record = Record {
                id: RecordId {
                    replica: self.inner.config.writer,
                    sequence,
                },
                delta,
            };
            if refuses(
                self.inner.context(ticket, true),
                record.id,
                record.delta.owned_payload(),
            ) {
                return Err(LocalError::Refused);
            }
            if log.insert_record(&state, record.clone()) != Admission::Accepted {
                return Err(LocalError::Refused);
            }
            state.apply_delta(record.delta.clone());
            if matches!(self.store, Store::Journal { .. }) {
                entries.push(journal::entry(
                    sequence,
                    &record.to_wire_bytes().map_err(encode_error)?,
                )?);
            }
            records.push(record);
        }
        // Keep the lock but disable writes before any uncertain persistence.
        self.inner.held = false;
        if let Some(counter) = &mut self.rollback {
            counter.advance_batch(first, last)?;
        }
        match &mut self.store {
            Store::Transaction(path) => Self::commit(path, self.inner.config, &log, last)?,
            Store::Journal { file, len } => {
                use std::os::unix::fs::FileExt;
                let mut offset = *len;
                for (index, bytes) in entries.iter().enumerate() {
                    file.write_all_at(bytes, offset)?;
                    offset += bytes.len() as u64;
                    if index == 0 {
                        persistence::checkpoint(21)?;
                    }
                }
                persistence::checkpoint(13)?;
                file.sync_data()?;
                persistence::checkpoint(14)?;
                *len = offset;
            }
        }
        self.inner.state = state;
        self.inner.log = log;
        self.inner.last_sequence = last;
        self.inner.held = true;
        Ok(records)
    }
}
impl<C: Crdt> DurableReplica<C>
where
    C::Delta: OwnedDelta + Clone + PartialEq + WireEncode + WireDecode + WireSchema,
{
    fn restart(root: &Path, config: WriterConfig, state: C) -> Result<Self, LocalError> {
        Self::restart_with_limit_origin(root, config, state, crate::DecodeLimits::default(), true)
    }

    // `max_collection_elements: None` keeps the ordinary stored-byte budget below.
    fn restart_with_limits(
        root: &Path,
        config: WriterConfig,
        state: C,
        limits: crate::DecodeLimits,
    ) -> Result<Self, LocalError> {
        Self::restart_with_limit_origin(root, config, state, limits, false)
    }

    fn restart_with_limit_origin(
        root: &Path,
        config: WriterConfig,
        state: C,
        limits: crate::DecodeLimits,
        default_limit: bool,
    ) -> Result<Self, LocalError> {
        config.validate().map_err(|_| LocalError::Configuration)?;
        let root = root.canonicalize()?;
        let fence = Self::lock_restart_fence(&root, config.writer)?;
        Self::restart_locked(&root, fence, config, state, limits, default_limit)
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
        default_limit: bool,
    ) -> Result<Self, LocalError> {
        let mut bytes = Vec::new();
        fence.file().read_to_end(&mut bytes)?;
        let at = fence_fields(&bytes)?;
        let word = |i| u64::from_le_bytes(bytes[at + i..at + i + 8].try_into().unwrap());
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
                return Err(LocalError::RecordLimitExceeded {
                    max_records,
                    default_limit,
                });
            }
        }
        let decode_error = |error| match error {
            crate::DecodeError::Wire(error) => LocalError::History(error),
            crate::DecodeError::RecordLimitExceeded { max_records }
            | crate::DecodeError::DefaultRecordLimitExceeded { max_records } => {
                LocalError::RecordLimitExceeded {
                    max_records,
                    default_limit,
                }
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
                        return Err(LocalError::RecordLimitExceeded {
                            max_records,
                            default_limit,
                        });
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
        let mut replica =
            Self::restart_locked(&root, fence, config, state, Default::default(), true)?;
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
    let at = transaction_fields(&bytes)?;
    if bytes.len() < at + 24 {
        return Err(LocalError::RecoveryRequired);
    }
    let word = |i| u64::from_le_bytes(bytes[at + i..at + i + 8].try_into().unwrap());
    if word(0) != config.writers || word(8) != config.writer {
        return Err(LocalError::Configuration);
    }
    Ok(CommittedTransaction {
        config,
        last_sequence: word(16),
        log_bytes: bytes[at + 24..].to_vec(),
    })
}

impl DurableReplica<GCounter> {
    /// Reacquire ownership, validate the committed history and replay fresh state.
    /// Any error returns no replica and grants no write ticket.
    pub fn restart_counter(root: &Path, config: WriterConfig) -> Result<Self, LocalError> {
        config.validate().map_err(|_| LocalError::Configuration)?;
        let n = usize::try_from(config.writers).map_err(|_| LocalError::Configuration)?;
        Self::restart(
            root,
            config,
            GCounter::try_new(n).map_err(LocalError::CounterWidth)?,
        )
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
        Self::restart_counter_from_store_with_origin(
            root,
            writer,
            crate::DecodeLimits::default(),
            true,
        )
    }
    /// [`restart_counter_from_store`](Self::restart_counter_from_store) with the
    /// restart budget of [`restart_counter_with_limits`](Self::restart_counter_with_limits).
    pub fn restart_counter_from_store_with_limits(
        root: &Path,
        writer: u64,
        limits: crate::DecodeLimits,
    ) -> Result<Self, LocalError> {
        Self::restart_counter_from_store_with_origin(root, writer, limits, false)
    }

    fn restart_counter_from_store_with_origin(
        root: &Path,
        writer: u64,
        limits: crate::DecodeLimits,
        default_limit: bool,
    ) -> Result<Self, LocalError> {
        let root = root.canonicalize()?;
        let fence = Self::lock_restart_fence(&root, writer)?;
        // The writer lock covers the metadata read and the same checked replay
        // used by restart_counter. A missing or truncated transaction is not fresh.
        let (bytes, at) = match store_format(&root, writer)? {
            Format::Transaction => {
                let bytes = read_prefix(&root.join(format!("writer-{writer}.transaction")), 34)?;
                let at = transaction_fields(&bytes)?;
                (bytes, at)
            }
            Format::Journal => {
                let bytes = read_prefix(&journal::path(&root, writer), journal::HEADER as u64)?;
                journal::check_magic(&bytes)?;
                if bytes.len() < journal::HEADER {
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
            default_limit,
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
    /// Append many increasing local tallies with one counter write and one
    /// journal sync. An empty batch has no effect.
    pub fn bump_batch(
        &mut self,
        ticket: WriteTicket,
        tallies: impl IntoIterator<Item = u64>,
    ) -> Result<Vec<Record<GCounterDelta>>, LocalError> {
        let replica = self.inner.config.writer as usize;
        self.append_batch(
            ticket,
            tallies
                .into_iter()
                .map(|tally| GCounterDelta { replica, tally }),
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
#[path = "durable_tests.rs"]
mod durable_tests;
