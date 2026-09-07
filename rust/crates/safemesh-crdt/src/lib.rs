// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

//! Delta-state G-Counter and PN-Counter for constrained mesh links.
//!
//! This crate is the PRODUCT half of the SafeMesh verification-guided loop:
//! a thin `no_std + alloc` implementation of exactly the model proven in
//! `lean/SafeMesh/` (carrier = join-semilattice, merge = join, delta =
//! single-coordinate bump), held to those proofs by a differential
//! conformance test (`tests/conformance.rs`) that replays a Lean-emitted
//! corpus through this code and requires byte-identical outputs.
//!
//! What is PROVEN (in Lean, kernel-checked) vs what is TESTED (here): the
//! Lean theorems are universal; this crate is checked against them over a
//! finite corpus. The bridge (Lean compiled eval → JSON → this crate) is the
//! named trusted component. See the repo README for the full TCB statement.

#![no_std]
#![deny(unsafe_code)]

extern crate alloc;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// State-based merge contract: `merge` is expected to be a semilattice join.
///
/// For in-house types, this contract is backed by the Lean proof suite and
/// differential conformance corpus. For user-defined types, it is a tested
/// contract enforced by the laws harness, not a proof.
pub trait Mergeable {
    fn merge(&mut self, other: &Self);
}

/// Delta application surface for CRDT product types.
pub trait Crdt: Mergeable {
    type Delta;

    fn apply_delta(&mut self, delta: Self::Delta);
}

/// Error raised by the checked full-state merge (`try_merge`).
///
/// The Lean model fixes the replica set (`Fin n`), so a length mismatch is
/// unrepresentable there. At the Rust boundary an untrusted peer can hand us a
/// state vector of a different width; merging it by `zip` would silently drop
/// the trailing coordinates (WS1 silent state loss). The checked path surfaces
/// the mismatch as an error instead of corrupting state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeError {
    /// The two counters cover a different number of replicas.
    ReplicaCountMismatch { own: usize, other: usize },
}

impl core::fmt::Display for MergeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            MergeError::ReplicaCountMismatch { own, other } => write!(
                f,
                "replica-count mismatch: own={own}, other={other} (fixed replica set violated)"
            ),
        }
    }
}

impl core::error::Error for MergeError {}

/// Delta for a grow-only counter: one replica coordinate and its asserted tally.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GCounterDelta {
    pub replica: usize,
    pub tally: u64,
}

/// Rejection of a counter delta with an invalid replica coordinate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoordinateError {
    /// The index is outside `0..replica_count`; the counter is unchanged.
    ReplicaOutOfRange {
        replica: usize,
        replica_count: usize,
    },
}

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
    /// Fresh counter for `n` replicas — the lattice bottom `⊥` (all zeros).
    pub fn new(n: usize) -> Self {
        GCounter { counts: vec![0; n] }
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
    /// Infallible variant for the proven equal-length invariant (fixed replica
    /// set). Panics on a replica-count mismatch rather than dropping state; use
    /// [`GCounter::try_merge`] for untrusted input.
    pub fn merge(&mut self, other: &GCounter) {
        self.try_merge(other).expect(
            "GCounter::merge requires equal replica counts; use try_merge for untrusted input",
        );
    }

    /// Per-coordinate state (the `ι → ℕ` vector).
    pub fn state(&self) -> &[u64] {
        &self.counts
    }

    /// The counter's read: sum of per-replica tallies — `Crdt.gcounterValue`.
    pub fn value(&self) -> u64 {
        self.counts.iter().sum()
    }
}

impl Mergeable for GCounter {
    fn merge(&mut self, other: &Self) {
        GCounter::merge(self, other);
    }
}

impl Crdt for GCounter {
    type Delta = GCounterDelta;

    fn apply_delta(&mut self, delta: Self::Delta) {
        self.apply_bump(delta.replica, delta.tally);
    }
}

/// A grow-only set: merge = union.
///
/// This mirrors the upstream `crdt-lean` G-Set carrier (`Finset α`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GSet<T: Ord> {
    elements: BTreeSet<T>,
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
    fn merge(&mut self, other: &Self) {
        GSet::merge(self, other);
    }
}

impl<T: Ord + Clone> Crdt for GSet<T> {
    type Delta = T;

    fn apply_delta(&mut self, delta: Self::Delta) {
        self.insert(delta);
    }
}

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
    pub fn new(n: usize) -> Self {
        PnCounter {
            p: GCounter::new(n),
            n: GCounter::new(n),
        }
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
    /// Infallible variant for the proven equal-length invariant; panics on a
    /// replica-count mismatch. Use [`PnCounter::try_merge`] for untrusted input.
    pub fn merge(&mut self, other: &PnCounter) {
        self.try_merge(other).expect(
            "PnCounter::merge requires equal replica counts; use try_merge for untrusted input",
        );
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
    /// `Crdt.pncounterValue` (ℤ in the model, `i64` here).
    pub fn value(&self) -> i64 {
        self.p.value() as i64 - self.n.value() as i64
    }
}

impl Mergeable for PnCounter {
    fn merge(&mut self, other: &Self) {
        PnCounter::merge(self, other);
    }
}

impl Crdt for PnCounter {
    type Delta = PnCounterDelta;

    fn apply_delta(&mut self, delta: Self::Delta) {
        match delta {
            PnCounterDelta::Inc { replica, tally } => self.apply_inc(replica, tally),
            PnCounterDelta::Dec { replica, tally } => self.apply_dec(replica, tally),
        }
    }
}

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
    adds: BTreeSet<(T, K)>,
    tombstones: BTreeSet<K>,
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
    fn merge(&mut self, other: &Self) {
        OrSet::merge(self, other);
    }
}

impl<T: Ord + Clone, K: Ord + Clone> Crdt for OrSet<T, K> {
    type Delta = OrSetDelta<T, K>;

