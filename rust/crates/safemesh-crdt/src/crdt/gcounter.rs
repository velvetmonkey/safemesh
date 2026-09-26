// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

use crate::{ownership, Crdt, MergeError, Mergeable, RecordId, WireError};
use alloc::vec::Vec;

/// Delta for a grow-only counter: one replica coordinate and its asserted tally.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GCounterDelta {
    pub replica: usize,
    pub tally: u64,
}

/// Rejection of an invalid counter domain or replica coordinate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoordinateError {
    /// The requested constructor domain exceeds [`GCounter::MAX_REPLICAS`].
    ReplicaLimitExceeded { requested: usize, maximum: usize },
    /// The index is outside `0..replica_count`; the counter is unchanged.
    ReplicaOutOfRange {
        replica: usize,
        replica_count: usize,
    },
}

impl core::fmt::Display for CoordinateError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ReplicaLimitExceeded { requested, maximum } => {
                write!(f, "replica count {requested} exceeds maximum {maximum}")
            }
            Self::ReplicaOutOfRange {
                replica,
                replica_count,
            } => write!(
                f,
                "replica coordinate {replica} is outside 0..{replica_count}"
            ),
        }
    }
}

impl core::error::Error for CoordinateError {}

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
    /// Maximum constructor domain: 4,096 replicas, matching the default wire
    /// version-vector author limit. Storage is at most 32 KiB per G-counter
    /// (64 KiB per PN-counter), below the WASM and Python numeric limits.
    pub const MAX_REPLICAS: usize = 4_096;

    /// Fresh counter for `n` replicas — the lattice bottom `⊥` (all zeros).
    ///
    /// # Panics
    /// Panics above [`Self::MAX_REPLICAS`] or on allocator refusal. Use
    /// [`Self::try_new`] to receive a named error instead.
    pub fn new(n: usize) -> Self {
        Self::try_new(n).expect("counter width cannot be allocated")
    }

    /// Fresh counter with a domain bounded by [`Self::MAX_REPLICAS`].
    ///
    /// The 4,096-replica maximum matches the default wire author limit and
    /// bounds storage to 32 KiB. Larger counts return `ReplicaLimitExceeded`
    /// before allocation. Admitted counts use the allocator's zeroed path,
    /// like `vec![0; n]`, without an explicit page-touching initialization loop.
    /// As on main, admitted allocations use `vec![0; n]`; an out-of-memory
    /// allocator can abort. The cap bounds this request to 32 KiB.
    pub fn try_new(n: usize) -> Result<Self, CoordinateError> {
        if n > Self::MAX_REPLICAS {
            return Err(CoordinateError::ReplicaLimitExceeded {
                requested: n,
                maximum: Self::MAX_REPLICAS,
            });
        }
        let counts = alloc::vec![0; n];
        Ok(GCounter { counts })
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

#[cfg(test)]
mod construction_tests {
    use super::*;
    use crate::WireEncode;

    #[test]
    fn unallocatable_counter_width_returns_error() {
        for width in [
            GCounter::MAX_REPLICAS + 1,
            usize::MAX,
            usize::try_from(1u64 << 40).unwrap_or(usize::MAX),
        ] {
            let expected = CoordinateError::ReplicaLimitExceeded {
                requested: width,
                maximum: GCounter::MAX_REPLICAS,
            };
            assert_eq!(GCounter::try_new(width), Err(expected));
            assert_eq!(crate::PnCounter::try_new(width), Err(expected));
        }
        let max = GCounter::try_new(GCounter::MAX_REPLICAS).unwrap();
        assert_eq!(max.len(), GCounter::MAX_REPLICAS);
        assert!(max.counts.iter().all(|&x| x == 0));
        let pn = crate::PnCounter::try_new(GCounter::MAX_REPLICAS).unwrap();
        assert!(pn.p_state().iter().chain(pn.n_state()).all(|&x| x == 0));
        assert!(crate::PnCounter::try_new(usize::MAX).is_err());
        let largest = isize::MAX as usize / core::mem::size_of::<u64>();
        assert!(GCounter::try_new(largest + 1).is_err());
        for n in [0, 1, 2] {
            assert_eq!(GCounter::try_new(n).unwrap(), GCounter::new(n));
            assert_eq!(
                crate::PnCounter::try_new(n).unwrap(),
                crate::PnCounter::new(n)
            );
        }
        let mut counter = GCounter::try_new(2).unwrap();
        counter.try_apply_bump(0, 7).unwrap();
        assert_eq!(counter.value(), 7);
        assert!(!GCounterDelta {
            replica: 0,
            tally: 7
        }
        .to_wire_bytes()
        .unwrap()
        .is_empty());
    }
}
