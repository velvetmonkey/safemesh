// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

use crate::RecordId;
use alloc::collections::{BTreeMap, BTreeSet};

/// Per-replica contiguous prefixes for anti-entropy pulls.
///
/// `get(replica) == n` means every sequence `1..=n` for that replica is known.
/// Later records that arrive before earlier records must not advance this
/// prefix, otherwise `since` could hide gaps. Sequence zero is acknowledged
/// independently by `zero_replicas`; it is not implied by a positive prefix.
/// There is no built-in version wire codec. Custom version exchanges must carry
/// both [`Self::entries`] and [`Self::zero_replicas`]. A receiver can rebuild
/// the vector with [`Self::from_peer_prefixes`] after checking application-owned
/// author and zero-acknowledgement budgets, as below; record payloads are not required.
/// Legacy prefix-only exchanges cannot acknowledge zeros and will keep receiving
/// them until upgraded.
///
/// These are claims about the sending peer's log. The caller is responsible for
/// their truth: neither `observe` nor `since` checks possession of record payloads.
/// Fabricated IDs can acknowledge missing records and cause `since` to omit them.
/// Use [`crate::EventLog::version`] to derive claims from records actually admitted.
///
/// Checked reconstruction validates each positive prefix, then clones both
/// collections: for `r` authors and `z` zero acknowledgements it takes O(r + z)
/// time and O(r + z) additional space, independently of the claimed sequences.
/// Bound both collection sizes before reconstruction and enforce transport byte
/// limits before decoding to bound the input allocation itself.
/// Zero-valued prefix entries are noncanonical and are rejected by the checked
/// constructor; only `zero_replicas()` acknowledges sequence zero.
///
/// ```
/// use safemesh_crdt::{EventLog, VersionVector};
/// use std::collections::{BTreeMap, BTreeSet};
///
/// fn receive(
///     prefixes: &BTreeMap<u64, u64>, zeros: &BTreeSet<u64>,
///     author_budget: usize, zero_budget: usize,
/// ) -> Result<VersionVector, &'static str> {
///     if prefixes.len() > author_budget || zeros.len() > zero_budget {
///         return Err("peer version exceeds application budget");
///     }
///     VersionVector::from_peer_prefixes(prefixes, zeros).map_err(|_| "invalid prefix")
/// }
///
/// let mut peer = EventLog::new();
/// peer.append(&mut safemesh_crdt::GSet::new(), 7, 42u64);
/// // Carry both collections over the application's transport.
/// let prefixes = peer.version().entries().clone();
/// let zeros = peer.version().zero_replicas().clone();
/// // This example has one publisher and allocates no sequence-zero records.
/// let received = receive(&prefixes, &zeros, 1, 0).unwrap();
/// assert_eq!(&received, peer.version());
/// assert_eq!(peer.since(&received), peer.since(peer.version()));
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VersionVector {
    entries: BTreeMap<u64, u64>,
    zero_replicas: BTreeSet<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VersionVectorError {
    /// Positive prefixes are canonical; sequence zero belongs in zero_replicas.
    ZeroPrefix { replica: u64 },
}

impl core::fmt::Display for VersionVectorError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            VersionVectorError::ZeroPrefix { replica } => {
                write!(f, "replica {replica} has a noncanonical zero prefix")
            }
        }
    }
}

impl core::error::Error for VersionVectorError {}

impl VersionVector {
    pub fn new() -> Self {
        VersionVector {
            entries: BTreeMap::new(),
            zero_replicas: BTreeSet::new(),
        }
    }

    /// Reconstruct a vector from a peer's canonical positive prefixes and
    /// independent sequence-zero acknowledgements.
    ///
    /// Accepts every positive `u64` prefix, including `u64::MAX`, without
    /// iterating over sequences. Zero remains noncanonical: use `zero_replicas`.
    /// Callers must retain three application-owned input limits:
    /// - author count bounds prefix validation and map cloning;
    /// - zero-acknowledgement count bounds set cloning;
    /// - encoded byte size, enforced before decoding, bounds input allocation
    ///   and parsing (including duplicate entries before collection deduplication).
    ///
    /// No built-in version wire codec or universal application budget is imposed.
    /// Reconstruction takes O(entries.len() + zero_replicas.len()) time and space.
    pub fn from_peer_prefixes(
        entries: &BTreeMap<u64, u64>,
        zero_replicas: &BTreeSet<u64>,
    ) -> Result<Self, VersionVectorError> {
        for (&replica, &prefix) in entries {
            if prefix == 0 {
                return Err(VersionVectorError::ZeroPrefix { replica });
            }
        }

        Ok(Self {
            entries: entries.clone(),
            zero_replicas: zero_replicas.clone(),
        })
    }

    pub fn get(&self, replica: u64) -> u64 {
        self.entries.get(&replica).copied().unwrap_or(0)
    }

    /// Advance this prefix only when `id` is the next contiguous sequence.
    ///
    /// Use `EventLog` to ingest out-of-order records; the log remembers gaps
    /// and advances this vector once the prefix is complete.
    pub fn observe(&mut self, id: RecordId) {
        if id.sequence == 0 {
            self.zero_replicas.insert(id.replica);
        } else if self.get(id.replica).checked_add(1) == Some(id.sequence) {
            self.set(id.replica, id.sequence);
        }
    }

    pub fn includes(&self, id: RecordId) -> bool {
        if id.sequence == 0 {
            self.zero_replicas.contains(&id.replica)
        } else {
            self.get(id.replica) >= id.sequence
        }
    }

    /// Positive contiguous prefixes only; does not describe zero possession.
    pub fn entries(&self) -> &BTreeMap<u64, u64> {
        &self.entries
    }

    /// Authors whose sequence-zero record has been observed.
    pub fn zero_replicas(&self) -> &BTreeSet<u64> {
        &self.zero_replicas
    }

    pub(super) fn set(&mut self, replica: u64, sequence: u64) {
        if sequence == 0 {
            self.entries.remove(&replica);
        } else {
            self.entries.insert(replica, sequence);
        }
    }
}