    fn apply_delta(&mut self, delta: Self::Delta) {
        match delta {
            OrSetDelta::Add { element, token } => self.add(element, token),
            OrSetDelta::Remove { tokens } => self.apply_remove(tokens),
        }
    }
}

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
    placed: BTreeSet<(P, V)>,
    tombstones: BTreeSet<P>,
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

    pub fn read_positions(&self) -> Vec<P> {
        self.live_entries()
            .into_iter()
            .map(|(position, _)| position)
            .collect::<BTreeSet<P>>()
            .into_iter()
            .collect()
    }
}

impl<P: Ord, V: Ord> Default for Rga<P, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Ord + Clone, V: Ord + Clone> Mergeable for Rga<P, V> {
    fn merge(&mut self, other: &Self) {
        Rga::merge(self, other);
    }
}

impl<P: Ord + Clone, V: Ord + Clone> Crdt for Rga<P, V> {
    type Delta = RgaDelta<P, V>;

    fn apply_delta(&mut self, delta: Self::Delta) {
        match delta {
            RgaDelta::Insert { position, value } => self.insert(position, value),
            RgaDelta::Delete { position } => self.delete(position),
        }
    }
}

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
    enables: BTreeSet<K>,
    tombstones: BTreeSet<K>,
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
    fn merge(&mut self, other: &Self) {
        EnableWinsFlag::merge(self, other);
    }
}

impl<K: Ord + Clone> Crdt for EnableWinsFlag<K> {
    type Delta = EnableWinsFlagDelta<K>;

    fn apply_delta(&mut self, delta: Self::Delta) {
        match delta {
            EnableWinsFlagDelta::Enable { token } => self.enable(token),
            EnableWinsFlagDelta::Disable { tokens } => self.disable(tokens),
        }
    }
}

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
    fn merge(&mut self, other: &Self) {
        LwwRegister::merge(self, other);
    }
}

impl<V: Ord + Clone> Crdt for LwwRegister<V> {
    type Delta = LwwRegisterDelta<V>;

    fn apply_delta(&mut self, delta: Self::Delta) {
        self.set(delta.timestamp, delta.replica, delta.value);
    }
}

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
    entries: BTreeMap<K, LwwEntry<V>>,
    removals: BTreeMap<K, LwwDot>,
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
    fn merge(&mut self, other: &Self) {
        LwwMap::merge(self, other);
    }
}

impl<K: Ord + Clone, V: Ord + Clone> Crdt for LwwMap<K, V> {
    type Delta = LwwMapDelta<K, V>;

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

/// Stable identity for an event-log record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RecordId {
    pub replica: u64,
    pub sequence: u64,
}

/// An event-log record carrying a CRDT delta.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record<D> {
    pub id: RecordId,
    pub delta: D,
}

/// Per-replica contiguous prefixes for anti-entropy pulls.
///
/// `get(replica) == n` means every sequence `1..=n` for that replica is known.
/// Later records that arrive before earlier records must not advance this
/// prefix, otherwise `since` could hide gaps.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VersionVector {
    entries: BTreeMap<u64, u64>,
}

impl VersionVector {
    pub fn new() -> Self {
        VersionVector {
            entries: BTreeMap::new(),
        }
    }

    pub fn get(&self, replica: u64) -> u64 {
        self.entries.get(&replica).copied().unwrap_or(0)
    }

    /// Advance this prefix only when `id` is the next contiguous sequence.
    ///
    /// Use `EventLog` to ingest out-of-order records; the log remembers gaps
    /// and advances this vector once the prefix is complete.
    pub fn observe(&mut self, id: RecordId) {
        if self.get(id.replica).checked_add(1) == Some(id.sequence) {
            self.set(id.replica, id.sequence);
        }
    }

    pub fn includes(&self, id: RecordId) -> bool {
        self.get(id.replica) >= id.sequence
    }

    pub fn entries(&self) -> &BTreeMap<u64, u64> {
        &self.entries
    }

    fn set(&mut self, replica: u64, sequence: u64) {
        if sequence == 0 {
            self.entries.remove(&replica);
        } else {
            self.entries.insert(replica, sequence);
        }
    }
}

/// Full decoded payload equality distinguishes redelivery from an ID collision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission {
    Accepted,
    Duplicate,
    Collision,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppendError {
    SequenceExhausted,
}

/// Append-only, deduplicating event log for CRDT deltas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventLog<D> {
    records: Vec<Record<D>>,
    seen: BTreeMap<RecordId, usize>,
    version: VersionVector,
}

impl<D> EventLog<D> {
    pub fn new() -> Self {
        EventLog {
            records: Vec::new(),
            seen: BTreeMap::new(),
            version: VersionVector::new(),
        }
    }

    pub fn append(&mut self, replica: u64, delta: D) -> RecordId
    where
        D: PartialEq,
    {
        self.append_with(replica, delta, |_| {})
            .expect("event log sequence exhausted")
    }

    /// Allocate a fresh ID, then use the same gate as incoming records.
    pub fn append_with<F>(
        &mut self,
        replica: u64,
        delta: D,
        apply: F,
    ) -> Result<RecordId, AppendError>
    where
        D: PartialEq,
        F: FnOnce(&D),
    {
        let sequence = self
            .seen
            .range(
                RecordId {
                    replica,
                    sequence: 0,
                }..=RecordId {
                    replica,
                    sequence: u64::MAX,
                },
            )
            .next_back()
            .map(|(id, _)| id.sequence)
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(AppendError::SequenceExhausted)?;
        let id = RecordId { replica, sequence };
        let outcome = self.admit_with(Record { id, delta }, apply);
        debug_assert_eq!(outcome, Admission::Accepted);
        Ok(id)
    }

    pub fn merge_records<I>(&mut self, records: I) -> Vec<Admission>
    where
        D: PartialEq,
        I: IntoIterator<Item = Record<D>>,
    {
        records.into_iter().map(|r| self.insert_record(r)).collect()
    }

