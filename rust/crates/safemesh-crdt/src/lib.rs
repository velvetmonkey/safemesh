// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: AGPL-3.0-or-later

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

/// Delta for a grow-only counter: one replica coordinate and its asserted tally.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GCounterDelta {
    pub replica: usize,
    pub tally: u64,
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
    pub fn apply_bump(&mut self, replica: usize, tally: u64) {
        if let Some(c) = self.counts.get_mut(replica) {
            if tally > *c {
                *c = tally;
            }
        }
    }

    /// Full-state merge: pointwise max — `Crdt.gcounter_merge_apply`. A
    /// delta replica and a full-state replica fed the same bumps agree:
    /// `SafeMesh.deltaGCounter_matches_full`.
    pub fn merge(&mut self, other: &GCounter) {
        for (c, o) in self.counts.iter_mut().zip(other.counts.iter()) {
            if *o > *c {
                *c = *o;
            }
        }
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
    pub fn apply_inc(&mut self, replica: usize, tally: u64) {
        self.p.apply_bump(replica, tally);
    }

    /// Apply `SafeMesh.deltaBumpN replica tally` — decrement side.
    pub fn apply_dec(&mut self, replica: usize, tally: u64) {
        self.n.apply_bump(replica, tally);
    }

    /// Full-state merge: componentwise G-Counter merge —
    /// `Crdt.pncounter_merge_apply`.
    pub fn merge(&mut self, other: &PnCounter) {
        self.p.merge(&other.p);
        self.n.merge(&other.n);
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

/// Append-only, deduplicating event log for CRDT deltas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventLog<D> {
    records: Vec<Record<D>>,
    seen: BTreeSet<RecordId>,
    version: VersionVector,
}

impl<D> EventLog<D> {
    pub fn new() -> Self {
        EventLog {
            records: Vec::new(),
            seen: BTreeSet::new(),
            version: VersionVector::new(),
        }
    }

    pub fn append(&mut self, replica: u64, delta: D) -> RecordId {
        let id = RecordId {
            replica,
            sequence: self.version.get(replica) + 1,
        };
        self.insert_record(Record { id, delta });
        id
    }

    pub fn merge_records<I>(&mut self, records: I)
    where
        I: IntoIterator<Item = Record<D>>,
    {
        for record in records {
            self.insert_record(record);
        }
    }

    pub fn version(&self) -> &VersionVector {
        &self.version
    }

    pub fn records(&self) -> &[Record<D>] {
        &self.records
    }

    fn insert_record(&mut self, record: Record<D>) {
        let id = record.id;
        if self.seen.insert(id) {
            self.records.push(record);
            self.advance_contiguous_version(id.replica);
        }
    }

    fn advance_contiguous_version(&mut self, replica: u64) {
        loop {
            let Some(next) = self.version.get(replica).checked_add(1) else {
                break;
            };
            if self.seen.contains(&RecordId {
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
const TAG_EVENT_LOG: u8 = 0x02;
const TAG_GCOUNTER_DELTA: u8 = 0x10;
const TAG_PNCOUNTER_INC: u8 = 0x11;
const TAG_PNCOUNTER_DEC: u8 = 0x12;
const TAG_GSET_U64: u8 = 0x20;
const TAG_ORSET_U64: u8 = 0x30;
const TAG_RGA_U64: u8 = 0x40;
const TAG_LWW_REGISTER_DELTA_U64: u8 = 0x50;
const TAG_LWW_REGISTER_U64: u8 = 0x51;
const TAG_ENABLE_WINS_FLAG_ENABLE_U64: u8 = 0x60;
const TAG_ENABLE_WINS_FLAG_DISABLE_U64: u8 = 0x61;
const TAG_ENABLE_WINS_FLAG_U64: u8 = 0x62;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireError {
    UnexpectedEof,
    InvalidTag,
    TrailingBytes,
    LengthOverflow,
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

impl<D: WireEncode> WireEncode for EventLog<D> {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        write_u8(out, TAG_EVENT_LOG);
        write_len(out, self.records.len())?;
        for record in &self.records {
            write_bytes(out, &record.to_wire_bytes()?)?;
        }
        Ok(())
    }
}

impl<D: WireDecode> WireDecode for EventLog<D> {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        read_tag(cursor, TAG_EVENT_LOG)?;
        let mut log = EventLog::new();
        for _ in 0..cursor.read_len()? {
            let record_len = cursor.read_len()?;
            let record_bytes = cursor.read_exact(record_len)?;
            log.merge_records([Record::<D>::from_wire_bytes(record_bytes)?]);
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
