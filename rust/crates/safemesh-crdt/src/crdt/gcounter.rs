// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

use crate::{ownership, Crdt, MergeError, Mergeable, RecordId, WireError};
use alloc::{vec, vec::Vec};

/// Delta for a grow-only counter: one replica coordinate and its asserted tally.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GCounterDelta {
    pub replica: usize,
    pub tally: u64,
}

/// Rejection of a counter delta with an invalid replica coordinate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoordinateError {
    /// The index is outside `0..replica_count`; the counter is unchanged.
    ReplicaOutOfRange {
        replica: usize,
        replica_count: usize,
    },
}

/// A grow-only counter: one tally per replica, join = pointwise max.
///
/// Model: `Crdt.GCounter ι = ι → ℕ` with the Pi join-semilattice. Applying a
/// delta is joining it in, so delivery order and redelivery are irrelevant —
/// `SafeMesh.delta_dissemination_sec` — and a replica that received bumps `B`
/// holds, per coordinate, the max over that coordinate's bumps —
/// `SafeMesh.deltaGCounter_correct`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GCounter {
    counts: Vec<u64>,
}

impl GCounter {
    /// Fresh counter for `n` replicas — the lattice bottom `⊥` (all zeros).
    pub fn new(n: usize) -> Self {
        GCounter { counts: vec![0; n] }
    }

    /// Number of replica coordinates.
    pub fn len(&self) -> usize {
        self.counts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }

    /// Apply the delta `SafeMesh.deltaBump replica tally` — a single
    /// coordinate on the wire. Joining the delta is `max` at that coordinate
    /// (`Pi.single` joined into the state); applying the same bump again is a
    /// no-op (idempotence, `SafeMesh.delta_dissemination_sec`).
    ///
    /// Out-of-range `replica` is ignored (a malformed delta must not corrupt
    /// state; the Lean model has no such case — `Fin n` makes it unrepresentable).
    /// Use [`Self::try_apply_bump`] to receive an error for an invalid index.
    pub fn apply_bump(&mut self, replica: usize, tally: u64) {
        if let Some(c) = self.counts.get_mut(replica) {
            if tally > *c {
                *c = tally;
            }
        }
    }

    /// Apply a single-coordinate delta, returning an error for an invalid index.
    ///
    /// Checks the coordinate before mutation. On error, the entire counter is
    /// unchanged; on success, behaves exactly like [`Self::apply_bump`], including
    /// accepting an equal or lower tally as a no-op.
    pub fn try_apply_bump(&mut self, replica: usize, tally: u64) -> Result<(), CoordinateError> {
        if replica >= self.counts.len() {
            return Err(CoordinateError::ReplicaOutOfRange {
                replica,
                replica_count: self.counts.len(),
            });
        }
        self.apply_bump(replica, tally);
        Ok(())
    }

    /// Checked full-state merge: pointwise max, erroring on a replica-count
    /// mismatch instead of silently truncating to the shorter vector (WS1).
    /// This is the boundary-safe entry point for state received from an
    /// untrusted peer.
    pub fn try_merge(&mut self, other: &GCounter) -> Result<(), MergeError> {
        if self.counts.len() != other.counts.len() {
            return Err(MergeError::ReplicaCountMismatch {
                own: self.counts.len(),
                other: other.counts.len(),
            });
        }
        for (c, o) in self.counts.iter_mut().zip(other.counts.iter()) {
            if *o > *c {
                *c = *o;
            }
        }
        Ok(())
    }

    /// Full-state merge: pointwise max — `Crdt.gcounter_merge_apply`. A
    /// delta replica and a full-state replica fed the same bumps agree:
    /// `SafeMesh.deltaGCounter_matches_full`.
    ///
    /// Returns [`MergeError::ReplicaCountMismatch`] without changing state if
    /// the replica counts differ, just like [`GCounter::try_merge`].
    pub fn merge(&mut self, other: &GCounter) -> Result<(), MergeError> {
        self.try_merge(other)
    }

    /// Per-coordinate state (the `ι → ℕ` vector).
    pub fn state(&self) -> &[u64] {
        &self.counts
    }

    /// The counter's read: sum of per-replica tallies — `Crdt.gcounterValue`,
    /// a natural number with no upper bound in the model.
    ///
    /// The read is exact for every state this counter can hold: at most
    /// `isize::MAX` coordinates, each at most `u64::MAX`, sum to strictly less
    /// than `2^127`, so the `u128` total never wraps and is never clamped. A
    /// total that no longer fits a narrower integer is still returned whole;
    /// a caller that needs `u64` narrows it with `u64::try_from` and handles
    /// the failure itself.
    pub fn value(&self) -> u128 {
        self.counts.iter().map(|&tally| u128::from(tally)).sum()
    }
}

impl Mergeable for GCounter {
    fn merge(&mut self, other: &Self) -> Result<(), MergeError> {
        GCounter::merge(self, other)
    }
}

impl Crdt for GCounter {
    type Delta = GCounterDelta;

    fn validate_record(&self, id: RecordId, delta: &Self::Delta) -> Result<(), WireError> {
        ownership::check_counter_record(self.len(), id, delta)
            .map_err(|_| WireError::OwnershipViolation)
    }

    fn replica_count(&self) -> Option<usize> {
        Some(self.len())
    }

    fn apply_delta(&mut self, delta: Self::Delta) {
        self.apply_bump(delta.replica, delta.tally);
    }
}