    /// The sole record admission decision. Only Accepted invokes `apply`.
    /// The callback must be infallible and use the same delta interpretation as
    /// replay. This is an in-memory transition, not a crash-durability guarantee.
    #[must_use]
    pub fn admit_with<F>(&mut self, record: Record<D>, apply: F) -> Admission
    where
        D: PartialEq,
        F: FnOnce(&D),
    {
        if let Some(&index) = self.seen.get(&record.id) {
            return if self.records[index].delta == record.delta {
                Admission::Duplicate
            } else {
                Admission::Collision
            };
        }
        let id = record.id;
        self.seen.insert(id, self.records.len());
        self.records.push(record);
        self.advance_contiguous_version(id.replica);
        apply(&self.records.last().expect("accepted record").delta);
        Admission::Accepted
    }

    pub fn version(&self) -> &VersionVector {
        &self.version
    }

    pub fn records(&self) -> &[Record<D>] {
        &self.records
    }

    pub fn insert_record(&mut self, record: Record<D>) -> Admission
    where
        D: PartialEq,
    {
        self.admit_with(record, |_| {})
    }

    fn advance_contiguous_version(&mut self, replica: u64) {
        loop {
            let Some(next) = self.version.get(replica).checked_add(1) else {
                break;
            };
            if self.seen.contains_key(&RecordId {
                replica,
                sequence: next,
            }) {
                self.version.set(replica, next);
            } else {
                break;
            }
        }
    }
}

impl<D: Clone> EventLog<D> {
    pub fn since(&self, version: &VersionVector) -> Vec<Record<D>> {
        self.records
            .iter()
            .filter(|record| !version.includes(record.id))
            .cloned()
            .collect()
    }
}

impl<D> Default for EventLog<D> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransportEnvelope<D> {
    pub from: u64,
    pub to: u64,
    pub records: Vec<Record<D>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportError {
    NotSubscribed { peer: u64 },
    Disconnected { from: u64, to: u64 },
}

/// Engineered transport coverage contract.
///
/// A transport adapter is responsible for peer subscription, connectivity
/// checks, and moving record batches. It is tested infrastructure, not a
/// proof that any real network delivers packets.
pub trait TransportAdapter<D> {
    fn subscribe(&mut self, peer: u64);
    fn is_subscribed(&self, peer: u64) -> bool;
    fn set_connected(&mut self, a: u64, b: u64, connected: bool);
    fn is_connected(&self, a: u64, b: u64) -> bool;
    fn send(&mut self, from: u64, to: u64, records: Vec<Record<D>>) -> Result<(), TransportError>;
    fn drain(&mut self, peer: u64) -> Vec<TransportEnvelope<D>>;
}

/// Deterministic in-memory adapter for coverage-contract fault campaigns.
///
/// This adapter can drop, duplicate, reorder, partition, and heal links. It is
/// useful for CI and demos; it is not a real radio/network adapter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InMemoryTransport<D> {
    subscribed: BTreeSet<u64>,
    disconnected: BTreeSet<(u64, u64)>,
    queue: Vec<TransportEnvelope<D>>,
    dropped: Vec<TransportEnvelope<D>>,
    drop_next: bool,
    duplicate_next: bool,
}

impl<D> InMemoryTransport<D> {
    pub fn new() -> Self {
        InMemoryTransport {
            subscribed: BTreeSet::new(),
            disconnected: BTreeSet::new(),
            queue: Vec::new(),
            dropped: Vec::new(),
            drop_next: false,
            duplicate_next: false,
        }
    }

    pub fn drop_next_send(&mut self) {
        self.drop_next = true;
    }

    pub fn duplicate_next_send(&mut self) {
        self.duplicate_next = true;
    }

    pub fn reverse_pending_for(&mut self, peer: u64) {
        let mut peer_items = Vec::new();
        let mut retained = Vec::new();
        for envelope in self.queue.drain(..) {
            if envelope.to == peer {
                peer_items.push(envelope);
            } else {
                retained.push(envelope);
            }
        }
        peer_items.reverse();
        retained.extend(peer_items);
        self.queue = retained;
    }

    pub fn pending_len(&self) -> usize {
        self.queue.len()
    }

    pub fn dropped_len(&self) -> usize {
        self.dropped.len()
    }

    fn link(a: u64, b: u64) -> (u64, u64) {
        if a <= b {
            (a, b)
        } else {
            (b, a)
        }
    }
}

impl<D> Default for InMemoryTransport<D> {
    fn default() -> Self {
        Self::new()
    }
}

impl<D: Clone> TransportAdapter<D> for InMemoryTransport<D> {
    fn subscribe(&mut self, peer: u64) {
        self.subscribed.insert(peer);
    }

    fn is_subscribed(&self, peer: u64) -> bool {
        self.subscribed.contains(&peer)
    }

    fn set_connected(&mut self, a: u64, b: u64, connected: bool) {
        let link = Self::link(a, b);
        if connected {
            self.disconnected.remove(&link);
        } else {
            self.disconnected.insert(link);
        }
    }

    fn is_connected(&self, a: u64, b: u64) -> bool {
        a == b || !self.disconnected.contains(&Self::link(a, b))
    }

    fn send(&mut self, from: u64, to: u64, records: Vec<Record<D>>) -> Result<(), TransportError> {
        if !self.is_subscribed(from) {
            return Err(TransportError::NotSubscribed { peer: from });
        }
        if !self.is_subscribed(to) {
            return Err(TransportError::NotSubscribed { peer: to });
        }
        if !self.is_connected(from, to) {
            return Err(TransportError::Disconnected { from, to });
        }

        let envelope = TransportEnvelope { from, to, records };
        if self.drop_next {
            self.drop_next = false;
            self.dropped.push(envelope);
            return Ok(());
        }

        self.queue.push(envelope.clone());
        if self.duplicate_next {
            self.duplicate_next = false;
            self.queue.push(envelope);
        }
        Ok(())
    }

