// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

use crate::{Crdt, MergeError, Mergeable, RecordId, WireError};
use alloc::{collections::BTreeSet, vec::Vec};

/// Delta for an RGA-family ordered sequence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RgaDelta<P, V> {
    Insert { position: P, value: V },
    Delete { position: P },
}

/// RGA-family sequence state: positioned values plus tombstoned positions.
///
/// Lean model: `Crdt.RGA.State ι α = Finset (ι × α) × Finset ι`. The Lean
/// read is the sorted list of live position identifiers; values are carried
/// by lookup through the live positioned set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rga<P: Ord, V: Ord> {
    pub(super) placed: BTreeSet<(P, V)>,
    pub(super) tombstones: BTreeSet<P>,
}

impl<P: Ord, V: Ord> Rga<P, V> {
    pub fn new() -> Self {
        Rga {
            placed: BTreeSet::new(),
            tombstones: BTreeSet::new(),
        }
    }

    pub fn insert(&mut self, position: P, value: V) {
        self.placed.insert((position, value));
    }

    pub fn delete(&mut self, position: P) {
        self.tombstones.insert(position);
    }

    pub fn merge(&mut self, other: &Self)
    where
        P: Clone,
        V: Clone,
    {
        self.placed.extend(other.placed.iter().cloned());
        self.tombstones.extend(other.tombstones.iter().cloned());
    }

    pub fn placed(&self) -> &BTreeSet<(P, V)> {
        &self.placed
    }

    pub fn tombstones(&self) -> &BTreeSet<P> {
        &self.tombstones
    }
}

impl<P: Ord + Clone, V: Ord + Clone> Rga<P, V> {
    /// Every live positioned value, in total `(position, value)` order.
    pub fn live_entries(&self) -> Vec<(P, V)> {
        self.placed
            .iter()
            .filter_map(|(position, value)| {
                if self.tombstones.contains(position) {
                    None
                } else {
                    Some((position.clone(), value.clone()))
                }
            })
            .collect()
    }

    /// One position per live entry, in the same order as `live_entries`.
    /// Repeated positions are retained: distinct live values at a shared
    /// position must not disappear from the sequence projection.
    pub fn read_positions(&self) -> Vec<P> {
        self.live_entries()
            .into_iter()
            .map(|(position, _)| position)
            .collect()
    }
}

impl<P: Ord, V: Ord> Default for Rga<P, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Ord + Clone, V: Ord + Clone> Mergeable for Rga<P, V> {
    fn merge(&mut self, other: &Self) -> Result<(), MergeError> {
        Rga::merge(self, other);
        Ok(())
    }
}

impl<P: Ord + Clone, V: Ord + Clone> Crdt for Rga<P, V> {
    type Delta = RgaDelta<P, V>;

    /// Accepts every record: an insert joins the placed set even at a
    /// tombstoned position, and a delete tombstones its position whether or not
    /// it was observed; a fresh sequence applies either. The records replay
    /// ignores (a duplicate insert, a repeated delete) are join idempotence.
    fn validate_record(&self, _id: RecordId, _delta: &Self::Delta) -> Result<(), WireError> {
        Ok(())
    }

    fn apply_delta(&mut self, delta: Self::Delta) {
        match delta {
            RgaDelta::Insert { position, value } => self.insert(position, value),
            RgaDelta::Delete { position } => self.delete(position),
        }
    }
}
