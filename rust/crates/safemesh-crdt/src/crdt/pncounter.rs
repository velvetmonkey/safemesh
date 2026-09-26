// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

use alloc::collections::TryReserveError;

use crate::{
    ownership, CoordinateError, Crdt, GCounter, MergeError, Mergeable, RecordId, WireError,
};

/// An increment/decrement counter: a pair of G-Counters (increments `P`,
/// decrements `N`), join = componentwise.
///
/// Model: `Crdt.PNCounter ι = (ι → ℕ) × (ι → ℕ)` with the Prod lattice.
/// Deltas bump one side, one coordinate (`SafeMesh.deltaBumpP` /
/// `deltaBumpN`); each side accumulates exactly as a delta G-Counter
/// (`SafeMesh.deltaPNCounter_correct_P` / `_N`), and equal state gives an
/// equal read (`SafeMesh.deltaPNCounter_value_matches`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PnCounter {
    p: GCounter,
    n: GCounter,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PnCounterDelta {
    Inc { replica: usize, tally: u64 },
    Dec { replica: usize, tally: u64 },
}

impl PnCounter {
    /// Fresh counter for `n` replicas — the lattice bottom `(⊥, ⊥)`.
    ///
    /// # Panics
    /// Panics if either side cannot be allocated. Use [`Self::try_new`] for a
    /// fallible constructor.
    pub fn new(n: usize) -> Self {
        Self::try_new(n).expect("counter width cannot be allocated")
    }

    /// Allocate both sides fallibly; if the second allocation fails, the first
    /// is released before returning the error.
    pub fn try_new(n: usize) -> Result<Self, TryReserveError> {
        Ok(PnCounter {
            p: GCounter::try_new(n)?,
            n: GCounter::try_new(n)?,
        })
    }

    /// Apply `SafeMesh.deltaBumpP replica tally` — increment side, one
    /// coordinate on the wire, `⊥` on the other side.
    /// Out-of-range indices are ignored; use [`Self::try_apply_inc`] for an error.
    pub fn apply_inc(&mut self, replica: usize, tally: u64) {
        self.p.apply_bump(replica, tally);
    }

    /// Apply an increment-side delta with a checked replica coordinate.
    ///
    /// Returns [`CoordinateError::ReplicaOutOfRange`] before any mutation if the
    /// index is invalid. Both sides remain unchanged on error. Valid input has
    /// the same effect as [`Self::apply_inc`].
    pub fn try_apply_inc(&mut self, replica: usize, tally: u64) -> Result<(), CoordinateError> {
        self.p.try_apply_bump(replica, tally)
    }

    /// Apply `SafeMesh.deltaBumpN replica tally` — decrement side.
    /// Out-of-range indices are ignored; use [`Self::try_apply_dec`] for an error.
    pub fn apply_dec(&mut self, replica: usize, tally: u64) {
        self.n.apply_bump(replica, tally);
    }

    /// Apply a decrement-side delta with a checked replica coordinate.
    ///
    /// Returns [`CoordinateError::ReplicaOutOfRange`] before any mutation if the
    /// index is invalid. Both sides remain unchanged on error. Valid input has
    /// the same effect as [`Self::apply_dec`].
    pub fn try_apply_dec(&mut self, replica: usize, tally: u64) -> Result<(), CoordinateError> {
        self.n.try_apply_bump(replica, tally)
    }

    /// Checked full-state merge: componentwise checked G-Counter merge,
    /// erroring on a replica-count mismatch on either side instead of silently
    /// truncating (WS1). Both sides are length-checked before any mutation, so
    /// the operation is all-or-nothing: on error `self` is left unchanged.
    pub fn try_merge(&mut self, other: &PnCounter) -> Result<(), MergeError> {
        if self.p.state().len() != other.p.state().len() {
            return Err(MergeError::ReplicaCountMismatch {
                own: self.p.state().len(),
                other: other.p.state().len(),
            });
        }
        if self.n.state().len() != other.n.state().len() {
            return Err(MergeError::ReplicaCountMismatch {
                own: self.n.state().len(),
                other: other.n.state().len(),
            });
        }
        self.p.try_merge(&other.p)?;
        self.n.try_merge(&other.n)?;
        Ok(())
    }

    /// Full-state merge: componentwise G-Counter merge —
    /// `Crdt.pncounter_merge_apply`.
    ///
    /// Returns [`MergeError::ReplicaCountMismatch`] without changing either
    /// side if the replica counts differ, just like [`PnCounter::try_merge`].
    pub fn merge(&mut self, other: &PnCounter) -> Result<(), MergeError> {
        self.try_merge(other)
    }

    /// Increment-side state.
    pub fn p_state(&self) -> &[u64] {
        self.p.state()
    }

    /// Decrement-side state.
    pub fn n_state(&self) -> &[u64] {
        self.n.state()
    }

    /// The counter's read: (sum of increments) − (sum of decrements) —
    /// `Crdt.pncounterValue`, an integer with no bound in the model.
    ///
    /// The read is exact for every representable state: each side's total is
    /// below `2^127` (see [`GCounter::value`]), so both fit in `i128` and their
    /// difference lies strictly inside `i128`'s range. No conversion here can
    /// change the sign or wrap; a positive total always reads positive.
    pub fn value(&self) -> i128 {
        let increments = i128::try_from(self.p.value()).expect("a G-Counter total is below 2^127");
        let decrements = i128::try_from(self.n.value()).expect("a G-Counter total is below 2^127");
        increments - decrements
    }
}

impl Mergeable for PnCounter {
    fn merge(&mut self, other: &Self) -> Result<(), MergeError> {
        PnCounter::merge(self, other)
    }
}

impl Crdt for PnCounter {
    type Delta = PnCounterDelta;

    fn validate_record(&self, id: RecordId, delta: &Self::Delta) -> Result<(), WireError> {
        ownership::check_counter_record(self.p.len(), id, delta)
            .map_err(|_| WireError::OwnershipViolation)
    }

    fn replica_count(&self) -> Option<usize> {
        Some(self.p.len())
    }

    fn apply_delta(&mut self, delta: Self::Delta) {
        match delta {
            PnCounterDelta::Inc { replica, tally } => self.apply_inc(replica, tally),
            PnCounterDelta::Dec { replica, tally } => self.apply_dec(replica, tally),
        }
    }
}