    fn drain(&mut self, peer: u64) -> Vec<TransportEnvelope<D>> {
        let mut incoming = Vec::new();
        let mut retained = Vec::new();
        for envelope in self.queue.drain(..) {
            if envelope.to == peer {
                incoming.push(envelope);
            } else {
                retained.push(envelope);
            }
        }
        self.queue = retained;
        incoming
    }
}

pub fn anti_entropy<D, T>(
    transport: &mut T,
    from: u64,
    to: u64,
    local: &EventLog<D>,
    remote_version: &VersionVector,
) -> Result<(), TransportError>
where
    D: Clone,
    T: TransportAdapter<D>,
{
    let records = local.since(remote_version);
    if records.is_empty() {
        Ok(())
    } else {
        transport.send(from, to, records)
    }
}

const TAG_RECORD: u8 = 0x01;
const TAG_EVENT_LOG: u8 = 0x03;
const TAG_GCOUNTER_DELTA: u8 = 0x10;
const TAG_PNCOUNTER_INC: u8 = 0x11;
const TAG_PNCOUNTER_DEC: u8 = 0x12;
const TAG_GSET_U64: u8 = 0x20;
const TAG_ORSET_U64: u8 = 0x30;
const TAG_ORSET_ADD_U64: u8 = 0x31;
const TAG_ORSET_REMOVE_U64: u8 = 0x32;
const TAG_ORSET_ADD_STRING: u8 = 0x33;
const TAG_ORSET_REMOVE_STRING: u8 = 0x34;
const TAG_RGA_U64: u8 = 0x40;
const TAG_LWW_REGISTER_DELTA_U64: u8 = 0x50;
const TAG_LWW_REGISTER_U64: u8 = 0x51;
const TAG_ENABLE_WINS_FLAG_ENABLE_U64: u8 = 0x60;
const TAG_ENABLE_WINS_FLAG_DISABLE_U64: u8 = 0x61;
const TAG_ENABLE_WINS_FLAG_U64: u8 = 0x62;
const TAG_LWW_MAP_SET_U64: u8 = 0x70;
const TAG_LWW_MAP_REMOVE_U64: u8 = 0x71;
const TAG_LWW_MAP_U64: u8 = 0x72;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireError {
    UnexpectedEof,
    InvalidTag,
    TrailingBytes,
    LengthOverflow,
    RecordCollision,
    IntegrityMismatch,
    InvalidUtf8,
}

pub trait WireEncode {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError>;

    fn to_wire_bytes(&self) -> Result<Vec<u8>, WireError> {
        let mut out = Vec::new();
        self.encode_wire(&mut out)?;
        Ok(out)
    }
}

pub trait WireDecode: Sized {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError>;

    fn from_wire_bytes(bytes: &[u8]) -> Result<Self, WireError> {
        let mut cursor = WireCursor::new(bytes);
        let value = Self::decode_wire(&mut cursor)?;
        if cursor.is_empty() {
            Ok(value)
        } else {
            Err(WireError::TrailingBytes)
        }
    }
}

pub struct WireCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> WireCursor<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        WireCursor { bytes, offset: 0 }
    }

    pub fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }

    fn read_u8(&mut self) -> Result<u8, WireError> {
        let byte = *self
            .bytes
            .get(self.offset)
            .ok_or(WireError::UnexpectedEof)?;
        self.offset += 1;
        Ok(byte)
    }

    fn read_u32(&mut self) -> Result<u32, WireError> {
        let bytes = self.read_exact(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn read_u64(&mut self) -> Result<u64, WireError> {
        let bytes = self.read_exact(8)?;
        Ok(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    fn read_len(&mut self) -> Result<usize, WireError> {
        usize::try_from(self.read_u32()?).map_err(|_| WireError::LengthOverflow)
    }

    fn read_exact(&mut self, len: usize) -> Result<&'a [u8], WireError> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or(WireError::LengthOverflow)?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or(WireError::UnexpectedEof)?;
        self.offset = end;
        Ok(bytes)
    }
}

fn write_u8(out: &mut Vec<u8>, value: u8) {
    out.push(value);
}

fn write_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn write_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn write_len(out: &mut Vec<u8>, len: usize) -> Result<(), WireError> {
    let len = u32::try_from(len).map_err(|_| WireError::LengthOverflow)?;
    write_u32(out, len);
    Ok(())
}

fn write_bytes(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), WireError> {
    write_len(out, bytes.len())?;
    out.extend_from_slice(bytes);
    Ok(())
}

fn read_tag(cursor: &mut WireCursor<'_>, expected: u8) -> Result<(), WireError> {
    match cursor.read_u8()? {
        tag if tag == expected => Ok(()),
        _ => Err(WireError::InvalidTag),
    }
}

impl WireEncode for GCounterDelta {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        write_u8(out, TAG_GCOUNTER_DELTA);
        write_u64(
            out,
            u64::try_from(self.replica).map_err(|_| WireError::LengthOverflow)?,
        );
        write_u64(out, self.tally);
        Ok(())
    }
}

impl WireDecode for GCounterDelta {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        read_tag(cursor, TAG_GCOUNTER_DELTA)?;
        let replica = usize::try_from(cursor.read_u64()?).map_err(|_| WireError::LengthOverflow)?;
        let tally = cursor.read_u64()?;
        Ok(GCounterDelta { replica, tally })
    }
}

impl WireEncode for PnCounterDelta {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        match self {
            PnCounterDelta::Inc { replica, tally } => {
                write_u8(out, TAG_PNCOUNTER_INC);
                write_u64(
                    out,
                    u64::try_from(*replica).map_err(|_| WireError::LengthOverflow)?,
                );
                write_u64(out, *tally);
            }
            PnCounterDelta::Dec { replica, tally } => {
                write_u8(out, TAG_PNCOUNTER_DEC);
                write_u64(
                    out,
                    u64::try_from(*replica).map_err(|_| WireError::LengthOverflow)?,
                );
                write_u64(out, *tally);
            }
        }
        Ok(())
    }
}

