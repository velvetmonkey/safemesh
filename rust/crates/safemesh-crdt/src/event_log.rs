// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

use crate::{Crdt, Record, RecordId, VersionVector, WireError};
use alloc::{collections::BTreeMap, vec::Vec};

/// Full decoded payload equality distinguishes redelivery from an ID collision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission {
    Accepted,
    Duplicate,
    Collision,
    /// The log shape or record is incompatible with the supplied carrier.
    Invalid(WireError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppendError {
    SequenceExhausted,
    InvalidRecord(WireError),
}

/// Append-only, deduplicating event log for CRDT deltas.
/// Equality compares log data, excluding the in-memory first-admission binding flag.
#[derive(Clone, Debug)]
pub struct EventLog<D> {
    pub(super) replica_count: Option<usize>,
    pub(super) shape_bound: bool,
    pub(super) records: Vec<Record<D>>,
    pub(super) seen: BTreeMap<RecordId, usize>,
    pub(super) version: VersionVector,
}

// Binding an empty unbounded log during decode must not change payload equality:
// logs can themselves be nested in records and compared for deduplication.
impl<D: PartialEq> PartialEq for EventLog<D> {
    fn eq(&self, other: &Self) -> bool {
        self.replica_count == other.replica_count
            && self.records == other.records
            && self.seen == other.seen
            && self.version == other.version
    }
}

impl<D: Eq> Eq for EventLog<D> {}

impl<D> EventLog<D> {
    /// Create an unbound log. The first accepted record binds its carrier shape.
    /// Use `for_crdt` to declare a shape before any records are accepted.
    pub fn new() -> Self {
        EventLog {
            replica_count: None,
            shape_bound: false,
            records: Vec::new(),
            seen: BTreeMap::new(),
            version: VersionVector::new(),
        }
    }

    /// Create a log for a fixed replica domain, including an empty domain.
    pub fn with_replica_count(replica_count: usize) -> Self {
        Self {
            replica_count: Some(replica_count),
            shape_bound: true,
            ..Self::new()
        }
    }

    /// Bind persistence metadata to the CRDT's actual domain before recording.
    pub fn for_crdt<C: Crdt<Delta = D>>(state: &C) -> Self {
        Self {
            replica_count: state.replica_count(),
            shape_bound: true,
            ..Self::new()
        }
    }

    pub fn replica_count(&self) -> Option<usize> {
        self.replica_count
    }

    /// Allocate a fresh ID and admit a delta without applying it to `state`.
    /// Returns an error if the sequence is exhausted or admission refuses the
    /// record; in either case the log and state remain unchanged.
    pub fn append<C: Crdt<Delta = D>>(
        &mut self,
        state: &mut C,
        replica: u64,
        delta: D,
    ) -> Result<RecordId, AppendError>
    where
        D: PartialEq,
    {
        self.append_with(state, replica, delta, |_, _| {})
    }

    /// Allocate a fresh ID, then use the same gate as incoming records.
    pub fn append_with<C, F>(
        &mut self,
        state: &mut C,
        replica: u64,
        delta: D,
        apply: F,
    ) -> Result<RecordId, AppendError>
    where
        D: PartialEq,
        C: Crdt<Delta = D>,
        F: FnOnce(&mut C, &D),
    {
        let sequence = self
            .seen
            .range(
                RecordId {
                    replica,
                    sequence: 0,
                }..=RecordId {
                    replica,
                    sequence: u64::MAX,
                },
            )
            .next_back()
            .map(|(id, _)| id.sequence)
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(AppendError::SequenceExhausted)?;
        let id = RecordId { replica, sequence };
        let outcome = self.admit_with(state, Record { id, delta }, apply);
        if let Admission::Invalid(error) = outcome {
            return Err(AppendError::InvalidRecord(error));
        }
        debug_assert_eq!(outcome, Admission::Accepted);
        Ok(id)
    }

    pub fn merge_records<C: Crdt<Delta = D>, I>(&mut self, state: &C, records: I) -> Vec<Admission>
    where
        D: PartialEq,
        I: IntoIterator<Item = Record<D>>,
    {
        records
            .into_iter()
            .map(|r| self.insert_record(state, r))
            .collect()
    }

    /// Validate the log shape and record against the destination carrier, then
    /// check record identity. An unbound log binds only on acceptance.
    /// Only Accepted invokes `apply`, with the same carrier used for validation.
    /// An invalid fresh record leaves the log, version, and carrier unchanged.
    /// Duplicate/Collision retain their identity verdicts, but also run validation.
    /// The callback must be infallible and use the same delta interpretation as
    /// replay. This is an in-memory transition, not a crash-durability guarantee.
    #[must_use]
    pub fn admit_with<C, F>(&mut self, state: &mut C, record: Record<D>, apply: F) -> Admission
    where
        D: PartialEq,
        C: Crdt<Delta = D>,
        F: FnOnce(&mut C, &D),
    {
        let outcome = self.admission(state, &record);
        if outcome != Admission::Accepted {
            return outcome;
        }
        self.bind_shape(state);
        self.commit_record(record);
        apply(state, &self.records.last().expect("accepted record").delta);
        Admission::Accepted
    }

    // Only called after admission, or by the inert wire decoder after identity checks.
    pub(super) fn commit_record(&mut self, record: Record<D>) {
        let id = record.id;
        self.seen.insert(id, self.records.len());
        self.records.push(record);
        self.version.observe(id);
        self.advance_contiguous_version(id.replica);
    }

    // Shared read-only decision for admission and transactional preflight.
    // Consult the identity index without copying any admitted payloads.
    pub(super) fn admission<C: Crdt<Delta = D>>(&self, state: &C, record: &Record<D>) -> Admission
    where
        D: PartialEq,
    {
        let identity = self.identity_admission(record);
        let record_validation = state.validate_record(record.id, &record.delta);
        let shape_validation = if self.shape_bound {
            self.validate_shape(state)
        } else {
            Ok(())
        };
        let validation = shape_validation.and(record_validation);
        match (identity, validation) {
            (Admission::Accepted, Err(error)) => Admission::Invalid(error),
            (outcome, _) => outcome,
        }
    }

    // Also used by checked restore, where even an empty decoded log has a shape.
    pub(super) fn validate_shape<C: Crdt<Delta = D>>(&self, state: &C) -> Result<(), WireError> {
        match (state.replica_count(), self.replica_count) {
            (Some(expected), Some(actual)) if expected != actual => {
                Err(WireError::ReplicaCountMismatch { expected, actual })
            }
            (None, Some(_)) | (Some(_), None) => Err(WireError::ArityKindMismatch),
            _ => Ok(()),
        }
    }

    fn bind_shape<C: Crdt<Delta = D>>(&mut self, state: &C) {
        if !self.shape_bound {
            self.replica_count = state.replica_count();
            self.shape_bound = true;
        }
    }

    // Wire decoding has no carrier yet; checked loaders validate before replay.
    pub(super) fn identity_admission(&self, record: &Record<D>) -> Admission
    where
        D: PartialEq,
    {
        match self.seen.get(&record.id) {
            Some(&index) if self.records[index].delta == record.delta => Admission::Duplicate,
            Some(_) => Admission::Collision,
            None => Admission::Accepted,
        }
    }

    pub fn version(&self) -> &VersionVector {
        &self.version
    }

    pub fn records(&self) -> &[Record<D>] {
        &self.records
    }

    /// Admit without applying; `state` must be the carrier used for later replay.
    pub fn insert_record<C: Crdt<Delta = D>>(&mut self, state: &C, record: Record<D>) -> Admission
    where
        D: PartialEq,
    {
        let outcome = self.admission(state, &record);
        if outcome == Admission::Accepted {
            self.bind_shape(state);
            self.commit_record(record);
        }
        outcome
    }

    fn advance_contiguous_version(&mut self, replica: u64) {
        while let Some(next) = self.version.get(replica).checked_add(1) {
            if self.seen.contains_key(&RecordId {
                replica,
                sequence: next,
            }) {
                self.version.set(replica, next);
            } else {
                break;
            }
        }
    }
}

impl<D: Clone> EventLog<D> {
    /// Return admitted records outside the peer's positive contiguous prefixes
    /// and independently acknowledged sequence-zero records.
    pub fn since(&self, version: &VersionVector) -> Vec<Record<D>> {
        self.records
            .iter()
            .filter(|record| !version.includes(record.id))
            .cloned()
            .collect()
    }
}

impl<D> Default for EventLog<D> {
    fn default() -> Self {
        Self::new()
    }
}
