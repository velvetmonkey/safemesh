// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

use crate::{Crdt, MergeError, Mergeable, RecordId, WireError};
use alloc::collections::BTreeSet;

/// A grow-only set: merge = union.
///
/// This mirrors the upstream `crdt-lean` G-Set carrier (`Finset α`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GSet<T: Ord> {
    pub(super) elements: BTreeSet<T>,
}

impl<T: Ord> GSet<T> {
    pub fn new() -> Self {
        GSet {
            elements: BTreeSet::new(),
        }
    }

    pub fn insert(&mut self, element: T) {
        self.elements.insert(element);
    }

    pub fn merge(&mut self, other: &Self)
    where
        T: Clone,
    {
        self.elements.extend(other.elements.iter().cloned());
    }

    pub fn contains(&self, element: &T) -> bool {
        self.elements.contains(element)
    }

    pub fn elements(&self) -> &BTreeSet<T> {
        &self.elements
    }
}

impl<T: Ord> Default for GSet<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Ord + Clone> Mergeable for GSet<T> {
    fn merge(&mut self, other: &Self) -> Result<(), MergeError> {
        GSet::merge(self, other);
        Ok(())
    }
}

impl<T: Ord + Clone> Crdt for GSet<T> {
    type Delta = T;

    /// Accepts every record: every `T` is in a G-Set's domain, and a fresh set
    /// inserts it. The only record replay ignores is an element already present,
    /// and ignoring it is set idempotence (join with a member), not loss.
    fn validate_record(&self, _id: RecordId, _delta: &Self::Delta) -> Result<(), WireError> {
        Ok(())
    }

    fn apply_delta(&mut self, delta: Self::Delta) {
        self.insert(delta);
    }
}