impl WireDecode for PnCounterDelta {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        let tag = cursor.read_u8()?;
        let replica = usize::try_from(cursor.read_u64()?).map_err(|_| WireError::LengthOverflow)?;
        let tally = cursor.read_u64()?;
        match tag {
            TAG_PNCOUNTER_INC => Ok(PnCounterDelta::Inc { replica, tally }),
            TAG_PNCOUNTER_DEC => Ok(PnCounterDelta::Dec { replica, tally }),
            _ => Err(WireError::InvalidTag),
        }
    }
}

impl WireEncode for GSet<u64> {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        write_u8(out, TAG_GSET_U64);
        write_len(out, self.elements.len())?;
        for element in &self.elements {
            write_u64(out, *element);
        }
        Ok(())
    }
}

impl WireDecode for GSet<u64> {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        read_tag(cursor, TAG_GSET_U64)?;
        let mut set = GSet::new();
        for _ in 0..cursor.read_len()? {
            set.insert(cursor.read_u64()?);
        }
        Ok(set)
    }
}

impl WireEncode for OrSetDelta<u64, u64> {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        match self {
            OrSetDelta::Add { element, token } => {
                write_u8(out, TAG_ORSET_ADD_U64);
                write_u64(out, *element);
                write_u64(out, *token);
            }
            OrSetDelta::Remove { tokens } => {
                write_u8(out, TAG_ORSET_REMOVE_U64);
                write_len(out, tokens.len())?;
                // Preserve order and duplicates for exact delta round trips.
                for token in tokens {
                    write_u64(out, *token);
                }
            }
        }
        Ok(())
    }
}

impl WireDecode for OrSetDelta<u64, u64> {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        match cursor.read_u8()? {
            TAG_ORSET_ADD_U64 => Ok(OrSetDelta::Add {
                element: cursor.read_u64()?,
                token: cursor.read_u64()?,
            }),
            TAG_ORSET_REMOVE_U64 => {
                let mut tokens = Vec::new();
                for _ in 0..cursor.read_len()? {
                    tokens.push(cursor.read_u64()?);
                }
                Ok(OrSetDelta::Remove { tokens })
            }
            _ => Err(WireError::InvalidTag),
        }
    }
}

// UTF-8 delta tags are distinct from the u64 delta tags, including Remove.
// Add carries a byte-length-prefixed UTF-8 element followed by a u64 token;
// Remove carries a token count followed by u64 tokens in their original order.
impl WireEncode for OrSetDelta<String, u64> {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        match self {
            OrSetDelta::Add { element, token } => {
                write_u8(out, TAG_ORSET_ADD_STRING);
                write_bytes(out, element.as_bytes())?;
                write_u64(out, *token);
            }
            OrSetDelta::Remove { tokens } => {
                write_u8(out, TAG_ORSET_REMOVE_STRING);
                write_len(out, tokens.len())?;
                for token in tokens {
                    write_u64(out, *token);
                }
            }
        }
        Ok(())
    }
}

impl WireDecode for OrSetDelta<String, u64> {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        match cursor.read_u8()? {
            TAG_ORSET_ADD_STRING => {
                let len = cursor.read_len()?;
                let element = core::str::from_utf8(cursor.read_exact(len)?)
                    .map_err(|_| WireError::InvalidUtf8)?;
                let token = cursor.read_u64()?;
                Ok(OrSetDelta::Add {
                    element: String::from(element),
                    token,
                })
            }
            TAG_ORSET_REMOVE_STRING => {
                let mut tokens = Vec::new();
                for _ in 0..cursor.read_len()? {
                    tokens.push(cursor.read_u64()?);
                }
                Ok(OrSetDelta::Remove { tokens })
            }
            _ => Err(WireError::InvalidTag),
        }
    }
}

impl WireEncode for OrSet<u64, u64> {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        write_u8(out, TAG_ORSET_U64);
        write_len(out, self.adds.len())?;
        for (element, token) in &self.adds {
            write_u64(out, *element);
            write_u64(out, *token);
        }
        write_len(out, self.tombstones.len())?;
        for token in &self.tombstones {
            write_u64(out, *token);
        }
        Ok(())
    }
}

impl WireDecode for OrSet<u64, u64> {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        read_tag(cursor, TAG_ORSET_U64)?;
        let mut set = OrSet::new();
        for _ in 0..cursor.read_len()? {
            let element = cursor.read_u64()?;
            let token = cursor.read_u64()?;
            set.add(element, token);
        }
        let mut tombstones = Vec::new();
        for _ in 0..cursor.read_len()? {
            tombstones.push(cursor.read_u64()?);
        }
        set.apply_remove(tombstones);
        Ok(set)
    }
}

impl WireEncode for Rga<u64, u64> {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        write_u8(out, TAG_RGA_U64);
        write_len(out, self.placed.len())?;
        for (position, value) in &self.placed {
            write_u64(out, *position);
            write_u64(out, *value);
        }
        write_len(out, self.tombstones.len())?;
        for position in &self.tombstones {
            write_u64(out, *position);
        }
        Ok(())
    }
}

impl WireDecode for Rga<u64, u64> {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        read_tag(cursor, TAG_RGA_U64)?;
        let mut rga = Rga::new();
        for _ in 0..cursor.read_len()? {
            let position = cursor.read_u64()?;
            let value = cursor.read_u64()?;
            rga.insert(position, value);
        }
        for _ in 0..cursor.read_len()? {
            rga.delete(cursor.read_u64()?);
        }
        Ok(rga)
    }
}

impl<D: WireEncode> WireEncode for Record<D> {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        write_u8(out, TAG_RECORD);
        write_u64(out, self.id.replica);
        write_u64(out, self.id.sequence);
        write_bytes(out, &self.delta.to_wire_bytes()?)?;
        Ok(())
    }
}

