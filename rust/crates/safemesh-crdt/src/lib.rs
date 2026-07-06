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

/// Per-replica high-water marks for anti-entropy pulls.
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

    pub fn observe(&mut self, id: RecordId) {
        let current = self.entries.entry(id.replica).or_insert(0);
        if id.sequence > *current {
            *current = id.sequence;
        }
    }

    pub fn includes(&self, id: RecordId) -> bool {
        self.get(id.replica) >= id.sequence
    }

    pub fn entries(&self) -> &BTreeMap<u64, u64> {
        &self.entries
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
        if self.seen.insert(record.id) {
            self.version.observe(record.id);
            self.records.push(record);
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
