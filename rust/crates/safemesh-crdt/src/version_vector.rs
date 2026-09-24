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
/// The built-in wire codec carries both [`Self::entries`] and
/// [`Self::zero_replicas`]. Custom version exchanges must carry both too. A
/// receiver can rebuild the vector with [`Self::from_peer_prefixes`] after
/// checking application-owned author and zero-acknowledgement budgets, as below;
/// record payloads are not required.
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
/// peer.append(&mut safemesh_crdt::GSet::new(), 7, 42u64).unwrap();
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

/// Optional collection budgets for [`VersionVector::from_peer_prefixes_with_limits`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VersionVectorLimits {
    /// Maximum positive-prefix entries. `None` is unbounded; `Some(0)` requires none.
    pub max_authors: Option<usize>,
    /// Maximum sequence-zero acknowledgements, independently of prefix entries.
    /// `None` is unbounded; `Some(0)` requires none.
    pub max_zero_replicas: Option<usize>,
}

impl VersionVectorLimits {
    /// Default wire entry ceilings. Callers can supply tighter or larger budgets
    /// with [`VersionVector::from_wire_bytes_with_limits`].
    pub const WIRE_DEFAULT: Self = Self {
        max_authors: Some(4096),
        max_zero_replicas: Some(4096),
    };
}

/// Failure from opt-in bounded peer-version reconstruction.
/// Separate from [`VersionVectorError`] to preserve existing exhaustive matches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VersionVectorLimitError {
    Prefix(VersionVectorError),
    AuthorLimitExceeded { max_authors: usize },
    ZeroReplicaLimitExceeded { max_zero_replicas: usize },
}

impl core::fmt::Display for VersionVectorLimitError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Prefix(error) => error.fmt(f),
            Self::AuthorLimitExceeded { max_authors } => {
                write!(f, "peer version exceeds author limit {max_authors}")
            }
            Self::ZeroReplicaLimitExceeded { max_zero_replicas } => {
                write!(
                    f,
                    "peer version exceeds zero-acknowledgement limit {max_zero_replicas}"
                )
            }
        }
    }
}

impl core::error::Error for VersionVectorLimitError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Prefix(error) => Some(error),
            _ => None,
        }
    }
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
    /// This method imposes no collection budget. Use
    /// [`Self::from_peer_prefixes_with_limits`] to enforce author and
    /// zero-acknowledgement budgets before validation or cloning.
    /// Callers must retain three application-owned input limits:
    /// - author count bounds prefix validation and map cloning;
    /// - zero-acknowledgement count bounds set cloning;
    /// - encoded byte size, enforced before decoding, bounds input allocation
    ///   and parsing (including duplicate entries before collection deduplication).
    ///
    /// The built-in wire decoder has default entry ceilings; callers can choose
    /// their own limits. This constructor imposes no universal application budget.
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

    /// Reconstruct a peer version with caller-supplied collection budgets.
    ///
    /// Checks author count first, then zero-acknowledgement count, before
    /// validating prefixes or cloning either collection. If both budgets are
    /// exceeded, returns [`VersionVectorLimitError::AuthorLimitExceeded`].
    /// No partial vector is returned. Exact limits are accepted.
    /// [`VersionVectorLimits::default`] preserves the unbounded constructor's
    /// values and wraps its errors in [`VersionVectorLimitError::Prefix`].
    /// These checks do not bound decoding or input allocation: callers must
    /// enforce encoded-byte and entry budgets at their decoding boundary too.
    pub fn from_peer_prefixes_with_limits(
        entries: &BTreeMap<u64, u64>,
        zero_replicas: &BTreeSet<u64>,
        limits: VersionVectorLimits,
    ) -> Result<Self, VersionVectorLimitError> {
        if let Some(max_authors) = limits.max_authors {
            if entries.len() > max_authors {
                return Err(VersionVectorLimitError::AuthorLimitExceeded { max_authors });
            }
        }
        if let Some(max_zero_replicas) = limits.max_zero_replicas {
            if zero_replicas.len() > max_zero_replicas {
                return Err(VersionVectorLimitError::ZeroReplicaLimitExceeded {
                    max_zero_replicas,
                });
            }
        }
        Self::from_peer_prefixes(entries, zero_replicas).map_err(VersionVectorLimitError::Prefix)
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