impl<D: WireDecode> WireDecode for Record<D> {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        read_tag(cursor, TAG_RECORD)?;
        let id = RecordId {
            replica: cursor.read_u64()?,
            sequence: cursor.read_u64()?,
        };
        let delta_len = cursor.read_len()?;
        let delta_bytes = cursor.read_exact(delta_len)?;
        let delta = D::from_wire_bytes(delta_bytes)?;
        Ok(Record { id, delta })
    }
}

// CRC-32/ISO-HDLC: reflected polynomial, all-ones initialization and final XOR.
// Detects accidental corruption, including every single-byte change; not a MAC.
fn frame_crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

// Replacement frame (no legacy decoder): tag, body length, complemented length,
// body (record count and length-prefixed records), CRC of length fields + body.
// Frame lengths, count and CRC are little-endian u32. Check the length pair before trusting it,
// then verify the CRC before decoding any record or invoking a payload decoder.
impl<D: WireEncode> WireEncode for EventLog<D> {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        let mut body = Vec::new();
        write_len(&mut body, self.records.len())?;
        for record in &self.records {
            write_bytes(&mut body, &record.to_wire_bytes()?)?;
        }
        let len = u32::try_from(body.len()).map_err(|_| WireError::LengthOverflow)?;
        write_u8(out, TAG_EVENT_LOG);
        let start = out.len();
        write_u32(out, len);
        write_u32(out, !len);
        out.extend_from_slice(&body);
        let checksum = frame_crc32(&out[start..]);
        write_u32(out, checksum);
        Ok(())
    }
}

impl<D: WireDecode + PartialEq> WireDecode for EventLog<D> {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        read_tag(cursor, TAG_EVENT_LOG)?;
        let start = cursor.offset;
        let len = cursor.read_u32()?;
        if cursor.read_u32()? != !len {
            return Err(WireError::IntegrityMismatch);
        }
        let body =
            cursor.read_exact(usize::try_from(len).map_err(|_| WireError::LengthOverflow)?)?;
        let checksum = frame_crc32(&cursor.bytes[start..cursor.offset]);
        if cursor.read_u32()? != checksum {
            return Err(WireError::IntegrityMismatch);
        }
        let mut body = WireCursor::new(body);
        let mut log = EventLog::new();
        for _ in 0..body.read_len()? {
            let record_len = body.read_len()?;
            let record_bytes = body.read_exact(record_len)?;
            if log.insert_record(Record::<D>::from_wire_bytes(record_bytes)?)
                == Admission::Collision
            {
                return Err(WireError::RecordCollision);
            }
        }
        if !body.is_empty() {
            return Err(WireError::TrailingBytes);
        }
        Ok(log)
    }
}

impl WireEncode for LwwRegisterDelta<u64> {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        write_u8(out, TAG_LWW_REGISTER_DELTA_U64);
        write_u64(out, self.timestamp);
        write_u64(out, self.replica);
        write_u64(out, self.value);
        Ok(())
    }
}

impl WireDecode for LwwRegisterDelta<u64> {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        read_tag(cursor, TAG_LWW_REGISTER_DELTA_U64)?;
        Ok(LwwRegisterDelta {
            timestamp: cursor.read_u64()?,
            replica: cursor.read_u64()?,
            value: cursor.read_u64()?,
        })
    }
}

impl WireEncode for LwwRegister<u64> {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        write_u8(out, TAG_LWW_REGISTER_U64);
        match self.entry() {
            Some(entry) => {
                write_u8(out, 1);
                write_u64(out, entry.dot.timestamp);
                write_u64(out, entry.dot.replica);
                write_u64(out, entry.value);
            }
            None => write_u8(out, 0),
        }
        Ok(())
    }
}

impl WireDecode for LwwRegister<u64> {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        read_tag(cursor, TAG_LWW_REGISTER_U64)?;
        let present = cursor.read_u8()?;
        match present {
            0 => Ok(LwwRegister::new()),
            1 => {
                let mut register = LwwRegister::new();
                register.set(cursor.read_u64()?, cursor.read_u64()?, cursor.read_u64()?);
                Ok(register)
            }
            _ => Err(WireError::InvalidTag),
        }
    }
}

impl WireEncode for EnableWinsFlagDelta<u64> {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        match self {
            EnableWinsFlagDelta::Enable { token } => {
                write_u8(out, TAG_ENABLE_WINS_FLAG_ENABLE_U64);
                write_u64(out, *token);
            }
            EnableWinsFlagDelta::Disable { tokens } => {
                write_u8(out, TAG_ENABLE_WINS_FLAG_DISABLE_U64);
                let mut sorted = tokens.clone();
                sorted.sort();
                sorted.dedup();
                write_len(out, sorted.len())?;
                for token in sorted {
                    write_u64(out, token);
                }
            }
        }
        Ok(())
    }
}

impl WireDecode for EnableWinsFlagDelta<u64> {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        let tag = cursor.read_u8()?;
        match tag {
            TAG_ENABLE_WINS_FLAG_ENABLE_U64 => Ok(EnableWinsFlagDelta::Enable {
                token: cursor.read_u64()?,
            }),
            TAG_ENABLE_WINS_FLAG_DISABLE_U64 => {
                let len = cursor.read_len()?;
                let mut tokens = Vec::new();
                for _ in 0..len {
                    tokens.push(cursor.read_u64()?);
                }
                Ok(EnableWinsFlagDelta::Disable { tokens })
            }
            _ => Err(WireError::InvalidTag),
        }
    }
}

impl WireEncode for EnableWinsFlag<u64> {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        write_u8(out, TAG_ENABLE_WINS_FLAG_U64);
        write_len(out, self.enables.len())?;
        for token in self.enables.iter() {
            write_u64(out, *token);
        }
        write_len(out, self.tombstones.len())?;
        for token in self.tombstones.iter() {
            write_u64(out, *token);
        }
        Ok(())
    }
}

