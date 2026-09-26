// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0
//! In-memory coordination, without filesystem durability or writer identity.
use crate::{
    Admission, AppendError, CollectionLimits, Crdt, DecodeError, DecodeLimits, EventLog, Record,
    VersionVector, WireDecode, WireEncode, WireError, WireSchema,
};
use alloc::vec::Vec;

/// The operation that failed; invalid admission remains a verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplicaError {
    RecordDecode(WireError),
    LogDecode(DecodeError),
    LogEncode(WireError),
    Append(AppendError),
}
impl core::fmt::Display for ReplicaError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::RecordDecode(e) | Self::LogEncode(e) | Self::LogDecode(DecodeError::Wire(e)) => {
                e.fmt(f)
            }
            Self::LogDecode(DecodeError::RecordLimitExceeded { max_records }) => {
                write!(f, "RecordLimitExceeded: {max_records}")
            }
            Self::Append(e) => e.fmt(f),
        }
    }
}
impl core::error::Error for ReplicaError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::RecordDecode(e) | Self::LogEncode(e) | Self::LogDecode(DecodeError::Wire(e)) => {
                Some(e)
            }
            Self::Append(e) => Some(e),
            Self::LogDecode(DecodeError::RecordLimitExceeded { .. }) => None,
        }
    }
}
/// Owned state and admitted history. A nonempty initial state need not be
/// reproducible from its initially empty log.
pub struct Replica<C: Crdt> {
    state: C,
    log: EventLog<C::Delta>,
}
impl<C: Crdt> Replica<C> {
    pub fn new(state: C) -> Self {
        let log = EventLog::for_crdt(&state);
        Self { state, log }
    }
    pub fn state(&self) -> &C {
        &self.state
    }
    pub fn log(&self) -> &EventLog<C::Delta> {
        &self.log
    }
    pub fn version(&self) -> &VersionVector {
        self.log.version()
    }
    pub fn since(&self, peer: &VersionVector) -> Vec<Record<C::Delta>>
    where
        C::Delta: Clone,
    {
        self.log.since(peer)
    }
    pub fn admit(&mut self, record: Record<C::Delta>) -> Admission
    where
        C::Delta: Clone + PartialEq,
    {
        self.log
            .admit_with(&mut self.state, record, |state, delta| {
                state.apply_delta(delta.clone())
            })
    }
    pub fn append(&mut self, author: u64, delta: C::Delta) -> Result<Record<C::Delta>, ReplicaError>
    where
        C::Delta: Clone + PartialEq,
    {
        let id = self
            .log
            .append_with(&mut self.state, author, delta.clone(), |state, delta| {
                state.apply_delta(delta.clone())
            })
            .map_err(ReplicaError::Append)?;
        Ok(Record { id, delta })
    }
    /// Decode only; no admission validation or mutation.
    pub fn inspect_record_bytes(
        bytes: &[u8],
        limits: CollectionLimits,
    ) -> Result<Record<C::Delta>, ReplicaError>
    where
        C::Delta: WireDecode,
    {
        Record::from_wire_bytes_with_collection_limits(bytes, limits)
            .map_err(ReplicaError::RecordDecode)
    }
    pub fn merge_record_bytes(
        &mut self,
        bytes: &[u8],
        limits: CollectionLimits,
    ) -> Result<Admission, ReplicaError>
    where
        C::Delta: Clone + PartialEq + WireDecode,
    {
        Ok(self.admit(Self::inspect_record_bytes(bytes, limits)?))
    }
    /// Validate the complete batch and preserve every input occurrence.
    pub fn decode_log_bytes(
        &self,
        bytes: &[u8],
        limits: DecodeLimits,
    ) -> Result<Vec<Record<C::Delta>>, ReplicaError>
    where
        C::Delta: Clone + PartialEq + WireDecode + WireSchema,
    {
        EventLog::records_from_wire_bytes_for_with_limits(bytes, &self.state, limits)
            .map_err(ReplicaError::LogDecode)
    }
    pub fn merge_log_bytes(
        &mut self,
        bytes: &[u8],
        limits: DecodeLimits,
    ) -> Result<Vec<Admission>, ReplicaError>
    where
        C::Delta: Clone + PartialEq + WireDecode + WireSchema,
    {
        let records = self.decode_log_bytes(bytes, limits)?;
        Ok(records
            .into_iter()
            .map(|record| self.admit(record))
            .collect())
    }
    pub fn log_bytes(&self) -> Result<Vec<u8>, ReplicaError>
    where
        C::Delta: WireEncode + WireSchema,
    {
        self.log.to_wire_bytes().map_err(ReplicaError::LogEncode)
    }
    /// Supply a matching empty/bottom state. Publish only a fully checked candidate.
    pub fn restore(mut state: C, bytes: &[u8], limits: DecodeLimits) -> Result<Self, ReplicaError>
    where
        C::Delta: Clone + PartialEq + WireDecode + WireSchema,
    {
        let log = EventLog::from_wire_bytes_for_with_limits(bytes, &state, limits)
            .map_err(ReplicaError::LogDecode)?;
        for record in log.records() {
            state.apply_delta(record.delta.clone());
        }
        Ok(Self { state, log })
    }
}
