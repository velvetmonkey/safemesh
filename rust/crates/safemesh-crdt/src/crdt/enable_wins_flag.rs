// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

use crate::{Crdt, MergeError, Mergeable, RecordId, WireError};
use alloc::{collections::BTreeSet, vec::Vec};

/// Delta for an observed-token enable-wins flag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnableWinsFlagDelta<K> {
    Enable { token: K },
    Disable { tokens: Vec<K> },
}

/// Enable-wins boolean flag.
///
/// This is a flat tested-not-proven type. It mirrors an OR-Set over a unit
/// element: enable adds a unique token, disable tombstones observed tokens, and
/// concurrent unobserved enables remain live.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnableWinsFlag<K: Ord> {
    pub(super) enables: BTreeSet<K>,
    pub(super) tombstones: BTreeSet<K>,
}

impl<K: Ord> EnableWinsFlag<K> {
    pub fn new() -> Self {
        EnableWinsFlag {
            enables: BTreeSet::new(),
            tombstones: BTreeSet::new(),
        }
    }

    pub fn enable(&mut self, token: K) {
        self.enables.insert(token);
    }

    pub fn disable<I>(&mut self, tokens: I)
    where
        I: IntoIterator<Item = K>,
    {
        self.tombstones.extend(tokens);
    }

    pub fn merge(&mut self, other: &Self)
    where
        K: Clone,
    {
        self.enables.extend(other.enables.iter().cloned());
        self.tombstones.extend(other.tombstones.iter().cloned());
    }

    pub fn enables(&self) -> &BTreeSet<K> {
        &self.enables
    }

    pub fn tombstones(&self) -> &BTreeSet<K> {
        &self.tombstones
    }
}

impl<K: Ord + Clone> EnableWinsFlag<K> {
    pub fn observed_tokens(&self) -> BTreeSet<K> {
        self.enables.iter().cloned().collect()
    }

    pub fn value(&self) -> bool {
        self.enables
            .iter()
            .any(|token| !self.tombstones.contains(token))
    }
}

impl<K: Ord> Default for EnableWinsFlag<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Ord + Clone> Mergeable for EnableWinsFlag<K> {
    fn merge(&mut self, other: &Self) -> Result<(), MergeError> {
        EnableWinsFlag::merge(self, other);
        Ok(())
    }
}

impl<K: Ord + Clone> Crdt for EnableWinsFlag<K> {
    type Delta = EnableWinsFlagDelta<K>;

    /// Accepts every record, by the same argument as [`crate::OrSet`]: an enable joins
    /// the enable set even when its token is tombstoned, a disable tombstones the
    /// tokens it names whether observed or not, and a disable naming no tokens is
    /// the lattice bottom. Every decodable record applies on a fresh flag.
    fn validate_record(&self, _id: RecordId, _delta: &Self::Delta) -> Result<(), WireError> {
        Ok(())
    }

    fn apply_delta(&mut self, delta: Self::Delta) {
        match delta {
            EnableWinsFlagDelta::Enable { token } => self.enable(token),
            EnableWinsFlagDelta::Disable { tokens } => self.disable(tokens),
        }
    }
}