impl WireDecode for EnableWinsFlag<u64> {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        read_tag(cursor, TAG_ENABLE_WINS_FLAG_U64)?;
        let enable_len = cursor.read_len()?;
        let mut flag = EnableWinsFlag::new();
        for _ in 0..enable_len {
            flag.enable(cursor.read_u64()?);
        }
        let tombstone_len = cursor.read_len()?;
        for _ in 0..tombstone_len {
            flag.disable([cursor.read_u64()?]);
        }
        Ok(flag)
    }
}

impl WireEncode for LwwMapDelta<u64, u64> {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        match self {
            LwwMapDelta::Set {
                key,
                timestamp,
                replica,
                value,
            } => {
                write_u8(out, TAG_LWW_MAP_SET_U64);
                write_u64(out, *key);
                write_u64(out, *timestamp);
                write_u64(out, *replica);
                write_u64(out, *value);
            }
            LwwMapDelta::Remove {
                key,
                timestamp,
                replica,
            } => {
                write_u8(out, TAG_LWW_MAP_REMOVE_U64);
                write_u64(out, *key);
                write_u64(out, *timestamp);
                write_u64(out, *replica);
            }
        }
        Ok(())
    }
}

impl WireDecode for LwwMapDelta<u64, u64> {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        let tag = cursor.read_u8()?;
        match tag {
            TAG_LWW_MAP_SET_U64 => Ok(LwwMapDelta::Set {
                key: cursor.read_u64()?,
                timestamp: cursor.read_u64()?,
                replica: cursor.read_u64()?,
                value: cursor.read_u64()?,
            }),
            TAG_LWW_MAP_REMOVE_U64 => Ok(LwwMapDelta::Remove {
                key: cursor.read_u64()?,
                timestamp: cursor.read_u64()?,
                replica: cursor.read_u64()?,
            }),
            _ => Err(WireError::InvalidTag),
        }
    }
}

impl WireEncode for LwwMap<u64, u64> {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        write_u8(out, TAG_LWW_MAP_U64);
        write_len(out, self.entries.len())?;
        for (key, entry) in self.entries.iter() {
            write_u64(out, *key);
            write_u64(out, entry.dot.timestamp);
            write_u64(out, entry.dot.replica);
            write_u64(out, entry.value);
        }
        write_len(out, self.removals.len())?;
        for (key, dot) in self.removals.iter() {
            write_u64(out, *key);
            write_u64(out, dot.timestamp);
            write_u64(out, dot.replica);
        }
        Ok(())
    }
}

impl WireDecode for LwwMap<u64, u64> {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        read_tag(cursor, TAG_LWW_MAP_U64)?;
        let entry_len = cursor.read_len()?;
        let mut map = LwwMap::new();
        for _ in 0..entry_len {
            map.set(
                cursor.read_u64()?,
                cursor.read_u64()?,
                cursor.read_u64()?,
                cursor.read_u64()?,
            );
        }
        let removal_len = cursor.read_len()?;
        for _ in 0..removal_len {
            map.remove(cursor.read_u64()?, cursor.read_u64()?, cursor.read_u64()?);
        }
        Ok(map)
    }
}

