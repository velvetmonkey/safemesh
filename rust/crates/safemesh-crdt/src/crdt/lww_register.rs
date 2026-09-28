// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

use crate::{Crdt, MergeError, Mergeable, RecordId, WireError};

/// Total-order dot for last-writer-wins registers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LwwDot {
    pub timestamp: u64,
    pub replica: u64,
}

/// One LWW register assignment.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LwwEntry<V: Ord> {
    pub dot: LwwDot,
    pub value: V,
}

/// Delta for a last-writer-wins register.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LwwRegisterDelta<V: Ord> {
    pub timestamp: u64,
    pub replica: u64,
    pub value: V,
}

/// Last-writer-wins register.
///
/// This is a flat tested-not-proven type. It is a max register over the total
/// order `(timestamp, replica, value)`, which gives deterministic merge and
/// tie-breaking. It is not currently part of the Lean-proven product surface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LwwRegister<V: Ord> {
    entry: Option<LwwEntry<V>>,
}

impl<V: Ord> LwwRegister<V> {
    pub fn new() -> Self {
        LwwRegister { entry: None }
    }

    pub fn set(&mut self, timestamp: u64, replica: u64, value: V) {
        self.apply_entry(LwwEntry {
            dot: LwwDot { timestamp, replica },
            value,
        });
    }

    pub fn entry(&self) -> Option<&LwwEntry<V>> {
        self.entry.as_ref()
    }

    pub fn value(&self) -> Option<&V> {
        self.entry.as_ref().map(|entry| &entry.value)
    }

    fn apply_entry(&mut self, entry: LwwEntry<V>) {
        match &self.entry {
            Some(current) if current >= &entry => {}
            _ => self.entry = Some(entry),
        }
    }
}

impl<V: Ord> Default for LwwRegister<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V: Ord + Clone> LwwRegister<V> {
    pub fn merge(&mut self, other: &Self) {
        if let Some(entry) = other.entry.clone() {
            self.apply_entry(entry);
        }
    }
}

impl<V: Ord + Clone> Mergeable for LwwRegister<V> {
    fn merge(&mut self, other: &Self) -> Result<(), MergeError> {
        LwwRegister::merge(self, other);
        Ok(())
    }
}

impl<V: Ord + Clone> Crdt for LwwRegister<V> {
    type Delta = LwwRegisterDelta<V>;

    /// A record may only assign its author's dot. Losing assignments with a
    /// matching author remain valid and are subsumed by the total-order rule.
    fn validate_record(&self, id: RecordId, delta: &Self::Delta) -> Result<(), WireError> {
        if delta.replica != id.replica {
            return Err(WireError::OwnershipViolation);
        }
        Ok(())
    }

    fn apply_delta(&mut self, delta: Self::Delta) {
        self.set(delta.timestamp, delta.replica, delta.value);
    }
}
