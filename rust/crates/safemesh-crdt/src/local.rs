// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0
//! Linux/local-filesystem packet-A adapter. All writers for a replica set must
//! use the same directory and fixed configuration. Keep fence files in place.
//! LocalReplica acknowledges in memory; DurableReplica commits before acknowledgement
//! and offers checked ordinary restart of an existing committed store.
use crate::{
    ownership::*, Admission, Crdt, EventLog, GCounter, GCounterDelta, OrSet, OrSetDelta, Record,
    RecordId, ResourceDimension, ResourceLimits, WireDecode, WireEncode, WireError, WireSchema,
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
    History(WireError),
    InvalidHistory,
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
/// This in-memory adapter owns writer-lease validation, not resource policy;
/// its retained `EventLog` has legacy unconfigured admission. Use
/// `DurableReplica` when the local adapter must own configured history limits.
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
        Read::by_ref(&mut fence).take(25).read_to_end(&mut bytes)?;
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
        Self::read_with_limits(root, config, ResourceLimits::default())
    }
    /// Read at most the configured frame allowance plus the unchanged 24-byte wrapper.
    pub fn read_with_limits(
        root: &Path,
        config: WriterConfig,
        limits: ResourceLimits,
    ) -> Result<Self, LocalError> {
        config.validate().map_err(|_| LocalError::Configuration)?;
        let file = File::open(transaction_path(root, config))?;
        let mut bytes = Vec::new();
        file.take((limits.history_encoded_bytes as u64).saturating_add(25))
            .read_to_end(&mut bytes)?;
        limits
            .check(
                ResourceDimension::HistoryEncodedBytes,
                bytes.len().saturating_sub(24),
            )
            .map_err(|e| LocalError::History(e.into()))?;
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
/// instance's writes, retaining its fence until drop. Use the explicit restart
/// constructors for existing stores; fresh constructors never reset a store.
pub struct DurableReplica<C: Crdt> {
    inner: LocalReplica<C>,
    path: PathBuf,
    limits: ResourceLimits,
    encoded_bytes: usize,
    payload_size: fn(&C::Delta, ResourceLimits) -> Result<usize, WireError>,
}
impl<C: Crdt> DurableReplica<C>
where
    C::Delta: OwnedDelta + Clone + PartialEq + WireEncode + WireSchema,
{
    fn fresh(
        root: &Path,
        config: WriterConfig,
        state: C,
        limits: ResourceLimits,
        payload_size: fn(&C::Delta, ResourceLimits) -> Result<usize, WireError>,
    ) -> Result<Self, LocalError> {
        let encoded_bytes = EventLog::for_crdt(&state)
            .to_wire_bytes()
            .map_err(LocalError::History)?
            .len();
        limits
            .check(ResourceDimension::HistoryEncodedBytes, encoded_bytes)
            .map_err(|e| LocalError::History(e.into()))?;
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
        let mut replica = Self {
            inner,
            path,
            limits,
            encoded_bytes,
            payload_size,
        };
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
    /// Development defaults are placeholders; operator policy can be changed
    /// without deleting retained history. A tighter policy controls future work.
    pub fn set_limits(&mut self, limits: ResourceLimits) {
        self.limits = limits;
    }
    pub fn limits(&self) -> ResourceLimits {
        self.limits
    }

    fn admit(
        &mut self,
        ticket: WriteTicket,
        record: Record<C::Delta>,
        local: bool,
    ) -> Result<Admission, LocalError> {
        // Check caller-owned payload sizes before cloning or encoding anything.
        // Only the two built-in constructors can install this size function.
        let payload =
            (self.payload_size)(&record.delta, self.limits).map_err(LocalError::History)?;
        let check = |dimension, requested| {
            self.limits
                .check(dimension, requested)
                .map_err(|e| LocalError::History(e.into()))
        };
        check(ResourceDimension::PerRecordPayloadBytes, payload)?;
        let existing = self.inner.log.records().iter().any(|r| r.id == record.id);
        let requested = self
            .inner
            .log
            .records()
            .len()
            .saturating_add(usize::from(!existing));
        check(ResourceDimension::HistoryRecordCount, requested)?;
        check(
            ResourceDimension::WriterReplicaCount,
            self.inner.config.writers as usize,
        )?;
        let encoded_bytes = self.encoded_bytes.saturating_add(if existing {
            0
        } else {
            payload.saturating_add(25)
        });
        check(ResourceDimension::HistoryEncodedBytes, encoded_bytes)?;
        if !local {
            check(ResourceDimension::RecordsPerSyncBatch, 1)?;
            check(
                ResourceDimension::BytesPerSyncBatch,
                payload.saturating_add(21),
            )?;
        }
        let path = &self.path;
        let config = self.inner.config;
        let outcome = self
            .inner
            .admit_committed(ticket, record, local, |log, sequence| {
                Self::commit(path, config, log, sequence)
            })?;
        if outcome == Admission::Accepted {
            self.encoded_bytes = encoded_bytes;
        }
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
        (self.payload_size)(&delta, self.limits).map_err(LocalError::History)?;
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
    fn restart(
        root: &Path,
        config: WriterConfig,
        state: C,
        limits: ResourceLimits,
        payload_size: fn(&C::Delta, ResourceLimits) -> Result<usize, WireError>,
    ) -> Result<Self, LocalError> {
        config.validate().map_err(|_| LocalError::Configuration)?;
        let root = root.canonicalize()?;
        // Opening without create is deliberate: missing ownership is not a new store.
        let mut fence = OpenOptions::new()
            .read(true)
            .write(true)
            .open(root.join(format!("writer-{}.fence", config.writer)))?;
        match fence.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(LocalError::Refused),
            Err(TryLockError::Error(e)) => return Err(e.into()),
        }
        let mut bytes = Vec::new();
        Read::by_ref(&mut fence).take(25).read_to_end(&mut bytes)?;
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
        // The lock covers reading, checking and replaying the complete transaction.
        let transaction = CommittedTransaction::read_with_limits(&root, config, limits)?;
        let log = EventLog::<C::Delta>::from_wire_bytes_for_with_limits(
            &transaction.log_bytes,
            &state,
            limits,
        )
        .map_err(LocalError::History)?;
        let mut inner = LocalReplica {
            config,
            fence,
            held: true,
            generation,
            log: EventLog::for_crdt(&state),
            state,
            last_sequence: 0,
        };
        // Compose packet A's corpus-bound ownedStep with M1 admission/replay.
        // This candidate is private until every record and allocation check passes.
        for record in log.records() {
            if inner.admit(inner.ticket(), record.clone(), false)? != Admission::Accepted {
                return Err(LocalError::InvalidHistory);
            }
        }
        if inner.last_sequence != transaction.last_sequence {
            return Err(LocalError::InvalidHistory);
        }
        // A ticket from before restart must not authorize the newly acquired lease.
        inner.renew(inner.ticket())?;
        Ok(Self {
            inner,
            path: transaction_path(&root, config),
            limits,
            encoded_bytes: transaction.log_bytes.len(),
            payload_size,
        })
    }
}

fn counter_payload_size(_: &GCounterDelta, limits: ResourceLimits) -> Result<usize, WireError> {
    limits.check(ResourceDimension::PerRecordPayloadBytes, 17)?;
    Ok(17)
}
fn set_payload_size(
    delta: &OrSetDelta<String, u64>,
    limits: ResourceLimits,
) -> Result<usize, WireError> {
    let size = match delta {
        OrSetDelta::Add { element, .. } => {
            limits.check(ResourceDimension::LiveCarrierEntries, 1)?;
            element.len().saturating_add(13)
        }
        OrSetDelta::Remove { tokens } => {
            limits.check(ResourceDimension::RetainedTombstoneCount, tokens.len())?;
            tokens.len().saturating_mul(8).saturating_add(5)
        }
    };
    limits.check(ResourceDimension::PerRecordPayloadBytes, size)?;
    Ok(size)
}

impl DurableReplica<GCounter> {
    /// Reacquire ownership, validate the committed history and replay fresh state.
    /// Any error returns no replica and grants no write ticket.
    /// Compatibility constructor using development-placeholder defaults.
    pub fn restart_counter(root: &Path, config: WriterConfig) -> Result<Self, LocalError> {
        Self::restart_counter_with_limits(root, config, ResourceLimits::default())
    }
    /// Construct with explicit operator resource policy; no history is reclaimed.
    pub fn restart_counter_with_limits(
        root: &Path,
        config: WriterConfig,
        limits: ResourceLimits,
    ) -> Result<Self, LocalError> {
        limits
            .check(
                ResourceDimension::WriterReplicaCount,
                usize::try_from(config.writers).map_err(|_| LocalError::Configuration)?,
            )
            .map_err(|e| LocalError::History(e.into()))?;
        config.validate().map_err(|_| LocalError::Configuration)?;
        let n = usize::try_from(config.writers).map_err(|_| LocalError::Configuration)?;
        Self::restart(root, config, GCounter::new(n), limits, counter_payload_size)
    }
    /// Compatibility constructor using development-placeholder defaults.
    pub fn counter(root: &Path, config: WriterConfig) -> Result<Self, LocalError> {
        Self::counter_with_limits(root, config, ResourceLimits::default())
    }
    /// Construct with explicit operator resource policy; no history is reclaimed.
    pub fn counter_with_limits(
        root: &Path,
        config: WriterConfig,
        limits: ResourceLimits,
    ) -> Result<Self, LocalError> {
        limits
            .check(
                ResourceDimension::WriterReplicaCount,
                usize::try_from(config.writers).map_err(|_| LocalError::Configuration)?,
            )
            .map_err(|e| LocalError::History(e.into()))?;
        config.validate().map_err(|_| LocalError::Configuration)?;
        let n = usize::try_from(config.writers).map_err(|_| LocalError::Configuration)?;
        Self::fresh(root, config, GCounter::new(n), limits, counter_payload_size)
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
    /// Compatibility constructor using development-placeholder defaults.
    pub fn restart_utf8_set(root: &Path, config: WriterConfig) -> Result<Self, LocalError> {
        Self::restart_utf8_set_with_limits(root, config, ResourceLimits::default())
    }
    /// Construct with explicit operator resource policy; no history is reclaimed.
    pub fn restart_utf8_set_with_limits(
        root: &Path,
        config: WriterConfig,
        limits: ResourceLimits,
    ) -> Result<Self, LocalError> {
        limits
            .check(
                ResourceDimension::WriterReplicaCount,
                usize::try_from(config.writers).map_err(|_| LocalError::Configuration)?,
            )
            .map_err(|e| LocalError::History(e.into()))?;
        Self::restart(root, config, OrSet::new(), limits, set_payload_size)
    }
    /// Compatibility constructor using development-placeholder defaults.
    pub fn utf8_set(root: &Path, config: WriterConfig) -> Result<Self, LocalError> {
        Self::utf8_set_with_limits(root, config, ResourceLimits::default())
    }
    /// Construct with explicit operator resource policy; no history is reclaimed.
    pub fn utf8_set_with_limits(
        root: &Path,
        config: WriterConfig,
        limits: ResourceLimits,
    ) -> Result<Self, LocalError> {
        limits
            .check(
                ResourceDimension::WriterReplicaCount,
                usize::try_from(config.writers).map_err(|_| LocalError::Configuration)?,
            )
            .map_err(|e| LocalError::History(e.into()))?;
        Self::fresh(root, config, OrSet::new(), limits, set_payload_size)
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
        // Count borrowed entries before observed_tokens allocates its collection.
        let count = self
            .inner
            .state
            .adds()
            .iter()
            .filter(|(candidate, _)| candidate == element)
            .count();
        self.limits
            .check(ResourceDimension::RetainedTombstoneCount, count)
            .map_err(|e| LocalError::History(e.into()))?;
        self.limits
            .check(
                ResourceDimension::PerRecordPayloadBytes,
                count.saturating_mul(8).saturating_add(5),
            )
            .map_err(|e| LocalError::History(e.into()))?;
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
                        log.admit_with(record, |d| state.apply_delta(d.clone())),
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
    #[test]
    fn durable_1100_bumps_policy_refusal_then_append_export_restart() {
        use crate::{ExportCursor, LimitedEventLog, ResourceLimit};
        let root = root();
        let config = WriterConfig {
            writers: 1,
            writer: 0,
        };
        let mut limits = ResourceLimits {
            history_record_count: 1000,
            ..ResourceLimits::default()
        };
        let mut r = DurableReplica::counter_with_limits(&root, config, limits).unwrap();
        for tally in 0..1100 {
            let result = r.bump(r.ticket(), tally);
            if tally < 1000 {
                assert_eq!(result.unwrap().id.sequence, tally + 1);
            } else {
                assert!(matches!(
                    result,
                    Err(LocalError::History(WireError::ResourceLimit(
                        ResourceLimit {
                            dimension: ResourceDimension::HistoryRecordCount,
                            limit: 1000,
                            requested: 1001
                        }
                    )))
                ));
            }
        }
        assert_eq!(r.log().records().len(), 1000);
        let before = r.log().to_wire_bytes().unwrap();
        assert_eq!(
            CommittedTransaction::read_with_limits(&root, config, limits)
                .unwrap()
                .log_bytes,
            before
        );
        assert_eq!(
            EventLog::<GCounterDelta>::from_wire_bytes_for_with_limits(
                &before,
                &GCounter::new(1),
                limits
            )
            .unwrap()
            .records()
            .len(),
            1000
        );
        limits.history_record_count = 1100;
        r.set_limits(limits);
        assert_eq!(r.bump(r.ticket(), 1000).unwrap().id.sequence, 1001);
        let mut export = LimitedEventLog::<GCounterDelta>::new(Some(1), limits).unwrap();
        for record in r.log().records() {
            export.admit_wire(&record.to_wire_bytes().unwrap()).unwrap();
        }
        let mut cursor = ExportCursor::default();
        let mut exported = Vec::new();
        while let Some(chunk) = export.export_chunk(cursor).unwrap() {
            exported.extend_from_slice(chunk.bytes);
            cursor = chunk.next;
        }
        let expected: Vec<u8> = r
            .log()
            .records()
            .iter()
            .flat_map(|r| r.to_wire_bytes().unwrap())
            .collect();
        assert_eq!(exported, expected);
        let bytes = r.log().to_wire_bytes().unwrap();
        let transaction = fs::read(transaction_path(&root, config)).unwrap();
        let mut expected_transaction = [
            1u64.to_le_bytes(),
            0u64.to_le_bytes(),
            1001u64.to_le_bytes(),
        ]
        .concat();
        expected_transaction.extend_from_slice(&bytes);
        assert_eq!(transaction, expected_transaction);
        drop(r);
        let r = DurableReplica::restart_counter_with_limits(&root, config, limits).unwrap();
        assert_eq!(r.log().to_wire_bytes().unwrap(), bytes);
        assert_eq!(r.state().state(), &[1000]);
        assert_eq!(r.inner.last_sequence, 1001);
    }

    #[test]
    fn durable_policy_inputs_and_refusal_preserve_ticket_and_bytes() {
        use crate::ResourceLimit;
        let root = root();
        let config = WriterConfig {
            writers: 1,
            writer: 0,
        };
        let mut limits = ResourceLimits::default();
        limits.writer_replica_count = 0;
        assert!(matches!(
            DurableReplica::counter_with_limits(&root, config, limits),
            Err(LocalError::History(WireError::ResourceLimit(
                ResourceLimit {
                    dimension: ResourceDimension::WriterReplicaCount,
                    limit: 0,
                    requested: 1
                }
            )))
        ));
        limits.writer_replica_count = 1;
        let mut r = DurableReplica::utf8_set_with_limits(&root, config, limits).unwrap();
        let bytes = fs::read(transaction_path(&root, config)).unwrap();
        limits.per_record_payload_bytes = 13;
        r.set_limits(limits);
        assert!(matches!(
            r.add(r.ticket(), "x".into()),
            Err(LocalError::History(WireError::ResourceLimit(
                ResourceLimit {
                    dimension: ResourceDimension::PerRecordPayloadBytes,
                    limit: 13,
                    requested: 14
                }
            )))
        ));
        assert_eq!(fs::read(transaction_path(&root, config)).unwrap(), bytes);
        limits.per_record_payload_bytes = 14;
        r.set_limits(limits);
        let added = r.add(r.ticket(), "x".into()).unwrap();
        assert_eq!(added.id.sequence, 1);
        let before = r.log().to_wire_bytes().unwrap();
        limits.retained_tombstone_count = 0;
        r.set_limits(limits);
        assert!(matches!(
            r.remove(r.ticket(), &"x".into()),
            Err(LocalError::History(WireError::ResourceLimit(
                ResourceLimit {
                    dimension: ResourceDimension::RetainedTombstoneCount,
                    limit: 0,
                    requested: 1
                }
            )))
        ));
        assert_eq!(r.log().to_wire_bytes().unwrap(), before);
        limits.retained_tombstone_count = 1;
        limits.records_per_sync_batch = 0;
        r.set_limits(limits);
        assert!(matches!(
            r.receive(r.ticket(), added.clone()),
            Err(LocalError::History(WireError::ResourceLimit(
                ResourceLimit {
                    dimension: ResourceDimension::RecordsPerSyncBatch,
                    limit: 0,
                    requested: 1
                }
            )))
        ));
        limits.records_per_sync_batch = 1;
        limits.bytes_per_sync_batch = added.to_wire_bytes().unwrap().len() - 1;
        r.set_limits(limits);
        assert!(matches!(
            r.receive(r.ticket(), added.clone()),
            Err(LocalError::History(WireError::ResourceLimit(
                ResourceLimit {
                    dimension: ResourceDimension::BytesPerSyncBatch,
                    ..
                }
            )))
        ));
        limits.bytes_per_sync_batch += 1;
        r.set_limits(limits);
        assert_eq!(r.receive(r.ticket(), added).unwrap(), Admission::Duplicate);
        limits.history_encoded_bytes = r.encoded_bytes;
        r.set_limits(limits);
        assert!(matches!(
            r.remove(r.ticket(), &"x".into()),
            Err(LocalError::History(WireError::ResourceLimit(
                ResourceLimit {
                    dimension: ResourceDimension::HistoryEncodedBytes,
                    ..
                }
            )))
        ));
        limits.history_encoded_bytes += 38;
        r.set_limits(limits);
        r.remove(r.ticket(), &"x".into()).unwrap();
        assert_eq!(r.encoded_bytes, r.log().to_wire_bytes().unwrap().len());
        let bytes = r.log().to_wire_bytes().unwrap();
        drop(r);
        limits.history_encoded_bytes = bytes.len() - 1;
        assert!(matches!(
            DurableReplica::restart_utf8_set_with_limits(&root, config, limits),
            Err(LocalError::History(WireError::ResourceLimit(
                ResourceLimit {
                    dimension: ResourceDimension::HistoryEncodedBytes,
                    ..
                }
            )))
        ));
        limits.history_encoded_bytes += 1;
        let r = DurableReplica::restart_utf8_set_with_limits(&root, config, limits).unwrap();
        assert_eq!(r.log().to_wire_bytes().unwrap(), bytes);
        assert_eq!(r.state().tombstones().len(), 1);
    }

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
            assert!(matches!(
                restart_and_write(
                    kind,
                    &root,
                    WriterConfig {
                        writers: 3,
                        writer: 0
                    }
                ),
                Err(LocalError::Configuration)
            ));
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
                let mut log = EventLog::for_crdt(&GCounter::new(2));
                assert_eq!(
                    log.insert_record(Record {
                        id: RecordId {
                            replica: 0,
                            sequence: 1
                        },
                        delta: GCounterDelta {
                            replica: 1,
                            tally: 5
                        },
                    }),
                    Admission::Accepted
                );
                DurableReplica::<GCounter>::commit(&path, config(), &log, 1).unwrap();
            } else {
                let mut log = EventLog::for_crdt(&OrSet::<String, u64>::new());
                assert_eq!(
                    log.insert_record(Record {
                        id: RecordId {
                            replica: 0,
                            sequence: 1
                        },
                        delta: OrSetDelta::Add {
                            element: "foreign".into(),
                            token: 3
                        },
                    }),
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
