// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

/// Stable identity for an event-log record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RecordId {
    pub replica: u64,
    pub sequence: u64,
}

impl RecordId {
    pub(crate) fn require_positive_sequence(
        self,
        kind: &'static str,
    ) -> Result<(), crate::WireError> {
        if self.sequence == 0 {
            Err(crate::WireError::ZeroSequenceRecord {
                replica: self.replica,
                kind,
            })
        } else {
            Ok(())
        }
    }
}

/// An event-log record carrying a CRDT delta.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record<D> {
    pub id: RecordId,
    pub delta: D,
}
