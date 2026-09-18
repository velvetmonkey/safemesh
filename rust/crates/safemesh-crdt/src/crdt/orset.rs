// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

use crate::{Crdt, MergeError, Mergeable, RecordId, WireError};
use alloc::{collections::BTreeSet, vec::Vec};

/// Delta for an observed-remove set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OrSetDelta<T, K> {
    Add { element: T, token: K },
    Remove { tokens: Vec<K> },
}

/// Observed-remove set with add-wins semantics.
///
/// Lean model: `Crdt.ORSet.State α τ = Finset (α × τ) × Finset τ`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrSet<T: Ord, K: Ord> {
    pub(super) adds: BTreeSet<(T, K)>,
    pub(super) tombstones: BTreeSet<K>,
}

impl<T: Ord, K: Ord> OrSet<T, K> {
    pub fn new() -> Self {
        OrSet {
            adds: BTreeSet::new(),
            tombstones: BTreeSet::new(),
        }
    }

    pub fn add(&mut self, element: T, token: K) {
        self.adds.insert((element, token));
    }

    pub fn apply_remove<I>(&mut self, tokens: I)
    where
        I: IntoIterator<Item = K>,
    {
        self.tombstones.extend(tokens);
    }

    pub fn merge(&mut self, other: &Self)
    where
        T: Clone,
        K: Clone,
    {
        self.adds.extend(other.adds.iter().cloned());
        self.tombstones.extend(other.tombstones.iter().cloned());
    }

    pub fn adds(&self) -> &BTreeSet<(T, K)> {
        &self.adds
    }

    pub fn tombstones(&self) -> &BTreeSet<K> {
        &self.tombstones
    }
}

impl<T: Ord + Clone, K: Ord + Clone> OrSet<T, K> {
    pub fn observed_tokens(&self, element: &T) -> BTreeSet<K> {
        self.adds
            .iter()
            .filter_map(|(candidate, token)| {
                if candidate == element {
                    Some(token.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn contains(&self, element: &T) -> bool {
        self.adds
            .iter()
            .any(|(candidate, token)| candidate == element && !self.tombstones.contains(token))
    }

    pub fn elements(&self) -> BTreeSet<T> {
        self.adds
            .iter()
            .filter_map(|(element, token)| {
                if self.tombstones.contains(token) {
                    None
                } else {
                    Some(element.clone())
                }
            })
            .collect()
    }
}

impl<T: Ord, K: Ord> Default for OrSet<T, K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Ord + Clone, K: Ord + Clone> Mergeable for OrSet<T, K> {
    fn merge(&mut self, other: &Self) -> Result<(), MergeError> {
        OrSet::merge(self, other);
        Ok(())
    }
}

impl<T: Ord + Clone, K: Ord + Clone> Crdt for OrSet<T, K> {
    type Delta = OrSetDelta<T, K>;

    /// Accepts every record. An add always joins the add set, even when its
    /// token is already tombstoned (that is the observed-remove rule, and a fresh
    /// set applies it). A remove tombstones every token it names whether or not
    /// this replica observed them (`RecordKernel.payloadOwned .remove`), so a
    /// fresh set applies it too. A remove naming no tokens is the lattice bottom:
    /// the local writer emits it for an absent element, and joining `⊥` on any
    /// carrier is the correct application of that record, not a dropped one.
    /// Nothing decodable is outside the carrier, so nothing is refused here.
    fn validate_record(&self, _id: RecordId, _delta: &Self::Delta) -> Result<(), WireError> {
        Ok(())
    }

    fn apply_delta(&mut self, delta: Self::Delta) {
        match delta {
            OrSetDelta::Add { element, token } => self.add(element, token),
            OrSetDelta::Remove { tokens } => self.apply_remove(tokens),
        }
    }
}
