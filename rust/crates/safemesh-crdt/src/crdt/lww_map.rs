// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

use crate::{Crdt, LwwDot, LwwEntry, MergeError, Mergeable, RecordId, WireError};
use alloc::collections::BTreeMap;

/// Delta for a last-writer-wins map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LwwMapDelta<K: Ord, V: Ord> {
    Set {
        key: K,
        timestamp: u64,
        replica: u64,
        value: V,
    },
    Remove {
        key: K,
        timestamp: u64,
        replica: u64,
    },
}

/// Last-writer-wins map.
///
/// This is a flat tested-not-proven type. Each key has an optional max-dot value
/// entry and an optional max-dot remove tombstone. A key is visible when its
/// value dot is greater than its remove dot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LwwMap<K: Ord, V: Ord> {
    pub(super) entries: BTreeMap<K, LwwEntry<V>>,
    pub(super) removals: BTreeMap<K, LwwDot>,
}

impl<K: Ord, V: Ord> LwwMap<K, V> {
    pub fn new() -> Self {
        LwwMap {
            entries: BTreeMap::new(),
            removals: BTreeMap::new(),
        }
    }

    pub fn set(&mut self, key: K, timestamp: u64, replica: u64, value: V) {
        self.apply_entry(
            key,
            LwwEntry {
                dot: LwwDot { timestamp, replica },
                value,
            },
        );
    }

    pub fn remove(&mut self, key: K, timestamp: u64, replica: u64) {
        self.apply_removal(key, LwwDot { timestamp, replica });
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        self.visible_entry(key).map(|entry| &entry.value)
    }

    pub fn visible_entry(&self, key: &K) -> Option<&LwwEntry<V>> {
        let entry = self.entries.get(key)?;
        match self.removals.get(key) {
            Some(removal) if entry.dot <= *removal => None,
            _ => Some(entry),
        }
    }

    pub fn entries(&self) -> &BTreeMap<K, LwwEntry<V>> {
        &self.entries
    }

    pub fn removals(&self) -> &BTreeMap<K, LwwDot> {
        &self.removals
    }

    fn apply_entry(&mut self, key: K, entry: LwwEntry<V>) {
        match self.entries.get(&key) {
            Some(current) if current >= &entry => {}
            _ => {
                self.entries.insert(key, entry);
            }
        }
    }

    fn apply_removal(&mut self, key: K, dot: LwwDot) {
        match self.removals.get(&key) {
            Some(current) if *current >= dot => {}
            _ => {
                self.removals.insert(key, dot);
            }
        }
    }
}

impl<K: Ord + Clone, V: Ord + Clone> LwwMap<K, V> {
    pub fn merge(&mut self, other: &Self) {
        for (key, entry) in other.entries.iter() {
            self.apply_entry(key.clone(), entry.clone());
        }
        for (key, dot) in other.removals.iter() {
            self.apply_removal(key.clone(), *dot);
        }
    }

    pub fn value(&self) -> BTreeMap<K, V> {
        self.entries
            .iter()
            .filter_map(|(key, entry)| {
                if self.visible_entry(key).is_some() {
                    Some((key.clone(), entry.value.clone()))
                } else {
                    None
                }
            })
            .collect()
    }
}

impl<K: Ord, V: Ord> Default for LwwMap<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Ord + Clone, V: Ord + Clone> Mergeable for LwwMap<K, V> {
    fn merge(&mut self, other: &Self) -> Result<(), MergeError> {
        LwwMap::merge(self, other);
        Ok(())
    }
}

impl<K: Ord + Clone, V: Ord + Clone> Crdt for LwwMap<K, V> {
    type Delta = LwwMapDelta<K, V>;

    /// Accepts every record. Per key, a set or remove that loses to the current
    /// dot is subsumed by the max-dot rule and applies on a fresh map; nothing a
    /// decoder can produce is outside the map's domain.
    fn validate_record(&self, _id: RecordId, _delta: &Self::Delta) -> Result<(), WireError> {
        Ok(())
    }

    fn apply_delta(&mut self, delta: Self::Delta) {
        match delta {
            LwwMapDelta::Set {
                key,
                timestamp,
                replica,
                value,
            } => self.set(key, timestamp, replica, value),
            LwwMapDelta::Remove {
                key,
                timestamp,
                replica,
            } => self.remove(key, timestamp, replica),
        }
    }
}