#[cfg(feature = "laws")]
pub mod laws {
    use super::{Crdt, Mergeable};
    use alloc::vec::Vec;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Law {
        Commutative,
        Associative,
        Idempotent,
        Identity,
        Convergence,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct LawFailure {
        pub law: Law,
        pub a: usize,
        pub b: Option<usize>,
        pub c: Option<usize>,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct LawReport {
        pub scenarios: usize,
    }

    pub fn check_merge_laws<T>(identity: &T, samples: &[T]) -> Result<LawReport, LawFailure>
    where
        T: Mergeable + Clone + PartialEq,
    {
        let mut scenarios = 0;

        for (a_idx, a) in samples.iter().enumerate() {
            let mut aa = a.clone();
            aa.merge(a);
            scenarios += 1;
            if aa != *a {
                return Err(LawFailure {
                    law: Law::Idempotent,
                    a: a_idx,
                    b: None,
                    c: None,
                });
            }

            let mut left_identity = identity.clone();
            left_identity.merge(a);
            let mut right_identity = a.clone();
            right_identity.merge(identity);
            scenarios += 2;
            if left_identity != *a || right_identity != *a {
                return Err(LawFailure {
                    law: Law::Identity,
                    a: a_idx,
                    b: None,
                    c: None,
                });
            }

            for (b_idx, b) in samples.iter().enumerate() {
                let mut ab = a.clone();
                ab.merge(b);
                let mut ba = b.clone();
                ba.merge(a);
                scenarios += 1;
                if ab != ba {
                    return Err(LawFailure {
                        law: Law::Commutative,
                        a: a_idx,
                        b: Some(b_idx),
                        c: None,
                    });
                }

                for (c_idx, c) in samples.iter().enumerate() {
                    let mut left = a.clone();
                    left.merge(b);
                    left.merge(c);

                    let mut right_inner = b.clone();
                    right_inner.merge(c);
                    let mut right = a.clone();
                    right.merge(&right_inner);

                    scenarios += 1;
                    if left != right {
                        return Err(LawFailure {
                            law: Law::Associative,
                            a: a_idx,
                            b: Some(b_idx),
                            c: Some(c_idx),
                        });
                    }
                }
            }
        }

        Ok(LawReport { scenarios })
    }

    pub fn check_crdt_convergence<C>(
        seed_state: &C,
        deltas: &[C::Delta],
    ) -> Result<LawReport, LawFailure>
    where
        C: Crdt + Clone + PartialEq,
        C::Delta: Clone,
    {
        let expected = apply_all(seed_state, deltas.iter().cloned());
        let mut scenarios = 1;

        let reverse = apply_all(seed_state, deltas.iter().cloned().rev());
        scenarios += 1;
        if reverse != expected {
            return Err(convergence_failure(0));
        }

        let duplicated = apply_all(
            seed_state,
            deltas.iter().cloned().flat_map(|d| [d.clone(), d]),
        );
        scenarios += 1;
        if duplicated != expected {
            return Err(convergence_failure(1));
        }

        for (seed_idx, seed) in [0x51a7_3eed_u64, 0xc0ff_ee13_u64, 0x5afe_0001_u64]
            .iter()
            .copied()
            .enumerate()
        {
            let shuffled = shuffled(deltas, seed);
            let shuffled_state = apply_all(seed_state, shuffled.into_iter());
            scenarios += 1;
            if shuffled_state != expected {
                return Err(convergence_failure(2 + seed_idx));
            }
        }

        let mut even = seed_state.clone();
        let mut odd = seed_state.clone();
        for (idx, delta) in deltas.iter().cloned().enumerate() {
            if idx % 2 == 0 {
                even.apply_delta(delta);
            } else {
                odd.apply_delta(delta);
            }
        }
        even.merge(&odd);
        scenarios += 1;
        if even != expected {
            return Err(convergence_failure(5));
        }

        let mut lossy_a = seed_state.clone();
        let mut lossy_b = seed_state.clone();
        for (idx, delta) in deltas.iter().cloned().enumerate() {
            if idx % 3 == 0 {
                lossy_b.apply_delta(delta);
            } else {
                lossy_a.apply_delta(delta);
            }
        }
        lossy_a.merge(&lossy_b);
        scenarios += 1;
        if lossy_a != expected {
            return Err(convergence_failure(6));
        }

        Ok(LawReport { scenarios })
    }

    fn apply_all<C, I>(seed_state: &C, deltas: I) -> C
    where
        C: Crdt + Clone,
        I: IntoIterator<Item = C::Delta>,
    {
        let mut state = seed_state.clone();
        for delta in deltas {
            state.apply_delta(delta);
        }
        state
    }

    fn convergence_failure(a: usize) -> LawFailure {
        LawFailure {
            law: Law::Convergence,
            a,
            b: None,
            c: None,
        }
    }

    fn shuffled<T: Clone>(items: &[T], seed: u64) -> Vec<T> {
        let mut out = items.to_vec();
        let mut rng = XorShift64::new(seed);
        let len = out.len();
        if len < 2 {
            return out;
        }
        let mut i = len - 1;
        while i > 0 {
            let j = rng.next_usize(i + 1);
            out.swap(i, j);
            i -= 1;
        }
        out
    }

    struct XorShift64 {
        state: u64,
    }

    impl XorShift64 {
        fn new(seed: u64) -> Self {
            let state = if seed == 0 {
                0x9e37_79b9_7f4a_7c15
            } else {
                seed
            };
            XorShift64 { state }
        }

        fn next_u64(&mut self) -> u64 {
            let mut x = self.state;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.state = x;
            x
        }

        fn next_usize(&mut self, upper: usize) -> usize {
            (self.next_u64() as usize) % upper
        }
    }
}

#[cfg(test)]
mod frame_tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn crc32_matches_standard_check_vector() {
        assert_eq!(frame_crc32(b"123456789"), 0xcbf4_3926);
    }

    #[test]
    fn binding_collision_fixtures_use_product_encoder() {
        extern crate std;
        fn fixture<D: WireEncode + WireDecode + PartialEq>(name: &str, deltas: [D; 2]) {
            let mut log = EventLog::new();
            log.records = deltas
                .into_iter()
                .map(|delta| Record {
                    id: RecordId {
                        replica: 1,
                        sequence: 1,
                    },
                    delta,
                })
                .collect();
            let bytes = log.to_wire_bytes().unwrap();
            assert!(matches!(
                EventLog::<D>::from_wire_bytes(&bytes),
                Err(WireError::RecordCollision)
            ));
            if let Some(directory) = std::env::var_os("SAFEMESH_FRAME_FIXTURE_DIR") {
                std::fs::write(std::path::Path::new(&directory).join(name), bytes).unwrap();
            }
        }
        fixture(
            "gcounter-collision.bin",
            [
                GCounterDelta {
                    replica: 1,
                    tally: 5,
                },
                GCounterDelta {
                    replica: 1,
                    tally: 9,
                },
            ],
        );
        fixture(
            "flag-collision.bin",
            [
                EnableWinsFlagDelta::Enable { token: 5u64 },
                EnableWinsFlagDelta::Enable { token: 9u64 },
            ],
        );
        fixture(
            "register-collision.bin",
            [
                LwwRegisterDelta {
                    timestamp: 1,
                    replica: 1,
                    value: 5u64,
                },
                LwwRegisterDelta {
                    timestamp: 2,
                    replica: 1,
                    value: 9u64,
                },
            ],
        );
        fixture(
            "map-collision.bin",
            [
                LwwMapDelta::Set {
                    key: 1u64,
                    timestamp: 1,
                    replica: 1,
                    value: 5u64,
                },
                LwwMapDelta::Remove {
                    key: 1u64,
                    timestamp: 2,
                    replica: 1,
                },
            ],
        );
    }

    fn record(sequence: u64, tally: u64) -> Record<GCounterDelta> {
        Record {
            id: RecordId {
                replica: 1,
                sequence,
            },
            delta: GCounterDelta { replica: 1, tally },
        }
    }

    // Deliberately bypass admission to exercise the decoder's collision gate.
    // The production encoder derives all frame bytes, lengths and checksums.
    fn wire_log(records: &[Record<GCounterDelta>]) -> Vec<u8> {
        let mut log = EventLog::new();
        log.records = records.to_vec();
        log.to_wire_bytes().unwrap()
    }
    #[test]
    fn decoder_surfaces_conflicts_before_deduplication() {
        for records in [
            vec![record(1, 5), record(1, 9)],
            vec![record(1, 9), record(1, 5)],
        ] {
            assert_eq!(
                EventLog::<GCounterDelta>::from_wire_bytes(&wire_log(&records)),
                Err(WireError::RecordCollision)
            );
        }
        let decoded =
            EventLog::<GCounterDelta>::from_wire_bytes(&wire_log(&[record(1, 5), record(1, 5)]))
                .unwrap();
        assert_eq!(decoded.records(), &[record(1, 5)]);
    }
}
